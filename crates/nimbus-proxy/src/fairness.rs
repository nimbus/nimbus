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
//! - **CPU**: [`TenantCpuAccounting`] — a named accounting primitive that
//!   records cooperative task time per tenant. There is no cgroup/CPU quota on
//!   the shared runtime today, so this is deliberately accounting, NOT
//!   preemptive CPU isolation; hard CPU fairness is scoped to this primitive
//!   and not claimed beyond it.
//!
//! Identity follows the capture-at-registration principle: the sandbox layer
//! resolves its tenant's [`TenantFairness`] handle once, at PEP registration,
//! and the handle is captured into the PEP's context — the request path never
//! performs a per-request tenant lookup and never touches the registry map.

use std::collections::HashMap;
use std::io;
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
    tenants: Mutex<HashMap<String, Arc<TenantFairness>>>,
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
    pub fn tenant(&self, tenant: &str) -> Arc<TenantFairness> {
        let mut map = match self.tenants.lock() {
            Ok(map) => map,
            Err(_) => {
                return Arc::new(TenantFairness::new(
                    tenant.to_owned(),
                    self.dns_permits_per_tenant,
                ));
            }
        };
        Arc::clone(map.entry(tenant.to_owned()).or_insert_with(|| {
            Arc::new(TenantFairness::new(
                tenant.to_owned(),
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
    tenant: String,
    dns: Option<Arc<Semaphore>>,
    bytes_to_upstream: AtomicU64,
    bytes_to_workload: AtomicU64,
    cpu: TenantCpuAccounting,
}

impl TenantFairness {
    fn new(tenant: String, dns_permits: Option<usize>) -> Self {
        Self {
            tenant,
            dns: dns_permits.map(|permits| Arc::new(Semaphore::new(permits))),
            bytes_to_upstream: AtomicU64::new(0),
            bytes_to_workload: AtomicU64::new(0),
            cpu: TenantCpuAccounting::new(),
        }
    }

    /// The owning tenant.
    pub fn tenant(&self) -> &str {
        &self.tenant
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

    /// The tenant's CPU accounting primitive.
    pub fn cpu(&self) -> &TenantCpuAccounting {
        &self.cpu
    }

    /// Open a cooperative task-time span attributed to this tenant; elapsed
    /// time is recorded when the span drops (including on error paths).
    pub fn cpu_span(self: &Arc<Self>) -> CpuAccountingSpan {
        CpuAccountingSpan {
            fairness: Arc::clone(self),
            started: Instant::now(),
        }
    }
}

/// Named per-tenant CPU accounting primitive (EE3).
///
/// Records cooperative task time. This is ACCOUNTING, not isolation: the
/// shared tokio runtime has no cgroup/CPU quota, so a hostile tenant is not
/// preempted by this primitive — it is measured by it. Hard CPU fairness, if
/// ever claimed, must be built ON this primitive plus a real scheduler/quota
/// mechanism (TAA owns the policy).
pub struct TenantCpuAccounting {
    task_nanos: AtomicU64,
}

impl TenantCpuAccounting {
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
pub struct CpuAccountingSpan {
    fairness: Arc<TenantFairness>,
    started: Instant,
}

impl Drop for CpuAccountingSpan {
    fn drop(&mut self) {
        self.fairness.cpu.record(self.started.elapsed());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_get_or_create_shares_one_handle_per_tenant() {
        let registry = FairnessRegistry::new();
        let a1 = registry.tenant("tenant-a");
        let a2 = registry.tenant("tenant-a");
        let b = registry.tenant("tenant-b");
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
        let a = registry.tenant("tenant-a");
        let b = registry.tenant("tenant-b");

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
        let handle = registry.tenant("tenant-a");
        let permit = handle
            .acquire_dns(Duration::from_millis(50))
            .await
            .expect("no budget configured means no gate");
        assert!(permit.is_none());
    }

    #[test]
    fn byte_meters_attribute_per_tenant_independently() {
        let registry = FairnessRegistry::new();
        let a = registry.tenant("tenant-a");
        let b = registry.tenant("tenant-b");

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
    fn cpu_span_records_on_drop_per_tenant() {
        let registry = FairnessRegistry::new();
        let a = registry.tenant("tenant-a");
        let b = registry.tenant("tenant-b");

        {
            let _span = a.cpu_span();
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(
            a.cpu().task_nanos() > 0,
            "span must record elapsed task time on drop"
        );
        assert_eq!(
            b.cpu().task_nanos(),
            0,
            "accounting must not leak across tenants"
        );
    }
}
