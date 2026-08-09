use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use nimbus_core::TenantId;
use nimbus_network::{
    NetworkAddressFamily, NetworkAttachmentCapabilitySet, NetworkAttachmentProviderRegistration,
    NetworkBindRealmKind, NetworkCapabilityBundle, NetworkCapabilityRegistry,
    NetworkControlPlaneLocality, NetworkEndpointCapabilitySet, NetworkExposure,
    NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
    NetworkIngressProviderRegistration, NetworkLifecycleCapabilitySet, NetworkManagementMode,
    NetworkPortAssignmentMode, NetworkProviderId, NetworkSovereigntyCapabilities, PortProtocol,
};
use nimbus_workloads::{
    WorkloadExecutionProviderId, WorkloadFailureEvidence, WorkloadProvisionSourceEvidence,
    WorkloadProvisionSourceIdentity, WorkloadSagaCommit, WorkloadSagaExpected, WorkloadSagaFuture,
    WorkloadSagaPage, WorkloadSagaPageRequest, WorkloadSagaRecord, WorkloadSagaStore,
    WorkloadSagaStoreError, WorkloadSagaTenantPage, WorkloadSagaTenantPageRequest,
    WorkloadTeardownCommandMode, WorkloadTeardownStep,
};

use super::recovery::tests::{evidence, teardown_success_evidence};
use super::teardown_driver::WorkloadTeardownDriver;
use super::{
    ConfirmedWorkloadTeardownCommand, FinalIngressWithdrawalCapability,
    IngressTeardownCapabilities, NetworkAttachmentTeardownCapabilities,
    NetworkDetachmentCapability, NetworkReleaseCapability, WorkloadExecutionDrainCapability,
    WorkloadExecutionStopCapability, WorkloadExecutionTeardownCapabilities,
    WorkloadProvisionSourceAuthority, WorkloadProvisionSourceAuthorityError,
    WorkloadProvisionSourceFuture, WorkloadSagaCoordinator, WorkloadTeardownCapabilityFuture,
    WorkloadTeardownCapabilityRegistry, WorkloadTeardownExecuteOutcome,
    WorkloadTeardownInspectOutcome, WorkloadTeardownProviderObservation,
    WorkloadTeardownProviderOutcome,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TeardownProviderCall {
    pub(super) step: WorkloadTeardownStep,
    pub(super) mode: WorkloadTeardownCommandMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TeardownProviderBehavior {
    Succeed,
    AmbiguousExecuteThenSatisfiedInspectAt(WorkloadTeardownStep),
    NotCompletedOnceAt(WorkloadTeardownStep),
    InProgressAt(WorkloadTeardownStep),
    AmbiguousAt(WorkloadTeardownStep),
    DefiniteFailureAt(WorkloadTeardownStep),
}

pub(super) struct RecordingTeardownProvider {
    behavior: TeardownProviderBehavior,
    one_shot_observed: AtomicBool,
    calls: Mutex<Vec<TeardownProviderCall>>,
}

impl RecordingTeardownProvider {
    pub(super) fn new(behavior: TeardownProviderBehavior) -> Arc<Self> {
        Arc::new(Self {
            behavior,
            one_shot_observed: AtomicBool::new(false),
            calls: Mutex::new(Vec::new()),
        })
    }

    pub(super) fn calls(&self) -> Vec<TeardownProviderCall> {
        self.calls
            .lock()
            .expect("teardown provider call lock is healthy")
            .clone()
    }

    fn outcome(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownProviderObservation {
        self.calls
            .lock()
            .expect("teardown provider call lock is healthy")
            .push(TeardownProviderCall {
                step: command.step(),
                mode: command.mode(),
            });
        let success = || teardown_success_evidence(command.step(), command.subjects());
        let outcome = match (self.behavior, command.mode()) {
            (
                TeardownProviderBehavior::DefiniteFailureAt(step),
                WorkloadTeardownCommandMode::Execute,
            ) if step == command.step() => WorkloadTeardownProviderOutcome::Execute(
                WorkloadTeardownExecuteOutcome::DefiniteFailure(
                    WorkloadFailureEvidence::new(
                        "fixture_teardown_failure",
                        evidence("fixture-teardown-failure"),
                    )
                    .expect("fixture teardown failure is valid"),
                ),
            ),
            (
                TeardownProviderBehavior::DefiniteFailureAt(step),
                WorkloadTeardownCommandMode::Inspect,
            ) if step == command.step() => WorkloadTeardownProviderOutcome::Inspect(
                WorkloadTeardownInspectOutcome::DefiniteFailure(
                    WorkloadFailureEvidence::new(
                        "fixture_teardown_failure",
                        evidence("fixture-teardown-failure"),
                    )
                    .expect("fixture teardown failure is valid"),
                ),
            ),
            (
                TeardownProviderBehavior::AmbiguousExecuteThenSatisfiedInspectAt(step),
                WorkloadTeardownCommandMode::Execute,
            ) if step == command.step() => {
                WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Ambiguous)
            }
            (
                TeardownProviderBehavior::NotCompletedOnceAt(step),
                WorkloadTeardownCommandMode::Execute,
            ) if step == command.step() && !self.one_shot_observed.load(Ordering::Acquire) => {
                WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Ambiguous)
            }
            (
                TeardownProviderBehavior::NotCompletedOnceAt(step),
                WorkloadTeardownCommandMode::Inspect,
            ) if step == command.step() && !self.one_shot_observed.swap(true, Ordering::AcqRel) => {
                WorkloadTeardownProviderOutcome::Inspect(
                    WorkloadTeardownInspectOutcome::NotCompleted(evidence(
                        "fixture-teardown-not-completed",
                    )),
                )
            }
            (
                TeardownProviderBehavior::InProgressAt(step),
                WorkloadTeardownCommandMode::Execute,
            ) if step == command.step() => {
                WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Ambiguous)
            }
            (
                TeardownProviderBehavior::InProgressAt(step),
                WorkloadTeardownCommandMode::Inspect,
            ) if step == command.step() => WorkloadTeardownProviderOutcome::Inspect(
                WorkloadTeardownInspectOutcome::InProgress(evidence(
                    "fixture-teardown-in-progress",
                )),
            ),
            (TeardownProviderBehavior::AmbiguousAt(step), WorkloadTeardownCommandMode::Execute)
                if step == command.step() =>
            {
                WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Ambiguous)
            }
            (TeardownProviderBehavior::AmbiguousAt(step), WorkloadTeardownCommandMode::Inspect)
                if step == command.step() =>
            {
                WorkloadTeardownProviderOutcome::Inspect(WorkloadTeardownInspectOutcome::Ambiguous)
            }
            (_, WorkloadTeardownCommandMode::Execute) => WorkloadTeardownProviderOutcome::Execute(
                WorkloadTeardownExecuteOutcome::Succeeded(Box::new(success())),
            ),
            (_, WorkloadTeardownCommandMode::Inspect) => WorkloadTeardownProviderOutcome::Inspect(
                WorkloadTeardownInspectOutcome::Satisfied(Box::new(success())),
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

pub(super) struct StaticSourceAuthority(Mutex<WorkloadProvisionSourceEvidence>);

impl StaticSourceAuthority {
    pub(super) fn exact(record: &WorkloadSagaRecord) -> Arc<Self> {
        Arc::new(Self(Mutex::new(record.active_intent().source().clone())))
    }

    pub(super) fn replace(&self, evidence: WorkloadProvisionSourceEvidence) {
        *self
            .0
            .lock()
            .expect("teardown source authority lock is healthy") = evidence;
    }
}

impl WorkloadProvisionSourceAuthority for StaticSourceAuthority {
    fn current_source<'a>(
        &'a self,
        _key: &'a nimbus_workloads::WorkloadSagaKey,
        identity: &'a WorkloadProvisionSourceIdentity,
    ) -> WorkloadProvisionSourceFuture<'a> {
        Box::pin(async move {
            let evidence = self
                .0
                .lock()
                .expect("teardown source authority lock is healthy");
            if evidence.source_identity() != identity {
                return Err(WorkloadProvisionSourceAuthorityError::NotFound);
            }
            Ok(evidence.clone())
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CasFault {
    AmbiguousBeforeApply,
    AmbiguousAfterApply,
}

#[derive(Default)]
pub(super) struct DurableTeardownStore {
    record: Mutex<Option<WorkloadSagaRecord>>,
    next_cas_fault: Mutex<Option<CasFault>>,
    always_conflict: bool,
    loads: AtomicUsize,
    compare_and_swaps: AtomicUsize,
}

impl DurableTeardownStore {
    pub(super) fn with_record(record: WorkloadSagaRecord) -> Arc<Self> {
        Arc::new(Self {
            record: Mutex::new(Some(record)),
            ..Self::default()
        })
    }

    pub(super) fn with_record_and_fault(record: WorkloadSagaRecord, fault: CasFault) -> Arc<Self> {
        Arc::new(Self {
            record: Mutex::new(Some(record)),
            next_cas_fault: Mutex::new(Some(fault)),
            ..Self::default()
        })
    }

    pub(super) fn with_record_and_repeating_conflict(record: WorkloadSagaRecord) -> Arc<Self> {
        Arc::new(Self {
            record: Mutex::new(Some(record)),
            always_conflict: true,
            ..Self::default()
        })
    }

    pub(super) fn counts(&self) -> (usize, usize) {
        (
            self.loads.load(Ordering::Acquire),
            self.compare_and_swaps.load(Ordering::Acquire),
        )
    }

    pub(super) fn record(&self) -> WorkloadSagaRecord {
        self.record
            .lock()
            .expect("teardown store lock is healthy")
            .clone()
            .expect("teardown fixture store retains a record")
    }
}

impl WorkloadSagaStore for DurableTeardownStore {
    fn load<'a>(
        &'a self,
        key: &'a nimbus_workloads::WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        Box::pin(async move {
            self.loads.fetch_add(1, Ordering::AcqRel);
            let record = self.record.lock().expect("teardown store lock is healthy");
            if record.as_ref().is_some_and(|current| current.key() != key) {
                return Err(WorkloadSagaStoreError::Corrupt);
            }
            Ok(record.clone())
        })
    }

    fn compare_and_swap<'a>(
        &'a self,
        expected: WorkloadSagaExpected,
        next: WorkloadSagaRecord,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaCommit> {
        Box::pin(async move {
            self.compare_and_swaps.fetch_add(1, Ordering::AcqRel);
            let mut current = self.record.lock().expect("teardown store lock is healthy");
            if current.as_ref() == Some(&next) {
                return Ok(WorkloadSagaCommit::Unchanged);
            }
            let matches = match (expected, current.as_ref()) {
                (WorkloadSagaExpected::Missing, None) => true,
                (WorkloadSagaExpected::Revision(expected), Some(record)) => {
                    expected == record.revision()
                }
                _ => false,
            };
            if !matches {
                return Err(WorkloadSagaStoreError::Conflict {
                    expected,
                    observed: current.as_ref().map(WorkloadSagaRecord::revision),
                });
            }
            if self.always_conflict {
                return Err(WorkloadSagaStoreError::Conflict {
                    expected,
                    observed: current.as_ref().map(WorkloadSagaRecord::revision),
                });
            }
            match self
                .next_cas_fault
                .lock()
                .expect("teardown CAS fault lock is healthy")
                .take()
            {
                Some(CasFault::AmbiguousBeforeApply) => {
                    return Err(WorkloadSagaStoreError::Ambiguous);
                }
                Some(CasFault::AmbiguousAfterApply) => {
                    *current = Some(next);
                    return Err(WorkloadSagaStoreError::Ambiguous);
                }
                None => {}
            }
            *current = Some(next);
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
    ) -> nimbus_workloads::WorkloadSagaFuture<'a, nimbus_workloads::WorkloadRestartCandidatePage>
    {
        Box::pin(async move {
            nimbus_workloads::WorkloadRestartCandidatePage::new(&request, Vec::new(), false)
        })
    }

    fn list_for_tenant<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        request: WorkloadSagaTenantPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaTenantPage> {
        Box::pin(async move { WorkloadSagaTenantPage::new(tenant_id, &request, Vec::new(), false) })
    }
}

pub(super) fn provider_reports() -> NetworkCapabilityRegistry {
    let lifecycle = NetworkLifecycleCapabilitySet::new([]);
    NetworkCapabilityRegistry::new([NetworkCapabilityBundle::new(
        NetworkAttachmentProviderRegistration::new(
            NetworkProviderId::for_registration_key("fixture-attachment"),
            NetworkAttachmentCapabilitySet::new(NetworkManagementMode::NimbusHostManaged, [], []),
            [NetworkAddressFamily::Ipv4],
            lifecycle.clone(),
            NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
        ),
        NetworkIngressProviderRegistration::new(
            NetworkProviderId::for_registration_key("fixture-ingress"),
            NetworkEndpointCapabilitySet::new(
                [NetworkAddressFamily::Ipv4],
                [NetworkBindRealmKind::Host],
                [NetworkExposure::Loopback],
                [PortProtocol::Tcp],
                [NetworkPortAssignmentMode::ProviderAssigned],
            ),
            NetworkIngressCapabilitySet::new([]),
            NetworkForwardingCapabilitySet::new([]),
            lifecycle,
            NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
        ),
    )])
    .expect("teardown fixture provider reports validate")
}

pub(super) fn teardown_capabilities(
    provider: Arc<RecordingTeardownProvider>,
) -> WorkloadTeardownCapabilityRegistry {
    WorkloadTeardownCapabilityRegistry::new(
        [NetworkAttachmentTeardownCapabilities::new(
            NetworkProviderId::for_registration_key("fixture-attachment"),
            provider.clone(),
            provider.clone(),
        )],
        [WorkloadExecutionTeardownCapabilities::new(
            WorkloadExecutionProviderId::for_registration_key("fixture-execution"),
            provider.clone(),
            provider.clone(),
        )],
        [IngressTeardownCapabilities::new(
            NetworkProviderId::for_registration_key("fixture-ingress"),
            provider,
        )],
    )
    .expect("teardown fixture capability registry validates")
}

pub(super) fn driver(
    store: Arc<DurableTeardownStore>,
    source_record: &WorkloadSagaRecord,
    provider: Arc<RecordingTeardownProvider>,
) -> WorkloadTeardownDriver {
    let coordinator = Arc::new(WorkloadSagaCoordinator::new(store));
    let dispatcher = Arc::new(super::teardown_dispatch::WorkloadTeardownDispatcher::new(
        StaticSourceAuthority::exact(source_record),
        provider_reports(),
        Arc::new(teardown_capabilities(provider)),
    ));
    WorkloadTeardownDriver::new(coordinator, dispatcher)
}
