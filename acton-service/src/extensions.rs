//! Actor-backed extensions for custom application state
//!
//! This module provides the [`ActorExtension`] trait and supporting types for adding
//! custom runtime state to your application. All extensions are backed by supervised
//! acton-reactive actors, providing:
//!
//! - **Supervision**: Automatic restart on failure via configurable [`RestartPolicy`]
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

use acton_reactive::prelude::{ActorHandle, ActorRuntime, Idle, ManagedActor, RestartPolicy};

/// Future type returned by [`ActorExtensionSpawner::spawn`].
type SpawnFuture<'a> =
    Pin<Box<dyn Future<Output = anyhow::Result<(TypeId, ActorHandle)>> + Send + 'a>>;

/// Trait for defining actor-backed extensions.
///
/// Implement this trait on an `#[acton_actor]` struct to register it as a
/// supervised extension via [`ServiceBuilder::with_actor`].
///
/// The [`configure`](ActorExtension::configure) method receives a mutable reference
/// to the actor builder, where you register message handlers and lifecycle hooks:
///
/// - `mutate_on` / `mutate_on_sync` — handlers that can modify actor state
/// - `act_on` / `act_on_sync` — read-only handlers (queries)
/// - `after_start` / `before_stop` — lifecycle hooks
/// - `handle.subscribe::<M>()` — broker subscriptions (in `after_start`)
///
/// ## Restart Policy
///
/// Override [`restart_policy`](ActorExtension::restart_policy) to control supervision behavior:
///
/// - [`Permanent`](RestartPolicy::Permanent) (default) — always restart on failure
/// - [`Transient`](RestartPolicy::Transient) — restart only on abnormal termination
/// - [`Temporary`](RestartPolicy::Temporary) — never restart
pub trait ActorExtension: Default + Debug + Send + 'static {
    /// Configure message handlers, lifecycle hooks, and broker subscriptions.
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
    /// Returns the `TypeId` of the concrete extension type and the spawned `ActorHandle`.
    fn spawn<'a>(
        &'a self,
        supervisor: &'a ActorHandle,
        runtime: &'a mut ActorRuntime,
    ) -> SpawnFuture<'a>;
}

/// Generic entry that captures a concrete `ActorExtension` type for type-erased spawning.
pub(crate) struct ActorExtensionEntry<A: ActorExtension>(pub(crate) PhantomData<A>);

impl<A: ActorExtension> ActorExtensionSpawner for ActorExtensionEntry<A> {
    fn spawn<'a>(
        &'a self,
        supervisor: &'a ActorHandle,
        runtime: &'a mut ActorRuntime,
    ) -> SpawnFuture<'a> {
        Box::pin(async move {
            let mut actor = runtime.new_actor::<A>();
            A::configure(&mut actor);
            let handle = supervisor.supervise(actor).await?;
            tracing::info!(
                actor_type = std::any::type_name::<A>(),
                "Actor extension spawned and supervised"
            );
            Ok((TypeId::of::<A>(), handle))
        })
    }
}

/// Immutable container mapping actor extension types to their handles.
///
/// Constructed during [`ServiceBuilder::build()`] and stored on [`AppState`].
/// Clone is cheap (Arc ref-count bump). When no actor extensions are registered,
/// the inner map is never allocated.
#[derive(Clone, Default)]
pub struct ActorExtensions {
    inner: Option<Arc<HashMap<TypeId, ActorHandle>>>,
}

impl ActorExtensions {
    /// Get the [`ActorHandle`] for a registered actor extension.
    ///
    /// Returns `None` if no actor of type `A` was registered.
    pub fn get<A: ActorExtension>(&self) -> Option<&ActorHandle> {
        self.inner.as_ref()?.get(&TypeId::of::<A>())
    }

    /// Returns `true` if no actor extensions are registered.
    pub fn is_empty(&self) -> bool {
        self.inner.as_ref().is_none_or(|m| m.is_empty())
    }
}

impl From<HashMap<TypeId, ActorHandle>> for ActorExtensions {
    fn from(map: HashMap<TypeId, ActorHandle>) -> Self {
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
    async fn spawn_actor_extension_and_send_message() {
        let mut runtime = ActonApp::launch_async().await;

        // Spawn supervisor
        let supervisor = runtime.new_actor::<ExtensionsSupervisorState>();
        let supervisor_handle = supervisor.start().await;

        // Spawn counter actor under supervision
        let mut actor = runtime.new_actor::<CounterActor>();
        CounterActor::configure(&mut actor);
        let handle = supervisor_handle.supervise(actor).await.unwrap();

        // Send fire-and-forget increment messages
        handle.send(Increment { amount: 5 }).await;
        handle.send(Increment { amount: 3 }).await;

        // Allow messages to process
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Verify actor is alive by sending another message (no panic = success)
        handle.send(Increment { amount: 1 }).await;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        runtime.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn actor_extension_spawner_produces_correct_type_id() {
        let mut runtime = ActonApp::launch_async().await;

        let supervisor = runtime.new_actor::<ExtensionsSupervisorState>();
        let supervisor_handle = supervisor.start().await;

        let entry = ActorExtensionEntry::<CounterActor>(PhantomData);
        let (type_id, _handle) = entry.spawn(&supervisor_handle, &mut runtime).await.unwrap();

        assert_eq!(
            type_id,
            TypeId::of::<CounterActor>(),
            "spawner must return TypeId matching the actor type"
        );

        runtime.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn actor_extensions_container_stores_and_retrieves_handle() {
        let mut runtime = ActonApp::launch_async().await;

        let supervisor = runtime.new_actor::<ExtensionsSupervisorState>();
        let supervisor_handle = supervisor.start().await;

        let entry = ActorExtensionEntry::<CounterActor>(PhantomData);
        let (type_id, handle) = entry.spawn(&supervisor_handle, &mut runtime).await.unwrap();

        let mut map = HashMap::new();
        map.insert(type_id, handle);

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
        let (type_id, handle) = entry.spawn(&supervisor_handle, &mut runtime).await.unwrap();

        let mut map = HashMap::new();
        map.insert(type_id, handle);

        let extensions = ActorExtensions::from(map);
        let cloned = extensions.clone();

        // Both the original and clone should resolve the same handle
        let h1 = extensions.get::<CounterActor>().unwrap();
        let h2 = cloned.get::<CounterActor>().unwrap();
        assert_eq!(h1.id(), h2.id(), "cloned extensions must share the same handles");

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

        let (counter_tid, counter_handle) =
            counter_entry.spawn(&supervisor_handle, &mut runtime).await.unwrap();
        let (alpha_tid, alpha_handle) =
            alpha_entry.spawn(&supervisor_handle, &mut runtime).await.unwrap();

        assert_ne!(counter_tid, alpha_tid, "different actor types must have different TypeIds");

        let mut map = HashMap::new();
        map.insert(counter_tid, counter_handle);
        map.insert(alpha_tid, alpha_handle);
        let extensions = ActorExtensions::from(map);

        // Both actors are accessible
        let counter = extensions.get::<CounterActor>().unwrap();
        let alpha = extensions.get::<AlphaActor>().unwrap();

        // Send messages to both — verifies they're independent running actors
        counter.send(Increment { amount: 42 }).await;
        alpha.send(SetValue("hello".into())).await;

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

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
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Send another — no panic means the actor survived
        handle.send(Increment { amount: 5 }).await;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
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
        state
            .actor::<CounterActor>()
            .unwrap()
            .send(Increment { amount: 1 })
            .await;
        state.actor::<PingActor>().unwrap().send(Ping).await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    #[tokio::test]
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
            }
        }

        #[derive(Clone, Debug)]
        struct SetInt(i32);

        let mut runtime = ActonApp::launch_async().await;
        let supervisor = runtime.new_actor::<ExtensionsSupervisorState>();
        let supervisor_handle = supervisor.start().await;

        let entry = ActorExtensionEntry::<SyncActor>(PhantomData);
        let (_tid, handle) = entry.spawn(&supervisor_handle, &mut runtime).await.unwrap();

        // Sync handler should process without issues
        handle.send(SetInt(42)).await;
        handle.send(SetInt(100)).await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        runtime.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn actor_extension_broker_subscription() {
        use std::sync::atomic::{AtomicU32, Ordering};

        /// Broadcast message type
        #[derive(Clone, Debug)]
        struct GlobalNotification {
            payload: String,
        }

        static RECEIVED_COUNT: AtomicU32 = AtomicU32::new(0);

        #[derive(Debug, Default)]
        struct ListenerActor;

        impl ActorExtension for ListenerActor {
            fn configure(actor: &mut ManagedActor<Idle, Self>) {
                // Handle the broadcast message
                actor.mutate_on::<GlobalNotification>(|_actor, _envelope| {
                    RECEIVED_COUNT.fetch_add(1, Ordering::SeqCst);
                    Reply::ready()
                });

                // Subscribe to broker broadcasts on startup
                actor.after_start(|actor| {
                    let handle = actor.handle().clone();
                    Reply::pending(async move {
                        handle.subscribe::<GlobalNotification>().await;
                    })
                });
            }
        }

        RECEIVED_COUNT.store(0, Ordering::SeqCst);

        let mut runtime = ActonApp::launch_async().await;
        let broker = runtime.broker();

        let supervisor = runtime.new_actor::<ExtensionsSupervisorState>();
        let supervisor_handle = supervisor.start().await;

        let entry = ActorExtensionEntry::<ListenerActor>(PhantomData);
        let (_tid, _handle) = entry.spawn(&supervisor_handle, &mut runtime).await.unwrap();

        // Allow after_start subscription to complete
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Broadcast via the broker
        broker.broadcast(GlobalNotification {
            payload: "test-1".into(),
        }).await;
        broker.broadcast(GlobalNotification {
            payload: "test-2".into(),
        }).await;

        // Allow messages to propagate
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let count = RECEIVED_COUNT.load(Ordering::SeqCst);
        assert_eq!(
            count, 2,
            "listener actor should have received 2 broker broadcasts, got {count}"
        );

        runtime.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn actor_extension_lifecycle_hooks() {
        use std::sync::atomic::{AtomicBool, Ordering};

        static STARTED: AtomicBool = AtomicBool::new(false);
        static STOPPED: AtomicBool = AtomicBool::new(false);

        #[derive(Debug, Default)]
        struct LifecycleActor;

        impl ActorExtension for LifecycleActor {
            fn configure(actor: &mut ManagedActor<Idle, Self>) {
                actor.after_start(|_actor| {
                    STARTED.store(true, Ordering::SeqCst);
                    Reply::ready()
                });

                actor.before_stop(|_actor| {
                    STOPPED.store(true, Ordering::SeqCst);
                    Reply::ready()
                });
            }
        }

        STARTED.store(false, Ordering::SeqCst);
        STOPPED.store(false, Ordering::SeqCst);

        let mut runtime = ActonApp::launch_async().await;
        let supervisor = runtime.new_actor::<ExtensionsSupervisorState>();
        let supervisor_handle = supervisor.start().await;

        let entry = ActorExtensionEntry::<LifecycleActor>(PhantomData);
        let (_tid, handle) = entry.spawn(&supervisor_handle, &mut runtime).await.unwrap();

        // Allow after_start to fire
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(STARTED.load(Ordering::SeqCst), "after_start should have fired");

        // Stop the actor
        handle.stop().await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(STOPPED.load(Ordering::SeqCst), "before_stop should have fired");

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
