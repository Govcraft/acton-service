//! Reload a value when the files it is read from change on disk.
//!
//! The watched-file poll behind TLS credential rotation, for any value a
//! service reads from files: a token, a key list, a policy. You implement
//! [`Reloadable`] (which files to watch, and how to reread, validate and
//! install the value). The poll decides *when*:
//!
//! - it fingerprints the files by **content**, never by modification time,
//!   so `cp -p` and certificate tooling that preserve mtimes are still seen,
//!   and a touch with identical bytes is not a reload;
//! - it reloads only when the fingerprint differs from what is serving;
//! - a file it cannot read is "unknown, retry next tick": neither changed nor
//!   unchanged;
//! - a reload that is **rejected**, or that panics, keeps the previous value
//!   serving and is reported once per distinct content, not once per tick. A
//!   half-written file that is later completed changes its content, so it is
//!   retried;
//! - each tick runs on the blocking pool, so a slow mount never stalls a
//!   runtime worker, and the task outlives any single failed tick.
//!
//! ```no_run
//! use std::borrow::Cow;
//! use std::path::PathBuf;
//! use std::sync::{Arc, RwLock};
//! use std::time::Duration;
//!
//! use acton_service::reload::{self, BoxError, Reloadable};
//!
//! #[derive(Clone)]
//! struct Token {
//!     path: PathBuf,
//!     current: Arc<RwLock<String>>,
//! }
//!
//! impl Reloadable for Token {
//!     fn label(&self) -> Cow<'_, str> {
//!         Cow::Borrowed("the API token")
//!     }
//!
//!     fn watched_paths(&self) -> Vec<PathBuf> {
//!         vec![self.path.clone()]
//!     }
//!
//!     fn reload(&self) -> Result<(), BoxError> {
//!         let token = std::fs::read_to_string(&self.path)?;
//!         if token.trim().is_empty() {
//!             return Err("the token file is empty".into());
//!         }
//!         // Validated: now, and only now, swap.
//!         *self.current.write().map_err(|_| "the token lock is poisoned")? = token;
//!         Ok(())
//!     }
//! }
//!
//! # async fn demo(token: Token) -> std::io::Result<()> {
//! let serving = reload::fingerprint_files(&token.watched_paths())?;
//! let _poll = reload::spawn_reload_poll(token, Duration::from_secs(30), Some(serving));
//! # Ok(()) }
//! ```

use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The error a [`Reloadable::reload`] returns: anything that says why.
pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// A value read from files that the poll can reload when they change.
///
/// Implementations are cheap handles (typically an `Arc` inside), because the
/// poll clones one onto the blocking pool for every tick.
pub trait Reloadable: Clone + Send + Sync + 'static {
    /// Names the value in the poll's log lines, e.g. `"the http listener's
    /// TLS credentials"`.
    fn label(&self) -> Cow<'_, str>;

    /// The files whose content decides whether a reload is due, in a stable
    /// order. An empty list means there is nothing to watch, and every tick
    /// is [`ReloadTick::Unchanged`].
    fn watched_paths(&self) -> Vec<PathBuf>;

    /// Rereads the files, validates the result, and only then installs it.
    ///
    /// Fail closed: on `Err` the value already serving must be left exactly as
    /// it was. A panic is caught and treated as a rejection of the content, so
    /// install the new value only as the last step. The poll reports the
    /// outcome (`INFO` on success, `ERROR` on a rejection, once per distinct
    /// content), so an implementation adds its own counters but need not log
    /// it again.
    ///
    /// Under `panic = "abort"` a panicking reload terminates the process: the
    /// catch does not apply, so do not rely on it there.
    ///
    /// # Errors
    ///
    /// Why the new content was not installed.
    fn reload(&self) -> Result<(), BoxError>;
}

/// A content fingerprint of a set of files. Only ever compared with another
/// fingerprint of the same files, in the same process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Fingerprint(u64);

/// Fingerprints `paths` by their content.
///
/// A fast non-cryptographic hash is the right tool: this detects *change*
/// and makes no claim about integrity. Anyone who can rewrite the files has
/// already won, whichever hash reads them. Each path is hashed with its
/// bytes, and each file is length-prefixed, so swapping which file holds
/// which bytes, or moving bytes across a file boundary, still changes the
/// fingerprint.
///
/// # Errors
///
/// The first file that cannot be read. Treat it as "unknown, try again", not
/// as "unchanged" (which would strand a rotation) and not as "changed" (which
/// would reload from a half-written file every tick).
pub fn fingerprint_files<P: AsRef<Path>>(paths: &[P]) -> std::io::Result<Fingerprint> {
    let mut hasher = DefaultHasher::new();
    for path in paths {
        let path = path.as_ref();
        path.hash(&mut hasher);
        let bytes = std::fs::read(path)?;
        bytes.len().hash(&mut hasher);
        bytes.hash(&mut hasher);
    }
    Ok(Fingerprint(hasher.finish()))
}

/// What the poll remembers between ticks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PollState {
    serving: Option<Fingerprint>,
    rejected: Option<Fingerprint>,
}

impl PollState {
    /// A poll whose value was loaded from files that fingerprinted as
    /// `serving`. Take the fingerprint *before* the initial load, so a change
    /// between the load and the first tick is caught rather than mistaken for
    /// the baseline. `None` (the files could not be hashed at load) makes the
    /// first readable tick reload once, redundantly, rather than miss a
    /// change.
    #[must_use]
    pub const fn new(serving: Option<Fingerprint>) -> Self {
        Self {
            serving,
            rejected: None,
        }
    }

    /// The fingerprint of the content now installed, if known.
    #[must_use]
    pub const fn serving(&self) -> Option<Fingerprint> {
        self.serving
    }

    /// The fingerprint of the last content a reload rejected, until the
    /// files change again.
    #[must_use]
    pub const fn rejected(&self) -> Option<Fingerprint> {
        self.rejected
    }
}

/// What one tick concluded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReloadTick {
    /// The files hold what is serving (or there are none to watch).
    Unchanged,
    /// The files changed and the new value is installed.
    Reloaded {
        /// The fingerprint now serving.
        fingerprint: Fingerprint,
    },
    /// The files could not be read. Nothing is recorded, so the next tick
    /// retries.
    ReadFailed,
    /// The files changed, and the reload rejected the new content. The
    /// previous value keeps serving.
    ReloadFailed {
        /// The rejected content's fingerprint.
        fingerprint: Fingerprint,
    },
    /// The files still hold the content the last reload rejected. It is not
    /// attempted or reported again until the files change.
    StillRejected {
        /// The rejected content's fingerprint.
        fingerprint: Fingerprint,
    },
}

/// Runs one tick against `source`: fingerprint, compare, reload on change, and
/// record the outcome in `state`.
///
/// Split from the timer loop so the decision can be tested without a clock.
/// Blocking: it reads files, so an async caller runs it on the blocking pool,
/// as [`spawn_reload_poll`] does. It logs `WARN` on a read failure, `INFO` on
/// a reload and `ERROR` on a rejection.
pub fn reload_tick<R: Reloadable>(source: &R, state: &mut PollState) -> ReloadTick {
    let paths = source.watched_paths();
    if paths.is_empty() {
        return ReloadTick::Unchanged;
    }
    let fingerprint = match fingerprint_files(&paths) {
        Ok(fingerprint) => fingerprint,
        Err(error) => {
            tracing::warn!(
                reloadable = %source.label(),
                error = %error,
                "could not read the watched files of {} while polling for changes; \
                 the current value keeps serving and the next tick retries",
                source.label()
            );
            return ReloadTick::ReadFailed;
        }
    };
    if state.serving == Some(fingerprint) {
        // Back to what is serving: a later return of the rejected content is
        // a fresh attempt, and is reported again.
        state.rejected = None;
        return ReloadTick::Unchanged;
    }
    if state.rejected == Some(fingerprint) {
        return ReloadTick::StillRejected { fingerprint };
    }
    match reload_catching_panics(source) {
        Ok(()) => {
            state.serving = Some(fingerprint);
            state.rejected = None;
            tracing::info!(
                reloadable = %source.label(),
                "{} changed on disk and was reloaded",
                source.label()
            );
            ReloadTick::Reloaded { fingerprint }
        }
        Err(error) => {
            state.rejected = Some(fingerprint);
            tracing::error!(
                reloadable = %source.label(),
                error = %error,
                "{} changed on disk, and the new content was rejected; the previous \
                 value keeps serving. This is reported once per distinct content: fix \
                 the files and the next tick retries",
                source.label()
            );
            ReloadTick::ReloadFailed { fingerprint }
        }
    }
}

/// Runs [`Reloadable::reload`], turning a panic into a rejection.
///
/// Without this a reload that panics on some content would leave the poll's
/// state untouched, so every later tick would reread the same files, panic
/// again, and log again: the per-tick noise the rejection memory exists to
/// stop. `AssertUnwindSafe` is sound here because a panicking reload has, by
/// the contract, installed nothing, and the poll keeps no state across the
/// call.
///
/// Under `panic = "abort"` a panicking reload terminates the process: there
/// is no unwind to catch, so this conversion does not apply.
fn reload_catching_panics<R: Reloadable>(source: &R) -> Result<(), BoxError> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| source.reload())).unwrap_or_else(
        |payload| {
            let message = payload
                .downcast_ref::<&str>()
                .map(|message| (*message).to_owned())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "a non-string panic payload".to_owned());
            Err(format!("the reload panicked: {message}").into())
        },
    )
}

/// Polls `source`'s files every `period` and reloads on a content change.
///
/// `serving` is the fingerprint of the content already installed (see
/// [`PollState::new`]). The first check happens one `period` after the
/// call. A tick delayed by a slow reload is skipped rather than made up in a
/// burst. The task runs until aborted and holds only a clone of `source`;
/// dropping the returned handle detaches it.
///
/// Call from within a Tokio runtime.
pub fn spawn_reload_poll<R: Reloadable>(
    source: R,
    period: Duration,
    serving: Option<Fingerprint>,
) -> tokio::task::JoinHandle<()> {
    tracing::info!(
        reloadable = %source.label(),
        interval_secs = period.as_secs(),
        "polling the watched files of {} for changes",
        source.label()
    );
    tokio::spawn(async move {
        let mut state = PollState::new(serving);
        let mut ticker = tokio::time::interval(period);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // A tokio interval's first tick completes at once; the first real
        // check is one period in.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let tick_source = source.clone();
            let mut tick_state = state;
            match tokio::task::spawn_blocking(move || {
                let outcome = reload_tick(&tick_source, &mut tick_state);
                (outcome, tick_state)
            })
            .await
            {
                Ok((_, next)) => state = next,
                // The blocking task was cancelled (the runtime is shutting
                // down). A panicking reload never lands here: `reload_tick`
                // records it as a rejection. The state is left as it was, and
                // the task lives on, because a poll that dies stops every
                // later reload silently.
                Err(error) => tracing::error!(
                    reloadable = %source.label(),
                    error = %error,
                    "a reload tick for {} did not run to completion; the current value \
                     keeps serving and the next tick retries",
                    source.label()
                ),
            }
        }
    })
}

/// A poll interval of zero, refused before anything is spawned.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error(
    "{section} sets {field} = 0, which would poll the watched files without pause. Omit \
     the field to disable polling, or set a positive number of seconds."
)]
pub struct ZeroReloadInterval {
    /// The config section, e.g. `[tls]`.
    pub section: String,
    /// The field, e.g. `reload_interval_secs`.
    pub field: String,
}

/// Resolves a configured poll interval in seconds.
///
/// `None` means polling is off. Run it before the service binds anything, so
/// a bad value is a refusal to start.
///
/// # Errors
///
/// [`ZeroReloadInterval`] for `Some(0)`: it cannot mean "never" (that is what
/// omitting the field means), and taken literally it would reread the files
/// as fast as the disk allows.
pub fn validate_reload_interval(
    secs: Option<u64>,
    section: &str,
    field: &str,
) -> Result<Option<Duration>, ZeroReloadInterval> {
    match secs {
        None => Ok(None),
        Some(0) => Err(ZeroReloadInterval {
            section: section.to_owned(),
            field: field.to_owned(),
        }),
        Some(secs) => Ok(Some(Duration::from_secs(secs))),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::*;

    /// A value read from one file, rejecting any content that starts with
    /// `bad`, and panicking on content that starts with `panic`.
    #[derive(Clone)]
    struct Probe {
        path: PathBuf,
        installed: Arc<Mutex<String>>,
        attempts: Arc<AtomicUsize>,
        attempted: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    }

    impl Probe {
        fn new(dir: &Path, content: &str) -> Self {
            let path = dir.join("value");
            std::fs::write(&path, content).expect("write");
            Self {
                path,
                installed: Arc::new(Mutex::new(content.to_owned())),
                attempts: Arc::new(AtomicUsize::new(0)),
                attempted: None,
            }
        }

        /// Replaces the file atomically (write aside, then rename), so the
        /// poll never reads a half-written file here. The half-written case
        /// has its own test.
        fn write(&self, content: &str) {
            let aside = self.path.with_extension("next");
            std::fs::write(&aside, content).expect("write");
            std::fs::rename(&aside, &self.path).expect("rename");
        }

        fn installed(&self) -> String {
            self.installed.lock().expect("unpoisoned").clone()
        }

        fn attempts(&self) -> usize {
            self.attempts.load(Ordering::SeqCst)
        }

        fn baseline(&self) -> PollState {
            PollState::new(Some(fingerprint_files(&[&self.path]).expect("readable")))
        }
    }

    impl Reloadable for Probe {
        fn label(&self) -> Cow<'_, str> {
            Cow::Borrowed("the probe value")
        }

        fn watched_paths(&self) -> Vec<PathBuf> {
            vec![self.path.clone()]
        }

        fn reload(&self) -> Result<(), BoxError> {
            self.attempts.fetch_add(1, Ordering::SeqCst);
            let content = std::fs::read_to_string(&self.path)?;
            // Report each attempt only once its outcome is settled, so a test
            // that sees the report also sees the side effect.
            let report = |content: &str| {
                if let Some(attempted) = &self.attempted {
                    let _ = attempted.send(content.to_owned());
                }
            };
            if content.starts_with("panic") {
                report(&content);
                panic!("the probe was told to panic");
            }
            if content.starts_with("bad") {
                report(&content);
                return Err(format!("rejected {content:?}").into());
            }
            *self.installed.lock().expect("unpoisoned") = content.clone();
            report(&content);
            Ok(())
        }
    }

    #[test]
    fn unchanged_content_is_not_reloaded() {
        let dir = tempfile::tempdir().expect("dir");
        let probe = Probe::new(dir.path(), "one");
        let mut state = probe.baseline();
        probe.write("one");
        assert_eq!(reload_tick(&probe, &mut state), ReloadTick::Unchanged);
        assert_eq!(probe.attempts(), 0);
    }

    #[test]
    fn changed_content_is_reloaded_and_becomes_the_baseline() {
        let dir = tempfile::tempdir().expect("dir");
        let probe = Probe::new(dir.path(), "one");
        let mut state = probe.baseline();
        probe.write("two");
        let ReloadTick::Reloaded { fingerprint } = reload_tick(&probe, &mut state) else {
            panic!("a change must reload");
        };
        assert_eq!(state.serving(), Some(fingerprint));
        assert_eq!(probe.installed(), "two");
        assert_eq!(reload_tick(&probe, &mut state), ReloadTick::Unchanged);
        assert_eq!(probe.attempts(), 1);
    }

    #[test]
    fn an_unreadable_file_records_nothing_and_is_retried() {
        let dir = tempfile::tempdir().expect("dir");
        let probe = Probe::new(dir.path(), "one");
        let mut state = probe.baseline();
        let before = state;
        std::fs::remove_file(&probe.path).expect("remove");
        assert_eq!(reload_tick(&probe, &mut state), ReloadTick::ReadFailed);
        assert_eq!(state, before);
        probe.write("two");
        assert!(matches!(
            reload_tick(&probe, &mut state),
            ReloadTick::Reloaded { .. }
        ));
    }

    #[test]
    fn rejected_content_is_attempted_once_and_keeps_the_old_value() {
        let dir = tempfile::tempdir().expect("dir");
        let probe = Probe::new(dir.path(), "one");
        let mut state = probe.baseline();
        probe.write("bad: truncated");
        let ReloadTick::ReloadFailed { fingerprint } = reload_tick(&probe, &mut state) else {
            panic!("bad content must be rejected");
        };
        assert_eq!(probe.installed(), "one");
        assert_eq!(
            reload_tick(&probe, &mut state),
            ReloadTick::StillRejected { fingerprint },
            "identical rejected content is neither retried nor reported again"
        );
        assert_eq!(probe.attempts(), 1);
    }

    #[test]
    fn a_panicking_reload_is_rejected_once_and_keeps_the_old_value() {
        let dir = tempfile::tempdir().expect("dir");
        let probe = Probe::new(dir.path(), "one");
        let mut state = probe.baseline();

        probe.write("panic");
        let tick = reload_tick(&probe, &mut state);
        assert!(matches!(tick, ReloadTick::ReloadFailed { .. }), "{tick:?}");
        assert_eq!(probe.installed(), "one");
        assert_eq!(probe.attempts(), 1);

        // The same content is remembered, not panicked on every tick.
        let tick = reload_tick(&probe, &mut state);
        assert!(matches!(tick, ReloadTick::StillRejected { .. }), "{tick:?}");
        assert_eq!(probe.attempts(), 1);

        probe.write("two");
        assert!(matches!(
            reload_tick(&probe, &mut state),
            ReloadTick::Reloaded { .. }
        ));
        assert_eq!(probe.installed(), "two");
    }

    #[test]
    fn a_half_written_file_completed_in_place_is_retried() {
        let dir = tempfile::tempdir().expect("dir");
        let probe = Probe::new(dir.path(), "one");
        let mut state = probe.baseline();
        probe.write("bad: half of two");
        assert!(matches!(
            reload_tick(&probe, &mut state),
            ReloadTick::ReloadFailed { .. }
        ));
        probe.write("two");
        assert!(matches!(
            reload_tick(&probe, &mut state),
            ReloadTick::Reloaded { .. }
        ));
        assert_eq!(probe.installed(), "two");
        assert_eq!(state.rejected(), None);
    }

    #[test]
    fn rejected_content_that_returns_after_a_revert_is_attempted_again() {
        let dir = tempfile::tempdir().expect("dir");
        let probe = Probe::new(dir.path(), "one");
        let mut state = probe.baseline();
        probe.write("bad");
        assert!(matches!(
            reload_tick(&probe, &mut state),
            ReloadTick::ReloadFailed { .. }
        ));
        probe.write("one");
        assert_eq!(reload_tick(&probe, &mut state), ReloadTick::Unchanged);
        probe.write("bad");
        assert!(
            matches!(
                reload_tick(&probe, &mut state),
                ReloadTick::ReloadFailed { .. }
            ),
            "a new attempt at the same bad content is reported again"
        );
        assert_eq!(probe.attempts(), 2);
    }

    #[test]
    fn an_unknown_baseline_reloads_on_the_first_readable_tick() {
        let dir = tempfile::tempdir().expect("dir");
        let probe = Probe::new(dir.path(), "one");
        let mut state = PollState::new(None);
        assert!(matches!(
            reload_tick(&probe, &mut state),
            ReloadTick::Reloaded { .. }
        ));
        assert_eq!(reload_tick(&probe, &mut state), ReloadTick::Unchanged);
    }

    #[derive(Clone)]
    struct Nothing;

    impl Reloadable for Nothing {
        fn label(&self) -> Cow<'_, str> {
            Cow::Borrowed("nothing")
        }

        fn watched_paths(&self) -> Vec<PathBuf> {
            Vec::new()
        }

        fn reload(&self) -> Result<(), BoxError> {
            Err("never called".into())
        }
    }

    #[test]
    fn a_source_with_no_files_is_always_unchanged() {
        let mut state = PollState::new(None);
        assert_eq!(reload_tick(&Nothing, &mut state), ReloadTick::Unchanged);
    }

    #[test]
    fn identical_bytes_fingerprint_identically_and_a_same_length_change_does_not() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("f");
        std::fs::write(&path, "abcd").expect("write");
        let before = fingerprint_files(&[&path]).expect("readable");
        std::fs::write(&path, "abcd").expect("rewrite");
        assert_eq!(fingerprint_files(&[&path]).expect("readable"), before);
        std::fs::write(&path, "abce").expect("rewrite");
        assert_ne!(fingerprint_files(&[&path]).expect("readable"), before);
    }

    #[test]
    fn moving_bytes_across_a_file_boundary_changes_the_fingerprint() {
        let dir = tempfile::tempdir().expect("dir");
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        std::fs::write(&a, "ab").expect("write");
        std::fs::write(&b, "c").expect("write");
        let before = fingerprint_files(&[&a, &b]).expect("readable");
        std::fs::write(&a, "a").expect("write");
        std::fs::write(&b, "bc").expect("write");
        assert_ne!(fingerprint_files(&[&a, &b]).expect("readable"), before);
    }

    #[test]
    fn a_zero_interval_is_refused_naming_the_section_and_field() {
        let error = validate_reload_interval(Some(0), "[gate.attestation]", "reload_interval_secs")
            .expect_err("zero is refused");
        let message = error.to_string();
        assert!(message.contains("[gate.attestation]"), "{message}");
        assert!(message.contains("reload_interval_secs = 0"), "{message}");
        assert_eq!(
            validate_reload_interval(None, "[tls]", "reload_interval_secs"),
            Ok(None)
        );
        assert_eq!(
            validate_reload_interval(Some(30), "[tls]", "reload_interval_secs"),
            Ok(Some(Duration::from_secs(30)))
        );
    }

    /// Waits for the poll's next reload attempt, bounded so a broken poll
    /// fails the test instead of hanging it.
    async fn next_attempt(attempted: &mut tokio::sync::mpsc::UnboundedReceiver<String>) -> String {
        tokio::time::timeout(Duration::from_secs(10), attempted.recv())
            .await
            .expect("the poll attempted a reload")
            .expect("the probe is alive")
    }

    #[tokio::test]
    async fn the_poll_reloads_a_change_and_survives_a_panicking_tick() {
        let dir = tempfile::tempdir().expect("dir");
        let (tx, mut attempted) = tokio::sync::mpsc::unbounded_channel();
        let probe = Probe {
            attempted: Some(tx),
            ..Probe::new(dir.path(), "one")
        };
        let serving = probe.baseline().serving();
        let poll = spawn_reload_poll(probe.clone(), Duration::from_millis(20), serving);

        probe.write("two");
        assert_eq!(next_attempt(&mut attempted).await, "two");

        // A reload that panics is a rejection: it costs one attempt, not the
        // task, and the same content is not attempted again. So the next
        // attempt after the rewrite is the new content, not a retry.
        probe.write("panic");
        assert_eq!(next_attempt(&mut attempted).await, "panic");
        probe.write("three");
        assert_eq!(next_attempt(&mut attempted).await, "three");
        assert_eq!(probe.installed(), "three");
        assert!(!poll.is_finished(), "the poll outlives the panicking tick");
        poll.abort();
    }
}
