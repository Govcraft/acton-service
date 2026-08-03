//! Actor-backed extensions for custom application state
//!
//! This module provides the [`ActorExtension`](crate::extensions::ActorExtension) trait and supporting types for adding
//! custom runtime state to your application. All extensions are backed by supervised
//! acton-reactive actors, providing:
//!
//! - **Supervision**: Automatic restart on failure via configurable [`RestartPolicy`](acton_reactive::prelude::RestartPolicy)
//! - **Broker subscriptions**: Subscribe to framework-wide broadcast events
//! - **Observability**: Built-in tracing instrumentation from the actor runtime
//! - **No mutexes**: State is encapsulated in actors, accessed via message passing
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use acton_service::prelude::*;
//! use acton_reactive::prelude::*;
//!
//! #[acton_actor]
//! pub struct MyCache {
//!     items: HashMap<String, String>,
//! }
//!
//! impl ActorExtension for MyCache {
//!     fn configure(actor: &mut ManagedActor<Idle, Self>) {
//!         actor.mutate_on::<CacheSet>(|a, env| {
//!             let msg = env.message();
//!             a.model.items.insert(msg.key.clone(), msg.value.clone());
//!             Reply::ready()
//!         });
//!     }
//! }
//!
//! // Register during service build
//! ServiceBuilder::new()
//!     .with_actor::<MyCache>()
//!     .with_routes(routes)
//!     .build()
//!     .serve()
//!     .await?;
//!
//! // Access in handlers
//! async fn handler(State(state): State<AppState>) -> impl IntoResponse {
//!     let cache = state.actor::<MyCache>().unwrap();
//!     cache.send(CacheSet { key: "k".into(), value: "v".into() }).await;
//! }
//! ```

use std::any::TypeId;
use std::collections::HashMap;
use std::fmt::Debug;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;

use acton_reactive::prelude::{
    ActorConfig, ActorHandle, ActorRuntime, Idle, ManagedActor, RestartPolicy, SupervisedChild,
};

/// Future type returned by [`ActorExtensionSpawner::spawn`].
type SpawnFuture<'a> =
    Pin<Box<dyn Future<Output = anyhow::Result<(TypeId, SupervisedChild)>> + Send + 'a>>;

/// Trait for defining actor-backed extensions.
///
/// Implement this trait on an `#[acton_actor]` struct to register it as a
/// supervised extension via
/// [`ServiceBuilder::with_actor`](crate::service_builder::ServiceBuilder::with_actor).
///
/// The [`configure`](ActorExtension::configure) method receives a mutable reference
/// to the actor builder, where you register message handlers and lifecycle hooks:
///
/// - `mutate_on` / `mutate_on_sync` — handlers that can modify actor state
/// - `act_on` / `act_on_sync` — read-only handlers (queries)
/// - `after_start` / `before_stop` — lifecycle hooks
/// - `handle.subscribe::<M>()` — broker subscriptions (in `after_start`)
///
/// `configure` doubles as the actor's restart blueprint: it is re-run for every
/// incarnation. That is why broker subscriptions belong in an `after_start`
/// registered here rather than being awaited once at startup — a subscription
/// taken outside `configure` is lost the first time the actor restarts.
///
/// Note that `after_start`'s future is drained alongside the message loop, not
/// ahead of it, so "the actor has started" does not imply "the actor has
/// subscribed". Code that broadcasts immediately after
/// [`ServiceBuilder::build()`](crate::service_builder::ServiceBuilder::build)
/// can race a subscription that has not yet reached the broker.
///
/// ## Restart Policy
///
/// Override [`restart_policy`](ActorExtension::restart_policy) to control supervision behavior:
///
/// - [`Permanent`](RestartPolicy::Permanent) (default) — restart on any termination, clean stops included
/// - [`Transient`](RestartPolicy::Transient) — restart only on abnormal termination
/// - [`Temporary`](RestartPolicy::Temporary) — never restart
///
/// Because a restart replaces the actor, hold the handle from
/// [`AppState::actor`](crate::state::AppState::actor) only for as long as you
/// need it and re-resolve per request rather than caching it.
pub trait ActorExtension: Default + Debug + Send + 'static {
    /// Configure message handlers, lifecycle hooks, and broker subscriptions.
    ///
    /// Re-run on every restart, so everything the actor needs to function must
    /// be registered here.
    fn configure(actor: &mut ManagedActor<Idle, Self>);

    /// Restart policy under supervision. Defaults to [`RestartPolicy::Permanent`].
    fn restart_policy() -> RestartPolicy {
        RestartPolicy::Permanent
    }
}

/// Type-erased spawner for heterogeneous actor extension registrations.
///
/// This trait allows `ServiceBuilder` to store registrations for different
/// concrete `ActorExtension` types in a single `Vec`.
pub(crate) trait ActorExtensionSpawner: Send {
    /// Spawn this actor extension under the given supervisor.
    ///
    /// Returns the `TypeId` of the concrete extension type and the
    /// [`SupervisedChild`] tracking it. `index` disambiguates the child's
    /// identifier so two extensions whose type names sanitize to the same
    /// string cannot collide.
    fn spawn<'a>(
        &'a self,
        supervisor: &'a ActorHandle,
        runtime: &'a ActorRuntime,
        index: usize,
    ) -> SpawnFuture<'a>;
}

/// Generic entry that captures a concrete `ActorExtension` type for type-erased spawning.
pub(crate) struct ActorExtensionEntry<A: ActorExtension>(pub(crate) PhantomData<A>);

impl<A: ActorExtension> ActorExtensionSpawner for ActorExtensionEntry<A> {
    fn spawn<'a>(
        &'a self,
        supervisor: &'a ActorHandle,
        runtime: &'a ActorRuntime,
        index: usize,
    ) -> SpawnFuture<'a> {
        Box::pin(async move {
            // `supervise_with` rather than `supervise`: a child adopted through
            // the legacy call is registered without a blueprint, so the runtime
            // reports its termination but can never recreate it. That made
            // `restart_policy` below decorative. Handing over `A::configure` as
            // the blueprint is what lets the restart engine rebuild the actor,
            // which is the whole point of declaring a policy.
            let config = ActorConfig::for_supervised_child(
                extension_child_name::<A>(index),
                supervisor.clone(),
                Some(runtime.broker()),
            )?
            .with_restart_policy(A::restart_policy());

            let child = supervisor
                .supervise_with::<A>(runtime, config, A::configure)
                .await?;

            tracing::info!(
                actor_type = std::any::type_name::<A>(),
                restart_policy = ?A::restart_policy(),
                "Actor extension spawned and supervised"
            );
            Ok((TypeId::of::<A>(), child))
        })
    }
}

/// Build a valid ERN segment naming an extension's supervised child.
///
/// ERN parts allow only alphanumerics, hyphens, underscores and dots, and cap
/// at 63 characters, so a raw `type_name` (which carries `::` and generic
/// brackets) cannot be used directly. The trailing `index` keeps the result
/// unique even when two distinct types reduce to the same readable stem.
fn extension_child_name<A: 'static>(index: usize) -> String {
    let raw = std::any::type_name::<A>();
    let stem: String = raw
        .rsplit("::")
        .next()
        .unwrap_or(raw)
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
        .take(48)
        .collect();

    if stem.is_empty() {
        format!("extension-{index}")
    } else {
        format!("{stem}-{index}")
    }
}

/// Immutable container mapping actor extension types to their handles.
///
/// Constructed during
/// [`ServiceBuilder::build()`](crate::service_builder::ServiceBuilder::build) and stored on
/// [`AppState`](crate::state::AppState).
/// Clone is cheap (Arc ref-count bump). When no actor extensions are registered,
/// the inner map is never allocated.
#[derive(Clone, Default)]
pub struct ActorExtensions {
    inner: Option<Arc<HashMap<TypeId, SupervisedChild>>>,
}

impl ActorExtensions {
    /// Get an [`ActorHandle`] for a registered actor extension.
    ///
    /// Returns `None` if no actor of type `A` was registered, and also while a
    /// supervised actor is between incarnations — a restart replaces the actor,
    /// and a handle to the previous one would silently address a corpse. The
    /// handle is resolved per call for that reason; hold the result only for as
    /// long as you need it, and re-resolve rather than caching it.
    pub fn get<A: ActorExtension>(&self) -> Option<ActorHandle> {
        self.inner.as_ref()?.get(&TypeId::of::<A>())?.current()
    }

    /// Get the [`SupervisedChild`] tracking a registered actor extension.
    ///
    /// Exposes restart generation and supervision state, and lets a caller wait
    /// for the actor to come back after a restart via
    /// [`wait_running`](acton_reactive::prelude::SupervisedChild::wait_running).
    pub fn supervised<A: ActorExtension>(&self) -> Option<&SupervisedChild> {
        self.inner.as_ref()?.get(&TypeId::of::<A>())
    }

    /// Returns `true` if no actor extensions are registered.
    pub fn is_empty(&self) -> bool {
        self.inner.as_ref().is_none_or(|m| m.is_empty())
    }
}

impl From<HashMap<TypeId, SupervisedChild>> for ActorExtensions {
    fn from(map: HashMap<TypeId, SupervisedChild>) -> Self {
        if map.is_empty() {
            Self { inner: None }
        } else {
            Self {
                inner: Some(Arc::new(map)),
            }
        }
    }
}

/// Minimal supervisor actor state for the extensions supervision tree.
///
/// This actor exists solely to parent user-registered actor extensions,
/// providing OneForOne supervision (restart only the failed child).
#[derive(Debug, Default)]
pub(crate) struct ExtensionsSupervisorState;

#[cfg(test)]
#[allow(dead_code)] // message fields are read inside actor handlers via envelope.message()
mod tests {
    use super::*;
    use acton_reactive::prelude::*;

    // ── Container unit tests ───────────────────────────────────────────

    #[test]
    fn actor_extensions_default_is_empty() {
        let ext = ActorExtensions::default();
        assert!(ext.is_empty());
    }

    #[test]
    fn actor_extensions_from_empty_map_allocates_nothing() {
        let ext = ActorExtensions::from(HashMap::new());
        assert!(ext.is_empty());
        assert!(ext.inner.is_none(), "empty map should not allocate Arc");
    }

    #[test]
    fn actor_extensions_get_missing_type_returns_none() {
        let ext = ActorExtensions::default();

        #[derive(Debug, Default)]
        struct NotRegistered;
        impl ActorExtension for NotRegistered {
            fn configure(_actor: &mut ManagedActor<Idle, Self>) {}
        }

        assert!(ext.get::<NotRegistered>().is_none());
    }

    #[test]
    fn default_restart_policy_is_permanent() {
        #[derive(Debug, Default)]
        struct TestActor;
        impl ActorExtension for TestActor {
            fn configure(_actor: &mut ManagedActor<Idle, Self>) {}
        }

        assert_eq!(TestActor::restart_policy(), RestartPolicy::Permanent);
    }

    #[test]
    fn custom_restart_policy_is_respected() {
        #[derive(Debug, Default)]
        struct TransientActor;
        impl ActorExtension for TransientActor {
            fn configure(_actor: &mut ManagedActor<Idle, Self>) {}
            fn restart_policy() -> RestartPolicy {
                RestartPolicy::Transient
            }
        }

        assert_eq!(TransientActor::restart_policy(), RestartPolicy::Transient);
    }

    // ── Message types for integration tests ────────────────────────────

    #[derive(Clone, Debug)]
    struct Increment {
        amount: u32,
    }

    #[derive(Clone, Debug)]
    struct GetCount;

    #[derive(Clone, Debug)]
    struct CountResponse {
        count: u32,
    }

    /// Makes `GetCount` usable with `ask`, which is what lets these tests
    /// synchronise on the actor having actually processed its mailbox instead
    /// of sleeping and hoping. Mailboxes are FIFO, so a resolved `ask` proves
    /// every message sent before it has been handled.
    impl Request for GetCount {
        type Response = CountResponse;
    }

    #[derive(Clone, Debug)]
    struct Reset;

    /// A counter actor used across multiple integration tests.
    #[derive(Debug, Default)]
    struct CounterActor {
        count: u32,
    }

    impl ActorExtension for CounterActor {
        fn configure(actor: &mut ManagedActor<Idle, Self>) {
            actor.mutate_on::<Increment>(|actor, envelope| {
                actor.model.count += envelope.message().amount;
                Reply::ready()
            });

            actor.act_on::<GetCount>(|actor, envelope| {
                let count = actor.model.count;
                let reply = envelope.reply_envelope();
                Reply::pending(async move {
                    reply.send(CountResponse { count }).await;
                })
            });

            actor.mutate_on::<Reset>(|actor, _envelope| {
                actor.model.count = 0;
                Reply::ready()
            });
        }
    }

    // ── Integration tests ──────────────────────────────────────────────

    #[tokio::test]
    async fn spawn_actor_extension_processes_messages_in_order() {
        let mut runtime = ActonApp::launch_async().await;

        // Spawn supervisor
        let supervisor = runtime.new_actor::<ExtensionsSupervisorState>();
        let supervisor_handle = supervisor.start().await;

        // Spawn counter actor under supervision
        let entry = ActorExtensionEntry::<CounterActor>(PhantomData);
        let (_tid, child) = entry.spawn(&supervisor_handle, &runtime, 0).await.unwrap();
        let handle = child.current().expect("child should be running");

        // Send fire-and-forget increment messages
        handle.send(Increment { amount: 5 }).await;
        handle.send(Increment { amount: 3 }).await;

        // The ask is the barrier: it cannot resolve until both increments
        // ahead of it in the mailbox have been applied.
        let count = handle.ask(GetCount).await.unwrap().count;
        assert_eq!(count, 8, "both increments should have been applied");

        handle.send(Increment { amount: 1 }).await;
        let count = handle.ask(GetCount).await.unwrap().count;
        assert_eq!(count, 9, "actor should still be processing after the query");

        runtime.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn actor_extension_spawner_produces_correct_type_id() {
        let mut runtime = ActonApp::launch_async().await;

        let supervisor = runtime.new_actor::<ExtensionsSupervisorState>();
        let supervisor_handle = supervisor.start().await;

        let entry = ActorExtensionEntry::<CounterActor>(PhantomData);
        let (type_id, _child) = entry.spawn(&supervisor_handle, &runtime, 0).await.unwrap();

        assert_eq!(
            type_id,
            TypeId::of::<CounterActor>(),
            "spawner must return TypeId matching the actor type"
        );

        runtime.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn spawner_applies_the_declared_restart_policy() {
        #[derive(Debug, Default)]
        struct TemporaryActor;
        impl ActorExtension for TemporaryActor {
            fn configure(_actor: &mut ManagedActor<Idle, Self>) {}
            fn restart_policy() -> RestartPolicy {
                RestartPolicy::Temporary
            }
        }

        let mut runtime = ActonApp::launch_async().await;
        let supervisor = runtime.new_actor::<ExtensionsSupervisorState>();
        let supervisor_handle = supervisor.start().await;

        let entry = ActorExtensionEntry::<TemporaryActor>(PhantomData);
        let (_tid, child) = entry.spawn(&supervisor_handle, &runtime, 0).await.unwrap();

        // Registration under a blueprint is what makes the policy reachable at
        // all; the legacy `supervise` path registered no spawner, so the
        // supervisor could never act on a policy. Seeing the child tracked
        // here is what distinguishes the two.
        assert_eq!(
            *child.supervisor(),
            supervisor_handle.id(),
            "child should be registered under the extensions supervisor"
        );
        assert!(
            child.current().is_some(),
            "a freshly supervised child should resolve to a live incarnation"
        );

        runtime.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn permanent_extension_is_restarted_after_termination() {
        // This is the test that distinguishes `supervise_with` from the legacy
        // `supervise`. Under `supervise` the child is registered without a
        // blueprint, so the supervisor has nothing to rebuild it from and this
        // wait would time out at a terminal state no matter what policy the
        // extension declared.
        let mut runtime = ActonApp::launch_async().await;
        let supervisor = runtime.new_actor::<ExtensionsSupervisorState>();
        let supervisor_handle = supervisor.start().await;

        // CounterActor takes the default policy, Permanent, which restarts on
        // any termination — a clean stop included.
        assert_eq!(CounterActor::restart_policy(), RestartPolicy::Permanent);

        let entry = ActorExtensionEntry::<CounterActor>(PhantomData);
        let (_tid, mut child) = entry.spawn(&supervisor_handle, &runtime, 0).await.unwrap();

        let first = child.current().expect("child should start running");
        first.send(Increment { amount: 7 }).await;
        assert_eq!(first.ask(GetCount).await.unwrap().count, 7);

        first.stop().await.unwrap();

        let second = child
            .wait_generation(RestartGeneration::FIRST.next())
            .await
            .expect("a Permanent child must come back after termination");

        assert_eq!(
            second.ask(GetCount).await.unwrap().count,
            0,
            "the restarted incarnation should start from Default state"
        );

        runtime.shutdown_all().await.unwrap();
    }

    #[test]
    fn extension_child_name_is_a_valid_ern_part() {
        #[derive(Debug, Default)]
        struct SomeActor;

        let name = extension_child_name::<SomeActor>(3);
        assert!(
            name.starts_with("SomeActor"),
            "name should keep the readable type stem, got {name}"
        );
        assert!(name.ends_with("-3"), "name should carry the index, got {name}");
        assert!(
            !name.contains(':') && !name.contains('/'),
            "ERN parts reject ':' and '/', got {name}"
        );
        assert!(name.len() <= 63, "ERN parts cap at 63 chars, got {name}");
    }

    #[tokio::test]
    async fn actor_extensions_container_stores_and_retrieves_handle() {
        let mut runtime = ActonApp::launch_async().await;

        let supervisor = runtime.new_actor::<ExtensionsSupervisorState>();
        let supervisor_handle = supervisor.start().await;

        let entry = ActorExtensionEntry::<CounterActor>(PhantomData);
        let (type_id, child) = entry.spawn(&supervisor_handle, &runtime, 0).await.unwrap();

        let mut map = HashMap::new();
        map.insert(type_id, child);

        let extensions = ActorExtensions::from(map);
        assert!(!extensions.is_empty());
        assert!(
            extensions.get::<CounterActor>().is_some(),
            "should retrieve handle by actor type"
        );

        // Wrong type returns None
        #[derive(Debug, Default)]
        struct OtherActor;
        impl ActorExtension for OtherActor {
            fn configure(_actor: &mut ManagedActor<Idle, Self>) {}
        }
        assert!(extensions.get::<OtherActor>().is_none());

        runtime.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn actor_extensions_clone_shares_handles() {
        let mut runtime = ActonApp::launch_async().await;

        let supervisor = runtime.new_actor::<ExtensionsSupervisorState>();
        let supervisor_handle = supervisor.start().await;

        let entry = ActorExtensionEntry::<CounterActor>(PhantomData);
        let (type_id, child) = entry.spawn(&supervisor_handle, &runtime, 0).await.unwrap();

        let mut map = HashMap::new();
        map.insert(type_id, child);

        let extensions = ActorExtensions::from(map);
        let cloned = extensions.clone();

        // Both the original and clone should resolve the same handle
        let h1 = extensions.get::<CounterActor>().unwrap();
        let h2 = cloned.get::<CounterActor>().unwrap();
        assert_eq!(
            h1.id(),
            h2.id(),
            "cloned extensions must share the same handles"
        );

        runtime.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn multiple_actor_extensions_coexist() {
        #[derive(Debug, Default)]
        struct AlphaActor {
            value: String,
        }
        impl ActorExtension for AlphaActor {
            fn configure(actor: &mut ManagedActor<Idle, Self>) {
                actor.mutate_on::<SetValue>(|actor, envelope| {
                    actor.model.value = envelope.message().0.clone();
                    Reply::ready()
                });
            }
        }

        #[derive(Clone, Debug)]
        struct SetValue(String);

        let mut runtime = ActonApp::launch_async().await;
        let supervisor = runtime.new_actor::<ExtensionsSupervisorState>();
        let supervisor_handle = supervisor.start().await;

        // Spawn both actors under the same supervisor
        let counter_entry = ActorExtensionEntry::<CounterActor>(PhantomData);
        let alpha_entry = ActorExtensionEntry::<AlphaActor>(PhantomData);

        let (counter_tid, counter_child) = counter_entry
            .spawn(&supervisor_handle, &runtime, 0)
            .await
            .unwrap();
        let (alpha_tid, alpha_child) = alpha_entry
            .spawn(&supervisor_handle, &runtime, 1)
            .await
            .unwrap();

        assert_ne!(
            counter_tid, alpha_tid,
            "different actor types must have different TypeIds"
        );

        let mut map = HashMap::new();
        map.insert(counter_tid, counter_child);
        map.insert(alpha_tid, alpha_child);
        let extensions = ActorExtensions::from(map);

        // Both actors are accessible
        let counter = extensions.get::<CounterActor>().unwrap();
        let alpha = extensions.get::<AlphaActor>().unwrap();

        // Send messages to both — verifies they're independent running actors
        counter.send(Increment { amount: 42 }).await;
        alpha.send(SetValue("hello".into())).await;

        assert_eq!(
            counter.ask(GetCount).await.unwrap().count,
            42,
            "the counter actor should have applied its own message"
        );

        runtime.shutdown_all().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn service_builder_with_actor_spawns_and_exposes_handle() {
        use crate::config::Config;
        use crate::prelude::ServiceBuilder;

        let config = Config::<()>::default();
        let service = ServiceBuilder::new()
            .with_config(config)
            .with_actor::<CounterActor>()
            .build();

        let state = service.state();

        // The actor handle should be accessible via state.actor()
        let handle = state
            .actor::<CounterActor>()
            .expect("CounterActor handle should be present after with_actor");

        // Send a message to verify the actor is alive and processing
        handle.send(Increment { amount: 10 }).await;
        assert_eq!(handle.ask(GetCount).await.unwrap().count, 10);

        handle.send(Increment { amount: 5 }).await;
        assert_eq!(
            handle.ask(GetCount).await.unwrap().count,
            15,
            "actor registered through with_actor should keep accumulating"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn service_builder_multiple_actors() {
        use crate::config::Config;
        use crate::prelude::ServiceBuilder;

        #[derive(Debug, Default)]
        struct PingActor;
        impl ActorExtension for PingActor {
            fn configure(actor: &mut ManagedActor<Idle, Self>) {
                actor.mutate_on::<Ping>(|_actor, _envelope| Reply::ready());
            }
            fn restart_policy() -> RestartPolicy {
                RestartPolicy::Transient
            }
        }

        #[derive(Clone, Debug)]
        struct Ping;

        let config = Config::<()>::default();
        let service = ServiceBuilder::new()
            .with_config(config)
            .with_actor::<CounterActor>()
            .with_actor::<PingActor>()
            .build();

        let state = service.state();

        assert!(
            state.actor::<CounterActor>().is_some(),
            "CounterActor should be registered"
        );
        assert!(
            state.actor::<PingActor>().is_some(),
            "PingActor should be registered"
        );

        // Unregistered actor returns None
        #[derive(Debug, Default)]
        struct Ghost;
        impl ActorExtension for Ghost {
            fn configure(_actor: &mut ManagedActor<Idle, Self>) {}
        }
        assert!(
            state.actor::<Ghost>().is_none(),
            "unregistered actor should return None"
        );

        // Both actors process messages independently
        let counter = state.actor::<CounterActor>().unwrap();
        counter.send(Increment { amount: 1 }).await;
        state.actor::<PingActor>().unwrap().send(Ping).await;
        assert_eq!(
            counter.ask(GetCount).await.unwrap().count,
            1,
            "the counter should see only its own message"
        );
    }

    // Multi-threaded: `build()` spawns the audit agent (enabled by default),
    // which requires a runtime where `block_in_place` can block.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn service_builder_without_actors_has_empty_extensions() {
        use crate::config::Config;
        use crate::prelude::ServiceBuilder;

        let config = Config::<()>::default();
        let service = ServiceBuilder::new().with_config(config).build();

        // No actors registered — actor() returns None for any type
        #[derive(Debug, Default)]
        struct Anything;
        impl ActorExtension for Anything {
            fn configure(_actor: &mut ManagedActor<Idle, Self>) {}
        }
        assert!(service.state().actor::<Anything>().is_none());
    }

    #[tokio::test]
    async fn actor_extension_with_sync_handler() {
        #[derive(Debug, Default)]
        struct SyncActor {
            value: i32,
        }
        impl ActorExtension for SyncActor {
            fn configure(actor: &mut ManagedActor<Idle, Self>) {
                // Use sync handler — zero async overhead
                actor.mutate_on_sync::<SetInt>(|actor, envelope| {
                    actor.model.value = envelope.message().0;
                });

                actor.act_on::<GetInt>(|actor, envelope| {
                    let value = actor.model.value;
                    let reply = envelope.reply_envelope();
                    Reply::pending(async move {
                        reply.send(IntValue(value)).await;
                    })
                });
            }
        }

        #[derive(Clone, Debug)]
        struct SetInt(i32);

        #[derive(Clone, Debug)]
        struct GetInt;

        #[derive(Clone, Debug)]
        struct IntValue(i32);

        impl Request for GetInt {
            type Response = IntValue;
        }

        let mut runtime = ActonApp::launch_async().await;
        let supervisor = runtime.new_actor::<ExtensionsSupervisorState>();
        let supervisor_handle = supervisor.start().await;

        let entry = ActorExtensionEntry::<SyncActor>(PhantomData);
        let (_tid, child) = entry.spawn(&supervisor_handle, &runtime, 0).await.unwrap();
        let handle = child.current().expect("child should be running");

        // Sync handlers are applied in mailbox order, so the later write wins.
        handle.send(SetInt(42)).await;
        handle.send(SetInt(100)).await;
        assert_eq!(
            handle.ask(GetInt).await.unwrap().0,
            100,
            "sync handler should have applied both writes in order"
        );

        runtime.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn actor_extension_broker_subscription() {
        use tokio::sync::Notify;

        /// Broadcast message type
        #[derive(Clone, Debug)]
        struct GlobalNotification {
            #[allow(dead_code)]
            payload: String,
        }

        #[derive(Clone, Debug)]
        struct GetNotified;

        #[derive(Clone, Debug)]
        struct NotifiedCount(u32);

        impl Request for GetNotified {
            type Response = NotifiedCount;
        }

        /// Signals that `after_start` finished handing the subscription to the
        /// broker. `after_start`'s future is drained alongside the message
        /// loop rather than before it, so "the actor started" does not imply
        /// "the actor subscribed" — without this the broadcast below could be
        /// published into a broker that has not yet heard of this subscriber.
        static SUBSCRIBED: Notify = Notify::const_new();

        #[derive(Debug, Default)]
        struct ListenerActor {
            seen: u32,
        }

        impl ActorExtension for ListenerActor {
            fn configure(actor: &mut ManagedActor<Idle, Self>) {
                actor.mutate_on::<GlobalNotification>(|actor, _envelope| {
                    actor.model.seen += 1;
                    Reply::ready()
                });

                actor.act_on::<GetNotified>(|actor, envelope| {
                    let seen = actor.model.seen;
                    let reply = envelope.reply_envelope();
                    Reply::pending(async move {
                        reply.send(NotifiedCount(seen)).await;
                    })
                });

                // Subscribing here rather than outside the blueprint is what
                // makes the subscription survive a restart: `configure` is
                // re-run for each incarnation, so `after_start` fires again.
                actor.after_start(|actor| {
                    let handle = actor.handle().clone();
                    Reply::pending(async move {
                        handle.subscribe::<GlobalNotification>().await;
                        SUBSCRIBED.notify_one();
                    })
                });
            }
        }

        let mut runtime = ActonApp::launch_async().await;
        let broker = runtime.broker();

        let supervisor = runtime.new_actor::<ExtensionsSupervisorState>();
        let supervisor_handle = supervisor.start().await;

        let entry = ActorExtensionEntry::<ListenerActor>(PhantomData);
        let (_tid, child) = entry.spawn(&supervisor_handle, &runtime, 0).await.unwrap();
        let handle = child.current().expect("listener should be running");

        // Barrier 1: the subscription request has reached the broker's inbox.
        SUBSCRIBED.notified().await;
        // Barrier 2: the broker has processed it. Its inbox is FIFO, so a
        // resolved flush proves everything queued earlier was handled.
        broker.ask(FlushBroadcasts).await.unwrap();

        broker
            .broadcast(GlobalNotification {
                payload: "test-1".into(),
            })
            .await;
        broker
            .broadcast(GlobalNotification {
                payload: "test-2".into(),
            })
            .await;

        // Barrier 3: both broadcasts have been delivered to every subscriber's
        // inbox. Barrier 4 is the ask itself, which cannot resolve until the
        // listener has drained those two messages ahead of it.
        broker.ask(FlushBroadcasts).await.unwrap();

        let count = handle.ask(GetNotified).await.unwrap().0;
        assert_eq!(
            count, 2,
            "listener actor should have received 2 broker broadcasts, got {count}"
        );

        runtime.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn actor_extension_lifecycle_hooks() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use tokio::sync::Notify;

        static STARTED: AtomicBool = AtomicBool::new(false);
        static STOPPED: AtomicBool = AtomicBool::new(false);
        /// `after_start` runs alongside the message loop, so nothing about the
        /// spawn call implies it has fired yet. This is the signal that it has.
        static START_FIRED: Notify = Notify::const_new();

        #[derive(Debug, Default)]
        struct LifecycleActor;

        impl ActorExtension for LifecycleActor {
            fn configure(actor: &mut ManagedActor<Idle, Self>) {
                actor.after_start(|_actor| {
                    STARTED.store(true, Ordering::SeqCst);
                    START_FIRED.notify_one();
                    Reply::ready()
                });

                actor.before_stop(|_actor| {
                    STOPPED.store(true, Ordering::SeqCst);
                    Reply::ready()
                });
            }
        }

        let mut runtime = ActonApp::launch_async().await;
        let supervisor = runtime.new_actor::<ExtensionsSupervisorState>();
        let supervisor_handle = supervisor.start().await;

        let entry = ActorExtensionEntry::<LifecycleActor>(PhantomData);
        let (_tid, child) = entry.spawn(&supervisor_handle, &runtime, 0).await.unwrap();
        let handle = child.current().expect("child should be running");

        START_FIRED.notified().await;
        assert!(
            STARTED.load(Ordering::SeqCst),
            "after_start should have fired"
        );

        // `stop` returns once the actor's task has finished, which is after
        // `before_stop` has run — so no barrier is needed beyond the await.
        handle.stop().await.unwrap();
        assert!(
            STOPPED.load(Ordering::SeqCst),
            "before_stop should have fired"
        );

        runtime.shutdown_all().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn service_builder_initializes_broker_for_extensions_only() {
        use crate::config::Config;
        use crate::prelude::ServiceBuilder;

        // When no pool features are enabled, the runtime is still initialized
        // for actor extensions, and the broker should be available.
        let config = Config::<()>::default();
        let service = ServiceBuilder::new()
            .with_config(config)
            .with_actor::<CounterActor>()
            .build();

        // Broker should be set on state when actor extensions are present
        assert!(
            service.state().broker().is_some(),
            "broker should be available when actor extensions are registered"
        );
    }
}
