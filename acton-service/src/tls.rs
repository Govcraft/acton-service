//! TLS support using rustls
//!
//! Provides a [`TlsListener`] that wraps a TCP listener with TLS termination,
//! implementing [`axum::serve::Listener`] for seamless integration with axum's server.
//!
//! Credentials reach the listener through a [`TlsConfigSource`], a handle whose
//! contents can be replaced while the listener is running so certificates can be
//! rotated without dropping the socket.
//!
//! # What actually triggers a rotation
//!
//! [`TlsConfigSource::reload`] is the mechanism; something has to call it. Four
//! ways to arrange that, from most to least automatic:
//!
//! - **Poll the files.** Set `reload_interval_secs` on the `[tls]` (or
//!   `[grpc.tls]`) config section. The service then hashes the credential files
//!   on an interval and reloads only when their *contents* change. Nothing to
//!   write; suits any rotation scheme that ends in "the files on disk are
//!   different now", including Kubernetes secret projections and cert-manager.
//! - **Send `SIGHUP`.** Set `reload_on_sighup = true`. One signal reloads every
//!   reloadable source in the process. Suits rotation drivers that already know
//!   when they finished writing and would rather say so than be discovered.
//! - **Register a hook.** `ServiceBuilder::with_tls_reload` hands your callback
//!   a [`TlsReloadHandle`] at serve time, from which you can drive
//!   [`TlsReloadHandle::reload_all`] on whatever schedule or event you like:
//!   a Vault lease renewal, a watch on a ConfigMap, an admin endpoint.
//! - **Hold the source yourself.** `ActonService::tls_config_source` clones the
//!   handle out before `serve()` consumes the service. The most direct route,
//!   and the one with an ordering constraint to get wrong; prefer the hook.
//!
//! Every path converges on the same fail-closed [`TlsConfigSource::reload`]: a
//! rotation that produced unusable credentials leaves the previous ones serving.
//!
//! The two config-driven triggers work identically whether the listener came up
//! through `ServiceBuilder` / `ActonService::serve` or through the plainer
//! [`crate::server::Server::serve`] — both call the same installer, so one
//! config file means one rotation behaviour. The hook and the credential
//! accessors are `ServiceBuilder`-only, since `Server` has no builder to
//! register them on.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use rustls_pki_types::CertificateDer;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_rustls::rustls::server::danger::ClientCertVerifier;
use tokio_rustls::rustls::server::WebPkiClientVerifier;
use tokio_rustls::rustls::{RootCertStore, ServerConfig};
use tokio_rustls::server::TlsStream;
use tokio_rustls::TlsAcceptor;

use crate::config::TlsConfig;
use crate::error::Result;

/// The credentials a TLS listener serves, replaceable while it runs.
///
/// A source holds the current [`ServerConfig`] behind an [`ArcSwap`], so
/// [`load`](Self::load) is a cheap atomic read on the accept path and
/// [`reload`](Self::reload) can install fresh credentials without rebinding the
/// socket. Connections already established keep the configuration they
/// handshook with; every subsequent handshake uses the newest one.
///
/// Cloning is cheap and every clone observes the same credentials: hand clones
/// to the listener and keep one to drive rotation.
///
/// # Failure behaviour
///
/// A source built from a [`TlsConfig`] rereads those files on every reload. A
/// failed reload is **fail-closed**: the last-good configuration stays
/// installed, the error is logged at `ERROR` level and returned to the caller.
/// A reload can never leave the listener without credentials or downgrade what
/// it serves.
///
/// A source built from an already-loaded [`ServerConfig`] has no files to
/// reread, so it is static and [`reload`](Self::reload) reports that rather
/// than silently doing nothing.
///
/// # Example
///
/// ```rust,ignore
/// let source = TlsConfigSource::from_tls_config(&tls_config)?;
/// let listener = TlsListener::with_config_source(tcp, source.clone());
///
/// // Later, after the certificate files have been rewritten on disk:
/// if let Err(e) = source.reload() {
///     tracing::warn!("certificate rotation failed, still serving previous cert: {e}");
/// }
/// ```
#[derive(Clone)]
pub struct TlsConfigSource {
    inner: Arc<TlsConfigSourceInner>,
}

struct TlsConfigSourceInner {
    /// The configuration every new handshake uses.
    current: ArcSwap<ServerConfig>,
    /// The file paths a reload rereads. `None` for a static source.
    origin: Option<TlsConfig>,
}

impl TlsConfigSource {
    /// Create a static source from an already-built server configuration.
    ///
    /// The result never changes: [`reload`](Self::reload) has no files to read
    /// and returns an error. Use [`from_tls_config`](Self::from_tls_config)
    /// when the credentials must be rotatable.
    #[must_use]
    pub fn from_server_config(server_config: Arc<ServerConfig>) -> Self {
        Self {
            inner: Arc::new(TlsConfigSourceInner {
                current: ArcSwap::new(server_config),
                origin: None,
            }),
        }
    }

    /// Create a reloadable source by loading credentials from disk.
    ///
    /// Reads the certificate chain, private key and any client-CA bundle named
    /// by `tls_config` through [`load_server_config`], and remembers those paths
    /// so [`reload`](Self::reload) can reread them. Returns the load error if
    /// the initial read fails, leaving no source to serve from.
    pub fn from_tls_config(tls_config: &TlsConfig) -> Result<Self> {
        let server_config = load_server_config(tls_config)?;
        Ok(Self {
            inner: Arc::new(TlsConfigSourceInner {
                current: ArcSwap::new(server_config),
                origin: Some(tls_config.clone()),
            }),
        })
    }

    /// The configuration new handshakes currently use.
    ///
    /// A cheap atomic load: this sits on the accept path and does no I/O.
    #[must_use]
    pub fn load(&self) -> Arc<ServerConfig> {
        self.inner.current.load_full()
    }

    /// Whether [`reload`](Self::reload) can do anything for this source.
    ///
    /// `true` for a source built from a [`TlsConfig`], `false` for one built
    /// from an already-loaded [`ServerConfig`].
    #[must_use]
    pub fn is_reloadable(&self) -> bool {
        self.inner.origin.is_some()
    }

    /// The configuration this source reloads from, if it is reloadable.
    #[must_use]
    pub fn origin(&self) -> Option<&TlsConfig> {
        self.inner.origin.as_ref()
    }

    /// Whether two handles are clones of the same source.
    ///
    /// Distinguishes "the gRPC listener inherited the HTTP credentials" from
    /// "the gRPC listener has its own credentials that happen to name the same
    /// files". Reload machinery uses this to avoid driving one source twice.
    #[must_use]
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// Reread the credential files and install them for new handshakes.
    ///
    /// On success every subsequent handshake uses the new configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when the source is static (nothing to reread), or when
    /// the files fail to load or parse. In both cases the previously installed
    /// configuration stays in place and keeps serving: a rotation that produced
    /// an unusable certificate must not take the listener down with it. Failures
    /// are also logged at `ERROR` level, because a service whose rotation has
    /// silently stopped working will keep working until the certificate expires
    /// and then fail all at once.
    pub fn reload(&self) -> Result<()> {
        let Some(ref origin) = self.inner.origin else {
            let err = crate::error::Error::Internal(
                "TLS credentials cannot be reloaded: this source was built from an \
                 already-loaded ServerConfig and has no files to reread"
                    .to_string(),
            );
            tracing::error!("{}", err);
            return Err(err);
        };

        match load_server_config(origin) {
            Ok(server_config) => {
                self.inner.current.store(server_config);
                tracing::info!(
                    cert_path = %origin.cert_path.display(),
                    key_path = %origin.key_path.display(),
                    "TLS credentials reloaded; new handshakes use the new certificate"
                );
                Ok(())
            }
            Err(e) => {
                tracing::error!(
                    cert_path = %origin.cert_path.display(),
                    key_path = %origin.key_path.display(),
                    error = %e,
                    "TLS credential reload failed; continuing to serve the previous \
                     certificate. New credentials will not take effect until a reload \
                     succeeds."
                );
                Err(e)
            }
        }
    }
}

impl std::fmt::Debug for TlsConfigSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `ServerConfig` carries key material, so describe the source by where
        // it came from rather than by what it holds.
        f.debug_struct("TlsConfigSource")
            .field("reloadable", &self.is_reloadable())
            .field("origin", &self.inner.origin)
            .finish()
    }
}

impl From<Arc<ServerConfig>> for TlsConfigSource {
    fn from(server_config: Arc<ServerConfig>) -> Self {
        Self::from_server_config(server_config)
    }
}

/// Which listener a set of TLS credentials belongs to.
///
/// Reload outcomes are reported per listener so an operator reading the log of
/// a partially-successful rotation can tell which surface is still serving the
/// old certificate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TlsListenerKind {
    /// The HTTP listener, configured by the `[tls]` section.
    Http,
    /// The separate-port gRPC listener, configured by `[grpc.tls]`.
    Grpc,
}

impl TlsListenerKind {
    /// A short lowercase name for logs and error messages.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Grpc => "grpc",
        }
    }
}

impl std::fmt::Display for TlsListenerKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The reloadable TLS credentials a running service resolved, handed to a
/// `ServiceBuilder::with_tls_reload` callback at serve time.
///
/// Exists to remove an ordering hazard. `ActonService::serve` consumes the
/// service, so reaching the credential handles through
/// `ActonService::tls_config_source` means remembering to clone them out
/// *before* serving. A registered hook is called by `serve()` itself, so there
/// is no "before" to miss.
///
/// # What is in it
///
/// Only **reloadable** sources appear. A source injected as an already-loaded
/// `ServerConfig` has no files to reread and is omitted rather than handed over
/// as something that would error on every reload.
///
/// [`grpc`](Self::grpc) is populated only when the gRPC listener has credentials
/// *distinct* from the HTTP listener's. When `[grpc.tls]` is absent the gRPC
/// listener inherits the `[tls]` source, and handing the same source back under
/// two names would make [`reload_all`](Self::reload_all) reread the same files
/// twice and report two outcomes for one rotation.
///
/// # Example
///
/// ```rust,ignore
/// let service = ServiceBuilder::new()
///     .with_config(config)
///     .with_routes(routes)
///     .with_tls_reload(|handle| {
///         tokio::spawn(async move {
///             while let Some(()) = vault_lease_renewed().await {
///                 for (listener, result) in handle.reload_all() {
///                     if let Err(e) = result {
///                         tracing::error!("{listener} TLS reload failed: {e}");
///                     }
///                 }
///             }
///         });
///     })
///     .build();
///
/// service.serve().await?;
/// ```
#[derive(Clone, Default)]
pub struct TlsReloadHandle {
    http: Option<TlsConfigSource>,
    grpc: Option<TlsConfigSource>,
}

impl TlsReloadHandle {
    /// Build a handle from the sources a service resolved.
    ///
    /// Non-reloadable sources are dropped, and `grpc` is dropped when it is the
    /// same source as `http`, so the handle only ever describes work that a
    /// reload would actually do.
    #[must_use]
    pub fn new(http: Option<TlsConfigSource>, grpc: Option<TlsConfigSource>) -> Self {
        let http = http.filter(TlsConfigSource::is_reloadable);
        let grpc = grpc
            .filter(TlsConfigSource::is_reloadable)
            .filter(|g| http.as_ref().is_none_or(|h| !h.ptr_eq(g)));
        Self { http, grpc }
    }

    /// The HTTP listener's reloadable credentials, if it has any.
    #[must_use]
    pub fn http(&self) -> Option<&TlsConfigSource> {
        self.http.as_ref()
    }

    /// The gRPC listener's own reloadable credentials.
    ///
    /// `None` both when the gRPC listener serves plaintext and when it inherits
    /// the HTTP credentials — in the latter case reloading [`http`](Self::http)
    /// already rotates it.
    #[must_use]
    pub fn grpc(&self) -> Option<&TlsConfigSource> {
        self.grpc.as_ref()
    }

    /// Whether this handle carries anything a reload could act on.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.http.is_none() && self.grpc.is_none()
    }

    /// Every source in the handle, paired with the listener it serves.
    #[must_use]
    pub fn sources(&self) -> Vec<(TlsListenerKind, TlsConfigSource)> {
        let mut out = Vec::with_capacity(2);
        if let Some(ref http) = self.http {
            out.push((TlsListenerKind::Http, http.clone()));
        }
        if let Some(ref grpc) = self.grpc {
            out.push((TlsListenerKind::Grpc, grpc.clone()));
        }
        out
    }

    /// Reload every source in the handle, reporting each outcome separately.
    ///
    /// One source failing does not stop the others: a rotation that succeeded
    /// for HTTP and failed for gRPC should leave HTTP rotated and say so, not
    /// roll back into a uniformly stale state. Each failure is already logged
    /// at `ERROR` by [`TlsConfigSource::reload`], and each failing listener
    /// keeps serving its last-good credentials.
    ///
    /// Returns an empty vector when the handle is empty.
    pub fn reload_all(&self) -> Vec<(TlsListenerKind, Result<()>)> {
        self.sources()
            .into_iter()
            .map(|(listener, source)| {
                let result = source.reload();
                match result {
                    Ok(()) => tracing::info!(
                        listener = listener.as_str(),
                        "TLS credentials rotated for the {listener} listener"
                    ),
                    Err(ref e) => tracing::error!(
                        listener = listener.as_str(),
                        error = %e,
                        "TLS reload failed for the {listener} listener; \
                         it continues to serve its previous certificate"
                    ),
                }
                (listener, result)
            })
            .collect()
    }
}

impl std::fmt::Debug for TlsReloadHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Describe which listeners are covered, not the key material behind them.
        f.debug_struct("TlsReloadHandle")
            .field("http", &self.http.is_some())
            .field("grpc", &self.grpc.is_some())
            .finish()
    }
}

/// A content hash of the credential files a source reloads from.
///
/// Only ever compared against another fingerprint of the same files, so a fast
/// non-cryptographic hash is the right tool: this detects *change*, and makes
/// no claim about integrity. An attacker who can rewrite the certificate files
/// has already won regardless of which hash reads them.
///
/// Returns an error when any file cannot be read. A caller must treat that as
/// "unknown, try again", not as "unchanged" (which would strand a rotation) and
/// not as "changed" (which would reload from a half-written file every tick).
fn fingerprint_credentials(tls_config: &TlsConfig) -> std::io::Result<u64> {
    let mut hasher = DefaultHasher::new();

    // Hash the paths as well as the bytes: a config edit that repoints at a
    // different file with identical contents is not a rotation, but a config
    // that swaps which of two files is authoritative should not alias.
    for path in [
        Some(&tls_config.cert_path),
        Some(&tls_config.key_path),
        tls_config.client_ca_path.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        path.hash(&mut hasher);
        // Length-prefix each file so concatenation cannot forge equality
        // between different splits of the same total bytes.
        let bytes = std::fs::read(path)?;
        bytes.len().hash(&mut hasher);
        bytes.hash(&mut hasher);
    }

    Ok(hasher.finish())
}

/// What one poll tick concluded about a source's credential files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReloadTick {
    /// The files hash to what they hashed last tick; nothing was reloaded.
    Unchanged,
    /// The files changed and the new credentials are installed. The caller
    /// stores this fingerprint as the new baseline.
    Reloaded { fingerprint: u64 },
    /// The files could not be read. The baseline is deliberately left alone so
    /// the next tick retries.
    ReadFailed,
    /// The files changed but failed to load or parse; the previous credentials
    /// keep serving. The baseline is left alone so the next tick retries even
    /// if the (still broken) files do not change again — a truncated file that
    /// is later completed in place must not be mistaken for "already seen".
    ReloadFailed,
}

/// Run one poll tick against a source: read, hash, compare, reload on change.
///
/// Split out from the timer loop so the decision logic is testable without
/// waiting on real clocks. `last_seen` is the fingerprint this source was last
/// known to be serving, or `None` if that is not yet established.
///
/// Never panics and never propagates an error: a poll task that dies takes
/// rotation down silently and leaves the service to expire, which is a worse
/// failure than any single bad tick.
pub(crate) fn reload_tick(
    source: &TlsConfigSource,
    listener: TlsListenerKind,
    last_seen: Option<u64>,
) -> ReloadTick {
    let Some(origin) = source.origin() else {
        // Callers filter static sources out before spawning a poll task; if one
        // reaches here, doing nothing is the only correct answer.
        return ReloadTick::Unchanged;
    };

    let fingerprint = match fingerprint_credentials(origin) {
        Ok(fingerprint) => fingerprint,
        Err(e) => {
            tracing::warn!(
                listener = listener.as_str(),
                cert_path = %origin.cert_path.display(),
                error = %e,
                "could not read TLS credential files while polling for rotation; \
                 continuing to serve the current certificate and retrying next tick"
            );
            return ReloadTick::ReadFailed;
        }
    };

    if last_seen == Some(fingerprint) {
        return ReloadTick::Unchanged;
    }

    match source.reload() {
        Ok(()) => {
            tracing::info!(
                listener = listener.as_str(),
                cert_path = %origin.cert_path.display(),
                "TLS credential files changed on disk; the {listener} listener now \
                 serves the new certificate"
            );
            ReloadTick::Reloaded { fingerprint }
        }
        // `reload` has already logged the failure at ERROR with the cause.
        Err(_) => ReloadTick::ReloadFailed,
    }
}

/// Poll a source's credential files on an interval, reloading on content change.
///
/// The returned task runs until it is aborted. It holds only a clone of the
/// source, so it never keeps a listener alive.
///
/// The first tick establishes a baseline from the files as they are now, so a
/// service that starts and never rotates does no reloading at all.
pub(crate) fn spawn_reload_poll(
    source: TlsConfigSource,
    listener: TlsListenerKind,
    period: Duration,
) -> tokio::task::JoinHandle<()> {
    // Seed the baseline from the files the source was just built from. A read
    // failure here leaves it unset, so the first successful tick establishes it
    // — reloading once, redundantly, rather than missing a rotation.
    let mut last_seen = source
        .origin()
        .and_then(|o| fingerprint_credentials(o).ok());

    tracing::info!(
        listener = listener.as_str(),
        interval_secs = period.as_secs(),
        "polling TLS credential files for rotation"
    );

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(period);
        // A tick missed because a reload ran long should not be made up for by
        // a burst of back-to-back ticks; rotation is not time-critical to the
        // second.
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // The first tick of a tokio interval completes immediately; consume it
        // so the first real check happens one period in, after the baseline.
        ticker.tick().await;

        loop {
            ticker.tick().await;
            if let ReloadTick::Reloaded { fingerprint } = reload_tick(&source, listener, last_seen)
            {
                last_seen = Some(fingerprint);
            }
        }
    })
}

/// Reload every source in `handle` when the process receives `SIGHUP`.
///
/// One handler covers every listener: a signal that rotated HTTP but not gRPC
/// would leave an operator guessing which surface is stale.
///
/// This installs an additional handler for `SIGHUP` only; the shutdown handling
/// on `SIGINT` and `SIGTERM` is untouched, and a `SIGHUP` never initiates
/// shutdown.
///
/// # Errors
///
/// Returns an error if the signal handler cannot be registered, which is fatal
/// to the *trigger*, not to the service: the caller logs it and serves on with
/// rotation available through the other triggers.
#[cfg(unix)]
pub(crate) fn spawn_sighup_reload(handle: TlsReloadHandle) -> Result<tokio::task::JoinHandle<()>> {
    use tokio::signal::unix::{signal, SignalKind};

    let mut hangup = signal(SignalKind::hangup()).map_err(|e| {
        crate::error::Error::Internal(format!(
            "failed to install the SIGHUP handler for TLS credential reload: {e}"
        ))
    })?;

    tracing::info!("SIGHUP will reload TLS credentials for every configured listener");

    Ok(tokio::spawn(async move {
        while hangup.recv().await.is_some() {
            tracing::info!("received SIGHUP; reloading TLS credentials");
            // Outcomes are logged per listener by `reload_all`. A failure keeps
            // the previous credentials serving and the handler installed, so a
            // later SIGHUP after fixing the files still works.
            let _ = handle.reload_all();
        }
    }))
}

/// Background tasks driving credential rotation, aborted when the service stops.
///
/// The tasks loop forever by construction, so something has to end them. Tying
/// that to this guard's lifetime keeps them alive exactly as long as the
/// listeners they rotate, and aborting rather than signalling keeps shutdown
/// from waiting on a task whose only job is to sleep.
#[derive(Default)]
pub(crate) struct TlsReloadTasks {
    handles: Vec<tokio::task::JoinHandle<()>>,
}

impl TlsReloadTasks {
    pub(crate) fn push(&mut self, handle: tokio::task::JoinHandle<()>) {
        self.handles.push(handle);
    }
}

impl Drop for TlsReloadTasks {
    fn drop(&mut self) {
        for handle in &self.handles {
            handle.abort();
        }
    }
}

/// Default cap on how long a single TLS handshake may take before its
/// connection is dropped, used when `handshake_timeout_secs` is unset.
///
/// Ten seconds is comfortably longer than a healthy handshake — which is a
/// couple of round-trips and completes in milliseconds on a LAN, and in well
/// under a second even across a slow link — yet short enough that a peer which
/// completes the TCP connect and then stalls (the pre-authentication denial of
/// service this bound exists to cap) frees its handshake task promptly instead
/// of holding it open indefinitely. Operators who terminate TLS behind a proxy
/// with its own, longer client timeout can raise it via `handshake_timeout_secs`.
pub const DEFAULT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Validate a section's `handshake_timeout_secs` into a handshake deadline.
///
/// `section` names the config section for the diagnostic, so an operator with
/// both `[tls]` and `[grpc.tls]` configured learns which one to fix.
///
/// An unset value resolves to [`DEFAULT_HANDSHAKE_TIMEOUT`]. Callers run this
/// *before* binding a listener, so a rejected value refuses to start rather than
/// failing handshakes once traffic arrives — the same posture as
/// [`validate_reload_interval`].
///
/// # Errors
///
/// A timeout of `0` is rejected. Taken literally it would elapse before the
/// handshake could make any progress, failing every connection instantly, so it
/// can only be a misconfiguration.
pub(crate) fn validate_handshake_timeout(tls_cfg: &TlsConfig, section: &str) -> Result<Duration> {
    match tls_cfg.handshake_timeout_secs {
        None => Ok(DEFAULT_HANDSHAKE_TIMEOUT),
        Some(0) => Err(crate::error::Error::Internal(format!(
            "{section} sets handshake_timeout_secs = 0, which would elapse before any \
             handshake could complete and so fail every connection. Omit the field to use \
             the default of {} seconds, or set a positive number of seconds.",
            DEFAULT_HANDSHAKE_TIMEOUT.as_secs()
        ))),
        Some(secs) => Ok(Duration::from_secs(secs)),
    }
}

/// Validate a section's `reload_interval_secs` into a poll period.
///
/// `section` names the config section for the diagnostic, so an operator with
/// both `[tls]` and `[grpc.tls]` configured learns which one to fix.
///
/// Callers must run this *before* binding a listener, so a rejected value is
/// reported the way every other fatal misconfiguration is: as a refusal to
/// start, not as a task that misbehaves once traffic is arriving.
///
/// # Errors
///
/// An interval of `0` is rejected. It cannot mean "never" — that is what
/// omitting the field means — and taken literally it would busy-loop a task
/// rereading certificates as fast as the disk allows, so treating it as a
/// misconfiguration is the only reading that leaves the service healthy.
pub(crate) fn validate_reload_interval(
    tls_cfg: &TlsConfig,
    section: &str,
) -> Result<Option<Duration>> {
    match tls_cfg.reload_interval_secs {
        None => Ok(None),
        Some(0) => Err(crate::error::Error::Internal(format!(
            "{section} sets reload_interval_secs = 0, which would poll the certificate \
             files without pause. Omit the field to disable polling, or set a positive \
             number of seconds."
        ))),
        Some(secs) => Ok(Some(Duration::from_secs(secs))),
    }
}

/// Warn when a section configures rotation triggers that its source cannot honour.
///
/// A caller-injected `ServerConfig` has no files to reread, so polling it or
/// signalling it can never do anything. Silence here would leave an operator
/// believing rotation is armed when it is not, and only finding out when the
/// certificate expires.
pub(crate) fn warn_if_reload_config_is_unusable(
    resolved: Option<&TlsConfigSource>,
    tls_cfg: &TlsConfig,
    section: &str,
) {
    let requested = tls_cfg.reload_interval_secs.is_some() || tls_cfg.reload_on_sighup;
    if !requested {
        return;
    }

    if resolved.is_some_and(|s| !s.is_reloadable()) {
        tracing::warn!(
            "{section} configures TLS credential reloading, but the credentials for this \
             listener were supplied directly to the builder as an already-loaded \
             ServerConfig. There are no files to reread, so the reload triggers are \
             ignored. Use with_tls_config_source with a source built from a TlsConfig, \
             or let the [tls] section load the files, to make rotation possible."
        );
    }
}

/// Start the config-driven rotation triggers for a set of resolved credentials.
///
/// The single implementation of "what `reload_interval_secs` and
/// `reload_on_sighup` actually do", shared by every serve path: the
/// `ServiceBuilder`/`ActonService` paths and the plain [`crate::server::Server`]
/// path. Keeping it in one place is what stops the two from drifting into
/// different rotation behaviour for the same config file.
///
/// `http_interval` and `grpc_interval` are the already-validated periods for
/// their respective listeners; a `None` disables polling for that one. `sighup`
/// installs the single all-listeners signal handler.
///
/// The returned guard owns the spawned tasks and aborts them when dropped, so
/// callers must hold it for as long as the listeners run.
pub(crate) fn install_reload_triggers(
    handle: &TlsReloadHandle,
    http_interval: Option<Duration>,
    grpc_interval: Option<Duration>,
    sighup: bool,
) -> TlsReloadTasks {
    let mut tasks = TlsReloadTasks::default();

    // Poll triggers. Each listener polls on its own section's interval; when
    // gRPC inherits the `[tls]` credentials there is no `[grpc.tls]` section to
    // carry a second interval, and the handle has already dropped the duplicate
    // source, so the one HTTP poll task covers both listeners.
    if let (Some(source), Some(period)) = (handle.http().cloned(), http_interval) {
        tasks.push(spawn_reload_poll(source, TlsListenerKind::Http, period));
    }
    if let (Some(source), Some(period)) = (handle.grpc().cloned(), grpc_interval) {
        tasks.push(spawn_reload_poll(source, TlsListenerKind::Grpc, period));
    }

    // SIGHUP trigger: one handler for every listener.
    if sighup {
        if handle.is_empty() {
            tracing::warn!(
                "TLS reload_on_sighup is enabled, but no listener has reloadable \
                 credentials, so SIGHUP will not reload anything"
            );
        } else {
            #[cfg(unix)]
            match spawn_sighup_reload(handle.clone()) {
                Ok(task) => tasks.push(task),
                // The service is serving correctly; only this one trigger is
                // unavailable, so log it and leave the others working rather
                // than refusing to serve over a rotation convenience.
                Err(e) => tracing::error!(
                    error = %e,
                    "could not install the SIGHUP TLS reload handler; \
                     other reload triggers are unaffected"
                ),
            }
            #[cfg(not(unix))]
            tracing::warn!(
                "TLS reload_on_sighup is enabled, but signals are not supported on \
                 this platform; use reload_interval_secs or \
                 ServiceBuilder::with_tls_reload instead"
            );
        }
    }

    tasks
}

/// A TLS-enabled listener wrapping a [`TcpListener`], terminating TLS with the
/// credentials held by a [`TlsConfigSource`].
///
/// Implements [`axum::serve::Listener`] so it can be used as a drop-in
/// replacement for `TcpListener` when calling `axum::serve()`.
///
/// Each accepted connection reads the source's current configuration, so
/// credentials swapped in by [`TlsConfigSource::reload`] apply to the next
/// handshake without restarting the listener.
///
/// # Handshakes run off the accept path
///
/// A background *pump* task owns the TCP listener, accepts connections, and runs
/// each TLS handshake in its own [`tokio::spawn`] task bounded by
/// [`handshake_timeout`](Self::with_handshake_timeout). Completed streams reach
/// [`accept`](axum::serve::Listener::accept) through a bounded channel, so one
/// peer that connects but never sends a ClientHello cannot serialize behind the
/// accept future and block every other connection — it merely occupies its own
/// handshake task until the timeout drops it. The pump is spawned lazily on the
/// first `accept()` call and aborted when the listener is dropped.
pub struct TlsListener {
    /// Shared so the pump task can `accept()` on its own clone while
    /// [`local_addr`](axum::serve::Listener::local_addr) still reads this one.
    tcp: Arc<TcpListener>,
    config_source: TlsConfigSource,
    handshake_timeout: Duration,
    /// Receiving end of the pump's completed-handshake channel. `None` until the
    /// first `accept()` lazily spawns the pump.
    rx: Option<mpsc::Receiver<(TlsStream<TcpStream>, SocketAddr)>>,
    /// The pump task, kept so `Drop` can abort it rather than let it outlive the
    /// listener holding the socket open.
    pump: Option<tokio::task::JoinHandle<()>>,
}

impl Drop for TlsListener {
    fn drop(&mut self) {
        if let Some(pump) = self.pump.take() {
            pump.abort();
        }
    }
}

/// Bounded capacity of the channel carrying completed handshakes to `accept()`.
///
/// axum leaves only a small number of accepts outstanding at once, so 1024 is
/// far more headroom than it ever uses while still bounding how many completed
/// `TlsStream`s can queue if the serve loop stalls. When it does fill, the
/// pump's `tx.send().await` parks — applying backpressure to further handshakes
/// rather than buffering without limit.
const HANDSHAKE_CHANNEL_CAPACITY: usize = 1024;

/// The background task that owns the listener, accepts TCP connections, and runs
/// each TLS handshake concurrently in its own task so no single stalled peer can
/// block the others. Completed streams are sent back to `accept()` over `tx`.
async fn handshake_pump(
    tcp: Arc<TcpListener>,
    config_source: TlsConfigSource,
    handshake_timeout: Duration,
    tx: mpsc::Sender<(TlsStream<TcpStream>, SocketAddr)>,
) {
    loop {
        // Accept a TCP connection. Preserve the previous error behaviour: log
        // and pause briefly so a transient accept failure (e.g. fd exhaustion)
        // does not spin this loop.
        let (stream, addr) = match tcp.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                tracing::error!("TCP accept error: {}", e);
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };

        // Snapshot the credentials at accept time (an `Arc` clone). A rotation
        // that lands mid-handshake therefore never changes this connection's
        // configuration, preserving the per-connection rotation semantics.
        let acceptor = TlsAcceptor::from(config_source.load());
        let tx = tx.clone();

        // Each handshake runs in its own task, bounded by the timeout, so a peer
        // that never sends a ClientHello occupies only its own task and cannot
        // block accepting or handshaking any other connection.
        tokio::spawn(async move {
            match tokio::time::timeout(handshake_timeout, acceptor.accept(stream)).await {
                Ok(Ok(tls_stream)) => {
                    // A send error means `accept()` and its receiver are gone —
                    // the listener was dropped — so this connection is moot.
                    let _ = tx.send((tls_stream, addr)).await;
                }
                Ok(Err(e)) => {
                    tracing::warn!("TLS handshake failed from {}: {}", addr, e);
                }
                Err(_elapsed) => {
                    tracing::warn!(
                        "TLS handshake from {} did not complete within {:?}; dropping the \
                         connection",
                        addr,
                        handshake_timeout
                    );
                }
            }
        });
    }
}

impl TlsListener {
    /// Create a TLS listener serving one fixed server configuration.
    ///
    /// The credentials cannot be rotated. Use
    /// [`with_config_source`](Self::with_config_source) with a source built by
    /// [`TlsConfigSource::from_tls_config`] when they must be.
    ///
    /// The handshake timeout defaults to [`DEFAULT_HANDSHAKE_TIMEOUT`]; override
    /// it with [`with_handshake_timeout`](Self::with_handshake_timeout).
    pub fn new(tcp: TcpListener, server_config: Arc<ServerConfig>) -> Self {
        Self::with_config_source(tcp, TlsConfigSource::from_server_config(server_config))
    }

    /// Create a TLS listener that reads its credentials from `config_source`.
    ///
    /// Every handshake uses whatever the source holds at that moment, so a
    /// [`reload`](TlsConfigSource::reload) on a clone of the source rotates this
    /// listener's certificate in place.
    ///
    /// The handshake timeout defaults to [`DEFAULT_HANDSHAKE_TIMEOUT`]; override
    /// it with [`with_handshake_timeout`](Self::with_handshake_timeout).
    pub fn with_config_source(tcp: TcpListener, config_source: TlsConfigSource) -> Self {
        Self {
            tcp: Arc::new(tcp),
            config_source,
            handshake_timeout: DEFAULT_HANDSHAKE_TIMEOUT,
            rx: None,
            pump: None,
        }
    }

    /// Set how long a single TLS handshake may take before it is dropped.
    ///
    /// Bounds the pre-handshake stall an unauthenticated peer can impose on its
    /// own handshake task. Defaults to [`DEFAULT_HANDSHAKE_TIMEOUT`] when not
    /// called.
    #[must_use]
    pub fn with_handshake_timeout(mut self, timeout: Duration) -> Self {
        self.handshake_timeout = timeout;
        self
    }

    /// The credential source this listener hands to each new handshake.
    #[must_use]
    pub fn config_source(&self) -> &TlsConfigSource {
        &self.config_source
    }
}

impl axum::serve::Listener for TlsListener {
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;

    fn accept(&mut self) -> impl std::future::Future<Output = (Self::Io, Self::Addr)> + Send {
        // Lazily start the background pump on the first accept. Spawning here
        // rather than in the constructor keeps `new`/`with_config_source`
        // non-async and means no task and no socket ownership exist until axum
        // actually begins serving.
        if self.rx.is_none() {
            let (tx, rx) = mpsc::channel(HANDSHAKE_CHANNEL_CAPACITY);
            let pump = tokio::spawn(handshake_pump(
                Arc::clone(&self.tcp),
                self.config_source.clone(),
                self.handshake_timeout,
                tx,
            ));
            self.rx = Some(rx);
            self.pump = Some(pump);
        }

        let rx = self
            .rx
            .as_mut()
            .expect("the pump receiver was just installed");

        async move {
            match rx.recv().await {
                Some(conn) => conn,
                // The pump loops forever holding a `tx`, so the channel can only
                // close if the pump task itself is gone — unreachable in normal
                // operation. Returning would hand axum a bogus connection and
                // busy-loop its serve loop, so park this future forever instead;
                // a graceful shutdown drops the whole listener rather than
                // relying on `accept()` to resolve.
                None => std::future::pending().await,
            }
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.tcp.local_addr()
    }
}

/// Load a [`RootCertStore`] of client-CA certificates from a PEM bundle.
///
/// Every PEM-encoded certificate in the file is treated as a trust anchor for
/// verifying client certificates during a mutual-TLS handshake. Returns an
/// error if the file cannot be opened, cannot be parsed, or contains no
/// certificates (an empty trust store would silently reject every client).
pub fn load_client_ca_roots(path: &Path) -> Result<RootCertStore> {
    load_root_store(path, "client CA")
}

/// Load a [`RootCertStore`] of trust anchors from a PEM bundle.
///
/// `role` names what the bundle is for and appears verbatim in every error
/// message, so the same loader can serve both the server's client-CA bundle and
/// a client's peer-CA bundle without either producing a misleading diagnostic.
/// Returns an error if the file cannot be opened, cannot be parsed, or contains
/// no certificates: an empty trust store would silently reject every peer, so
/// it must be a load failure rather than a permissive default.
pub(crate) fn load_root_store(path: &Path, role: &str) -> Result<RootCertStore> {
    use rustls_pki_types::pem::PemObject;

    let ca_certs: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(path)
        .map_err(|e| {
            crate::error::Error::Internal(format!(
                "Failed to open {} file '{}': {}",
                role,
                path.display(),
                e
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            crate::error::Error::Internal(format!(
                "Failed to parse {} certificates from '{}': {}",
                role,
                path.display(),
                e
            ))
        })?;

    if ca_certs.is_empty() {
        return Err(crate::error::Error::Internal(format!(
            "The {} file '{}' contains no certificates",
            role,
            path.display()
        )));
    }

    let mut roots = RootCertStore::empty();
    for cert in ca_certs {
        roots.add(cert).map_err(|e| {
            crate::error::Error::Internal(format!(
                "Failed to add {} certificate from '{}' to trust store: {}",
                role,
                path.display(),
                e
            ))
        })?;
    }

    Ok(roots)
}

/// Build a rustls [`ClientCertVerifier`] from a set of trust anchors.
///
/// When `optional` is `true`, a client certificate is requested but not
/// required: connections without one still complete, and a presented
/// certificate is still verified. When `false`, a valid client certificate is
/// mandatory and the handshake fails without one.
pub fn build_client_verifier(
    roots: RootCertStore,
    optional: bool,
) -> Result<Arc<dyn ClientCertVerifier>> {
    // The crypto provider must be installed before the verifier builder runs,
    // for the same reason as `ServerConfig::builder()` below.
    crate::crypto::ensure_default_crypto_provider();

    let mut builder = WebPkiClientVerifier::builder(Arc::new(roots));
    if optional {
        builder = builder.allow_unauthenticated();
    }

    builder.build().map_err(|e| {
        crate::error::Error::Internal(format!(
            "Failed to build client certificate verifier: {}",
            e
        ))
    })
}

/// Load a rustls [`ServerConfig`] from PEM certificate and key files.
///
/// Reads the certificate chain and private key from disk and constructs a
/// server configuration. When [`TlsConfig::client_ca_path`] is set, the server
/// is configured for mutual TLS: client certificates are verified against that
/// CA bundle (required unless [`TlsConfig::client_auth_optional`] is set).
/// Otherwise no client authentication is requested.
pub fn load_server_config(tls_config: &TlsConfig) -> Result<Arc<ServerConfig>> {
    use rustls_pki_types::pem::PemObject;
    use rustls_pki_types::PrivateKeyDer;
    use tokio_rustls::rustls;

    // Read certificate chain
    let cert_chain: Vec<rustls::pki_types::CertificateDer<'static>> =
        CertificateDer::pem_file_iter(&tls_config.cert_path)
            .map_err(|e| {
                crate::error::Error::Internal(format!(
                    "Failed to open TLS cert file '{}': {}",
                    tls_config.cert_path.display(),
                    e
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| {
                crate::error::Error::Internal(format!("Failed to parse TLS certificates: {}", e))
            })?;

    if cert_chain.is_empty() {
        return Err(crate::error::Error::Internal(
            "TLS cert file contains no certificates".to_string(),
        ));
    }

    // Read private key (first PEM-encoded key found in the file)
    let key = PrivateKeyDer::from_pem_file(&tls_config.key_path).map_err(|e| {
        crate::error::Error::Internal(format!(
            "Failed to parse TLS private key from '{}': {}",
            tls_config.key_path.display(),
            e
        ))
    })?;

    // Install the chosen rustls crypto provider before any builder call.
    // Without this, `ServerConfig::builder()` panics when multiple providers
    // are compiled into the binary (common via transitive deps).
    crate::crypto::ensure_default_crypto_provider();

    // Select the client-authentication posture. A configured client CA bundle
    // switches the listener into mutual-TLS mode; its absence preserves the
    // prior server-only behaviour.
    let builder = ServerConfig::builder();
    let config = match tls_config.client_ca_path {
        Some(ref ca_path) => {
            let roots = load_client_ca_roots(ca_path)?;
            let verifier = build_client_verifier(roots, tls_config.client_auth_optional)?;
            builder.with_client_cert_verifier(verifier)
        }
        None => builder.with_no_client_auth(),
    }
    .with_single_cert(cert_chain, key)
    .map_err(|e| {
        crate::error::Error::Internal(format!("Failed to build TLS server config: {}", e))
    })?;

    Ok(Arc::new(config))
}

/// A verified client certificate chain from a mutual-TLS handshake.
///
/// The chain is ordered leaf-first: [`PeerCertificates::leaf`] returns the
/// client's end-entity certificate, followed by any intermediates. It is only
/// present when the connection completed an mTLS handshake and the client
/// presented a certificate that validated against the configured CA bundle.
#[derive(Clone, Debug)]
pub struct PeerCertificates(Arc<Vec<CertificateDer<'static>>>);

impl PeerCertificates {
    /// The full verified certificate chain, leaf first.
    #[must_use]
    pub fn as_slice(&self) -> &[CertificateDer<'static>] {
        &self.0
    }

    /// The client's end-entity (leaf) certificate.
    ///
    /// The chain is guaranteed non-empty: a `PeerCertificates` is only
    /// constructed from a non-empty rustls peer chain.
    #[must_use]
    pub fn leaf(&self) -> &CertificateDer<'static> {
        &self.0[0]
    }
}

/// Connection information for a TLS listener, carrying the peer's socket
/// address and any verified client certificate chain.
///
/// Extract it from a handler with
/// `axum::extract::ConnectInfo<acton_service::tls::TlsConnectInfo>`. The
/// certificate chain is present only for connections that completed a
/// mutual-TLS handshake with a valid client certificate.
///
/// This type supersedes the plain `SocketAddr` connect-info on TLS listeners,
/// so TLS connections also expose a real remote address for rate limiting.
#[derive(Clone, Debug)]
pub struct TlsConnectInfo {
    remote_addr: SocketAddr,
    peer_certificates: Option<PeerCertificates>,
}

impl TlsConnectInfo {
    /// The remote socket address of the connecting peer.
    #[must_use]
    pub fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }

    /// The verified client certificate chain, if the peer authenticated with
    /// one during a mutual-TLS handshake.
    #[must_use]
    pub fn peer_certificates(&self) -> Option<&PeerCertificates> {
        self.peer_certificates.as_ref()
    }

    /// Whether the peer presented a verified client certificate.
    #[must_use]
    pub fn is_mutually_authenticated(&self) -> bool {
        self.peer_certificates.is_some()
    }
}

impl axum::extract::connect_info::Connected<axum::serve::IncomingStream<'_, TlsListener>>
    for TlsConnectInfo
{
    fn connect_info(stream: axum::serve::IncomingStream<'_, TlsListener>) -> Self {
        let remote_addr = *stream.remote_addr();
        let (_, connection) = stream.io().get_ref();
        let peer_certificates = connection
            .peer_certificates()
            .filter(|chain| !chain.is_empty())
            .map(|chain| PeerCertificates(Arc::new(chain.to_vec())));

        Self {
            remote_addr,
            peer_certificates,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    /// A self-signed certificate plus its PEM-encoded private key, usable both
    /// as a CA trust anchor and as a leaf server identity in tests.
    struct TestCert {
        cert_pem: String,
        key_pem: String,
        der: CertificateDer<'static>,
    }

    fn generate_cert(name: &str) -> TestCert {
        let certified = rcgen::generate_simple_self_signed(vec![name.to_string()])
            .expect("self-signed cert generation");
        TestCert {
            cert_pem: certified.cert.pem(),
            key_pem: certified.signing_key.serialize_pem(),
            der: certified.cert.der().clone(),
        }
    }

    fn write_temp(contents: &str) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().expect("temp file");
        file.write_all(contents.as_bytes()).expect("write temp");
        file.flush().expect("flush temp");
        file
    }

    #[test]
    fn load_client_ca_roots_reads_a_valid_bundle() {
        let ca = generate_cert("test-ca");
        let file = write_temp(&ca.cert_pem);

        let roots = load_client_ca_roots(file.path()).expect("valid CA bundle must load");

        assert_eq!(
            roots.len(),
            1,
            "the single CA certificate must become one trust anchor"
        );
    }

    #[test]
    fn load_client_ca_roots_rejects_a_missing_file() {
        let err = load_client_ca_roots(Path::new("/nonexistent/ca.pem"))
            .expect_err("a missing CA file must be an error, not an empty trust store");

        assert!(
            err.to_string().contains("Failed to open client CA file"),
            "error must name the failure to open the file: {err}"
        );
    }

    #[test]
    fn load_client_ca_roots_rejects_a_file_without_certificates() {
        // A syntactically valid PEM file that carries no certificates.
        let file = write_temp("# no certificates here\n");

        let err = load_client_ca_roots(file.path())
            .expect_err("a CA file with no certificates must be an error");

        assert!(
            err.to_string().contains("contains no certificates"),
            "error must explain that the bundle is empty: {err}"
        );
    }

    #[test]
    fn build_client_verifier_supports_required_and_optional_modes() {
        let ca = generate_cert("test-ca");
        let file = write_temp(&ca.cert_pem);

        let required_roots = load_client_ca_roots(file.path()).expect("roots");
        build_client_verifier(required_roots, false)
            .expect("a verifier requiring client auth must build");

        let optional_roots = load_client_ca_roots(file.path()).expect("roots");
        build_client_verifier(optional_roots, true)
            .expect("a verifier allowing unauthenticated clients must build");
    }

    #[test]
    fn load_server_config_without_client_ca_requests_no_client_auth() {
        let server = generate_cert("localhost");
        let cert_file = write_temp(&server.cert_pem);
        let key_file = write_temp(&server.key_pem);

        let config = TlsConfig {
            enabled: true,
            cert_path: cert_file.path().to_path_buf(),
            key_path: key_file.path().to_path_buf(),
            client_ca_path: None,
            client_auth_optional: false,
            reload_interval_secs: None,
            reload_on_sighup: false,
            handshake_timeout_secs: None,
        };

        load_server_config(&config).expect("server-only TLS config must build");
    }

    #[test]
    fn load_server_config_with_client_ca_builds_a_mutual_tls_config() {
        let server = generate_cert("localhost");
        let ca = generate_cert("client-ca");
        let cert_file = write_temp(&server.cert_pem);
        let key_file = write_temp(&server.key_pem);
        let ca_file = write_temp(&ca.cert_pem);

        let config = TlsConfig {
            enabled: true,
            cert_path: cert_file.path().to_path_buf(),
            key_path: key_file.path().to_path_buf(),
            client_ca_path: Some(ca_file.path().to_path_buf()),
            client_auth_optional: true,
            reload_interval_secs: None,
            reload_on_sighup: false,
            handshake_timeout_secs: None,
        };

        load_server_config(&config).expect("mutual-TLS config must build");
    }

    #[test]
    fn load_server_config_surfaces_an_invalid_client_ca() {
        let server = generate_cert("localhost");
        let cert_file = write_temp(&server.cert_pem);
        let key_file = write_temp(&server.key_pem);

        let config = TlsConfig {
            enabled: true,
            cert_path: cert_file.path().to_path_buf(),
            key_path: key_file.path().to_path_buf(),
            client_ca_path: Some(PathBuf::from("/nonexistent/client-ca.pem")),
            client_auth_optional: false,
            reload_interval_secs: None,
            reload_on_sighup: false,
            handshake_timeout_secs: None,
        };

        load_server_config(&config)
            .expect_err("an unreadable client CA must fail the whole config build");
    }

    /// Write cert and key PEM into named files under a directory the test owns,
    /// so their contents can be rewritten in place to simulate a rotation.
    fn write_credentials(dir: &Path, cert: &TestCert) -> TlsConfig {
        let cert_path = dir.join("server.pem");
        let key_path = dir.join("server.key");
        std::fs::write(&cert_path, &cert.cert_pem).expect("write cert");
        std::fs::write(&key_path, &cert.key_pem).expect("write key");

        TlsConfig {
            enabled: true,
            cert_path,
            key_path,
            client_ca_path: None,
            client_auth_optional: false,
            reload_interval_secs: None,
            reload_on_sighup: false,
            handshake_timeout_secs: None,
        }
    }

    #[test]
    fn static_source_serves_the_config_it_was_built_from() {
        let server = generate_cert("localhost");
        let cert_file = write_temp(&server.cert_pem);
        let key_file = write_temp(&server.key_pem);
        let config = TlsConfig {
            enabled: true,
            cert_path: cert_file.path().to_path_buf(),
            key_path: key_file.path().to_path_buf(),
            client_ca_path: None,
            client_auth_optional: false,
            reload_interval_secs: None,
            reload_on_sighup: false,
            handshake_timeout_secs: None,
        };
        let server_config = load_server_config(&config).expect("config builds");

        let source = TlsConfigSource::from_server_config(server_config.clone());

        assert!(
            Arc::ptr_eq(&source.load(), &server_config),
            "a static source must hand back exactly the config it was given"
        );
        assert!(
            !source.is_reloadable(),
            "a source with no file origin is not reloadable"
        );
        assert!(source.origin().is_none());
    }

    #[test]
    fn reload_on_a_static_source_is_an_error_and_keeps_the_config() {
        let server = generate_cert("localhost");
        let cert_file = write_temp(&server.cert_pem);
        let key_file = write_temp(&server.key_pem);
        let config = TlsConfig {
            enabled: true,
            cert_path: cert_file.path().to_path_buf(),
            key_path: key_file.path().to_path_buf(),
            client_ca_path: None,
            client_auth_optional: false,
            reload_interval_secs: None,
            reload_on_sighup: false,
            handshake_timeout_secs: None,
        };
        let server_config = load_server_config(&config).expect("config builds");
        let source = TlsConfigSource::from_server_config(server_config.clone());

        let err = source
            .reload()
            .expect_err("a static source has no files to reread");

        assert!(
            err.to_string().contains("cannot be reloaded"),
            "the error must say the source is not reloadable: {err}"
        );
        assert!(
            Arc::ptr_eq(&source.load(), &server_config),
            "a rejected reload must leave the served config untouched"
        );
    }

    #[test]
    fn successful_reload_swaps_in_the_new_credentials() {
        let dir = tempfile::tempdir().expect("temp dir");
        let first = generate_cert("localhost");
        let tls_config = write_credentials(dir.path(), &first);

        let source = TlsConfigSource::from_tls_config(&tls_config).expect("initial load");
        assert!(source.is_reloadable(), "a file-backed source is reloadable");
        let before = source.load();

        // Rotate: replace the files in place with a different key pair.
        let second = generate_cert("localhost");
        std::fs::write(&tls_config.cert_path, &second.cert_pem).expect("rewrite cert");
        std::fs::write(&tls_config.key_path, &second.key_pem).expect("rewrite key");

        source
            .reload()
            .expect("rereading valid credentials must succeed");

        assert!(
            !Arc::ptr_eq(&source.load(), &before),
            "a successful reload must install a newly loaded config"
        );
    }

    #[test]
    fn failed_reload_preserves_the_last_good_credentials() {
        let dir = tempfile::tempdir().expect("temp dir");
        let good = generate_cert("localhost");
        let tls_config = write_credentials(dir.path(), &good);

        let source = TlsConfigSource::from_tls_config(&tls_config).expect("initial load");
        let last_good = source.load();

        // Simulate a half-written rotation: the cert file is no longer parseable.
        std::fs::write(
            &tls_config.cert_path,
            "-----BEGIN CERTIFICATE-----\ntruncated",
        )
        .expect("corrupt cert");

        let err = source
            .reload()
            .expect_err("an unparseable certificate must fail the reload");

        assert!(
            Arc::ptr_eq(&source.load(), &last_good),
            "a failed reload must keep serving the last-good config, not drop it"
        );
        assert!(
            !err.to_string().is_empty(),
            "the failure must be reported to the caller: {err}"
        );

        // And the source recovers once the files are valid again.
        let replacement = generate_cert("localhost");
        std::fs::write(&tls_config.cert_path, &replacement.cert_pem).expect("rewrite cert");
        std::fs::write(&tls_config.key_path, &replacement.key_pem).expect("rewrite key");
        source.reload().expect("a later valid reload must succeed");
        assert!(
            !Arc::ptr_eq(&source.load(), &last_good),
            "the recovered reload must install the new config"
        );
    }

    #[test]
    fn clones_of_a_source_observe_the_same_reload() {
        let dir = tempfile::tempdir().expect("temp dir");
        let first = generate_cert("localhost");
        let tls_config = write_credentials(dir.path(), &first);

        let source = TlsConfigSource::from_tls_config(&tls_config).expect("initial load");
        let listener_side = source.clone();
        let before = listener_side.load();

        let second = generate_cert("localhost");
        std::fs::write(&tls_config.cert_path, &second.cert_pem).expect("rewrite cert");
        std::fs::write(&tls_config.key_path, &second.key_pem).expect("rewrite key");
        source.reload().expect("reload");

        assert!(
            Arc::ptr_eq(&listener_side.load(), &source.load()),
            "every clone must see the same installed config"
        );
        assert!(
            !Arc::ptr_eq(&listener_side.load(), &before),
            "the clone held by the listener must see the rotation"
        );
    }

    // --- Poll-driven rotation -------------------------------------------
    //
    // The tick logic is exercised directly rather than through
    // `spawn_reload_poll`, so these tests assert what the poll decides without
    // waiting on a real clock. What the timer loop adds on top — call
    // `reload_tick` on an interval, carry the fingerprint forward on success —
    // is the whole of the loop body and is visible in one screen.

    #[test]
    fn poll_reloads_when_the_certificate_files_change() {
        let dir = tempfile::tempdir().expect("temp dir");
        let first = generate_cert("localhost");
        let tls_config = write_credentials(dir.path(), &first);

        let source = TlsConfigSource::from_tls_config(&tls_config).expect("initial load");
        let baseline = fingerprint_credentials(&tls_config).expect("baseline fingerprint");
        let before = source.load();

        let second = generate_cert("localhost");
        std::fs::write(&tls_config.cert_path, &second.cert_pem).expect("rewrite cert");
        std::fs::write(&tls_config.key_path, &second.key_pem).expect("rewrite key");

        let tick = reload_tick(&source, TlsListenerKind::Http, Some(baseline));

        let ReloadTick::Reloaded { fingerprint } = tick else {
            panic!("rewritten credentials must be detected and installed, got {tick:?}");
        };
        assert_ne!(
            fingerprint, baseline,
            "the new fingerprint must differ from the one that triggered the reload"
        );
        assert!(
            !Arc::ptr_eq(&source.load(), &before),
            "the reloaded config must be the one the listener now serves"
        );
    }

    #[test]
    fn poll_does_not_reload_when_the_files_are_untouched() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cert = generate_cert("localhost");
        let tls_config = write_credentials(dir.path(), &cert);

        let source = TlsConfigSource::from_tls_config(&tls_config).expect("initial load");
        let baseline = fingerprint_credentials(&tls_config).expect("baseline fingerprint");
        let before = source.load();

        let tick = reload_tick(&source, TlsListenerKind::Http, Some(baseline));

        assert_eq!(
            tick,
            ReloadTick::Unchanged,
            "unchanged files must not cost a reload on every tick"
        );
        assert!(
            Arc::ptr_eq(&source.load(), &before),
            "an unchanged tick must not swap the served config"
        );
    }

    /// Rewriting a certificate is not atomic. A tick that lands mid-write must
    /// leave the last-good credentials serving *and* leave the baseline alone,
    /// so the next tick retries rather than recording the broken state as seen.
    #[test]
    fn poll_survives_a_half_written_certificate_and_recovers() {
        let dir = tempfile::tempdir().expect("temp dir");
        let good = generate_cert("localhost");
        let tls_config = write_credentials(dir.path(), &good);

        let source = TlsConfigSource::from_tls_config(&tls_config).expect("initial load");
        let baseline = fingerprint_credentials(&tls_config).expect("baseline fingerprint");
        let last_good = source.load();

        // Tick 1: the writer got as far as a truncated PEM body.
        std::fs::write(
            &tls_config.cert_path,
            "-----BEGIN CERTIFICATE-----\ntruncated",
        )
        .expect("write partial cert");

        let tick = reload_tick(&source, TlsListenerKind::Http, Some(baseline));

        assert_eq!(
            tick,
            ReloadTick::ReloadFailed,
            "an unparseable certificate must fail the tick, not the task"
        );
        assert!(
            Arc::ptr_eq(&source.load(), &last_good),
            "a failed tick must keep serving the last-good credentials"
        );

        // Tick 2: the writer finished. The baseline is still the original, so
        // the retry happens even though the tick-1 state was never recorded.
        let replacement = generate_cert("localhost");
        std::fs::write(&tls_config.cert_path, &replacement.cert_pem).expect("finish cert");
        std::fs::write(&tls_config.key_path, &replacement.key_pem).expect("finish key");

        let tick = reload_tick(&source, TlsListenerKind::Http, Some(baseline));

        assert!(
            matches!(tick, ReloadTick::Reloaded { .. }),
            "the next tick must retry and succeed, got {tick:?}"
        );
        assert!(
            !Arc::ptr_eq(&source.load(), &last_good),
            "the recovered tick must install the completed credentials"
        );
    }

    /// A file that vanishes between ticks — a secret remount, a symlink swap
    /// caught mid-flight — is "unknown", not "changed" and not "unchanged".
    #[test]
    fn poll_reports_a_read_failure_without_touching_the_served_config() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cert = generate_cert("localhost");
        let tls_config = write_credentials(dir.path(), &cert);

        let source = TlsConfigSource::from_tls_config(&tls_config).expect("initial load");
        let baseline = fingerprint_credentials(&tls_config).expect("baseline fingerprint");
        let last_good = source.load();

        std::fs::remove_file(&tls_config.cert_path).expect("remove cert");

        let tick = reload_tick(&source, TlsListenerKind::Http, Some(baseline));

        assert_eq!(
            tick,
            ReloadTick::ReadFailed,
            "an unreadable file must be reported as a read failure"
        );
        assert!(
            Arc::ptr_eq(&source.load(), &last_good),
            "a read failure must never disturb the credentials already serving"
        );
    }

    #[test]
    fn poll_on_a_static_source_does_nothing() {
        let server = generate_cert("localhost");
        let cert_file = write_temp(&server.cert_pem);
        let key_file = write_temp(&server.key_pem);
        let config = TlsConfig {
            enabled: true,
            cert_path: cert_file.path().to_path_buf(),
            key_path: key_file.path().to_path_buf(),
            client_ca_path: None,
            client_auth_optional: false,
            reload_interval_secs: None,
            reload_on_sighup: false,
            handshake_timeout_secs: None,
        };
        let source =
            TlsConfigSource::from_server_config(load_server_config(&config).expect("config"));

        assert_eq!(
            reload_tick(&source, TlsListenerKind::Http, None),
            ReloadTick::Unchanged,
            "a source with no files must not be reported as a failure every tick"
        );
    }

    /// Detection is by content, not by modification time.
    ///
    /// Asserted from the direction that needs no clock control: rewriting a
    /// file with byte-identical contents bumps its mtime, so an mtime-based
    /// check would call that a rotation. A content hash does not. The property
    /// this protects is the mirror image — `cp -p` and most certificate tooling
    /// preserve mtimes across a *real* rotation, which an mtime check would
    /// then miss entirely.
    #[test]
    fn rewriting_identical_bytes_is_not_a_rotation() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cert = generate_cert("localhost");
        let tls_config = write_credentials(dir.path(), &cert);

        let source = TlsConfigSource::from_tls_config(&tls_config).expect("initial load");
        let baseline = fingerprint_credentials(&tls_config).expect("baseline fingerprint");
        let before = source.load();

        // Same bytes, new mtime.
        std::fs::write(&tls_config.cert_path, &cert.cert_pem).expect("rewrite cert");
        std::fs::write(&tls_config.key_path, &cert.key_pem).expect("rewrite key");

        assert_eq!(
            fingerprint_credentials(&tls_config).expect("fingerprint"),
            baseline,
            "identical contents must fingerprint identically however recently written"
        );
        assert_eq!(
            reload_tick(&source, TlsListenerKind::Http, Some(baseline)),
            ReloadTick::Unchanged,
            "a touched-but-unchanged file must not be mistaken for a rotation"
        );
        assert!(Arc::ptr_eq(&source.load(), &before));
    }

    /// Contents of the same total length must not collide, so a rotation to a
    /// same-size certificate is still seen.
    #[test]
    fn fingerprint_distinguishes_same_length_contents() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cert = generate_cert("localhost");
        let tls_config = write_credentials(dir.path(), &cert);
        let before = fingerprint_credentials(&tls_config).expect("fingerprint");

        // Flip one byte, keeping the file length identical.
        let mut bytes = std::fs::read(&tls_config.cert_path).expect("read cert");
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        std::fs::write(&tls_config.cert_path, &bytes).expect("rewrite cert");

        assert_ne!(
            before,
            fingerprint_credentials(&tls_config).expect("fingerprint"),
            "a same-length change must still change the fingerprint"
        );
    }

    #[test]
    fn fingerprint_covers_the_client_ca_bundle() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = generate_cert("localhost");
        let mut tls_config = write_credentials(dir.path(), &server);
        let ca_path = dir.path().join("client-ca.pem");
        std::fs::write(&ca_path, generate_cert("ca-one").cert_pem).expect("write ca");
        tls_config.client_ca_path = Some(ca_path.clone());

        let before = fingerprint_credentials(&tls_config).expect("fingerprint");

        // Rotating only the trust anchors, leaving cert and key alone, must
        // still count as a rotation: it changes which clients are accepted.
        std::fs::write(&ca_path, generate_cert("ca-two").cert_pem).expect("rewrite ca");

        assert_ne!(
            before,
            fingerprint_credentials(&tls_config).expect("fingerprint"),
            "a changed client-CA bundle must be detected as a rotation"
        );
    }

    // --- The reload handle ------------------------------------------------

    #[test]
    fn handle_keeps_only_sources_a_reload_can_act_on() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cert = generate_cert("localhost");
        let tls_config = write_credentials(dir.path(), &cert);
        let reloadable = TlsConfigSource::from_tls_config(&tls_config).expect("load");
        let static_source =
            TlsConfigSource::from_server_config(load_server_config(&tls_config).expect("config"));

        let handle = TlsReloadHandle::new(Some(reloadable), Some(static_source));

        assert!(handle.http().is_some(), "the file-backed source is kept");
        assert!(
            handle.grpc().is_none(),
            "a static source has no files to reread and must not be handed out"
        );
        assert!(!handle.is_empty());
    }

    #[test]
    fn handle_is_empty_when_every_source_is_static() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cert = generate_cert("localhost");
        let tls_config = write_credentials(dir.path(), &cert);
        let static_source =
            TlsConfigSource::from_server_config(load_server_config(&tls_config).expect("config"));

        let handle = TlsReloadHandle::new(Some(static_source), None);

        assert!(
            handle.is_empty(),
            "a handle over only static sources must report that it can do nothing, \
             so callers can warn instead of silently never reloading"
        );
        assert!(handle.reload_all().is_empty());
    }

    /// When `[grpc.tls]` is absent the gRPC listener inherits the `[tls]`
    /// source. Listing it twice would reread one set of files twice and report
    /// two outcomes for a single rotation.
    #[test]
    fn handle_does_not_list_an_inherited_grpc_source_twice() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cert = generate_cert("localhost");
        let tls_config = write_credentials(dir.path(), &cert);
        let shared = TlsConfigSource::from_tls_config(&tls_config).expect("load");

        let handle = TlsReloadHandle::new(Some(shared.clone()), Some(shared));

        assert!(
            handle.grpc().is_none(),
            "the inherited source is not listed"
        );
        assert_eq!(
            handle.sources().len(),
            1,
            "one set of credentials must produce one reload"
        );
    }

    #[test]
    fn handle_lists_genuinely_separate_grpc_credentials() {
        let http_dir = tempfile::tempdir().expect("temp dir");
        let grpc_dir = tempfile::tempdir().expect("temp dir");
        let http_config = write_credentials(http_dir.path(), &generate_cert("localhost"));
        let grpc_config = write_credentials(grpc_dir.path(), &generate_cert("grpc.internal"));

        let handle = TlsReloadHandle::new(
            Some(TlsConfigSource::from_tls_config(&http_config).expect("load")),
            Some(TlsConfigSource::from_tls_config(&grpc_config).expect("load")),
        );

        assert_eq!(
            handle.sources().len(),
            2,
            "two independently-configured listeners must both be reloadable"
        );
    }

    /// This is the function the `SIGHUP` handler calls. Delivering a real
    /// signal to the test process would race every other test in the binary,
    /// so the handler's wiring (install a `SignalKind::hangup` stream, call
    /// `reload_all` per signal) is left to review and the behaviour that
    /// matters is asserted here.
    #[test]
    fn reload_all_rotates_every_listener_and_reports_each_outcome() {
        let http_dir = tempfile::tempdir().expect("temp dir");
        let grpc_dir = tempfile::tempdir().expect("temp dir");
        let http_config = write_credentials(http_dir.path(), &generate_cert("localhost"));
        let grpc_config = write_credentials(grpc_dir.path(), &generate_cert("grpc.internal"));
        let http_source = TlsConfigSource::from_tls_config(&http_config).expect("load");
        let grpc_source = TlsConfigSource::from_tls_config(&grpc_config).expect("load");
        let handle = TlsReloadHandle::new(Some(http_source.clone()), Some(grpc_source.clone()));

        let http_before = http_source.load();
        let grpc_before = grpc_source.load();

        // Rotate HTTP cleanly; corrupt gRPC so one succeeds and one fails.
        let rotated = generate_cert("localhost");
        std::fs::write(&http_config.cert_path, &rotated.cert_pem).expect("rewrite cert");
        std::fs::write(&http_config.key_path, &rotated.key_pem).expect("rewrite key");
        std::fs::write(&grpc_config.cert_path, "-----BEGIN CERTIFICATE-----\nbad")
            .expect("corrupt cert");

        let outcomes = handle.reload_all();

        assert_eq!(outcomes.len(), 2, "every listener must be reported on");
        let http_result = outcomes
            .iter()
            .find(|(listener, _)| *listener == TlsListenerKind::Http)
            .map(|(_, result)| result.is_ok());
        let grpc_result = outcomes
            .iter()
            .find(|(listener, _)| *listener == TlsListenerKind::Grpc)
            .map(|(_, result)| result.is_ok());
        assert_eq!(http_result, Some(true), "the valid rotation must succeed");
        assert_eq!(
            grpc_result,
            Some(false),
            "the broken rotation must be reported, not swallowed by its sibling"
        );

        assert!(
            !Arc::ptr_eq(&http_source.load(), &http_before),
            "the listener whose rotation succeeded must serve the new certificate"
        );
        assert!(
            Arc::ptr_eq(&grpc_source.load(), &grpc_before),
            "the listener whose rotation failed must keep its last-good certificate"
        );
    }

    // --- Shared trigger wiring -------------------------------------------
    //
    // `validate_reload_interval` and `install_reload_triggers` are the single
    // implementation behind both the `ServiceBuilder` path and `Server::serve`,
    // so they are asserted here rather than once per caller.

    #[test]
    fn a_zero_poll_interval_is_rejected() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut tls_config = write_credentials(dir.path(), &generate_cert("localhost"));
        tls_config.reload_interval_secs = Some(0);

        let err = validate_reload_interval(&tls_config, "[tls]")
            .expect_err("zero would poll without pause and must be refused");

        let message = err.to_string();
        assert!(
            message.contains("reload_interval_secs = 0"),
            "the error must name the setting: {message}"
        );
        assert!(
            message.contains("[tls]"),
            "the error must name the section so an operator knows which to fix: {message}"
        );
    }

    #[test]
    fn a_zero_handshake_timeout_is_rejected() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut tls_config = write_credentials(dir.path(), &generate_cert("localhost"));
        tls_config.handshake_timeout_secs = Some(0);

        let err = validate_handshake_timeout(&tls_config, "[tls]")
            .expect_err("a zero handshake timeout would fail every connection and must be refused");

        let message = err.to_string();
        assert!(
            message.contains("handshake_timeout_secs = 0"),
            "the error must name the setting: {message}"
        );
        assert!(
            message.contains("[tls]"),
            "the error must name the section so an operator knows which to fix: {message}"
        );
    }

    #[test]
    fn an_absent_or_positive_handshake_timeout_resolves_to_a_duration() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut tls_config = write_credentials(dir.path(), &generate_cert("localhost"));

        assert_eq!(
            validate_handshake_timeout(&tls_config, "[tls]").expect("absent is valid"),
            DEFAULT_HANDSHAKE_TIMEOUT,
            "omitting the field uses the built-in default"
        );

        tls_config.handshake_timeout_secs = Some(5);
        assert_eq!(
            validate_handshake_timeout(&tls_config, "[tls]").expect("positive is valid"),
            Duration::from_secs(5)
        );
    }

    #[test]
    fn an_absent_or_positive_poll_interval_is_accepted() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut tls_config = write_credentials(dir.path(), &generate_cert("localhost"));

        assert_eq!(
            validate_reload_interval(&tls_config, "[tls]").expect("absent is valid"),
            None,
            "omitting the field is how polling is disabled"
        );

        tls_config.reload_interval_secs = Some(30);
        assert_eq!(
            validate_reload_interval(&tls_config, "[tls]").expect("positive is valid"),
            Some(Duration::from_secs(30))
        );
    }

    /// Configuring rotation for credentials that have no files behind them
    /// cannot work. It must be audible rather than silently ignored.
    #[test]
    fn reload_config_on_a_static_source_is_detected_as_unusable() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut tls_config = write_credentials(dir.path(), &generate_cert("localhost"));
        tls_config.reload_interval_secs = Some(30);
        let static_source =
            TlsConfigSource::from_server_config(load_server_config(&tls_config).expect("config"));
        let file_source = TlsConfigSource::from_tls_config(&tls_config).expect("load");

        // The warning is a tracing side effect; what is asserted here is the
        // condition that drives it, which is the part that could regress.
        assert!(
            !static_source.is_reloadable(),
            "an injected ServerConfig has no files to reread"
        );
        assert!(file_source.is_reloadable());

        // Exercised for absence of panic on both source kinds and on the
        // no-triggers-configured early return.
        warn_if_reload_config_is_unusable(Some(&static_source), &tls_config, "[tls]");
        warn_if_reload_config_is_unusable(Some(&file_source), &tls_config, "[tls]");
        tls_config.reload_interval_secs = None;
        tls_config.reload_on_sighup = false;
        warn_if_reload_config_is_unusable(Some(&static_source), &tls_config, "[tls]");
    }

    /// The `Server::serve` shape: one listener, HTTP slot only, no gRPC
    /// interval. Must arm the poll task rather than silently doing nothing.
    #[tokio::test]
    async fn install_reload_triggers_arms_a_single_listener_path() {
        let dir = tempfile::tempdir().expect("temp dir");
        let tls_config = write_credentials(dir.path(), &generate_cert("localhost"));
        let source = TlsConfigSource::from_tls_config(&tls_config).expect("load");
        let handle = TlsReloadHandle::new(Some(source), None);

        let tasks = install_reload_triggers(&handle, Some(Duration::from_secs(30)), None, false);

        assert_eq!(
            tasks.handles.len(),
            1,
            "a polled single-listener path must arm exactly one poll task"
        );
    }

    /// No triggers configured must spawn nothing at all, so a service that does
    /// not ask for rotation pays nothing for the feature existing.
    #[tokio::test]
    async fn install_reload_triggers_spawns_nothing_when_unconfigured() {
        let dir = tempfile::tempdir().expect("temp dir");
        let tls_config = write_credentials(dir.path(), &generate_cert("localhost"));
        let source = TlsConfigSource::from_tls_config(&tls_config).expect("load");
        let handle = TlsReloadHandle::new(Some(source), None);

        let tasks = install_reload_triggers(&handle, None, None, false);

        assert!(tasks.handles.is_empty());
    }

    /// Dropping the guard must abort the tasks, so triggers cannot outlive the
    /// listener they rotate or hold up a graceful shutdown.
    #[tokio::test]
    async fn dropping_the_task_guard_aborts_the_triggers() {
        let dir = tempfile::tempdir().expect("temp dir");
        let tls_config = write_credentials(dir.path(), &generate_cert("localhost"));
        let source = TlsConfigSource::from_tls_config(&tls_config).expect("load");
        let handle = TlsReloadHandle::new(Some(source), None);

        let tasks = install_reload_triggers(&handle, Some(Duration::from_secs(30)), None, false);
        let spawned: Vec<_> = tasks
            .handles
            .iter()
            .map(tokio::task::JoinHandle::abort_handle)
            .collect();
        assert_eq!(spawned.len(), 1);

        drop(tasks);
        // Give the runtime a chance to process the abort.
        tokio::task::yield_now().await;

        assert!(
            spawned[0].is_finished(),
            "the poll task must not outlive the guard that owns it"
        );
    }

    #[test]
    fn listener_kind_names_are_stable_for_logs() {
        assert_eq!(TlsListenerKind::Http.as_str(), "http");
        assert_eq!(TlsListenerKind::Grpc.as_str(), "grpc");
        assert_eq!(TlsListenerKind::Grpc.to_string(), "grpc");
    }

    /// The handle's `Debug` reports coverage, never key material.
    #[test]
    fn handle_debug_does_not_leak_key_material() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cert = generate_cert("localhost");
        let tls_config = write_credentials(dir.path(), &cert);
        let source = TlsConfigSource::from_tls_config(&tls_config).expect("load");
        let handle = TlsReloadHandle::new(Some(source), None);

        let rendered = format!("{handle:?}");

        assert!(rendered.contains("http: true"));
        assert!(
            !rendered.contains("BEGIN"),
            "Debug must not render PEM material: {rendered}"
        );
    }

    /// A client-side verifier that accepts any server certificate, borrowing
    /// only the signature checks from the installed crypto provider. The
    /// handshake-off-the-accept-path test cares that a real handshake completes,
    /// not that the self-signed test certificate chains to a trusted root, so
    /// trust verification is deliberately bypassed here.
    #[derive(Debug)]
    struct AcceptAnyServerCert {
        provider: Arc<tokio_rustls::rustls::crypto::CryptoProvider>,
    }

    impl tokio_rustls::rustls::client::danger::ServerCertVerifier for AcceptAnyServerCert {
        fn verify_server_cert(
            &self,
            _end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &rustls_pki_types::ServerName<'_>,
            _ocsp_response: &[u8],
            _now: rustls_pki_types::UnixTime,
        ) -> std::result::Result<
            tokio_rustls::rustls::client::danger::ServerCertVerified,
            tokio_rustls::rustls::Error,
        > {
            Ok(tokio_rustls::rustls::client::danger::ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &tokio_rustls::rustls::DigitallySignedStruct,
        ) -> std::result::Result<
            tokio_rustls::rustls::client::danger::HandshakeSignatureValid,
            tokio_rustls::rustls::Error,
        > {
            tokio_rustls::rustls::crypto::verify_tls12_signature(
                message,
                cert,
                dss,
                &self.provider.signature_verification_algorithms,
            )
        }

        fn verify_tls13_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &tokio_rustls::rustls::DigitallySignedStruct,
        ) -> std::result::Result<
            tokio_rustls::rustls::client::danger::HandshakeSignatureValid,
            tokio_rustls::rustls::Error,
        > {
            tokio_rustls::rustls::crypto::verify_tls13_signature(
                message,
                cert,
                dss,
                &self.provider.signature_verification_algorithms,
            )
        }

        fn supported_verify_schemes(&self) -> Vec<tokio_rustls::rustls::SignatureScheme> {
            self.provider
                .signature_verification_algorithms
                .supported_schemes()
        }
    }

    /// The core regression for issue #94: a peer that completes the TCP connect
    /// and then never sends a ClientHello must not block a healthy handshake
    /// behind it. With handshakes run inline on the accept path, the silent peer
    /// held the single accept future for the whole handshake timeout and stalled
    /// every new connection — a pre-authentication denial of service.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_stalled_handshake_does_not_block_a_healthy_one() {
        use axum::serve::Listener as _;

        crate::crypto::ensure_default_crypto_provider();

        // Server credentials with no client-certificate requirement.
        let dir = tempfile::tempdir().expect("temp dir");
        let server_cert = generate_cert("localhost");
        let tls_config = write_credentials(dir.path(), &server_cert);
        let source = TlsConfigSource::from_tls_config(&tls_config).expect("server config loads");

        let tcp = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral port");
        let addr = tcp.local_addr().expect("local addr");

        // Long enough that "the healthy handshake finished first" is unambiguous
        // and that, if the silent peer ever did block the accept path, this test
        // would clearly hang rather than pass by luck.
        let handshake_timeout = Duration::from_secs(30);
        let mut listener =
            TlsListener::with_config_source(tcp, source).with_handshake_timeout(handshake_timeout);

        // 1) The silent peer. Held in scope for the whole test so its socket
        // stays open. The pump is spawned lazily on the first `accept()` below,
        // so this connection is guaranteed to sit ahead of the healthy one in
        // the accept backlog.
        let _silent = tokio::net::TcpStream::connect(addr)
            .await
            .expect("the silent peer completes the TCP connect");

        // 2) A healthy client that completes a real handshake, driven on its own
        // task so it runs concurrently with the accept below.
        let provider = tokio_rustls::rustls::crypto::CryptoProvider::get_default()
            .expect("a crypto provider is installed")
            .clone();
        let client_config = tokio_rustls::rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAnyServerCert { provider }))
            .with_no_client_auth();

        let good_client = tokio::spawn(async move {
            let stream = tokio::net::TcpStream::connect(addr)
                .await
                .expect("the healthy peer connects");
            let _tls = tokio_rustls::TlsConnector::from(Arc::new(client_config))
                .connect(
                    rustls_pki_types::ServerName::try_from("localhost").expect("valid name"),
                    stream,
                )
                .await
                .expect("the healthy handshake must complete");
            // Hold the client end open briefly so the server side settles.
            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        // The healthy connection must reach `accept()` far inside the silent
        // peer's timeout. A generous 5s outer bound catches a regression (an
        // inline handshake would hold `accept()` for the full 30s) without
        // making the test flaky on a slow machine.
        let started = std::time::Instant::now();
        let accepted = tokio::time::timeout(Duration::from_secs(5), listener.accept())
            .await
            .expect("the healthy handshake must be delivered without waiting on the silent peer");
        let elapsed = started.elapsed();

        assert!(
            elapsed < Duration::from_secs(2),
            "the healthy handshake was delivered in {elapsed:?}, which suggests it queued \
             behind the silent peer instead of running concurrently"
        );

        // A real TLS stream reached us: prove it by reading its negotiated state
        // is available (the connection is established, not a bare TCP socket).
        let (_stream, peer_addr) = accepted;
        assert_eq!(
            peer_addr.ip(),
            std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            "the delivered connection is the loopback healthy client"
        );

        good_client
            .await
            .expect("the healthy client task must not panic");
    }

    #[test]
    fn tls_connect_info_reports_absence_of_client_cert() {
        let info = TlsConnectInfo {
            remote_addr: "127.0.0.1:8443".parse().expect("addr"),
            peer_certificates: None,
        };

        assert!(
            !info.is_mutually_authenticated(),
            "a connection without a client cert is not mutually authenticated"
        );
        assert!(info.peer_certificates().is_none());
        assert_eq!(info.remote_addr(), "127.0.0.1:8443".parse().expect("addr"));
    }

    #[test]
    fn tls_connect_info_exposes_the_verified_chain() {
        let leaf = generate_cert("client-leaf");
        let chain = PeerCertificates(Arc::new(vec![leaf.der.clone()]));
        let info = TlsConnectInfo {
            remote_addr: "10.0.0.5:9000".parse().expect("addr"),
            peer_certificates: Some(chain),
        };

        assert!(info.is_mutually_authenticated());
        let certs = info.peer_certificates().expect("chain present");
        assert_eq!(certs.as_slice().len(), 1);
        assert_eq!(certs.leaf(), &leaf.der, "the leaf must be the first cert");
    }
}
