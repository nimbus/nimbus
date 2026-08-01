use std::collections::{BTreeMap, VecDeque};

use nimbus_core::{Error, TenantId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesiredWorkloadState {
    Running,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesiredWorkloadKind {
    Service,
    Sandbox,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesiredWorkload {
    tenant_id: TenantId,
    workload_id: String,
    kind: DesiredWorkloadKind,
    desired_state: DesiredWorkloadState,
    generation: u64,
    binding_key: Option<String>,
}

impl DesiredWorkload {
    pub fn service(
        tenant_id: TenantId,
        service_name: impl Into<String>,
        desired_state: DesiredWorkloadState,
        generation: u64,
    ) -> Result<Self, Error> {
        let service_name = validate_component("service name", service_name)?;
        Ok(Self {
            tenant_id,
            workload_id: format!("service:{service_name}"),
            kind: DesiredWorkloadKind::Service,
            desired_state,
            generation,
            binding_key: Some(format!("service:{service_name}")),
        })
    }

    pub fn sandbox(
        tenant_id: TenantId,
        sandbox_id: impl Into<String>,
        desired_state: DesiredWorkloadState,
        generation: u64,
    ) -> Result<Self, Error> {
        let sandbox_id = validate_component("sandbox id", sandbox_id)?;
        Ok(Self {
            tenant_id,
            workload_id: format!("sandbox:{sandbox_id}"),
            kind: DesiredWorkloadKind::Sandbox,
            desired_state,
            generation,
            binding_key: Some(format!("sandbox:{sandbox_id}")),
        })
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn workload_id(&self) -> &str {
        &self.workload_id
    }

    pub fn kind(&self) -> DesiredWorkloadKind {
        self.kind
    }

    pub fn desired_state(&self) -> DesiredWorkloadState {
        self.desired_state
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn binding_key(&self) -> Option<&str> {
        self.binding_key.as_deref()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DesiredWorkloadSnapshot {
    workloads: BTreeMap<(TenantId, String), DesiredWorkload>,
}

impl DesiredWorkloadSnapshot {
    pub fn workloads(&self) -> impl Iterator<Item = &DesiredWorkload> {
        self.workloads.values()
    }

    pub fn len(&self) -> usize {
        self.workloads.len()
    }

    pub fn is_empty(&self) -> bool {
        self.workloads.is_empty()
    }
}

pub trait DesiredWorkloadStore {
    fn upsert_desired_workload(&mut self, workload: DesiredWorkload) -> DesiredWorkload;
    fn desired_workload(&self, tenant_id: &TenantId, workload_id: &str) -> Option<DesiredWorkload>;
    fn snapshot_desired_workloads(&self) -> DesiredWorkloadSnapshot;
    fn restore_desired_workloads(&mut self, snapshot: DesiredWorkloadSnapshot);
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InMemoryDesiredWorkloadStore {
    workloads: BTreeMap<(TenantId, String), DesiredWorkload>,
}

impl DesiredWorkloadStore for InMemoryDesiredWorkloadStore {
    fn upsert_desired_workload(&mut self, workload: DesiredWorkload) -> DesiredWorkload {
        let key = (workload.tenant_id.clone(), workload.workload_id.clone());
        self.workloads.insert(key, workload.clone());
        workload
    }

    fn desired_workload(&self, tenant_id: &TenantId, workload_id: &str) -> Option<DesiredWorkload> {
        self.workloads
            .get(&(tenant_id.clone(), workload_id.to_owned()))
            .cloned()
    }

    fn snapshot_desired_workloads(&self) -> DesiredWorkloadSnapshot {
        DesiredWorkloadSnapshot {
            workloads: self.workloads.clone(),
        }
    }

    fn restore_desired_workloads(&mut self, snapshot: DesiredWorkloadSnapshot) {
        self.workloads = snapshot.workloads;
    }
}

#[derive(Debug)]
pub struct WorkloadController<S> {
    store: S,
}

impl<S> WorkloadController<S>
where
    S: DesiredWorkloadStore,
{
    pub fn new(store: S) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut S {
        &mut self.store
    }

    pub fn record_desired_workload(&mut self, workload: DesiredWorkload) -> DesiredWorkload {
        self.store.upsert_desired_workload(workload)
    }

    pub fn snapshot(&self) -> DesiredWorkloadSnapshot {
        self.store.snapshot_desired_workloads()
    }

    pub fn restore(&mut self, snapshot: DesiredWorkloadSnapshot) {
        self.store.restore_desired_workloads(snapshot);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadEvaluation {
    workload_id: String,
    observed_generation: u64,
    reason: String,
    ready_after_millis: u64,
}

impl WorkloadEvaluation {
    pub fn new(
        workload_id: impl Into<String>,
        observed_generation: u64,
        reason: impl Into<String>,
    ) -> Result<Self, Error> {
        Ok(Self {
            workload_id: validate_component("workload id", workload_id)?,
            observed_generation,
            reason: validate_component("evaluation reason", reason)?,
            ready_after_millis: 0,
        })
    }

    pub fn workload_id(&self) -> &str {
        &self.workload_id
    }

    pub fn observed_generation(&self) -> u64 {
        self.observed_generation
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    pub fn ready_after_millis(&self) -> u64 {
        self.ready_after_millis
    }
}

#[derive(Debug, Default)]
pub struct WorkloadEventQueue {
    queue: VecDeque<WorkloadEvaluation>,
}

impl WorkloadEventQueue {
    pub fn push(&mut self, evaluation: WorkloadEvaluation) {
        self.queue.push_back(evaluation);
    }

    pub fn pop_ready(&mut self, now_millis: u64) -> Option<WorkloadEvaluation> {
        let ready = self
            .queue
            .front()
            .is_some_and(|evaluation| evaluation.ready_after_millis <= now_millis);
        if ready { self.queue.pop_front() } else { None }
    }

    pub fn requeue_with_reason(
        &mut self,
        mut evaluation: WorkloadEvaluation,
        reason: impl Into<String>,
    ) -> Result<(), Error> {
        evaluation.reason = validate_component("requeue reason", reason)?;
        self.queue.push_back(evaluation);
        Ok(())
    }

    pub fn requeue_after(
        &mut self,
        mut evaluation: WorkloadEvaluation,
        reason: impl Into<String>,
        ready_after_millis: u64,
    ) -> Result<(), Error> {
        evaluation.reason = validate_component("requeue reason", reason)?;
        evaluation.ready_after_millis = ready_after_millis;
        self.queue.push_back(evaluation);
        Ok(())
    }

    pub fn reject_stale_snapshot(
        &mut self,
        evaluation: WorkloadEvaluation,
        current_generation: u64,
    ) -> Result<Option<WorkloadEvaluation>, Error> {
        if evaluation.observed_generation < current_generation {
            self.requeue_with_reason(evaluation, "stale_snapshot")?;
            Ok(None)
        } else {
            Ok(Some(evaluation))
        }
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadExecutionPhase {
    Pending,
    Ready,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadExecutionStatus {
    workload_id: String,
    phase: WorkloadExecutionPhase,
}

impl WorkloadExecutionStatus {
    pub fn ready(workload_id: impl Into<String>) -> Result<Self, Error> {
        Ok(Self {
            workload_id: validate_component("workload id", workload_id)?,
            phase: WorkloadExecutionPhase::Ready,
        })
    }

    pub fn stopped(workload_id: impl Into<String>) -> Result<Self, Error> {
        Ok(Self {
            workload_id: validate_component("workload id", workload_id)?,
            phase: WorkloadExecutionPhase::Stopped,
        })
    }

    pub fn workload_id(&self) -> &str {
        &self.workload_id
    }

    pub fn phase(&self) -> WorkloadExecutionPhase {
        self.phase
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadChannelDescriptor {
    workload_id: String,
    channel: String,
}

impl WorkloadChannelDescriptor {
    pub fn new(workload_id: impl Into<String>, channel: impl Into<String>) -> Result<Self, Error> {
        Ok(Self {
            workload_id: validate_component("workload id", workload_id)?,
            channel: validate_component("channel", channel)?,
        })
    }

    pub fn workload_id(&self) -> &str {
        &self.workload_id
    }

    pub fn channel(&self) -> &str {
        &self.channel
    }
}

pub trait WorkloadExecutor {
    fn start(&mut self, desired: &DesiredWorkload) -> Result<WorkloadExecutionStatus, Error>;
    fn stop(&mut self, workload_id: &str) -> Result<WorkloadExecutionStatus, Error>;
    fn inspect(&self, workload_id: &str) -> Result<WorkloadExecutionStatus, Error>;
    fn open_channel(
        &mut self,
        workload_id: &str,
        channel: &str,
    ) -> Result<WorkloadChannelDescriptor, Error>;
}

#[derive(Debug)]
pub struct EmbeddedNodeClient<E> {
    executor: E,
}

impl<E> EmbeddedNodeClient<E>
where
    E: WorkloadExecutor,
{
    pub fn new(executor: E) -> Self {
        Self { executor }
    }

    pub fn start(&mut self, desired: &DesiredWorkload) -> Result<WorkloadExecutionStatus, Error> {
        self.executor.start(desired)
    }

    pub fn stop(&mut self, workload_id: &str) -> Result<WorkloadExecutionStatus, Error> {
        self.executor.stop(workload_id)
    }

    pub fn inspect(&self, workload_id: &str) -> Result<WorkloadExecutionStatus, Error> {
        self.executor.inspect(workload_id)
    }

    pub fn open_channel(
        &mut self,
        workload_id: &str,
        channel: &str,
    ) -> Result<WorkloadChannelDescriptor, Error> {
        self.executor.open_channel(workload_id, channel)
    }
}

pub(crate) fn validate_component(label: &str, value: impl Into<String>) -> Result<String, Error> {
    let value = value.into();
    if value.trim() != value || value.is_empty() {
        return Err(Error::InvalidInput(format!(
            "{label} must be non-empty and must not have leading or trailing whitespace"
        )));
    }
    if value.contains('/') || value.contains('\0') || value.contains('\n') {
        return Err(Error::InvalidInput(format!(
            "{label} must not contain path separators or control characters"
        )));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant_id() -> TenantId {
        TenantId::new("tenant-a").expect("tenant id should parse")
    }

    #[test]
    fn desired_workload_replay_after_restart() {
        let service =
            DesiredWorkload::service(tenant_id(), "api", DesiredWorkloadState::Running, 3)
                .expect("desired service should build");
        let mut controller = WorkloadController::new(InMemoryDesiredWorkloadStore::default());
        controller.record_desired_workload(service.clone());
        let snapshot = controller.snapshot();

        let mut restarted = WorkloadController::new(InMemoryDesiredWorkloadStore::default());
        restarted.restore(snapshot);
        let replayed = restarted
            .store()
            .desired_workload(service.tenant_id(), service.workload_id())
            .expect("desired workload should replay after restart");

        assert_eq!(replayed, service);
    }

    #[test]
    fn stale_snapshot_requeues_workload_evaluation() {
        let mut queue = WorkloadEventQueue::default();
        let evaluation =
            WorkloadEvaluation::new("service:api", 2, "status_update").expect("eval should build");

        let accepted = queue
            .reject_stale_snapshot(evaluation, 3)
            .expect("stale check should complete");

        assert!(accepted.is_none());
        assert_eq!(queue.len(), 1);
        let requeued = queue
            .pop_ready(0)
            .expect("stale snapshot should be requeued immediately");
        assert_eq!(requeued.reason(), "stale_snapshot");
    }

    #[test]
    fn reservation_expiry_unblocks_workload() {
        let mut queue = WorkloadEventQueue::default();
        let evaluation =
            WorkloadEvaluation::new("service:api", 1, "reservation").expect("eval should build");
        queue
            .requeue_after(evaluation, "reservation_active", 100)
            .expect("reservation requeue should succeed");

        assert!(queue.pop_ready(99).is_none());
        let ready = queue
            .pop_ready(100)
            .expect("reservation expiry should unblock evaluation");
        assert_eq!(ready.reason(), "reservation_active");
    }

    #[test]
    fn binding_conflict_requeues_with_reason() {
        let mut queue = WorkloadEventQueue::default();
        let evaluation =
            WorkloadEvaluation::new("service:api", 1, "placement").expect("eval should build");
        queue
            .requeue_with_reason(evaluation, "binding_conflict")
            .expect("binding conflict should requeue");

        let requeued = queue.pop_ready(0).expect("conflict should be ready");
        assert_eq!(requeued.reason(), "binding_conflict");
    }

    #[derive(Default)]
    struct FakeRemoteWorkloadExecutor {
        statuses: BTreeMap<String, WorkloadExecutionStatus>,
        opened_channels: Vec<WorkloadChannelDescriptor>,
    }

    impl WorkloadExecutor for FakeRemoteWorkloadExecutor {
        fn start(&mut self, desired: &DesiredWorkload) -> Result<WorkloadExecutionStatus, Error> {
            let status = WorkloadExecutionStatus::ready(desired.workload_id())?;
            self.statuses
                .insert(desired.workload_id().to_owned(), status.clone());
            Ok(status)
        }

        fn stop(&mut self, workload_id: &str) -> Result<WorkloadExecutionStatus, Error> {
            let status = WorkloadExecutionStatus::stopped(workload_id)?;
            self.statuses.insert(workload_id.to_owned(), status.clone());
            Ok(status)
        }

        fn inspect(&self, workload_id: &str) -> Result<WorkloadExecutionStatus, Error> {
            self.statuses
                .get(workload_id)
                .cloned()
                .ok_or_else(|| Error::NotFound(format!("workload `{workload_id}` was not found")))
        }

        fn open_channel(
            &mut self,
            workload_id: &str,
            channel: &str,
        ) -> Result<WorkloadChannelDescriptor, Error> {
            self.inspect(workload_id)?;
            let descriptor = WorkloadChannelDescriptor::new(workload_id, channel)?;
            self.opened_channels.push(descriptor.clone());
            Ok(descriptor)
        }
    }

    #[test]
    fn fake_remote_executor_lifecycle_reaches_ready() {
        let desired =
            DesiredWorkload::sandbox(tenant_id(), "desktop-1", DesiredWorkloadState::Running, 1)
                .expect("desired sandbox should build");
        let mut client = EmbeddedNodeClient::new(FakeRemoteWorkloadExecutor::default());

        let started = client.start(&desired).expect("fake executor should start");
        assert_eq!(started.workload_id(), "sandbox:desktop-1");
        assert_eq!(started.phase(), WorkloadExecutionPhase::Ready);
        let inspected = client
            .inspect(desired.workload_id())
            .expect("ready workload should inspect");
        assert_eq!(inspected.phase(), WorkloadExecutionPhase::Ready);
        let channel = client
            .open_channel(desired.workload_id(), "stdio")
            .expect("ready workload should open channel");
        assert_eq!(channel.channel(), "stdio");
    }
}
