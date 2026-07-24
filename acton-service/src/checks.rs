//! App-defined liveness and readiness checks.
//!
//! The framework's built-in `/ready` handler probes only the backends it
//! manages (database, cache, events, …). Services whose real readiness lives
//! in application state — a writer task, a consensus quorum, a sidecar —
//! register checks here via
//! [`ServiceBuilder::with_readiness_check`](crate::service::ServiceBuilder::with_readiness_check)
//! and
//! [`ServiceBuilder::with_liveness_check`](crate::service::ServiceBuilder::with_liveness_check).
//!
//! Semantics:
//!
//! - **Liveness** (`/health`) answers "should the process be restarted?". Any
//!   liveness check returning [`CheckOutcome::Unready`] turns `/health` into
//!   `503`. A `Degraded` outcome from a liveness check counts as healthy —
//!   liveness is binary; a degraded-but-working process must not be killed.
//! - **Readiness** (`/ready`) answers "should the process receive traffic?".
//!   `Unready` flips the endpoint to `503`; `Degraded` renders the check as an
//!   unhealthy dependency in the response body **without** flipping overall
//!   readiness — visible to operators, invisible to the load balancer.
//!
//! All registered checks for an endpoint run **concurrently under one shared
//! deadline** (default [`DEFAULT_CHECK_DEADLINE`]): N stalled checks cost one
//! deadline, not N. A check unresolved at the deadline reports
//! `Unready("check timed out")`.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

/// The shared deadline applied to one endpoint's registered checks when the
/// builder is not given an explicit one.
pub const DEFAULT_CHECK_DEADLINE: Duration = Duration::from_secs(2);

/// The message reported for a check still unresolved at the shared deadline.
const TIMED_OUT: &str = "check timed out";

/// The outcome of one app-defined check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckOutcome {
    /// The checked concern is healthy.
    Ready,
    /// The checked concern is impaired but the endpoint's overall answer is
    /// unaffected: rendered as an unhealthy dependency on `/ready`, ignored
    /// for `/health`. The message should say what is impaired and why traffic
    /// can still flow.
    Degraded(String),
    /// The checked concern has failed: `/ready` answers `503` (drain traffic);
    /// on a liveness check, `/health` answers `503` (restart the process).
    Unready(String),
}

/// A boxed future produced by one invocation of a registered check.
type CheckFuture = Pin<Box<dyn Future<Output = CheckOutcome> + Send>>;

/// One registered check: a display name plus the closure that produces a fresh
/// outcome future per probe hit.
#[derive(Clone)]
pub struct RegisteredCheck {
    name: String,
    run: Arc<dyn Fn() -> CheckFuture + Send + Sync>,
}

impl fmt::Debug for RegisteredCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegisteredCheck")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl RegisteredCheck {
    /// Wrap a check closure under its display name.
    pub fn new<F, Fut>(name: impl Into<String>, check: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = CheckOutcome> + Send + 'static,
    {
        Self {
            name: name.into(),
            run: Arc::new(move || Box::pin(check())),
        }
    }

    /// The name the check reports under in the readiness body.
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// The immutable set of app-defined checks carried on the application state.
///
/// Cheap to clone (the check lists are shared); empty by default, in which
/// case both endpoints behave exactly as they did before checks existed.
#[derive(Debug, Clone)]
pub struct HealthChecks {
    liveness: Arc<[RegisteredCheck]>,
    readiness: Arc<[RegisteredCheck]>,
    deadline: Duration,
}

impl Default for HealthChecks {
    fn default() -> Self {
        Self {
            liveness: Arc::from([]),
            readiness: Arc::from([]),
            deadline: DEFAULT_CHECK_DEADLINE,
        }
    }
}

impl HealthChecks {
    /// Assemble the frozen check set (called once by the service builder).
    pub(crate) fn new(
        liveness: Vec<RegisteredCheck>,
        readiness: Vec<RegisteredCheck>,
        deadline: Duration,
    ) -> Self {
        Self {
            liveness: liveness.into(),
            readiness: readiness.into(),
            deadline,
        }
    }

    /// Run every registered liveness check; `true` when none answered
    /// `Unready` (a `Degraded` liveness outcome counts as alive).
    pub async fn liveness_ok(&self) -> bool {
        run_concurrent(&self.liveness, self.deadline)
            .await
            .iter()
            .all(|(_, outcome)| !matches!(outcome, CheckOutcome::Unready(_)))
    }

    /// Run every registered readiness check concurrently under the shared
    /// deadline, returning `(name, outcome)` pairs in registration order.
    pub async fn readiness_outcomes(&self) -> Vec<(String, CheckOutcome)> {
        run_concurrent(&self.readiness, self.deadline).await
    }

    /// Whether any readiness checks are registered.
    pub fn has_readiness_checks(&self) -> bool {
        !self.readiness.is_empty()
    }
}

/// Run `checks` concurrently under one shared deadline. A check unresolved at
/// the deadline reports [`CheckOutcome::Unready`] with [`TIMED_OUT`].
async fn run_concurrent(
    checks: &[RegisteredCheck],
    deadline: Duration,
) -> Vec<(String, CheckOutcome)> {
    if checks.is_empty() {
        return Vec::new();
    }
    let expiry = tokio::time::Instant::now() + deadline;
    let futures = checks.iter().map(|check| {
        let name = check.name.clone();
        let fut = (check.run)();
        async move {
            let outcome = tokio::time::timeout_at(expiry, fut)
                .await
                .unwrap_or_else(|_| CheckOutcome::Unready(TIMED_OUT.to_string()));
            (name, outcome)
        }
    });
    futures::future::join_all(futures).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checks_of(list: Vec<RegisteredCheck>) -> HealthChecks {
        HealthChecks::new(list.clone(), list, Duration::from_millis(200))
    }

    #[tokio::test]
    async fn empty_check_set_is_ready_and_alive() {
        let checks = HealthChecks::default();
        assert!(checks.liveness_ok().await);
        assert!(checks.readiness_outcomes().await.is_empty());
        assert!(!checks.has_readiness_checks());
    }

    #[tokio::test]
    async fn outcomes_preserve_registration_order_and_values() {
        let checks = checks_of(vec![
            RegisteredCheck::new("a", || async { CheckOutcome::Ready }),
            RegisteredCheck::new("b", || async {
                CheckOutcome::Degraded("impaired".to_string())
            }),
            RegisteredCheck::new("c", || async {
                CheckOutcome::Unready("failed".to_string())
            }),
        ]);
        let outcomes = checks.readiness_outcomes().await;
        assert_eq!(outcomes.len(), 3);
        assert_eq!(outcomes[0], ("a".to_string(), CheckOutcome::Ready));
        assert_eq!(
            outcomes[1],
            ("b".to_string(), CheckOutcome::Degraded("impaired".to_string()))
        );
        assert_eq!(
            outcomes[2],
            ("c".to_string(), CheckOutcome::Unready("failed".to_string()))
        );
    }

    #[tokio::test]
    async fn degraded_liveness_counts_as_alive_but_unready_does_not() {
        let degraded = checks_of(vec![RegisteredCheck::new("d", || async {
            CheckOutcome::Degraded("limping".to_string())
        })]);
        assert!(degraded.liveness_ok().await);

        let unready = checks_of(vec![RegisteredCheck::new("u", || async {
            CheckOutcome::Unready("dead".to_string())
        })]);
        assert!(!unready.liveness_ok().await);
    }

    #[tokio::test]
    async fn stalled_checks_share_one_deadline_and_report_timeout() {
        let stall = || async {
            tokio::time::sleep(Duration::from_secs(60)).await;
            CheckOutcome::Ready
        };
        let checks = HealthChecks::new(
            Vec::new(),
            vec![
                RegisteredCheck::new("slow-1", stall),
                RegisteredCheck::new("slow-2", stall),
                RegisteredCheck::new("fast", || async { CheckOutcome::Ready }),
            ],
            Duration::from_millis(100),
        );
        let started = tokio::time::Instant::now();
        let outcomes = checks.readiness_outcomes().await;
        // Two stalled checks cost one shared deadline, not two.
        assert!(started.elapsed() < Duration::from_millis(1_500));
        assert_eq!(
            outcomes[0].1,
            CheckOutcome::Unready(TIMED_OUT.to_string())
        );
        assert_eq!(
            outcomes[1].1,
            CheckOutcome::Unready(TIMED_OUT.to_string())
        );
        assert_eq!(outcomes[2].1, CheckOutcome::Ready);
    }
}
