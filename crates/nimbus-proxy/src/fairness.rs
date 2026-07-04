//! EE3: per-tenant fairness budgets — the mechanism/seam, not the values.
//!
//! Today's guards are node-wide capacity (the substrate's DNS `Semaphore(32)`)
//! or per-PEP (the per-sandbox connection semaphore); nothing is per-tenant.
//! This module adds the per-tenant *mechanism* on three axes; the budget
//! VALUES and quota policy ride the tenant-admission-audit plan (TAA5):
//!
//! - **DNS**: a per-tenant acquire budget, distinct from (and acquired before)
//!   the shared node-wide guard, so one tenant exhausting its budget queues on
//!   its own semaphore instead of consuming shared capacity.
//! - **Bandwidth**: per-connection byte metering recorded from the relay copy
//!   loops (splice tunnel and intercept relay), attributed to the owning
//!   tenant.
//! - **Task time (the EE3 "CPU" axis)**: [`TenantTaskTimeAccounting`] —
//!   records per-tenant request-task WALL-CLOCK occupancy (the span covers
//!   I/O waits, so it is NOT a CPU-seconds measure; the name says what it
//!   measures). There is no cgroup/CPU quota on the shared runtime today, so
//!   this is deliberately accounting, not preemptive isolation; real
//!   CPU-seconds measurement is the upgrade path when quota policy (TAA)
//!   needs it.
//!
//! Identity follows the capture-at-registration principle: the sandbox layer
//! resolves its tenant's [`TenantFairness`] handle once, at PEP registration,
//! and the handle is captured into the PEP's context — the request path never
//! performs a per-request tenant lookup and never touches the registry map.

use std::collections::HashMap;
use std::io;

use nimbus_core::TenantId;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Node-wide registry of per-tenant fairness state. Owned by the node-scoped
/// engine (`engine.rs` — deliberately not named here: request paths hold
/// `TenantFairness` handles from this module, so the EE1 reachability lint
/// forbids this file from referencing the engine or its key type); touched at
/// PEP registration time only (get-or-create), so the map stays off the
/// request path.
pub struct FairnessRegistry {
    /// DNS acquire budget per tenant. `None` = no per-tenant budget configured
    /// (the mechanism is present, the value is TAA's to set).
    dns_permits_per_tenant: Option<usize>,
    tenants: Mutex<HashMap<TenantId, Arc<TenantFairness>>>,
}

impl FairnessRegistry {
    /// A registry with no budgets configured: handles still meter bytes and
    /// account CPU, and DNS is unbudgeted per-tenant (node guard still holds).
    pub fn new() -> Self {
        Self {
            dns_permits_per_tenant: None,
            tenants: Mutex::new(HashMap::new()),
        }
    }

    /// Configure the per-tenant DNS acquire budget applied to tenants
    /// resolved AFTER this registry was constructed with it.
    pub fn with_dns_permits_per_tenant(permits: usize) -> Self {
        Self {
            dns_permits_per_tenant: Some(permits),
            tenants: Mutex::new(HashMap::new()),
        }
    }

    /// Get or create the fairness handle for `tenant`.
    ///
    /// Lifecycle-time only (PEP registration); a poisoned map falls back to a
    /// detached handle rather than poisoning registration — budgets are
    /// fairness aids, not correctness gates, and a detached handle still
    /// meters and accounts (it just is not shared with other PEPs).
    pub fn tenant(&self, tenant: &TenantId) -> Arc<TenantFairness> {
        let mut map = match self.tenants.lock() {
            Ok(map) => map,
            // Fail-OPEN by design (deliberately the opposite polarity of the
            // engine's fail-closed registry lock): budgets and meters are
            // fairness aids, not authorization gates, so a poisoned map
            // degrades to an unshared handle instead of failing registration.
            Err(_) => {
                return Arc::new(TenantFairness::new(
                    tenant.clone(),
                    self.dns_permits_per_tenant,
                ));
            }
        };
        Arc::clone(map.entry(tenant.clone()).or_insert_with(|| {
            Arc::new(TenantFairness::new(
                tenant.clone(),
                self.dns_permits_per_tenant,
            ))
        }))
    }

    /// Number of tenants with fairness state (lifecycle observability).
    pub fn len(&self) -> usize {
        self.tenants.lock().map(|map| map.len()).unwrap_or(0)
    }

    /// True when no tenant state exists yet.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for FairnessRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-tenant fairness state: one instance per tenant per node, shared by all
/// of that tenant's PEPs via `Arc`. Captured into the PEP context at
/// registration.
pub struct TenantFairness {
    tenant: TenantId,
    dns: Option<Arc<Semaphore>>,
    bytes_to_upstream: AtomicU64,
    bytes_to_workload: AtomicU64,
    decisions_allowed: AtomicU64,
    decisions_denied: AtomicU64,
    task_time: TenantTaskTimeAccounting,
}

impl TenantFairness {
    fn new(tenant: TenantId, dns_permits: Option<usize>) -> Self {
        Self {
            tenant,
            dns: dns_permits.map(|permits| Arc::new(Semaphore::new(permits))),
            bytes_to_upstream: AtomicU64::new(0),
            bytes_to_workload: AtomicU64::new(0),
            decisions_allowed: AtomicU64::new(0),
            decisions_denied: AtomicU64::new(0),
            task_time: TenantTaskTimeAccounting::new(),
        }
    }

    /// The owning tenant.
    pub fn tenant(&self) -> &str {
        self.tenant.as_str()
    }

    /// Acquire this tenant's DNS budget (bounded), BEFORE the node-wide guard.
    ///
    /// `Ok(None)` when no per-tenant budget is configured. A tenant over its
    /// budget times out here — failing ITS request closed — without ever
    /// touching the shared node guard, so it cannot starve other tenants'
    /// resolver capacity. The permit must be held for the resolution's
    /// lifetime (travel it into the blocking closure, like the node permit).
    pub(crate) async fn acquire_dns(
        &self,
        wait_timeout: Duration,
    ) -> io::Result<Option<OwnedSemaphorePermit>> {
        let Some(dns) = &self.dns else {
            return Ok(None);
        };
        match tokio::time::timeout(wait_timeout, Arc::clone(dns).acquire_owned()).await {
            Ok(Ok(permit)) => Ok(Some(permit)),
            Ok(Err(_)) => Err(io::Error::other(format!(
                "per-tenant DNS budget closed for tenant {}",
                self.tenant
            ))),
            Err(_) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("per-tenant DNS budget exhausted for tenant {}", self.tenant),
            )),
        }
    }

    /// Record bytes relayed workload→upstream (attributed to this tenant).
    pub fn record_bytes_to_upstream(&self, bytes: u64) {
        self.bytes_to_upstream.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Record bytes relayed upstream→workload.
    pub fn record_bytes_to_workload(&self, bytes: u64) {
        self.bytes_to_workload.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Total bytes metered workload→upstream.
    pub fn bytes_to_upstream(&self) -> u64 {
        self.bytes_to_upstream.load(Ordering::Relaxed)
    }

    /// Total bytes metered upstream→workload.
    pub fn bytes_to_workload(&self) -> u64 {
        self.bytes_to_workload.load(Ordering::Relaxed)
    }

    /// Count an allowed egress decision (EE4 counter seam).
    pub fn record_decision_allowed(&self) {
        self.decisions_allowed.fetch_add(1, Ordering::Relaxed);
    }

    /// Count a denied egress decision.
    pub fn record_decision_denied(&self) {
        self.decisions_denied.fetch_add(1, Ordering::Relaxed);
    }

    /// Total allowed decisions counted for this tenant.
    pub fn decisions_allowed(&self) -> u64 {
        self.decisions_allowed.load(Ordering::Relaxed)
    }

    /// Total denied decisions counted for this tenant.
    pub fn decisions_denied(&self) -> u64 {
        self.decisions_denied.load(Ordering::Relaxed)
    }

    /// The tenant's task-time accounting primitive.
    pub fn task_time(&self) -> &TenantTaskTimeAccounting {
        &self.task_time
    }

    /// Open a cooperative task-time span attributed to this tenant; elapsed
    /// time is recorded when the span drops (including on error paths).
    pub fn task_time_span(self: &Arc<Self>) -> TaskTimeSpan {
        TaskTimeSpan {
            fairness: Arc::clone(self),
            started: Instant::now(),
        }
    }
}

/// Named per-tenant task-time accounting primitive (the EE3 "CPU" axis).
///
/// Records request-task wall-clock occupancy — the span covers I/O waits, so
/// this is NOT CPU-seconds (a tenant parked on a slow upstream accrues
/// occupancy at near-zero CPU). It is ACCOUNTING, not isolation: the shared
/// tokio runtime has no cgroup/CPU quota, so a hostile tenant is not
/// preempted by this primitive — it is measured by it. Real CPU-seconds and
/// hard fairness ride a scheduler/quota mechanism (TAA owns the policy).
pub struct TenantTaskTimeAccounting {
    task_nanos: AtomicU64,
}

impl TenantTaskTimeAccounting {
    fn new() -> Self {
        Self {
            task_nanos: AtomicU64::new(0),
        }
    }

    /// Record `elapsed` of cooperative task time.
    pub fn record(&self, elapsed: Duration) {
        let nanos = u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX);
        self.task_nanos.fetch_add(nanos, Ordering::Relaxed);
    }

    /// Total recorded cooperative task nanoseconds.
    pub fn task_nanos(&self) -> u64 {
        self.task_nanos.load(Ordering::Relaxed)
    }
}

/// Drop guard recording a span of cooperative task time to its tenant.
pub struct TaskTimeSpan {
    fairness: Arc<TenantFairness>,
    started: Instant,
}

impl Drop for TaskTimeSpan {
    fn drop(&mut self) {
        self.fairness.task_time.record(self.started.elapsed());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tid(raw: &str) -> TenantId {
        TenantId::new(raw).expect("test tenant id")
    }

    #[test]
    fn registry_get_or_create_shares_one_handle_per_tenant() {
        let registry = FairnessRegistry::new();
        let a1 = registry.tenant(&tid("tenant-a"));
        let a2 = registry.tenant(&tid("tenant-a"));
        let b = registry.tenant(&tid("tenant-b"));
        assert!(Arc::ptr_eq(&a1, &a2), "same tenant shares one handle");
        assert!(!Arc::ptr_eq(&a1, &b), "tenants get distinct handles");
        assert_eq!(registry.len(), 2);
    }

    #[tokio::test]
    async fn dns_budget_exhaustion_cannot_starve_another_tenant() {
        // Tenant A's budget of 1 is held; A's next acquire times out (fails
        // ITS request closed) while tenant B acquires immediately — the
        // budgets are independent, so exhaustion cannot cross tenants.
        let registry = FairnessRegistry::with_dns_permits_per_tenant(1);
        let a = registry.tenant(&tid("tenant-a"));
        let b = registry.tenant(&tid("tenant-b"));

        let held = a
            .acquire_dns(Duration::from_millis(200))
            .await
            .expect("first acquire fits the budget")
            .expect("budget configured");

        let starved = a.acquire_dns(Duration::from_millis(50)).await;
        assert!(
            matches!(&starved, Err(error) if error.kind() == io::ErrorKind::TimedOut),
            "tenant A over budget must time out, got {starved:?}"
        );

        let b_permit = b
            .acquire_dns(Duration::from_millis(200))
            .await
            .expect("tenant B unaffected by A's exhaustion")
            .expect("budget configured");
        drop(b_permit);
        drop(held);

        // A's budget frees once its resolution finishes.
        assert!(
            a.acquire_dns(Duration::from_millis(200)).await.is_ok(),
            "released budget must be reusable"
        );
    }

    #[tokio::test]
    async fn unbudgeted_registry_returns_no_dns_permit() {
        let registry = FairnessRegistry::new();
        let handle = registry.tenant(&tid("tenant-a"));
        let permit = handle
            .acquire_dns(Duration::from_millis(50))
            .await
            .expect("no budget configured means no gate");
        assert!(permit.is_none());
    }

    #[test]
    fn byte_meters_attribute_per_tenant_independently() {
        let registry = FairnessRegistry::new();
        let a = registry.tenant(&tid("tenant-a"));
        let b = registry.tenant(&tid("tenant-b"));

        a.record_bytes_to_upstream(100);
        a.record_bytes_to_workload(50);
        b.record_bytes_to_upstream(7);

        assert_eq!(a.bytes_to_upstream(), 100);
        assert_eq!(a.bytes_to_workload(), 50);
        assert_eq!(b.bytes_to_upstream(), 7);
        assert_eq!(
            b.bytes_to_workload(),
            0,
            "one tenant's traffic must never appear on another's meter"
        );
    }

    #[test]
    fn task_time_span_records_on_drop_per_tenant() {
        let registry = FairnessRegistry::new();
        let a = registry.tenant(&tid("tenant-a"));
        let b = registry.tenant(&tid("tenant-b"));

        {
            let _span = a.task_time_span();
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(
            a.task_time().task_nanos() > 0,
            "span must record elapsed task time on drop"
        );
        assert_eq!(
            b.task_time().task_nanos(),
            0,
            "accounting must not leak across tenants"
        );
    }
}
