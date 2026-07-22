use std::collections::HashSet;
use std::num::NonZeroU64;
use std::sync::Mutex as StdMutex;

use tokio::sync::Notify;

use super::*;
use crate::host::{HostBridge, HostBridgeFuture, HostCallOperation, HostCallRequest};

#[derive(Default)]
struct RetirementBarrierHost {
    started: StdMutex<Vec<String>>,
    started_notify: Arc<Notify>,
    released: Arc<StdMutex<HashSet<String>>>,
    release_notify: Arc<Notify>,
}

impl RetirementBarrierHost {
    async fn wait_until_started(&self, id: &str) {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let notified = self.started_notify.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if self
                    .started
                    .lock()
                    .expect("retirement host started lock should not be poisoned")
                    .iter()
                    .any(|started| started == id)
                {
                    return;
                }
                notified.await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("host call {id} should start"));
    }

    fn release(&self, id: &str) {
        self.released
            .lock()
            .expect("retirement host release lock should not be poisoned")
            .insert(id.to_string());
        self.release_notify.notify_waiters();
    }

    fn started(&self, id: &str) -> bool {
        self.started
            .lock()
            .expect("retirement host started lock should not be poisoned")
            .iter()
            .any(|started| started == id)
    }
}

impl HostBridge for RetirementBarrierHost {
    fn call(&self, _request: HostCallRequest) -> Result<Value> {
        Err(NimbusRuntimeError::Contract(
            "retirement barrier host expects async db.get".to_string(),
        ))
    }

    fn call_async(
        &self,
        request: HostCallRequest,
        cancellation: HostCallCancellation,
    ) -> HostBridgeFuture {
        assert_eq!(request.operation, HostCallOperation::DocumentGet);
        let id = request
            .payload
            .get("id")
            .and_then(Value::as_str)
            .expect("db.get payload should contain id")
            .to_string();
        self.started
            .lock()
            .expect("retirement host started lock should not be poisoned")
            .push(id.clone());
        self.started_notify.notify_waiters();
        let released = self.released.clone();
        let release_notify = self.release_notify.clone();
        Box::pin(async move {
            loop {
                let release = release_notify.notified();
                tokio::pin!(release);
                release.as_mut().enable();
                if released
                    .lock()
                    .expect("retirement host release lock should not be poisoned")
                    .contains(&id)
                {
                    return Ok(json!({
                        "status": "ok",
                        "value": { "id": id },
                    }));
                }
                tokio::select! {
                    () = cancellation.cancelled() => return Err(NimbusRuntimeError::Cancelled),
                    () = &mut release => {}
                }
            }
        })
    }
}

struct TestAuthority {
    owner: crate::RuntimeOwnerLease,
    owner_revocation: crate::RuntimeOwnerRevocation,
    deployment: crate::RuntimeDeploymentAuthorityLease,
    deployment_revocation: crate::RuntimeDeploymentAuthorityRevocation,
}

fn authority(subject: &str, incarnation: u64, deployment_generation: u64) -> TestAuthority {
    let owner_id = crate::RuntimeOwnerId::tenant(
        subject,
        NonZeroU64::new(incarnation).expect("test incarnation is nonzero"),
        Some(subject),
    )
    .expect("test owner should build");
    let (owner, owner_revocation) = crate::RuntimeOwnerLeaseIssuer.issue(owner_id);
    let deployment_id = crate::RuntimeDeploymentAuthorityId::new(
        "executor-retirement-test",
        NonZeroU64::new(deployment_generation).expect("test deployment generation is nonzero"),
    )
    .expect("test deployment authority should build");
    let (deployment, deployment_revocation) =
        crate::RuntimeDeploymentAuthorityLeaseIssuer.issue(deployment_id);
    TestAuthority {
        owner,
        owner_revocation,
        deployment,
        deployment_revocation,
    }
}

fn context(
    request: &InvocationRequest,
    tenant: &str,
    authority: &TestAuthority,
) -> RuntimeInvocationContext {
    RuntimeInvocationContext::top_level_for_tenant_with_owner(
        request,
        tenant,
        authority.owner.clone(),
    )
    .with_deployment_authority(authority.deployment.clone())
}

fn retirement_policy() -> Arc<RuntimePolicy> {
    let mut limits = cooperative_warm_pool_runtime_test_limits();
    limits.worker_threads = 1;
    limits.max_concurrent_runtime_instances = 1;
    limits.max_active_top_level_invocations_per_tenant = 1;
    limits.max_in_flight_top_level_invocations_per_tenant = 1;
    limits.max_queued_top_level_invocations_per_tenant = 4;
    Arc::new(RuntimePolicy::new(limits))
}

async fn wait_for_admission_queue(executor: &RuntimeExecutor, count: usize) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if executor.inner.admission.queued_job_count_for_test() == count {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("runtime admission queue should reach expected depth");
}

async fn wait_for_worker_dispatches(policy: &RuntimePolicy, count: u64) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if policy.metrics_snapshot().worker_dispatched_invocations >= count {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("runtime worker dispatch count should advance");
}

#[tokio::test]
async fn owner_retirement_cancels_active_guest_and_queued_job_before_entry() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
    let policy = retirement_policy();
    let executor = RuntimeExecutor::new(policy.clone());
    let host = Arc::new(RetirementBarrierHost::default());
    let authority = authority("retire-active-owner", 1, 1);

    let active_request = test_request("slow-active");
    let active = {
        let executor = executor.clone();
        let policy = policy.clone();
        let host = host.clone();
        let bundle_path = bundle_path.clone();
        let context = context(&active_request, "retire-active-owner", &authority);
        tokio::spawn(async move {
            executor
                .invoke_on_worker(
                    NimbusRuntime::with_policy(
                        host,
                        policy,
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    ),
                    RuntimeBundle::new(bundle_path),
                    active_request,
                    context,
                    None,
                )
                .await
        })
    };
    host.wait_until_started("slow-active").await;

    let queued_request = test_request("must-not-enter-guest");
    let queued = {
        let executor = executor.clone();
        let policy = policy.clone();
        let host = host.clone();
        let bundle_path = bundle_path.clone();
        let context = context(&queued_request, "retire-active-owner", &authority);
        tokio::spawn(async move {
            executor
                .invoke_on_worker(
                    NimbusRuntime::with_policy(
                        host,
                        policy,
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    ),
                    RuntimeBundle::new(bundle_path),
                    queued_request,
                    context,
                    None,
                )
                .await
        })
    };
    wait_for_admission_queue(&executor, 1).await;

    let report = executor
        .retire_owner(&authority.owner_revocation, Duration::from_secs(10))
        .await
        .expect("owner retirement should acknowledge its worker");
    assert_eq!(report.workers_acknowledged, 1);
    assert!(report.invocations_cancelled >= 2);
    assert!(matches!(
        active.await.expect("active task should join"),
        Err(NimbusRuntimeError::Cancelled)
    ));
    assert!(matches!(
        queued.await.expect("queued task should join"),
        Err(NimbusRuntimeError::Cancelled)
    ));
    assert!(!host.started("must-not-enter-guest"));
    assert_eq!(policy.metrics_snapshot().active_runtime_instances, 0);
}

#[tokio::test]
async fn owner_retirement_cancels_direct_executor_invocation_and_waits_for_drain() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
    let policy = retirement_policy();
    let executor = RuntimeExecutor::new(policy.clone());
    let host = Arc::new(RetirementBarrierHost::default());
    let authority = authority("retire-direct-owner", 1, 1);
    let request = test_request("slow-direct");
    let direct = executor.invoke(
        NimbusRuntime::with_policy(
            host.clone(),
            policy,
            crate::RuntimeEgressPosture::CoarsePermissions,
        ),
        RuntimeBundle::new(bundle_path),
        request.clone(),
        context(&request, "retire-direct-owner", &authority),
    );
    let retirement = async {
        host.wait_until_started("slow-direct").await;
        executor
            .retire_owner(&authority.owner_revocation, Duration::from_secs(10))
            .await
    };
    let (direct, report) = tokio::join!(direct, retirement);
    let report = report.expect("direct owner retirement should acknowledge and drain");
    assert_eq!(report.workers_acknowledged, 1);
    assert_eq!(report.invocations_cancelled, 1);
    assert!(matches!(direct, Err(NimbusRuntimeError::Cancelled)));
}

#[tokio::test]
async fn owner_retirement_purges_only_matching_routing_locality() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let (_bundle_dir, bundle_path) = write_constant_bundle();
    let policy = retirement_policy();
    let executor = RuntimeExecutor::new(policy.clone());
    let owner_a = authority("affinity-owner-a", 1, 1);
    let owner_b = authority("affinity-owner-b", 1, 1);

    for (tenant, authority) in [
        ("affinity-owner-a", &owner_a),
        ("affinity-owner-b", &owner_b),
    ] {
        let request = test_request("messages:list");
        executor
            .invoke_on_worker(
                NimbusRuntime::with_policy(
                    Arc::new(NoopHost),
                    policy.clone(),
                    crate::RuntimeEgressPosture::CoarsePermissions,
                ),
                RuntimeBundle::new(&bundle_path),
                request.clone(),
                context(&request, tenant, authority),
                None,
            )
            .await
            .expect("affinity fixture invocation should succeed");
    }
    assert_eq!(policy.metrics_snapshot().worker_affinity_cache_entries, 2);

    let report = executor
        .retire_owner(&owner_a.owner_revocation, Duration::from_secs(10))
        .await
        .expect("owner retirement should purge matching affinity");
    assert_eq!(report.affinity_entries_purged, 1);
    assert_eq!(policy.metrics_snapshot().worker_affinity_cache_entries, 1);
}

#[tokio::test]
async fn owner_retirement_after_worker_dispatch_prevents_guest_entry() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
    let policy = retirement_policy();
    let executor = RuntimeExecutor::new(policy.clone());
    let host = Arc::new(RetirementBarrierHost::default());
    let blocker_authority = authority("dispatch-blocker", 1, 1);
    let retired_authority = authority("dispatched-retired", 1, 1);

    let blocker_request = test_request("slow-blocker");
    let blocker = {
        let executor = executor.clone();
        let host = host.clone();
        let policy = policy.clone();
        let bundle_path = bundle_path.clone();
        let context = context(&blocker_request, "dispatch-blocker", &blocker_authority);
        tokio::spawn(async move {
            executor
                .invoke_on_worker(
                    NimbusRuntime::with_policy(
                        host,
                        policy,
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    ),
                    RuntimeBundle::new(bundle_path),
                    blocker_request,
                    context,
                    None,
                )
                .await
        })
    };
    host.wait_until_started("slow-blocker").await;

    let target_request = test_request("dispatched-before-retirement");
    let target = {
        let executor = executor.clone();
        let host = host.clone();
        let policy = policy.clone();
        let bundle_path = bundle_path.clone();
        let context = context(&target_request, "dispatched-retired", &retired_authority);
        tokio::spawn(async move {
            executor
                .invoke_on_worker(
                    NimbusRuntime::with_policy(
                        host,
                        policy,
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    ),
                    RuntimeBundle::new(bundle_path),
                    target_request,
                    context,
                    None,
                )
                .await
        })
    };
    wait_for_worker_dispatches(&policy, 2).await;

    let retirement = {
        let executor = executor.clone();
        let revocation = retired_authority.owner_revocation.clone();
        tokio::spawn(async move {
            executor
                .retire_owner(&revocation, Duration::from_secs(10))
                .await
        })
    };
    host.release("slow-blocker");
    assert!(blocker.await.expect("blocker task should join").is_ok());
    assert!(matches!(
        target.await.expect("target task should join"),
        Err(NimbusRuntimeError::Cancelled)
    ));
    let report = retirement
        .await
        .expect("retirement task should join")
        .expect("retirement should acknowledge");
    assert_eq!(report.workers_acknowledged, 1);
    assert!(!host.started("dispatched-before-retirement"));
}

#[tokio::test]
async fn deployment_retirement_cancels_queued_work_but_drains_checked_out_generation() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
    let policy = retirement_policy();
    let executor = RuntimeExecutor::new(policy.clone());
    let host = Arc::new(RetirementBarrierHost::default());
    let old = authority("deployment-drain-owner", 1, 7);

    let active_request = test_request("slow-old-generation");
    let active = {
        let executor = executor.clone();
        let policy = policy.clone();
        let host = host.clone();
        let bundle_path = bundle_path.clone();
        let context = context(&active_request, "deployment-drain-owner", &old);
        tokio::spawn(async move {
            executor
                .invoke_on_worker(
                    NimbusRuntime::with_policy(
                        host,
                        policy,
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    ),
                    RuntimeBundle::new(bundle_path),
                    active_request,
                    context,
                    None,
                )
                .await
        })
    };
    host.wait_until_started("slow-old-generation").await;

    let queued_request = test_request("queued-old-generation");
    let queued = {
        let executor = executor.clone();
        let policy = policy.clone();
        let host = host.clone();
        let bundle_path = bundle_path.clone();
        let context = context(&queued_request, "deployment-drain-owner", &old);
        tokio::spawn(async move {
            executor
                .invoke_on_worker(
                    NimbusRuntime::with_policy(
                        host,
                        policy,
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    ),
                    RuntimeBundle::new(bundle_path),
                    queued_request,
                    context,
                    None,
                )
                .await
        })
    };
    wait_for_admission_queue(&executor, 1).await;

    let retirement = {
        let executor = executor.clone();
        let revocation = old.deployment_revocation.clone();
        tokio::spawn(async move {
            executor
                .retire_deployment_authority(&revocation, Duration::from_secs(10))
                .await
        })
    };
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(2), queued)
            .await
            .expect("queued old generation should cancel promptly")
            .expect("queued task should join"),
        Err(NimbusRuntimeError::Cancelled)
    ));
    assert!(!host.started("queued-old-generation"));
    host.release("slow-old-generation");
    assert!(
        active
            .await
            .expect("active old-generation task should join")
            .is_ok(),
        "already executing old-generation work may drain"
    );
    let report = retirement
        .await
        .expect("deployment retirement task should join")
        .expect("deployment retirement should acknowledge");
    assert_eq!(report.workers_acknowledged, 1);
    assert_eq!(report.invocations_cancelled, 1);
    let snapshot = policy.metrics_snapshot();
    assert_eq!(snapshot.retained_runtime_pool_entries, 0);
    assert_eq!(snapshot.retained_owners.return_after_revoke_discards, 1);
}

#[tokio::test]
async fn owner_retirement_cancels_response_ready_background_drain() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let (_bundle_dir, bundle_path) = write_wait_until_bundle();
    let policy = retirement_policy();
    let executor = RuntimeExecutor::new(policy.clone());
    let host = Arc::new(RetirementBarrierHost::default());
    let authority = authority("response-ready-retirement", 1, 1);
    let request = test_request("messages:http_action");

    let response_task = tokio::spawn({
        let executor = executor.clone();
        let host = host.clone();
        let policy = policy.clone();
        let request = request.clone();
        let context = context(&request, "response-ready-retirement", &authority);
        async move {
            executor
                .invoke_on_worker_response_ready(
                    NimbusRuntime::with_policy(
                        host,
                        policy,
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    ),
                    RuntimeBundle::new(bundle_path),
                    request,
                    context,
                    None,
                )
                .await
        }
    });
    host.wait_until_started("response").await;
    host.release("response");
    let response = response_task
        .await
        .expect("response-ready task should join")
        .expect("response-ready invocation should publish its response");
    assert_eq!(response.response(), &json!({ "responseReady": true }));
    host.wait_until_started("slow-background").await;

    let report = executor
        .retire_owner(&authority.owner_revocation, Duration::from_secs(10))
        .await
        .expect("response-ready owner retirement should acknowledge");
    assert_eq!(report.workers_acknowledged, 1);
    assert!(
        response.wait_until_complete().await.is_err(),
        "revoked background drain must not complete as reusable work"
    );
    assert_eq!(policy.metrics_snapshot().retained_runtime_pool_entries, 0);
}

#[tokio::test]
async fn retirement_acknowledgement_timeout_is_fail_closed_and_counted() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let policy = retirement_policy();
    let executor = RuntimeExecutor::new(policy.clone());
    let authority = authority("retirement-timeout", 1, 1);

    let error = executor
        .retire_owner(&authority.owner_revocation, Duration::ZERO)
        .await
        .expect_err("zero-budget retirement must fail closed");
    assert!(matches!(
        error,
        NimbusRuntimeError::RetirementTimeout { .. }
    ));
    assert_eq!(
        policy
            .metrics_snapshot()
            .retained_owners
            .retirement_acknowledgement_failures,
        1
    );
}
