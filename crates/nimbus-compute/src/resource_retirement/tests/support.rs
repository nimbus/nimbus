use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use nimbus_core::{TenantId, WorkloadId};
use nimbus_network::{
    NetworkAddressFamily, NetworkAttachmentProviderRegistration, NetworkBindRealmKind,
    NetworkCapabilityBundle, NetworkCapabilityRegistry, NetworkCapabilitySelection,
    NetworkControlPlaneLocality, NetworkEndpointCapabilitySet, NetworkExposure,
    NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
    NetworkIngressProviderRegistration, NetworkLifecycleCapabilitySet, NetworkLifecycleFeature,
    NetworkPortAssignmentMode, NetworkProviderId, NetworkResourceGeneration,
    NetworkSovereigntyCapabilities, NetworkSovereigntyRequirements, PortProtocol,
};
use nimbus_sandbox::{
    ProviderCommandAttemptJournal, SandboxBackendKind, SandboxHandle, SandboxId, SandboxInspection,
    SandboxOwnerSpec, SandboxProcessSpec, SandboxRootSpec, SandboxSpec,
};
use nimbus_services::{EmptyServiceDefinitionCatalog, ServiceBackend, ServiceManager};
use nimbus_tenant::{TenantIsolationContext, WorkloadLocation};
use nimbus_workloads::{
    DesiredWorkloadState, WorkloadGeneration, WorkloadOwnerEvidenceDigest,
    WorkloadProvisionInspectionResult, WorkloadRestartDisposition, WorkloadRestartStep,
    WorkloadSagaCommit, WorkloadSagaExpected, WorkloadSagaFuture, WorkloadSagaKey,
    WorkloadSagaPage, WorkloadSagaPageRequest, WorkloadSagaPhase, WorkloadSagaRecord,
    WorkloadSagaStore, WorkloadSagaStoreError, WorkloadSagaTenantPage,
    WorkloadSagaTenantPageRequest, WorkloadTeardownCommandMode, WorkloadTeardownStep,
    WorkloadTeardownSubjects, WorkloadTeardownSuccessEvidence,
};
use tokio::sync::Semaphore;

use super::super::ComputeResourceRetirer;
use crate::embedded_local_node_identity;
use crate::resource_provision::ComputeResourceProvisioner;
use crate::workload_projection::ServiceManagerWorkloadProjectionSink;
use crate::workload_provision_source::ServiceManagerWorkloadProvisionSourceAuthority;
use crate::workload_provisioner::{WorkloadProvisionCancellation, WorkloadProvisioner};
use crate::workload_saga::restart_provider_command::{
    ProviderRestartEffectObservation, ProviderRestartPhaseAdapter,
};
use crate::workload_saga::restart_runtime::WorkloadRestartRuntime;
use crate::workload_saga::{
    ConfirmedWorkloadProvisionCommand, ConfirmedWorkloadRestartCommand,
    ConfirmedWorkloadTeardownCommand, FinalIngressWithdrawalCapability,
    IngressProvisionCapabilities, IngressPublicationCapability,
    IngressPublicationInspectionCapability, IngressTeardownCapabilities,
    NetworkAttachmentCapability, NetworkAttachmentProvisionCapabilities,
    NetworkAttachmentTeardownCapabilities, NetworkDetachmentCapability, NetworkReleaseCapability,
    NetworkReservationCapability, NetworkRestartAttachmentCapability, RestartPublicationCapability,
    RestartPublicationObservationCapability, RestartPublicationWithdrawalCapability,
    WorkloadActivationCapability, WorkloadActivationPrerequisiteCapability,
    WorkloadExecutionDrainCapability, WorkloadExecutionProvisionCapabilities,
    WorkloadExecutionQuiescenceCapability, WorkloadExecutionStopCapability,
    WorkloadExecutionTeardownCapabilities, WorkloadPreparationCapability,
    WorkloadProvisionCapabilityFuture, WorkloadProvisionCapabilityRegistry,
    WorkloadProvisionSourceAuthority, WorkloadReadinessCapability,
    WorkloadRestartActivationCapability, WorkloadRestartActivationPrerequisiteCapability,
    WorkloadRestartCapabilities, WorkloadRestartCapabilityFuture,
    WorkloadRestartCapabilityRegistry, WorkloadRestartCommandMode,
    WorkloadRestartPreparationCapability, WorkloadRestartReadinessCapability,
    WorkloadSagaCoordinator, WorkloadTeardownCapabilityFuture, WorkloadTeardownCapabilityRegistry,
    WorkloadTeardownExecuteOutcome, WorkloadTeardownInspectOutcome,
    WorkloadTeardownProviderObservation, WorkloadTeardownProviderOutcome, WorkloadTeardownRuntime,
    sandbox_execution_provider_id,
};

pub(super) const SERVICE_NAME: &str = "service-worker";
pub(super) const SANDBOX_ID: &str = "standalone-worker";

pub(super) fn run_async_test(test: impl Future<Output = ()> + Send + 'static) {
    std::thread::Builder::new()
        .name("nimbus-resource-retirement-test".to_owned())
        .stack_size(16 * 1024 * 1024)
        .spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("fixture runtime should build")
                .block_on(test);
        })
        .expect("fixture thread should start")
        .join()
        .expect("fixture thread should complete");
}

pub(super) fn tenant() -> TenantId {
    TenantId::new("tenant-retirement").expect("fixture tenant should validate")
}

pub(super) fn key(stable_id: &str) -> WorkloadSagaKey {
    WorkloadSagaKey::new(
        tenant(),
        WorkloadId::new(stable_id).expect("fixture workload ID should validate"),
    )
}

pub(super) fn service_spec() -> SandboxSpec {
    SandboxSpec::new(
        tenant(),
        SandboxOwnerSpec::service(SERVICE_NAME),
        SandboxBackendKind::Krun,
        SandboxRootSpec::rootfs("/fixture/service-rootfs"),
        SandboxProcessSpec::new(["/bin/service"]),
    )
}

pub(super) fn sandbox_spec() -> SandboxSpec {
    SandboxSpec::new(
        tenant(),
        SandboxOwnerSpec::standalone_named(SANDBOX_ID),
        SandboxBackendKind::Krun,
        SandboxRootSpec::rootfs("/fixture/sandbox-rootfs"),
        SandboxProcessSpec::new(["/bin/worker"]),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LifecycleEvent {
    Store {
        phase: WorkloadSagaPhase,
        restart_active: bool,
    },
    Provision(
        WorkloadSagaKey,
        nimbus_workloads::WorkloadProvisionStep,
        nimbus_workloads::WorkloadProvisionCommandMode,
    ),
    Restart(
        WorkloadSagaKey,
        WorkloadRestartStep,
        WorkloadRestartCommandMode,
    ),
    Teardown(
        WorkloadSagaKey,
        WorkloadTeardownStep,
        WorkloadTeardownCommandMode,
    ),
}

#[derive(Default)]
pub(super) struct EventLog(Mutex<Vec<LifecycleEvent>>);

impl EventLog {
    pub(super) fn push(&self, event: LifecycleEvent) {
        self.0
            .lock()
            .expect("event log should remain healthy")
            .push(event);
    }

    pub(super) fn clear(&self) {
        self.0
            .lock()
            .expect("event log should remain healthy")
            .clear();
    }

    pub(super) fn entries(&self) -> Vec<LifecycleEvent> {
        self.0
            .lock()
            .expect("event log should remain healthy")
            .clone()
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum NextCasFault {
    AmbiguousBeforeApply,
}

#[derive(Debug, Clone)]
pub(super) enum TenantPageFault {
    Error(WorkloadSagaStoreError),
    Insert(Box<WorkloadSagaRecord>),
    Page(WorkloadSagaTenantPage),
}

#[derive(Default)]
pub(super) struct RetirementSagaStore {
    records: Mutex<BTreeMap<WorkloadSagaKey, WorkloadSagaRecord>>,
    next_fault: Mutex<Option<NextCasFault>>,
    next_load_failure_for: Mutex<Option<WorkloadSagaKey>>,
    tenant_page_fault: Mutex<Option<(usize, TenantPageFault)>>,
    first_missing_cas_gate: Mutex<Option<(Arc<Semaphore>, Arc<Semaphore>)>>,
    loads: AtomicUsize,
    compare_and_swaps: AtomicUsize,
    tenant_page_calls: AtomicUsize,
    log: Arc<EventLog>,
}

impl RetirementSagaStore {
    pub(super) fn new(log: Arc<EventLog>) -> Arc<Self> {
        Arc::new(Self {
            log,
            ..Self::default()
        })
    }

    pub(super) fn record(&self, key: &WorkloadSagaKey) -> WorkloadSagaRecord {
        self.records
            .lock()
            .expect("saga store should remain healthy")
            .get(key)
            .cloned()
            .expect("fixture saga record should exist")
    }

    pub(super) fn replace(&self, record: WorkloadSagaRecord) {
        self.records
            .lock()
            .expect("saga store should remain healthy")
            .insert(record.key().clone(), record);
    }

    pub(super) fn remove(&self, key: &WorkloadSagaKey) {
        self.records
            .lock()
            .expect("saga store should remain healthy")
            .remove(key);
    }

    pub(super) fn fail_next_cas(&self, fault: NextCasFault) {
        *self
            .next_fault
            .lock()
            .expect("CAS fault lock should remain healthy") = Some(fault);
    }

    pub(super) fn fail_next_load_for(&self, key: WorkloadSagaKey) {
        *self
            .next_load_failure_for
            .lock()
            .expect("load-failure lock should remain healthy") = Some(key);
    }

    pub(super) fn fault_tenant_page(&self, call: usize, fault: TenantPageFault) {
        assert!(call > 0, "tenant page call index is one-based");
        *self
            .tenant_page_fault
            .lock()
            .expect("tenant-page fault lock should remain healthy") = Some((call, fault));
    }

    pub(super) fn tenant_page_call_count(&self) -> usize {
        self.tenant_page_calls.load(Ordering::Acquire)
    }

    pub(super) fn install_first_missing_cas_gate(&self) -> (Arc<Semaphore>, Arc<Semaphore>) {
        let entered = Arc::new(Semaphore::new(0));
        let release = Arc::new(Semaphore::new(0));
        *self
            .first_missing_cas_gate
            .lock()
            .expect("first missing-CAS gate lock should remain healthy") =
            Some((entered.clone(), release.clone()));
        (entered, release)
    }

    pub(super) fn counts(&self) -> (usize, usize) {
        (
            self.loads.load(Ordering::Acquire),
            self.compare_and_swaps.load(Ordering::Acquire),
        )
    }
}

impl WorkloadSagaStore for RetirementSagaStore {
    fn load<'a>(
        &'a self,
        key: &'a WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        Box::pin(async move {
            self.loads.fetch_add(1, Ordering::AcqRel);
            let fail = {
                let mut next = self
                    .next_load_failure_for
                    .lock()
                    .expect("load-failure lock should remain healthy");
                if next.as_ref() == Some(key) {
                    next.take();
                    true
                } else {
                    false
                }
            };
            if fail {
                return Err(WorkloadSagaStoreError::Unavailable);
            }
            Ok(self
                .records
                .lock()
                .expect("saga store should remain healthy")
                .get(key)
                .cloned())
        })
    }

    fn compare_and_swap<'a>(
        &'a self,
        expected: WorkloadSagaExpected,
        next: WorkloadSagaRecord,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaCommit> {
        Box::pin(async move {
            self.compare_and_swaps.fetch_add(1, Ordering::AcqRel);
            let first_missing_cas_gate = if matches!(&expected, WorkloadSagaExpected::Missing) {
                self.first_missing_cas_gate
                    .lock()
                    .expect("first missing-CAS gate lock should remain healthy")
                    .take()
            } else {
                None
            };
            if let Some((entered, release)) = first_missing_cas_gate {
                entered.add_permits(1);
                release
                    .acquire()
                    .await
                    .expect("first missing-CAS gate should remain open")
                    .forget();
            }
            if matches!(
                self.next_fault
                    .lock()
                    .expect("CAS fault lock should remain healthy")
                    .take(),
                Some(NextCasFault::AmbiguousBeforeApply)
            ) {
                return Err(WorkloadSagaStoreError::Ambiguous);
            }
            let key = next.key().clone();
            let mut records = self
                .records
                .lock()
                .expect("saga store should remain healthy");
            if records.get(&key) == Some(&next) {
                return Ok(WorkloadSagaCommit::Unchanged);
            }
            let observed = records.get(&key);
            let matches = match (expected, observed) {
                (WorkloadSagaExpected::Missing, None) => true,
                (WorkloadSagaExpected::Revision(expected), Some(record)) => {
                    record.revision() == expected
                }
                _ => false,
            };
            if !matches {
                return Err(WorkloadSagaStoreError::Conflict {
                    expected,
                    observed: observed.map(WorkloadSagaRecord::revision),
                });
            }
            let phase = next.phase();
            let restart_active = next.restart_state().active().is_some();
            records.insert(key, next);
            drop(records);
            self.log.push(LifecycleEvent::Store {
                phase,
                restart_active,
            });
            Ok(WorkloadSagaCommit::Applied)
        })
    }

    fn list_recoverable<'a>(
        &'a self,
        request: WorkloadSagaPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaPage> {
        Box::pin(async move { WorkloadSagaPage::new(&request, Vec::new(), false) })
    }

    fn list_restart_candidates<'a>(
        &'a self,
        request: nimbus_workloads::WorkloadRestartCandidatePageRequest,
    ) -> WorkloadSagaFuture<'a, nimbus_workloads::WorkloadRestartCandidatePage> {
        Box::pin(async move {
            nimbus_workloads::WorkloadRestartCandidatePage::new(&request, Vec::new(), false)
        })
    }

    fn list_for_tenant<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        request: WorkloadSagaTenantPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaTenantPage> {
        Box::pin(async move {
            request.validate_for_tenant(tenant_id)?;
            let call = self.tenant_page_calls.fetch_add(1, Ordering::AcqRel) + 1;
            let fault = {
                let mut fault = self
                    .tenant_page_fault
                    .lock()
                    .expect("tenant-page fault lock should remain healthy");
                if fault.as_ref().is_some_and(|(target, _)| *target == call) {
                    fault.take().map(|(_, fault)| fault)
                } else {
                    None
                }
            };
            match fault {
                Some(TenantPageFault::Error(error)) => return Err(error),
                Some(TenantPageFault::Insert(record)) => {
                    self.records
                        .lock()
                        .expect("saga store should remain healthy")
                        .insert(record.key().clone(), *record);
                }
                Some(TenantPageFault::Page(page)) => return Ok(page),
                None => {}
            }
            let mut records = self
                .records
                .lock()
                .expect("saga store should remain healthy")
                .values()
                .filter(|record| record.key().tenant_id() == tenant_id)
                .filter(|record| {
                    request
                        .after()
                        .is_none_or(|cursor| record.key() > cursor.key())
                })
                .cloned()
                .collect::<Vec<_>>();
            records.sort_by(|left, right| left.key().cmp(right.key()));
            let has_more = records.len() > usize::from(request.limit());
            records.truncate(usize::from(request.limit()));
            WorkloadSagaTenantPage::new(tenant_id, &request, records, has_more)
        })
    }
}

#[derive(Clone)]
struct ProvisionGate {
    step: nimbus_workloads::WorkloadProvisionStep,
    entered: Arc<Semaphore>,
    release: Arc<Semaphore>,
}

#[derive(Default)]
pub(super) struct RecordingProvisionProvider {
    log: Arc<EventLog>,
    calls: AtomicUsize,
    gate: Mutex<Option<ProvisionGate>>,
    gate_entered: AtomicBool,
}

impl RecordingProvisionProvider {
    pub(super) fn new(log: Arc<EventLog>) -> Arc<Self> {
        Arc::new(Self {
            log,
            ..Self::default()
        })
    }

    pub(super) fn install_gate(
        &self,
        step: nimbus_workloads::WorkloadProvisionStep,
    ) -> (Arc<Semaphore>, Arc<Semaphore>) {
        let entered = Arc::new(Semaphore::new(0));
        let release = Arc::new(Semaphore::new(0));
        *self
            .gate
            .lock()
            .expect("provision gate lock should remain healthy") = Some(ProvisionGate {
            step,
            entered: entered.clone(),
            release: release.clone(),
        });
        self.gate_entered.store(false, Ordering::Release);
        (entered, release)
    }

    pub(super) fn call_count(&self) -> usize {
        self.calls.load(Ordering::Acquire)
    }

    async fn outcome(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionInspectionResult {
        self.calls.fetch_add(1, Ordering::AcqRel);
        self.log.push(LifecycleEvent::Provision(
            command.claim().attempt().key().clone(),
            command.step(),
            command.mode(),
        ));
        let gate = self
            .gate
            .lock()
            .expect("provision gate lock should remain healthy")
            .clone();
        if let Some(gate) = gate
            && gate.step == command.step()
            && !self.gate_entered.swap(true, Ordering::AcqRel)
        {
            gate.entered.add_permits(1);
            gate.release
                .acquire()
                .await
                .expect("provision gate should remain open")
                .forget();
        }
        WorkloadProvisionInspectionResult::Succeeded {
            attempt_id: command.attempt_id().clone(),
            dispatch_epoch: command.dispatch_epoch(),
            provider_target: command.provider_target().clone(),
            evidence: crate::workload_saga::test_support::success_for(command.claim().attempt()),
        }
    }
}

macro_rules! provision_effect_capability {
    ($trait_name:ident) => {
        impl $trait_name for RecordingProvisionProvider {
            fn execute<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move { self.outcome(command).await })
            }

            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move { self.outcome(command).await })
            }
        }
    };
}

macro_rules! provision_inspection_capability {
    ($trait_name:ident) => {
        impl $trait_name for RecordingProvisionProvider {
            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move { self.outcome(command).await })
            }
        }
    };
}

provision_effect_capability!(NetworkReservationCapability);
provision_effect_capability!(WorkloadPreparationCapability);
provision_effect_capability!(NetworkAttachmentCapability);
provision_inspection_capability!(WorkloadActivationPrerequisiteCapability);
provision_effect_capability!(WorkloadActivationCapability);
provision_inspection_capability!(WorkloadReadinessCapability);
provision_effect_capability!(IngressPublicationCapability);
provision_inspection_capability!(IngressPublicationInspectionCapability);

impl crate::workload_projection::WorkloadExecutionObservationCapability
    for RecordingProvisionProvider
{
    fn observe<'a>(
        &'a self,
        request: &'a crate::workload_projection::WorkloadExecutionObservationRequest,
    ) -> crate::workload_projection::WorkloadExecutionObservationFuture<'a> {
        Box::pin(async move {
            let spec = crate::workload_executable::decode_sandbox_spec(request.executable())
                .expect("fixture executable should decode");
            crate::workload_projection::WorkloadProviderObservation::Present(
                SandboxInspection::provider_authenticated_running(
                    SandboxHandle::new(
                        request.key().tenant_id().clone(),
                        SandboxId::new(request.execution().execution_id().as_str()),
                        spec.display_name(),
                        spec.backend,
                        nimbus_sandbox::SandboxStatus::Ready,
                        Vec::new(),
                    ),
                    nimbus_sandbox::SandboxExecutionAttemptId::new(
                        request.execution().attempt_id().to_string(),
                    )
                    .expect("fixture attempt ID should validate"),
                    b"retirement-provision-provider",
                ),
            )
        })
    }
}

impl crate::workload_projection::WorkloadIngressObservationCapability
    for RecordingProvisionProvider
{
    fn observe<'a>(
        &'a self,
        _request: &'a crate::workload_projection::WorkloadIngressObservationRequest,
    ) -> crate::workload_projection::WorkloadIngressObservationFuture<'a> {
        Box::pin(async { crate::workload_projection::WorkloadProviderObservation::Ambiguous })
    }
}

pub(super) struct RecordingRestartProvider {
    log: Arc<EventLog>,
    phases: ProviderRestartPhaseAdapter,
    _state_root: tempfile::TempDir,
    execute_calls: AtomicUsize,
    inspect_calls: AtomicUsize,
}

impl RecordingRestartProvider {
    pub(super) fn new(log: Arc<EventLog>) -> Arc<Self> {
        let state_root = tempfile::tempdir().expect("restart provider state root should build");
        let journal =
            ProviderCommandAttemptJournal::open(state_root.path(), "resource-retirement-restart")
                .expect("restart provider journal should open");
        Arc::new(Self {
            log,
            phases: ProviderRestartPhaseAdapter::new(journal),
            _state_root: state_root,
            execute_calls: AtomicUsize::new(0),
            inspect_calls: AtomicUsize::new(0),
        })
    }

    pub(super) fn execute_call_count(&self) -> usize {
        self.execute_calls.load(Ordering::Acquire)
    }

    pub(super) fn inspect_call_count(&self) -> usize {
        self.inspect_calls.load(Ordering::Acquire)
    }

    fn observe(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
        mode: WorkloadRestartCommandMode,
    ) -> crate::workload_saga::WorkloadRestartProviderObservation {
        self.log.push(LifecycleEvent::Restart(
            command.key().clone(),
            command.step(),
            mode,
        ));
        let evidence = || ProviderRestartEffectObservation::Succeeded {
            evidence: format!(
                "resource-retirement-restart-{:?}-{:?}",
                command.step(),
                mode
            )
            .into_bytes(),
        };
        match mode {
            WorkloadRestartCommandMode::Execute => {
                self.execute_calls.fetch_add(1, Ordering::AcqRel);
                self.phases.execute(command, evidence)
            }
            WorkloadRestartCommandMode::Inspect => {
                self.inspect_calls.fetch_add(1, Ordering::AcqRel);
                self.phases.inspect(command, evidence)
            }
        }
    }
}

macro_rules! restart_effect_capability {
    ($trait_name:ident) => {
        impl $trait_name for RecordingRestartProvider {
            fn execute(
                &self,
                command: &ConfirmedWorkloadRestartCommand,
            ) -> WorkloadRestartCapabilityFuture<'_> {
                Box::pin(std::future::ready(
                    self.observe(command, WorkloadRestartCommandMode::Execute),
                ))
            }

            fn inspect(
                &self,
                command: &ConfirmedWorkloadRestartCommand,
            ) -> WorkloadRestartCapabilityFuture<'_> {
                Box::pin(std::future::ready(
                    self.observe(command, WorkloadRestartCommandMode::Inspect),
                ))
            }
        }
    };
}

macro_rules! restart_inspection_capability {
    ($trait_name:ident) => {
        impl $trait_name for RecordingRestartProvider {
            fn inspect(
                &self,
                command: &ConfirmedWorkloadRestartCommand,
            ) -> WorkloadRestartCapabilityFuture<'_> {
                Box::pin(std::future::ready(
                    self.observe(command, WorkloadRestartCommandMode::Inspect),
                ))
            }
        }
    };
}

restart_effect_capability!(RestartPublicationWithdrawalCapability);
restart_effect_capability!(WorkloadExecutionQuiescenceCapability);
restart_effect_capability!(WorkloadRestartPreparationCapability);
restart_effect_capability!(NetworkRestartAttachmentCapability);
restart_inspection_capability!(WorkloadRestartActivationPrerequisiteCapability);
restart_effect_capability!(WorkloadRestartActivationCapability);
restart_inspection_capability!(WorkloadRestartReadinessCapability);
restart_effect_capability!(RestartPublicationCapability);
restart_inspection_capability!(RestartPublicationObservationCapability);

pub(super) struct RecordingTeardownProvider {
    log: Arc<EventLog>,
    calls: AtomicUsize,
}

impl RecordingTeardownProvider {
    pub(super) fn new(log: Arc<EventLog>) -> Arc<Self> {
        Arc::new(Self {
            log,
            calls: AtomicUsize::new(0),
        })
    }

    pub(super) fn call_count(&self) -> usize {
        self.calls.load(Ordering::Acquire)
    }

    fn outcome(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownProviderObservation {
        self.calls.fetch_add(1, Ordering::AcqRel);
        self.log.push(LifecycleEvent::Teardown(
            command.key().clone(),
            command.step(),
            command.mode(),
        ));
        let success = teardown_success(command.step(), command.subjects());
        let outcome = match command.mode() {
            WorkloadTeardownCommandMode::Execute => WorkloadTeardownProviderOutcome::Execute(
                WorkloadTeardownExecuteOutcome::Succeeded(Box::new(success)),
            ),
            WorkloadTeardownCommandMode::Inspect => WorkloadTeardownProviderOutcome::Inspect(
                WorkloadTeardownInspectOutcome::Satisfied(Box::new(success)),
            ),
        };
        WorkloadTeardownProviderObservation::for_command(command, outcome)
    }
}

macro_rules! teardown_capability {
    ($trait_name:ident) => {
        impl $trait_name for RecordingTeardownProvider {
            fn execute<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadTeardownCommand,
            ) -> WorkloadTeardownCapabilityFuture<'a> {
                Box::pin(async move { self.outcome(command) })
            }

            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadTeardownCommand,
            ) -> WorkloadTeardownCapabilityFuture<'a> {
                Box::pin(async move { self.outcome(command) })
            }
        }
    };
}

teardown_capability!(FinalIngressWithdrawalCapability);
teardown_capability!(WorkloadExecutionDrainCapability);
teardown_capability!(WorkloadExecutionStopCapability);
teardown_capability!(NetworkDetachmentCapability);
teardown_capability!(NetworkReleaseCapability);

fn teardown_success(
    step: WorkloadTeardownStep,
    subjects: &WorkloadTeardownSubjects,
) -> WorkloadTeardownSuccessEvidence {
    let evidence = WorkloadOwnerEvidenceDigest::sha256(format!("retirement-{step:?}"));
    match (step, subjects) {
        (
            WorkloadTeardownStep::WithdrawPublication,
            WorkloadTeardownSubjects::Publication(reference),
        ) => WorkloadTeardownSuccessEvidence::PublicationAbsent {
            reference: reference.clone(),
            evidence,
        },
        (WorkloadTeardownStep::DrainExecution, WorkloadTeardownSubjects::Execution(reference)) => {
            WorkloadTeardownSuccessEvidence::ExecutionDrained {
                reference: reference.clone(),
                evidence,
            }
        }
        (WorkloadTeardownStep::StopExecution, WorkloadTeardownSubjects::Execution(reference)) => {
            WorkloadTeardownSuccessEvidence::ExecutionStopped {
                reference: reference.clone(),
                evidence,
            }
        }
        (WorkloadTeardownStep::DetachNetwork, WorkloadTeardownSubjects::Network(reference)) => {
            WorkloadTeardownSuccessEvidence::NetworkDetached {
                reference: reference.clone(),
                evidence,
            }
        }
        (WorkloadTeardownStep::ReleaseNetwork, WorkloadTeardownSubjects::Network(reference)) => {
            WorkloadTeardownSuccessEvidence::NetworkReleased {
                reference: reference.clone(),
                evidence,
            }
        }
        _ => panic!("teardown step and subjects should stay correlated"),
    }
}

pub(super) fn provider_realm() -> (NetworkCapabilityRegistry, NetworkCapabilitySelection) {
    let requirements = nimbus_sandbox::sandbox_network_plan_requirements(SandboxBackendKind::Krun);
    let ingress_provider = NetworkProviderId::for_registration_key("retirement-fixture-ingress");
    let lifecycle = NetworkLifecycleCapabilitySet::new([
        NetworkLifecycleFeature::DurableInspect,
        NetworkLifecycleFeature::Reconcile,
        NetworkLifecycleFeature::Delete,
    ]);
    let attachment = NetworkAttachmentProviderRegistration::new(
        requirements.required_attachment_provider_id().clone(),
        requirements.capability_requirements().attachment().clone(),
        [NetworkAddressFamily::Ipv4, NetworkAddressFamily::Ipv6],
        lifecycle.clone(),
        NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let ingress = NetworkIngressProviderRegistration::new(
        ingress_provider.clone(),
        NetworkEndpointCapabilitySet::new(
            [NetworkAddressFamily::Ipv4, NetworkAddressFamily::Ipv6],
            [NetworkBindRealmKind::Host],
            [NetworkExposure::Loopback, NetworkExposure::Private],
            [PortProtocol::Tcp],
            [
                NetworkPortAssignmentMode::Exact,
                NetworkPortAssignmentMode::ProviderAssigned,
            ],
        ),
        NetworkIngressCapabilitySet::new([]),
        NetworkForwardingCapabilitySet::new([]),
        lifecycle,
        NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let selection = NetworkCapabilitySelection::new(
        requirements.required_attachment_provider_id().clone(),
        ingress_provider,
    );
    (
        NetworkCapabilityRegistry::new([NetworkCapabilityBundle::new(attachment, ingress)])
            .expect("fixture provider realm should validate"),
        selection,
    )
}

pub(super) fn provision_capabilities(
    provider: Arc<RecordingProvisionProvider>,
    selection: &NetworkCapabilitySelection,
) -> WorkloadProvisionCapabilityRegistry {
    let attachment_provider =
        nimbus_sandbox::sandbox_network_plan_requirements(SandboxBackendKind::Krun)
            .required_attachment_provider_id()
            .clone();
    WorkloadProvisionCapabilityRegistry::new(
        [NetworkAttachmentProvisionCapabilities::new(
            attachment_provider,
            provider.clone(),
        )],
        [WorkloadExecutionProvisionCapabilities::new(
            sandbox_execution_provider_id(SandboxBackendKind::Krun),
            provider.clone(),
        )],
        [IngressProvisionCapabilities::new(
            selection.ingress_provider_id().clone(),
            provider,
        )],
    )
    .expect("fixture provision capabilities should validate")
}

fn restart_capabilities(
    provider: Arc<RecordingRestartProvider>,
    selection: &NetworkCapabilitySelection,
) -> WorkloadRestartCapabilityRegistry {
    WorkloadRestartCapabilityRegistry::new([WorkloadRestartCapabilities::new(
        sandbox_execution_provider_id(SandboxBackendKind::Krun),
        Some(selection.clone()),
        provider.clone(),
        provider.clone(),
        provider,
    )])
    .expect("fixture restart capabilities should validate")
}

fn teardown_capabilities(
    provider: Arc<RecordingTeardownProvider>,
    selection: &NetworkCapabilitySelection,
) -> WorkloadTeardownCapabilityRegistry {
    WorkloadTeardownCapabilityRegistry::new(
        [NetworkAttachmentTeardownCapabilities::new(
            selection.attachment_provider_id().clone(),
            provider.clone(),
            provider.clone(),
        )],
        [WorkloadExecutionTeardownCapabilities::new(
            sandbox_execution_provider_id(SandboxBackendKind::Krun),
            provider.clone(),
            provider.clone(),
        )],
        [IngressTeardownCapabilities::new(
            selection.ingress_provider_id().clone(),
            provider,
        )],
    )
    .expect("fixture teardown capabilities should validate")
}

pub(super) struct RetirementHarness {
    pub(super) context: TenantIsolationContext,
    pub(super) manager: Arc<ServiceManager>,
    pub(super) store: Arc<RetirementSagaStore>,
    pub(super) provision_provider: Arc<RecordingProvisionProvider>,
    pub(super) restart_provider: Arc<RecordingRestartProvider>,
    pub(super) teardown_provider: Arc<RecordingTeardownProvider>,
    pub(super) provision: ComputeResourceProvisioner,
    pub(super) provisioner: Arc<WorkloadProvisioner>,
    pub(super) retire: ComputeResourceRetirer,
    pub(super) log: Arc<EventLog>,
    _restart_runtime: Arc<WorkloadRestartRuntime>,
}

impl RetirementHarness {
    pub(super) fn new() -> Self {
        let log = Arc::new(EventLog::default());
        let manager = Arc::new(ServiceManager::new(
            Arc::new(EmptyServiceDefinitionCatalog),
            SandboxBackendKind::Krun,
        ));
        let store = RetirementSagaStore::new(log.clone());
        let provision_provider = RecordingProvisionProvider::new(log.clone());
        let restart_provider = RecordingRestartProvider::new(log.clone());
        let teardown_provider = RecordingTeardownProvider::new(log.clone());
        let (provider_reports, selection) = provider_realm();
        let coordinator = Arc::new(WorkloadSagaCoordinator::new(store.clone()));
        let source_authority: Arc<dyn WorkloadProvisionSourceAuthority> = Arc::new(
            ServiceManagerWorkloadProvisionSourceAuthority::new(manager.clone()),
        );
        let provision_capabilities = Arc::new(provision_capabilities(
            provision_provider.clone(),
            &selection,
        ));
        let restart_runtime = Arc::new(
            WorkloadRestartRuntime::start(
                coordinator.clone(),
                source_authority.clone(),
                provider_reports.clone(),
                provision_capabilities.clone(),
                Arc::new(restart_capabilities(restart_provider.clone(), &selection)),
            )
            .expect("fixture restart runtime should start"),
        );
        let teardown_runtime = Arc::new(WorkloadTeardownRuntime::new(
            coordinator.clone(),
            source_authority.clone(),
            provider_reports.clone(),
            Arc::new(teardown_capabilities(teardown_provider.clone(), &selection)),
        ));
        let provisioner = Arc::new(
            WorkloadProvisioner::new(
                embedded_local_node_identity(),
                provider_reports.clone(),
                selection.clone(),
                NetworkSovereigntyRequirements::new(
                    NetworkControlPlaneLocality::LocalOnly,
                    BTreeSet::new(),
                    true,
                ),
                coordinator.clone(),
                teardown_runtime.clone(),
                source_authority.clone(),
                (*provision_capabilities).clone(),
                Arc::new(ServiceManagerWorkloadProjectionSink::new(manager.clone())),
            )
            .expect("fixture provisioner should compose"),
        );
        let provision = ComputeResourceProvisioner::new(manager.clone(), provisioner.clone());
        let retire = ComputeResourceRetirer::new(
            manager.clone(),
            provisioner.clone(),
            coordinator,
            restart_runtime.clone(),
            teardown_runtime,
        );
        Self {
            context: TenantIsolationContext::system(tenant(), "resource-retirement-test"),
            manager,
            store,
            provision_provider,
            restart_provider,
            teardown_provider,
            provision,
            provisioner,
            retire,
            log,
            _restart_runtime: restart_runtime,
        }
    }

    pub(super) fn declare_service(&self) {
        self.manager
            .create_service_definition(
                &tenant(),
                SERVICE_NAME,
                ServiceBackend::sandbox(service_spec()),
                BTreeMap::new(),
            )
            .expect("fixture service source should be declared");
    }

    pub(super) async fn start_service(&self) {
        self.provision
            .provision_sandbox_service(
                &self.context,
                SERVICE_NAME,
                &WorkloadProvisionCancellation::default(),
            )
            .await
            .expect("fixture service should provision");
    }

    pub(super) async fn start_sandbox(&self) {
        self.provision
            .provision_standalone_sandbox(
                &self.context,
                SANDBOX_ID,
                "worker",
                sandbox_spec(),
                BTreeMap::new(),
                &WorkloadProvisionCancellation::default(),
            )
            .await
            .expect("fixture sandbox should provision");
    }

    pub(super) fn reset_retirement_evidence(&self) {
        self.log.clear();
    }

    pub(super) fn install_source_claim_signal(&self) -> Arc<Semaphore> {
        let entered = Arc::new(Semaphore::new(0));
        self.provisioner
            .install_test_retirement_claim_boundary(entered.clone());
        entered
    }

    pub(super) async fn wait_for_source_claim(&self, entered: &Arc<Semaphore>, source: &str) {
        self.wait_for_signal(
            entered,
            &format!("{source} retirement did not install its source claim"),
        )
        .await;
    }

    pub(super) async fn wait_for_signal(&self, entered: &Arc<Semaphore>, diagnostic: &str) {
        tokio::time::timeout(std::time::Duration::from_secs(2), entered.acquire())
            .await
            .unwrap_or_else(|_| panic!("{diagnostic}"))
            .expect("source-claim signal should remain open")
            .forget();
    }

    pub(super) fn service_source_is_fenced(&self) -> bool {
        let prepared = self
            .manager
            .prepare_sandbox_service_provision_source(self.context.tenant_id(), SERVICE_NAME)
            .expect("fixture service source should remain readable while fenced");
        let decision = self
            .context
            .clone()
            .with_deployment_generation(1)
            .with_workload_location(
                WorkloadLocation::new().with_node_id(embedded_local_node_identity().as_str()),
            )
            .admit_decision(prepared.policy_input().clone())
            .expect("fixture service decision should admit");
        self.manager
            .reserve_sandbox_service_provision_source(&decision, prepared)
            .is_err()
    }

    pub(super) fn sandbox_source_is_fenced(&self) -> bool {
        let prepared = self
            .manager
            .prepare_standalone_sandbox_provision_source(
                self.context.tenant_id(),
                SANDBOX_ID,
                "worker",
                sandbox_spec(),
                BTreeMap::new(),
            )
            .expect("fixture sandbox source should remain readable while fenced");
        let decision = self
            .context
            .clone()
            .with_deployment_generation(1)
            .with_workload_location(
                WorkloadLocation::new().with_node_id(embedded_local_node_identity().as_str()),
            )
            .admit_decision(prepared.policy_input().clone())
            .expect("fixture sandbox decision should admit");
        self.manager
            .reserve_standalone_sandbox_provision_source(&decision, prepared)
            .is_err()
    }
}

pub(super) fn assert_complete_teardown_order(
    events: &[LifecycleEvent],
    expected_key: &WorkloadSagaKey,
) {
    let mut first_store_phase = Vec::new();
    for event in events {
        if let LifecycleEvent::Store { phase, .. } = event
            && first_store_phase.last() != Some(phase)
        {
            first_store_phase.push(*phase);
        }
    }
    for phase in [
        WorkloadSagaPhase::WithdrawalCommitted,
        WorkloadSagaPhase::Withdrawn,
        WorkloadSagaPhase::Drained,
        WorkloadSagaPhase::WorkloadStopped,
        WorkloadSagaPhase::NetworkDetached,
        WorkloadSagaPhase::NetworkReleased,
        WorkloadSagaPhase::Recorded,
    ] {
        assert!(
            first_store_phase.contains(&phase),
            "durable teardown should include {phase:?}: {events:?}"
        );
    }
    assert_teardown_effect_order(events, expected_key);
    let recorded = events
        .iter()
        .position(|event| {
            matches!(
                event,
                LifecycleEvent::Store {
                    phase: WorkloadSagaPhase::Recorded,
                    ..
                }
            )
        })
        .expect("recorded teardown should be durable");
    let final_effect = events
        .iter()
        .rposition(|event| matches!(event, LifecycleEvent::Teardown(..)))
        .expect("teardown should invoke exact provider capabilities");
    let withdrawal_committed = events
        .iter()
        .position(|event| {
            matches!(
                event,
                LifecycleEvent::Store {
                    phase: WorkloadSagaPhase::WithdrawalCommitted,
                    ..
                }
            )
        })
        .expect("withdrawal must be durable before provider effects");
    let first_effect = events
        .iter()
        .position(|event| matches!(event, LifecycleEvent::Teardown(..)))
        .expect("teardown should invoke exact provider capabilities");
    assert!(
        withdrawal_committed < first_effect,
        "WithdrawalCommitted must be durable before the first provider effect"
    );
    assert!(
        final_effect < recorded,
        "terminal observation must follow provider completion"
    );
}

pub(super) fn prior_process_issued_provision_record(
    base: &WorkloadSagaRecord,
) -> WorkloadSagaRecord {
    let initial = WorkloadSagaRecord::new(base.key().clone(), base.active_intent().clone())
        .expect("prior-process provision fixture should validate");
    crate::workload_saga::test_support::first_proposed_candidate(&initial)
        .dispatch_to_inspection()
        .expect("an issued prior-process claim must require inspection")
}

pub(super) fn assert_teardown_effect_order(
    events: &[LifecycleEvent],
    expected_key: &WorkloadSagaKey,
) {
    let teardown = events
        .iter()
        .filter_map(|event| match event {
            LifecycleEvent::Teardown(key, step, mode) => Some((key, *step, *mode)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        teardown,
        vec![
            (
                expected_key,
                WorkloadTeardownStep::WithdrawPublication,
                WorkloadTeardownCommandMode::Execute,
            ),
            (
                expected_key,
                WorkloadTeardownStep::DrainExecution,
                WorkloadTeardownCommandMode::Execute
            ),
            (
                expected_key,
                WorkloadTeardownStep::StopExecution,
                WorkloadTeardownCommandMode::Execute
            ),
            (
                expected_key,
                WorkloadTeardownStep::DetachNetwork,
                WorkloadTeardownCommandMode::Execute
            ),
            (
                expected_key,
                WorkloadTeardownStep::ReleaseNetwork,
                WorkloadTeardownCommandMode::Execute
            ),
        ],
        "provider effects should follow the exact retained teardown order"
    );
}

pub(super) fn max_generation_record(base: &WorkloadSagaRecord) -> WorkloadSagaRecord {
    let key = base.key().clone();
    let current = base.active_intent().network().compiled_plan().content();
    let identity = nimbus_workloads::WorkloadNetworkPlanIdentity::new(
        key.tenant_id().clone(),
        current.identity().workload_incarnation_key(),
        NetworkResourceGeneration::new(u64::MAX),
    )
    .expect("maximum-generation network identity should validate");
    let content = nimbus_workloads::WorkloadNetworkPlanContent::new(
        identity,
        current.capability_requirements().clone(),
        current.capability_selection().cloned(),
        current.capability_selection_evidence().cloned(),
        current.attachment().cloned(),
        current.routes().iter().cloned(),
        current.listeners().iter().cloned(),
        current.dependency_listeners().iter().cloned(),
        current.activation(),
        current.publication(),
    )
    .expect("maximum-generation network content should validate");
    let network = nimbus_workloads::WorkloadNetworkIntent::new(
        nimbus_workloads::CompiledWorkloadNetworkPlan::from_content(content)
            .expect("maximum-generation network plan should compile"),
    );
    let intent = nimbus_workloads::WorkloadSagaIntent::new_with_restart_policy(
        base.active_intent().kind(),
        DesiredWorkloadState::Running,
        WorkloadGeneration::new(u64::MAX),
        base.active_intent().executable().clone(),
        base.active_intent().source().clone(),
        base.active_intent().restart_policy(),
        network,
        base.active_intent().activation(),
        base.active_intent().publication(),
        base.active_intent().admission().clone(),
    )
    .expect("maximum-generation fixture should validate");
    observed_record(key, intent)
}

pub(super) fn observed_record(
    key: WorkloadSagaKey,
    intent: nimbus_workloads::WorkloadSagaIntent,
) -> WorkloadSagaRecord {
    let mut record = WorkloadSagaRecord::new(key, intent).expect("fixture record should validate");
    for _ in 0..24 {
        if record.phase() == WorkloadSagaPhase::Observed {
            return record;
        }
        record = crate::workload_saga::test_support::confirmed_provision(&record);
    }
    panic!("fixture should reach Observed")
}

pub(super) fn active_restart_record(base: &WorkloadSagaRecord) -> WorkloadSagaRecord {
    let active = base.active_intent();
    let restartable = nimbus_workloads::WorkloadSagaIntent::new_with_restart_policy(
        active.kind(),
        active.desired_state(),
        active.generation(),
        active.executable().clone(),
        active.source().clone(),
        nimbus_workloads::WorkloadRestartPolicy::Always { max_restarts: 1 },
        active.network().clone(),
        active.activation(),
        active.publication(),
        active.admission().clone(),
    )
    .expect("restartable fixture should validate");
    let observed = observed_record(base.key().clone(), restartable);
    let version = nimbus_workloads::WorkloadInspectionVersion::from_bytes([0x51; 32]);
    let input = nimbus_workloads::WorkloadRestartAdmissionInput {
        expected_revision: observed.revision(),
        trigger: nimbus_workloads::WorkloadRestartTrigger::Automatic { exit_code: 17 },
        inspection_version: Some(version),
        request_id: nimbus_workloads::WorkloadRestartRequestId::for_automatic(
            observed.saga_id(),
            version,
        ),
        not_before_unix_millis: nimbus_workloads::WorkloadRestartNotBeforeUnixMillis::new(0),
    };
    let nimbus_workloads::WorkloadRestartAdmissionUpdate::Transition(admitted) = observed
        .admit_restart(input)
        .expect("fixture restart should admit")
    else {
        panic!("fixture restart should transition");
    };
    *admitted
}

pub(super) fn issued_restart_record(base: &WorkloadSagaRecord) -> WorkloadSagaRecord {
    let admitted = active_restart_record(base);
    let request_id = admitted
        .restart_state()
        .active()
        .expect("issued restart fixture should be active")
        .admission()
        .request_id()
        .clone();
    let quiescence = admitted
        .advance_restart_without_effect(&request_id)
        .expect("withheld restart should enter execution quiescence");
    let claimed = quiescence
        .claim_restart_command(&request_id)
        .expect("issued restart fixture should retain exact provider authority");
    assert!(matches!(
        claimed
            .restart_state()
            .active()
            .expect("issued restart fixture should remain active")
            .disposition(),
        WorkloadRestartDisposition::DispatchPending { claim }
            if claim.step() == WorkloadRestartStep::QuiesceExecution
    ));
    claimed
}
