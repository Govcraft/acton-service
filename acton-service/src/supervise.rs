//! One shutdown policy for every listener a serve path runs.
//!
//! Before this module, each serve path wired
//! `axum::serve(...).with_graceful_shutdown` on its own, and anything that ran
//! beside the main listener was fire and forget: the separate-port gRPC task's
//! result was discarded and read only after HTTP had already stopped, the
//! metrics exporter was never looked at, and a dead TLS handshake pump left
//! `accept()` parked forever. Any of those could stop while the process ran on,
//! healthy by every outside measure, and serving less than it was configured
//! to.
//!
//! A [`Supervisor`] owns one "stop requested" token. It is cancelled by an OS
//! signal, by the caller's shutdown future, or by the first listener or
//! watched task that ends on its own. Every listener drains on that token, and
//! the supervisor reports every early exit and every failure as one error.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use futures::stream::{FuturesUnordered, StreamExt};
use tokio::task::{JoinHandle, JoinSet};
use tokio_util::sync::CancellationToken;

/// A caller-supplied future that requests a graceful shutdown when it resolves.
pub(crate) type ShutdownFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

/// Resolves on SIGINT (Ctrl+C) or, on Unix, SIGTERM.
///
/// A handler that cannot be installed is logged and then never fires, so the
/// other shutdown sources keep working; it does not panic the serve task the
/// way an `expect` here would.
pub(crate) async fn os_shutdown_signal() {
    let ctrl_c = async {
        match tokio::signal::ctrl_c().await {
            Ok(()) => tracing::info!("Received SIGINT (Ctrl+C), starting graceful shutdown"),
            Err(e) => {
                tracing::error!("could not install the Ctrl+C handler: {e}");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sigterm) => {
                sigterm.recv().await;
                tracing::info!("Received SIGTERM, starting graceful shutdown");
            }
            Err(e) => {
                tracing::error!("could not install the SIGTERM handler: {e}");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}

/// Aborts the wrapped task when dropped, so a cancelled serve future does not
/// leave its shutdown trigger waiting on a signal forever.
struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Owns every listener task of one serve call and the token that stops them.
pub(crate) struct Supervisor {
    stop: CancellationToken,
    listeners: JoinSet<std::io::Result<()>>,
    names: HashMap<tokio::task::Id, String>,
    alarms: FuturesUnordered<Pin<Box<dyn Future<Output = String> + Send>>>,
    errors: Vec<String>,
    _trigger: AbortOnDrop,
}

impl Supervisor {
    /// Start the shutdown trigger: OS signals plus the caller's future, if any.
    ///
    /// A caller future that is already complete stops the supervisor before
    /// anything is spawned, which [`stop_requested`](Self::stop_requested)
    /// reports so the serve path can return without accepting a connection.
    pub(crate) fn new(shutdown: Option<ShutdownFuture>) -> Self {
        let stop = CancellationToken::new();
        let mut shutdown = shutdown;
        if let Some(future) = shutdown.as_mut() {
            if futures::FutureExt::now_or_never(future.as_mut()).is_some() {
                tracing::info!("shutdown was requested before serve started");
                stop.cancel();
                shutdown = None;
            }
        }
        let trigger = tokio::spawn(trigger(stop.clone(), shutdown));
        Self {
            stop,
            listeners: JoinSet::new(),
            names: HashMap::new(),
            alarms: FuturesUnordered::new(),
            errors: Vec::new(),
            _trigger: AbortOnDrop(trigger),
        }
    }

    /// Whether shutdown has been requested, by any source.
    pub(crate) fn stop_requested(&self) -> bool {
        self.stop.is_cancelled()
    }

    /// A future that resolves when shutdown is requested; hand it to
    /// `with_graceful_shutdown`.
    pub(crate) fn stopping(&self) -> impl Future<Output = ()> + Send + 'static {
        self.stop.clone().cancelled_owned()
    }

    /// Run one listener. `name` reads as "the {name} stopped", e.g.
    /// "gRPC listener on 127.0.0.1:50051".
    pub(crate) fn listener<F>(&mut self, name: String, serve: F)
    where
        F: Future<Output = std::io::Result<()>> + Send + 'static,
    {
        let handle = self.listeners.spawn(serve);
        self.names.insert(handle.id(), name);
    }

    /// Watch something that must not stop while the listeners run: `fired`
    /// resolves only when it has. Once shutdown is requested it is no longer
    /// watched, because whatever it watches is then expected to stop.
    #[cfg(any(feature = "tls", feature = "prometheus-metrics"))]
    pub(crate) fn watch<F>(&mut self, fired: F, message: String)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.alarms.push(Box::pin(async move {
            fired.await;
            message
        }));
    }

    /// Record a failure found outside the supervised tasks.
    #[cfg(feature = "prometheus-metrics")]
    pub(crate) fn record(&mut self, error: String) {
        self.errors.push(error);
    }

    /// Wait until every listener has stopped.
    ///
    /// The first listener or watched task to stop before shutdown was requested
    /// is recorded as a failure and requests shutdown for the rest. Failures
    /// while draining are recorded too.
    pub(crate) async fn drain(&mut self) {
        loop {
            tokio::select! {
                joined = self.listeners.join_next_with_id() => match joined {
                    None => break,
                    Some(Ok((id, result))) => {
                        let name = self.names.remove(&id).unwrap_or_else(|| "listener".into());
                        if self.stop.is_cancelled() {
                            if let Err(e) = result {
                                self.errors.push(format!("the {name} failed while draining: {e}"));
                            }
                        } else {
                            self.errors.push(match result {
                                Ok(()) => format!("the {name} stopped before shutdown was requested"),
                                Err(e) => format!(
                                    "the {name} stopped before shutdown was requested: {e}"
                                ),
                            });
                            self.stop.cancel();
                        }
                    }
                    Some(Err(e)) => {
                        let name = self.names.remove(&e.id()).unwrap_or_else(|| "listener".into());
                        let how = if e.is_panic() { "panicked" } else { "was cancelled" };
                        self.errors.push(format!("the {name} {how}: {e}"));
                        self.stop.cancel();
                    }
                },
                Some(message) = self.alarms.next(), if !self.stop.is_cancelled() => {
                    // Checked again: the branch was armed before this wait, and
                    // shutdown may have been requested while it waited. A watched
                    // task that stops after that is stopping on request.
                    if !self.stop.is_cancelled() {
                        self.errors.push(message);
                        self.stop.cancel();
                    }
                }
            }
        }
    }

    /// Every recorded failure as one error, or `Ok` if there were none.
    pub(crate) fn finish(self) -> crate::error::Result<()> {
        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(crate::error::Error::Internal(self.errors.join("; ")))
        }
    }
}

/// TLS termination for one listener: its credentials and handshake timeout.
#[cfg(feature = "tls")]
pub(crate) struct TlsServing {
    pub(crate) source: crate::tls::TlsConfigSource,
    pub(crate) handshake_timeout: std::time::Duration,
}

impl Supervisor {
    /// Serve `app` on `listener` under this supervisor, terminating TLS when
    /// `tls` is set.
    ///
    /// Connect-info is `TlsConnectInfo` (remote address plus any verified
    /// client certificate) behind TLS and the remote `SocketAddr` otherwise. A
    /// TLS listener's handshake pump is watched: if it stops, the listener
    /// accepts nothing further, and that is a failure like any other early exit.
    pub(crate) fn serve_router(
        &mut self,
        name: String,
        listener: tokio::net::TcpListener,
        app: axum::Router,
        #[cfg(feature = "tls")] tls: Option<TlsServing>,
    ) {
        let stopping = self.stopping();

        #[cfg(feature = "tls")]
        if let Some(tls) = tls {
            let listener = crate::tls::TlsListener::with_config_source(listener, tls.source)
                .with_handshake_timeout(tls.handshake_timeout);
            self.watch(
                listener.stopped(),
                format!(
                    "the TLS handshake pump of the {name} stopped, so it accepts no further \
                     connections"
                ),
            );
            self.listener(name, async move {
                axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<crate::tls::TlsConnectInfo>(),
                )
                .with_graceful_shutdown(stopping)
                .await
            });
            return;
        }

        self.listener(name, async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .with_graceful_shutdown(stopping)
            .await
        });
    }
}

/// Cancel `stop` on an OS signal or the caller's future, whichever is first.
async fn trigger(stop: CancellationToken, shutdown: Option<ShutdownFuture>) {
    let caller = async move {
        match shutdown {
            Some(future) => {
                future.await;
                tracing::info!("shutdown requested by the caller, starting graceful shutdown");
            }
            None => std::future::pending().await,
        }
    };
    tokio::select! {
        () = os_shutdown_signal() => {}
        () = caller => {}
        () = stop.cancelled() => return,
    }
    stop.cancel();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const SETTLE: Duration = Duration::from_secs(5);

    async fn drained(supervisor: &mut Supervisor) {
        tokio::time::timeout(SETTLE, supervisor.drain())
            .await
            .expect("the supervisor drains");
    }

    #[tokio::test]
    async fn the_caller_future_stops_every_listener_cleanly() {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let mut supervisor = Supervisor::new(Some(Box::pin(async move {
            let _ = rx.await;
        })));
        for name in ["HTTP listener", "gRPC listener"] {
            let stopping = supervisor.stopping();
            supervisor.listener(name.into(), async move {
                stopping.await;
                Ok(())
            });
        }
        tx.send(()).expect("the trigger is waiting");
        drained(&mut supervisor).await;
        supervisor
            .finish()
            .expect("a requested stop is not a failure");
    }

    #[tokio::test]
    async fn an_already_resolved_shutdown_is_seen_before_anything_starts() {
        let supervisor = Supervisor::new(Some(Box::pin(async {})));
        assert!(supervisor.stop_requested());
        supervisor.finish().expect("nothing failed");
    }

    #[tokio::test]
    async fn a_listener_that_stops_early_stops_the_rest_and_is_an_error() {
        let mut supervisor = Supervisor::new(None);
        let stopping = supervisor.stopping();
        supervisor.listener("HTTP listener on 127.0.0.1:1".into(), async move {
            stopping.await;
            Ok(())
        });
        supervisor.listener("gRPC listener on 127.0.0.1:2".into(), async {
            Err(std::io::Error::other("accept loop died"))
        });
        drained(&mut supervisor).await;
        let error = supervisor.finish().expect_err("an early exit is a failure");
        assert_eq!(
            error.to_string(),
            "Internal server error: the gRPC listener on 127.0.0.1:2 stopped before shutdown \
             was requested: accept loop died"
        );
    }

    #[tokio::test]
    async fn a_panicking_listener_is_named() {
        let mut supervisor = Supervisor::new(None);
        supervisor.listener("gRPC listener on 127.0.0.1:2".into(), async {
            panic!("injected by the test")
        });
        drained(&mut supervisor).await;
        let error = supervisor.finish().expect_err("a panic is a failure");
        assert!(
            error
                .to_string()
                .contains("the gRPC listener on 127.0.0.1:2 panicked"),
            "{error}"
        );
    }

    #[cfg(any(feature = "tls", feature = "prometheus-metrics"))]
    #[tokio::test]
    async fn a_watched_task_that_stops_stops_the_listeners() {
        let mut supervisor = Supervisor::new(None);
        let stopping = supervisor.stopping();
        supervisor.listener("HTTP listener".into(), async move {
            stopping.await;
            Ok(())
        });
        supervisor.watch(async {}, "the TLS handshake pump stopped".into());
        drained(&mut supervisor).await;
        let error = supervisor
            .finish()
            .expect_err("a watched stop is a failure");
        assert!(error
            .to_string()
            .ends_with("the TLS handshake pump stopped"));
    }

    #[cfg(any(feature = "tls", feature = "prometheus-metrics"))]
    #[tokio::test]
    async fn a_watch_that_fires_after_shutdown_is_not_a_failure() {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let mut supervisor = Supervisor::new(Some(Box::pin(async move {
            let _ = rx.await;
        })));
        let stopping = supervisor.stopping();
        let after = supervisor.stopping();
        supervisor.listener("HTTP listener".into(), async move {
            stopping.await;
            // Keep draining a moment, so the watch below becomes ready while
            // the listener is still running.
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok(())
        });
        supervisor.watch(after, "watched after shutdown".into());
        tx.send(()).expect("the trigger is waiting");
        drained(&mut supervisor).await;
        supervisor
            .finish()
            .expect("stopping during shutdown is expected");
    }

    #[tokio::test]
    async fn a_drain_error_is_recorded() {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let mut supervisor = Supervisor::new(Some(Box::pin(async move {
            let _ = rx.await;
        })));
        let stopping = supervisor.stopping();
        supervisor.listener("HTTP listener".into(), async move {
            stopping.await;
            Err(std::io::Error::other("flush failed"))
        });
        tx.send(()).expect("the trigger is waiting");
        drained(&mut supervisor).await;
        let error = supervisor
            .finish()
            .expect_err("a drain failure is a failure");
        assert!(error
            .to_string()
            .contains("the HTTP listener failed while draining: flush failed"));
    }
}
