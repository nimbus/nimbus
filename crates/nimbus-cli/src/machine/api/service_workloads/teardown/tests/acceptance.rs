use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use nimbus_core::{Error, TenantId, WorkloadId};
use nimbus_machine::api::MachineApiWorkloadTeardownCommandEnvelopeInput;
use nimbus_network::{
    ListenerId, NetworkAttachmentCapabilitySet, NetworkAttachmentProviderRegistration,
    NetworkCapabilityBundle, NetworkCapabilityRequirements, NetworkCapabilitySelection,
    NetworkControlPlaneLocality, NetworkEndpointCapabilitySet, NetworkForwardingCapabilitySet,
    NetworkIngressCapabilitySet, NetworkIngressProviderRegistration, NetworkLifecycleCapabilitySet,
    NetworkManagementMode, NetworkProviderHandle, NetworkProviderId, NetworkResourceGeneration,
    NetworkSovereigntyCapabilities, NetworkSovereigntyRequirements, PublishedEndpointId,
};
use nimbus_node::{
    HostExecutionDrainProvider, HostExecutionStopProvider, HostLifecycleBackend,
    HostLifecycleFuture, HostLifecyclePlan, HostLifecycleRequest, HostLifecycleStatus,
    HostTeardownFuture,
};
use nimbus_sandbox::{
    ProviderCommandClaimDecision, SandboxBackendKind, SandboxExecutionAttemptId, SandboxId,
    SandboxOwnerSpec, SandboxProcessSpec, SandboxProvisionDependencyListener,
    SandboxProvisionNetworkPlan, SandboxRootSpec, SandboxSpec,
    backends::container::{ContainerSandboxBackend, ContainerSandboxBackendConfig},
    sandbox_network_plan_requirements,
};
use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, DesiredWorkloadKind, DesiredWorkloadState,
    LocalEnforcementBinding, NodeIdentity, WorkloadActivationIntent, WorkloadAdmissionEvidence,
    WorkloadDesiredDigest, WorkloadExecutableEncoding, WorkloadExecutableIntent,
    WorkloadExecutionProviderId, WorkloadExecutionReference, WorkloadGeneration,
    WorkloadNetworkAttachmentBlueprint, WorkloadNetworkIntent, WorkloadNetworkPlanContent,
    WorkloadNetworkPlanIdentity, WorkloadNetworkReference, WorkloadOwnerEvidenceDigest,
    WorkloadProvisionSourceEvidence, WorkloadProvisionSourceGeneration,
    WorkloadProvisionSourceIdentity, WorkloadProvisionSourceResourceVersion,
    WorkloadPublicationIntent, WorkloadPublicationReference, WorkloadSagaIntent, WorkloadSagaKey,
    WorkloadSagaRevision, WorkloadSagaTransitionId, WorkloadTeardownAttempt,
    WorkloadTeardownAttemptInput, WorkloadTeardownClaim, WorkloadTeardownCommandId,
    WorkloadTeardownCommandMode, WorkloadTeardownDispatchEpoch, WorkloadTeardownProviderTarget,
    WorkloadTeardownReceipt, WorkloadTeardownReceiptPrefix, WorkloadTeardownResultConfirmation,
    WorkloadTeardownRetryEvidence, WorkloadTeardownStep, WorkloadTeardownSubjects,
    WorkloadTeardownSuccessEvidence,
};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::sync::{Notify, Semaphore};
use tokio::time::{sleep, timeout};

use super::*;

const WAIT: Duration = Duration::from_secs(5);
const PROCESS_CHILD_TEST: &str = "machine::api::service_workloads::teardown::tests::acceptance::guest_workload_teardown_process_child";
const PROCESS_ROLE_ENV: &str = "NIMBUS_NNC65D4_GUEST_TEARDOWN_PROCESS_ROLE";
const PROCESS_STATE_ROOT_ENV: &str = "NIMBUS_NNC65D4_GUEST_TEARDOWN_STATE_ROOT";
const PROCESS_BUNDLES_ROOT_ENV: &str = "NIMBUS_NNC65D4_GUEST_TEARDOWN_BUNDLES_ROOT";
const PROCESS_RUNTIME_ENV: &str = "NIMBUS_NNC65D4_GUEST_TEARDOWN_RUNTIME";
const PROCESS_COMMAND_ENV: &str = "NIMBUS_NNC65D4_GUEST_TEARDOWN_COMMAND";
const PROCESS_FORWARDER_ENV: &str = "NIMBUS_NNC65D4_GUEST_TEARDOWN_FORWARDER";
const PROCESS_NODE_ENV: &str = "NIMBUS_NNC65D4_GUEST_TEARDOWN_NODE";
const PROCESS_RESULT_ENV: &str = "NIMBUS_NNC65D4_GUEST_TEARDOWN_RESULT";

struct ScriptedHostProvider {
    drain_executes: AtomicUsize,
    stop_executes: AtomicUsize,
    drain_inspects: AtomicUsize,
    stop_inspects: AtomicUsize,
    block_drain: bool,
    entered: Notify,
    release: Semaphore,
}

impl Default for ScriptedHostProvider {
    fn default() -> Self {
        Self {
            drain_executes: AtomicUsize::new(0),
            stop_executes: AtomicUsize::new(0),
            drain_inspects: AtomicUsize::new(0),
            stop_inspects: AtomicUsize::new(0),
            block_drain: false,
            entered: Notify::new(),
            release: Semaphore::new(0),
        }
    }
}

impl ScriptedHostProvider {
    fn blocking_drain() -> Self {
        Self {
            block_drain: true,
            ..Self::default()
        }
    }

    async fn wait_for_drain(&self) {
        timeout(WAIT, async {
            while self.drain_executes.load(Ordering::SeqCst) == 0 {
                self.entered.notified().await;
            }
        })
        .await
        .expect("the bounded host drain must start");
    }

    fn release_drain(&self) {
        self.release.add_permits(1);
    }
}

impl HostLifecycleBackend for ScriptedHostProvider {
    fn validate(
        &self,
        _binding: &LocalEnforcementBinding,
        _request: HostLifecycleRequest,
    ) -> nimbus_core::Result<HostLifecyclePlan> {
        Err(Error::PermissionDenied(
            "teardown acceptance provider has no lifecycle plan".to_owned(),
        ))
    }

    fn stop<'a>(
        &'a self,
        _execution_id: nimbus_workloads::WorkloadExecutionId,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        Box::pin(async {
            Err(Error::PermissionDenied(
                "teardown acceptance provider uses exact stop".to_owned(),
            ))
        })
    }

    fn inspect<'a>(
        &'a self,
        _execution_id: nimbus_workloads::WorkloadExecutionId,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        Box::pin(async {
            Err(Error::PermissionDenied(
                "teardown acceptance provider uses exact inspection".to_owned(),
            ))
        })
    }
}

impl HostExecutionDrainProvider for ScriptedHostProvider {
    fn execute_drain<'a>(
        &'a self,
        claim: HostTeardownExecuteClaim,
    ) -> HostTeardownFuture<'a, HostTeardownExecuteObservation> {
        Box::pin(async move {
            self.drain_executes.fetch_add(1, Ordering::SeqCst);
            self.entered.notify_waiters();
            if self.block_drain {
                self.release
                    .acquire()
                    .await
                    .expect("the test release semaphore must stay open")
                    .forget();
            }
            HostTeardownExecuteObservation::Succeeded(Box::new(execution_success(
                WorkloadTeardownStep::DrainExecution,
                claim.execution(),
            )))
        })
    }

    fn inspect_drain<'a>(
        &'a self,
        claim: HostTeardownInspectClaim,
    ) -> HostTeardownFuture<'a, HostTeardownInspectObservation> {
        Box::pin(async move {
            self.drain_inspects.fetch_add(1, Ordering::SeqCst);
            HostTeardownInspectObservation::Satisfied(Box::new(execution_success(
                WorkloadTeardownStep::DrainExecution,
                claim.execution(),
            )))
        })
    }
}

impl HostExecutionStopProvider for ScriptedHostProvider {
    fn execute_stop<'a>(
        &'a self,
        claim: HostTeardownExecuteClaim,
    ) -> HostTeardownFuture<'a, HostTeardownExecuteObservation> {
        Box::pin(async move {
            self.stop_executes.fetch_add(1, Ordering::SeqCst);
            HostTeardownExecuteObservation::Succeeded(Box::new(execution_success(
                WorkloadTeardownStep::StopExecution,
                claim.execution(),
            )))
        })
    }

    fn inspect_stop<'a>(
        &'a self,
        claim: HostTeardownInspectClaim,
    ) -> HostTeardownFuture<'a, HostTeardownInspectObservation> {
        Box::pin(async move {
            self.stop_inspects.fetch_add(1, Ordering::SeqCst);
            HostTeardownInspectObservation::Satisfied(Box::new(execution_success(
                WorkloadTeardownStep::StopExecution,
                claim.execution(),
            )))
        })
    }
}

fn execution_success(
    step: WorkloadTeardownStep,
    execution: &WorkloadExecutionReference,
) -> WorkloadTeardownSuccessEvidence {
    let evidence = WorkloadOwnerEvidenceDigest::sha256(format!("host-{step:?}"));
    match step {
        WorkloadTeardownStep::DrainExecution => WorkloadTeardownSuccessEvidence::ExecutionDrained {
            reference: execution.clone(),
            evidence,
        },
        WorkloadTeardownStep::StopExecution => WorkloadTeardownSuccessEvidence::ExecutionStopped {
            reference: execution.clone(),
            evidence,
        },
        _ => panic!("the guest host provider accepts only drain or stop"),
    }
}

struct AcceptanceHarness {
    _root: TempDir,
    state_root: PathBuf,
    backend: Arc<ContainerSandboxBackend>,
    host: Arc<ScriptedHostProvider>,
    service: Arc<GuestNodeWorkloadService>,
    fixture: TeardownFixture,
}

impl AcceptanceHarness {
    fn new(host: ScriptedHostProvider) -> Self {
        let root = tempfile::tempdir().expect("acceptance root should be created");
        let state_root = root.path().join("state");
        let fixture = TeardownFixture::new(false);
        let runtime = root.path().join("runtime-state-fixture");
        fs::write(
            &runtime,
            format!(
                "#!/bin/sh\nprintf '%s\\n' '{{\"id\":\"{}\",\"status\":\"running\"}}'\n",
                fixture.execution.execution_id().as_str()
            ),
        )
        .expect("runtime fixture should be written");
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o755))
            .expect("runtime fixture should be executable");
        let mut config =
            ContainerSandboxBackendConfig::plan_only(root.path().join("bundles"), &state_root);
        config.runtime_path = runtime;
        config.use_buildah_unshare = false;
        let backend = Arc::new(ContainerSandboxBackend::new(config));
        let sandbox_id = SandboxId::new(fixture.execution.execution_id().as_str());
        let execution_attempt_id =
            SandboxExecutionAttemptId::new(fixture.execution.attempt_id().as_str().to_owned())
                .expect("execution attempt should be a valid sandbox attempt");
        let spec = SandboxSpec::new(
            fixture.key.tenant_id().clone(),
            SandboxOwnerSpec::service("acceptance-service"),
            SandboxBackendKind::Container,
            SandboxRootSpec::rootfs(root.path().join("rootfs")),
            SandboxProcessSpec::new(["/bin/sh", "-c", "sleep 60"]),
        );
        let compiled = fixture.intent.network().compiled_plan();
        let dependency = SandboxProvisionDependencyListener::new(
            ListenerId::for_tenant_workload_listener(
                fixture.key.tenant_id(),
                "guest-teardown-incarnation",
                "egress-pep",
            ),
            "egress-pep",
            sandbox_network_plan_requirements(SandboxBackendKind::Container)
                .pep_provider_id()
                .clone(),
        );
        let provision_plan = SandboxProvisionNetworkPlan::new(
            compiled.plan().clone(),
            fixture.key.tenant_id().clone(),
            NetworkResourceGeneration::new(fixture.intent.generation().as_u64()),
            compiled
                .content()
                .attachment()
                .expect("fixture has an attachment")
                .attachment_id()
                .clone(),
            [],
            [dependency],
        )
        .expect("the exact compiled plan should lower to Container");
        backend
            .reserve_provision_network(
                spec,
                sandbox_id.clone(),
                execution_attempt_id.clone(),
                provision_plan,
            )
            .expect("PlanOnly network reservation should succeed");
        backend
            .prepare_provision_workload(&sandbox_id, &execution_attempt_id)
            .expect("PlanOnly workload preparation should succeed");

        let host = Arc::new(host);
        let service = Arc::new(GuestNodeWorkloadService::new_for_teardown_test(
            fixture.node.clone(),
            Arc::clone(&host),
            Arc::clone(&backend),
            &state_root,
        ));
        Self {
            _root: root,
            state_root,
            backend,
            host,
            service,
            fixture,
        }
    }

    fn command(
        &self,
        step: WorkloadTeardownStep,
        mode: WorkloadTeardownCommandMode,
    ) -> MachineApiWorkloadTeardownCommandEnvelope {
        self.fixture.initial_command(step, mode)
    }

    fn journal(&self) -> ProviderCommandAttemptJournal {
        self.backend
            .attempt_idempotency_journal()
            .expect("real Container provider journal should open")
    }

    fn seed_in_progress(
        &self,
        command: &MachineApiWorkloadTeardownCommandEnvelope,
    ) -> ProviderCommandClaim {
        let claim = provider_claim(command, &self.fixture.forwarder, &self.fixture.node)
            .expect("the exact guest provider claim should validate");
        assert!(matches!(
            self.journal().claim_dispatch_epoch(&claim),
            Ok(ProviderCommandClaimDecision::ExecuteClaimed(_))
        ));
        self.journal()
            .record_observation(
                &claim,
                ProviderCommandObservationKind::InProgress,
                b"durable composite child progress",
            )
            .expect("the real journal should retain prior composite progress");
        claim
    }

    async fn dispatch(
        &self,
        command: &MachineApiWorkloadTeardownCommandEnvelope,
    ) -> MachineApiWorkloadTeardownObservation {
        timeout(
            WAIT,
            super::dispatch(&self.service, command, &self.fixture.forwarder),
        )
        .await
        .expect("guest teardown dispatch must stay bounded")
        .expect("the private adapter should return a protocol observation")
        .observation()
        .clone()
    }
}

struct TeardownFixture {
    intent: WorkloadSagaIntent,
    key: WorkloadSagaKey,
    execution: WorkloadExecutionReference,
    network: WorkloadNetworkReference,
    publication: WorkloadPublicationReference,
    node: NodeIdentity,
    forwarder: MachineForwarderAuthority,
}

pub(crate) fn teardown_wire_fixture(
    step: WorkloadTeardownStep,
    mode: WorkloadTeardownCommandMode,
) -> (
    MachineForwarderAuthority,
    MachineApiWorkloadTeardownCommandEnvelope,
) {
    let fixture = TeardownFixture::new(false);
    let command = fixture.initial_command(step, mode);
    (fixture.forwarder, command)
}

pub(crate) fn teardown_wire_fixture_for_forwarder(
    step: WorkloadTeardownStep,
    mode: WorkloadTeardownCommandMode,
    provider_instance: &str,
    generation: u64,
) -> (
    MachineForwarderAuthority,
    MachineApiWorkloadTeardownCommandEnvelope,
) {
    let fixture = TeardownFixture::with_forwarder(false, provider_instance, generation);
    let command = fixture.initial_command(step, mode);
    (fixture.forwarder, command)
}

impl TeardownFixture {
    fn new(crossed_provider: bool) -> Self {
        Self::with_forwarder(crossed_provider, "guest-teardown-forwarder-instance", 7)
    }

    fn with_forwarder(
        crossed_provider: bool,
        forwarder_provider_instance: &str,
        forwarder_generation: u64,
    ) -> Self {
        let tenant_id = TenantId::new("tenant-guest-teardown").unwrap();
        let generation = WorkloadGeneration::new(1);
        let node = NodeIdentity::new("node-guest-teardown").unwrap();
        let attachment_provider =
            NetworkProviderId::for_registration_key("guest-teardown-attachment");
        let ingress_provider = NetworkProviderId::for_registration_key("guest-teardown-ingress");
        let selection =
            NetworkCapabilitySelection::new(attachment_provider.clone(), ingress_provider.clone());
        let selection_evidence = NetworkCapabilityBundle::new(
            NetworkAttachmentProviderRegistration::new(
                attachment_provider.clone(),
                NetworkAttachmentCapabilitySet::new(
                    NetworkManagementMode::NimbusHostManaged,
                    [],
                    [],
                ),
                [],
                NetworkLifecycleCapabilitySet::new([]),
                NetworkSovereigntyCapabilities::new(
                    NetworkControlPlaneLocality::LocalOnly,
                    [],
                    true,
                ),
            ),
            NetworkIngressProviderRegistration::new(
                ingress_provider,
                NetworkEndpointCapabilitySet::new([], [], [], [], []),
                NetworkIngressCapabilitySet::new([]),
                NetworkForwardingCapabilitySet::new([]),
                NetworkLifecycleCapabilitySet::new([]),
                NetworkSovereigntyCapabilities::new(
                    NetworkControlPlaneLocality::LocalOnly,
                    [],
                    true,
                ),
            ),
        )
        .selection_evidence();
        let identity = WorkloadNetworkPlanIdentity::new(
            tenant_id.clone(),
            "guest-teardown-incarnation",
            NetworkResourceGeneration::new(generation.as_u64()),
        )
        .unwrap();
        let requirements = NetworkCapabilityRequirements::new(
            NetworkAttachmentCapabilitySet::new(NetworkManagementMode::NimbusHostManaged, [], []),
            NetworkEndpointCapabilitySet::new([], [], [], [], []),
            NetworkIngressCapabilitySet::new([]),
            NetworkForwardingCapabilitySet::new([]),
            nimbus_network::NetworkLifecycleRequirements::new(
                NetworkLifecycleCapabilitySet::new([]),
                NetworkLifecycleCapabilitySet::new([]),
            ),
            NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
        );
        let content = WorkloadNetworkPlanContent::new(
            identity.clone(),
            requirements,
            Some(selection),
            Some(selection_evidence),
            Some(WorkloadNetworkAttachmentBlueprint::new(&identity, "primary").unwrap()),
            [],
            [],
            [],
            WorkloadActivationIntent::ActivateWhenAttached,
            WorkloadPublicationIntent::Withheld,
        )
        .unwrap();
        let compiled = CompiledWorkloadNetworkPlan::from_content(content).unwrap();
        let executable = WorkloadExecutableIntent::new(
            WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1,
            r#"{"fixture":"guest-teardown"}"#,
        )
        .unwrap();
        let execution_provider = if crossed_provider {
            WorkloadExecutionProviderId::for_registration_key("crossed-execution-provider")
        } else {
            crate::machine::backend::provision::forwarded_machine_execution_provider_id()
        };
        let source = WorkloadProvisionSourceEvidence::standalone_sandbox(
            WorkloadProvisionSourceIdentity::standalone_sandbox(
                "guest-teardown-workload",
                "guest-teardown-profile",
            )
            .unwrap(),
            WorkloadProvisionSourceGeneration::new(1),
            WorkloadProvisionSourceResourceVersion::new("guest-teardown-version").unwrap(),
            executable.content_digest(),
            attachment_provider,
            execution_provider,
        )
        .unwrap();
        let intent = WorkloadSagaIntent::new_without_automatic_restart(
            DesiredWorkloadKind::Sandbox,
            DesiredWorkloadState::Running,
            generation,
            executable,
            source,
            WorkloadNetworkIntent::new(compiled),
            WorkloadActivationIntent::ActivateWhenAttached,
            WorkloadPublicationIntent::Withheld,
            WorkloadAdmissionEvidence::new(
                format!("tid_{}", "a".repeat(64)).try_into().unwrap(),
                format!("twu_{}", "b".repeat(64)).try_into().unwrap(),
                node.clone(),
            ),
        )
        .unwrap();
        let key = WorkloadSagaKey::new(
            tenant_id,
            WorkloadId::new("guest-teardown-workload").unwrap(),
        );
        let execution = WorkloadExecutionReference::for_intent(&intent);
        let network = WorkloadNetworkReference::for_intent(&intent);
        let publication = WorkloadPublicationReference::new(
            [PublishedEndpointId::for_workload_endpoint(
                "guest-teardown-incarnation",
                "api",
            )],
            &intent,
        )
        .unwrap();
        let forwarder = MachineForwarderAuthority::new(
            NetworkProviderHandle::new(
                NetworkProviderId::for_registration_key("guest-teardown-forwarder"),
                forwarder_provider_instance,
            )
            .unwrap(),
            NetworkResourceGeneration::new(forwarder_generation),
        );
        Self {
            intent,
            key,
            execution,
            network,
            publication,
            node,
            forwarder,
        }
    }

    fn transition(&self, ordinal: usize) -> WorkloadSagaTransitionId {
        let value = (b'a' + u8::try_from(ordinal % 6).unwrap()) as char;
        format!("wst_{}", value.to_string().repeat(64))
            .try_into()
            .unwrap()
    }

    fn attempt(&self, step: WorkloadTeardownStep) -> WorkloadTeardownAttempt {
        let index = step_index(step);
        let (source_phase, target_phase) = step.phases();
        WorkloadTeardownAttempt::new(WorkloadTeardownAttemptInput {
            key: self.key.clone(),
            saga_id: self.key.saga_id(),
            issuing_revision: WorkloadSagaRevision::new((index * 2 + 1) as u64),
            issuing_transition_id: self.transition(index * 2),
            generation: self.intent.generation(),
            desired_digest: self.intent.desired_digest(),
            required_node: self.node.clone(),
            source_digest: self.intent.source().source_digest(),
            execution_provider_id: self.intent.source().execution_provider_id().clone(),
            network_plan_digest: self.intent.network().digest(),
            selection_evidence: self
                .intent
                .network()
                .compiled_plan()
                .content()
                .capability_selection_evidence()
                .cloned(),
            cause: nimbus_workloads::WorkloadTeardownCause::Successor {
                generation: WorkloadGeneration::new(2),
                desired_digest: WorkloadDesiredDigest::sha256("guest-teardown-successor"),
            },
            successor_fence: None,
            source_phase,
            target_phase,
            step,
            subjects: match step {
                WorkloadTeardownStep::WithdrawPublication => {
                    WorkloadTeardownSubjects::Publication(self.publication.clone())
                }
                WorkloadTeardownStep::DrainExecution | WorkloadTeardownStep::StopExecution => {
                    WorkloadTeardownSubjects::Execution(self.execution.clone())
                }
                WorkloadTeardownStep::DetachNetwork | WorkloadTeardownStep::ReleaseNetwork => {
                    WorkloadTeardownSubjects::Network(self.network.clone())
                }
            },
        })
        .unwrap()
    }

    fn claim(&self, step: WorkloadTeardownStep) -> WorkloadTeardownClaim {
        let attempt = self.attempt(step);
        let provider_target = WorkloadTeardownProviderTarget::for_attempt(&attempt)
            .unwrap()
            .unwrap();
        serde_json::from_value(json!({
            "attempt": attempt,
            "claimedRevision": attempt.issuing_revision().checked_next().unwrap(),
            "dispatchEpoch": "0",
            "providerTarget": provider_target,
            "authorization": { "kind": "initial" },
        }))
        .unwrap()
    }

    fn receipt(&self, step: WorkloadTeardownStep) -> WorkloadTeardownReceipt {
        let claim = self.claim(step);
        let evidence = match claim.attempt().subjects() {
            WorkloadTeardownSubjects::Publication(reference) => {
                WorkloadTeardownSuccessEvidence::PublicationAbsent {
                    reference: reference.clone(),
                    evidence: WorkloadOwnerEvidenceDigest::sha256("prior-publication"),
                }
            }
            WorkloadTeardownSubjects::Execution(reference) => execution_success(step, reference),
            WorkloadTeardownSubjects::Network(reference) => match step {
                WorkloadTeardownStep::DetachNetwork => {
                    WorkloadTeardownSuccessEvidence::NetworkDetached {
                        reference: reference.clone(),
                        evidence: WorkloadOwnerEvidenceDigest::sha256("prior-network-detached"),
                    }
                }
                WorkloadTeardownStep::ReleaseNetwork => {
                    WorkloadTeardownSuccessEvidence::NetworkReleased {
                        reference: reference.clone(),
                        evidence: WorkloadOwnerEvidenceDigest::sha256("prior-network-released"),
                    }
                }
                _ => panic!("network receipt fixture crossed the teardown step"),
            },
        };
        serde_json::from_value(json!({
            "claim": claim,
            "evidence": evidence,
            "confirmation": WorkloadTeardownResultConfirmation::Dispatch,
        }))
        .unwrap()
    }

    fn prefix(&self, step: WorkloadTeardownStep) -> WorkloadTeardownReceiptPrefix {
        let receipts: Vec<_> = all_steps()
            .into_iter()
            .take(step_index(step))
            .map(|prior| self.receipt(prior))
            .collect();
        serde_json::from_value(json!({ "receipts": receipts })).unwrap()
    }

    fn initial_command(
        &self,
        step: WorkloadTeardownStep,
        mode: WorkloadTeardownCommandMode,
    ) -> MachineApiWorkloadTeardownCommandEnvelope {
        let claim = self.claim(step);
        let confirmed_revision = match mode {
            WorkloadTeardownCommandMode::Execute => claim.claimed_revision(),
            WorkloadTeardownCommandMode::Inspect => {
                claim.claimed_revision().checked_next().unwrap()
            }
        };
        self.command(
            claim,
            mode,
            confirmed_revision,
            self.transition(step_index(step) * 2 + 1),
        )
    }

    fn command(
        &self,
        claim: WorkloadTeardownClaim,
        mode: WorkloadTeardownCommandMode,
        confirmed_revision: WorkloadSagaRevision,
        confirmed_transition_id: WorkloadSagaTransitionId,
    ) -> MachineApiWorkloadTeardownCommandEnvelope {
        let provider_translation = match claim.attempt().step() {
            WorkloadTeardownStep::DrainExecution | WorkloadTeardownStep::StopExecution => {
                MachineApiWorkloadTeardownProviderTranslation::GuestExecutionComposition
            }
            WorkloadTeardownStep::DetachNetwork | WorkloadTeardownStep::ReleaseNetwork => {
                MachineApiWorkloadTeardownProviderTranslation::GuestContainerAttachment
            }
            WorkloadTeardownStep::WithdrawPublication => {
                panic!("guest teardown transport must not lower parent-local withdrawal")
            }
        };
        let command_id = WorkloadTeardownCommandId::for_confirmed_dispatch(
            &claim,
            confirmed_revision,
            &confirmed_transition_id,
            mode,
        )
        .unwrap();
        MachineApiWorkloadTeardownCommandEnvelope::new(
            MachineApiWorkloadTeardownCommandEnvelopeInput {
                command_id,
                confirmed_revision,
                confirmed_transition_id,
                source: self.intent.source().clone(),
                compiled_network_plan: self.intent.network().compiled_plan().clone(),
                execution_locator: self.execution.clone(),
                prior_receipt_prefix: self.prefix(claim.attempt().step()),
                mode,
                claim,
                machine_forwarder_authority: self.forwarder.clone(),
                machine_provider_generation: self.forwarder.generation(),
                provider_translation,
            },
        )
        .unwrap()
    }

    fn retry_command(
        &self,
        inspected: &MachineApiWorkloadTeardownCommandEnvelope,
        evidence: WorkloadOwnerEvidenceDigest,
    ) -> MachineApiWorkloadTeardownCommandEnvelope {
        let retry_evidence: WorkloadTeardownRetryEvidence = serde_json::from_value(json!({
            "attemptId": inspected.attempt_id(),
            "dispatchEpoch": inspected.claim().dispatch_epoch(),
            "inspectedRevision": inspected.confirmed_revision(),
            "inspectedTransitionId": inspected.confirmed_transition_id(),
            "inspectionCommandId": inspected.command_id(),
            "providerTarget": inspected.claim().provider_target(),
            "step": inspected.step(),
            "evidence": evidence,
        }))
        .unwrap();
        let claimed_revision = inspected.confirmed_revision().checked_next().unwrap();
        let retry_claim: WorkloadTeardownClaim = serde_json::from_value(json!({
            "attempt": inspected.claim().attempt(),
            "claimedRevision": claimed_revision,
            "dispatchEpoch": WorkloadTeardownDispatchEpoch::new(1),
            "providerTarget": inspected.claim().provider_target(),
            "authorization": {
                "kind": "retry_after_not_completed",
                "evidence": retry_evidence,
            },
        }))
        .unwrap();
        self.command(
            retry_claim,
            WorkloadTeardownCommandMode::Execute,
            claimed_revision,
            self.transition(6),
        )
    }
}

fn all_steps() -> [WorkloadTeardownStep; 5] {
    [
        WorkloadTeardownStep::WithdrawPublication,
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownStep::StopExecution,
        WorkloadTeardownStep::DetachNetwork,
        WorkloadTeardownStep::ReleaseNetwork,
    ]
}

fn step_index(step: WorkloadTeardownStep) -> usize {
    all_steps()
        .iter()
        .position(|candidate| *candidate == step)
        .unwrap()
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn collect(root: &Path, path: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        if !path.exists() {
            return;
        }
        let metadata = fs::symlink_metadata(path).unwrap();
        if metadata.is_file() {
            out.insert(
                path.strip_prefix(root).unwrap().to_owned(),
                fs::read(path).unwrap(),
            );
            return;
        }
        if metadata.is_dir() {
            let mut children: Vec<_> = fs::read_dir(path)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect();
            children.sort();
            for child in children {
                collect(root, &child, out);
            }
        }
    }
    let mut out = BTreeMap::new();
    collect(root, root, &mut out);
    out
}

fn record_files(state_root: &Path) -> Vec<PathBuf> {
    snapshot(state_root)
        .into_keys()
        .filter(|path| {
            path.components()
                .any(|component| component.as_os_str() == ".nimbus-provider-command-attempts")
                && path
                    .extension()
                    .is_some_and(|extension| extension == "json")
        })
        .collect()
}

fn write_terminal_runtime_receipt(state_root: &Path) {
    let manifest = snapshot(state_root)
        .into_iter()
        .find(|(path, _)| path.file_name().is_some_and(|name| name == "manifest.json"))
        .expect("the real PlanOnly manifest must exist")
        .1;
    let manifest: Value = serde_json::from_slice(&manifest).unwrap();
    let exit = PathBuf::from(
        manifest["conmon_layout"]["exit_status_file"]
            .as_str()
            .expect("manifest must retain the exact conmon exit receipt path"),
    );
    fs::create_dir_all(exit.parent().unwrap()).unwrap();
    fs::write(exit, b"0\n").unwrap();
}

fn assert_execute_succeeded(observation: &MachineApiWorkloadTeardownObservation) {
    assert!(matches!(
        observation,
        MachineApiWorkloadTeardownObservation::Execute(
            MachineApiWorkloadTeardownExecuteObservation::Succeeded { .. }
        )
    ));
}

fn not_completed_evidence(
    observation: &MachineApiWorkloadTeardownObservation,
) -> WorkloadOwnerEvidenceDigest {
    match observation {
        MachineApiWorkloadTeardownObservation::Inspect(
            MachineApiWorkloadTeardownInspectObservation::NotCompleted { evidence },
        ) => *evidence,
        other => panic!("expected exact NotCompleted inspection, got {other:?}"),
    }
}

#[tokio::test]
async fn guest_workload_teardown_duplicate_replays_one_composite_result() {
    let harness = AcceptanceHarness::new(ScriptedHostProvider::default());
    let command = harness.command(
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownCommandMode::Execute,
    );

    let first = harness.dispatch(&command).await;
    assert_execute_succeeded(&first);
    let after_first = snapshot(&harness.state_root);
    let replay = harness.dispatch(&command).await;

    assert_eq!(replay, first, "an exact duplicate must replay one result");
    assert_eq!(
        snapshot(&harness.state_root),
        after_first,
        "replay must not write a second journal or child receipt"
    );
    assert_eq!(harness.host.drain_executes.load(Ordering::SeqCst), 1);
    assert_eq!(record_files(&harness.state_root).len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn guest_workload_teardown_two_thread_contenders_have_one_winner() {
    let harness = Arc::new(AcceptanceHarness::new(
        ScriptedHostProvider::blocking_drain(),
    ));
    let command = Arc::new(harness.command(
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownCommandMode::Execute,
    ));

    let first = {
        let harness = Arc::clone(&harness);
        let command = Arc::clone(&command);
        tokio::spawn(async move { harness.dispatch(&command).await })
    };
    harness.host.wait_for_drain().await;
    let second = {
        let harness = Arc::clone(&harness);
        let command = Arc::clone(&command);
        tokio::spawn(async move { harness.dispatch(&command).await })
    };
    sleep(Duration::from_millis(100)).await;
    assert_eq!(
        harness.host.drain_executes.load(Ordering::SeqCst),
        1,
        "the contender must not reach the host child"
    );
    harness.host.release_drain();

    let (first, second) = timeout(WAIT, async {
        (first.await.unwrap(), second.await.unwrap())
    })
    .await
    .expect("both bounded contenders must converge");
    assert_execute_succeeded(&first);
    assert_eq!(second, first);
    assert_eq!(harness.host.drain_executes.load(Ordering::SeqCst), 1);
    assert_eq!(record_files(&harness.state_root).len(), 1);
}

#[tokio::test]
async fn guest_workload_teardown_inspect_join_is_closed_and_deterministic() {
    let harness = AcceptanceHarness::new(ScriptedHostProvider::default());
    let execute = harness.command(
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownCommandMode::Execute,
    );
    harness.seed_in_progress(&execute);
    let inspect = harness.command(
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownCommandMode::Inspect,
    );
    let before = snapshot(&harness.state_root);

    let first = harness.dispatch(&inspect).await;
    let replay = harness.dispatch(&inspect).await;

    let _ = not_completed_evidence(&first);
    assert_eq!(replay, first, "the closed child join must be deterministic");
    assert_eq!(
        snapshot(&harness.state_root),
        before,
        "Inspect must be byte-stable across the real journal and Container state"
    );
    assert_eq!(harness.host.drain_inspects.load(Ordering::SeqCst), 2);
    assert_eq!(record_files(&harness.state_root).len(), 1);
}

#[tokio::test]
async fn guest_workload_teardown_no_record_inspect_cannot_authorize_adjacent_retry() {
    let harness = AcceptanceHarness::new(ScriptedHostProvider::default());
    let inspect = harness.command(
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownCommandMode::Inspect,
    );
    let before_inspect = snapshot(&harness.state_root);

    let missing = harness.dispatch(&inspect).await;
    assert!(matches!(
        missing,
        MachineApiWorkloadTeardownObservation::Inspect(
            MachineApiWorkloadTeardownInspectObservation::Ambiguous
        )
    ));
    assert_eq!(snapshot(&harness.state_root), before_inspect);

    let forged_retry = harness.fixture.retry_command(
        &inspect,
        WorkloadOwnerEvidenceDigest::sha256("no durable epoch-zero observation"),
    );
    let rejected = harness.dispatch(&forged_retry).await;
    assert_definite_failure(&rejected);
    assert!(record_files(&harness.state_root).is_empty());
    assert_eq!(harness.host.drain_executes.load(Ordering::SeqCst), 0);

    let delayed_execute = harness.command(
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownCommandMode::Execute,
    );
    let executed = harness.dispatch(&delayed_execute).await;
    assert_execute_succeeded(&executed);
    let after_execute = snapshot(&harness.state_root);
    let replay = harness.dispatch(&delayed_execute).await;
    assert_eq!(replay, executed);
    assert_eq!(snapshot(&harness.state_root), after_execute);
    assert_eq!(harness.host.drain_executes.load(Ordering::SeqCst), 1);
    assert_eq!(record_files(&harness.state_root).len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn guest_workload_teardown_inspect_contender_cannot_cross_older_execute() {
    let harness = Arc::new(AcceptanceHarness::new(
        ScriptedHostProvider::blocking_drain(),
    ));
    let execute = Arc::new(harness.command(
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownCommandMode::Execute,
    ));
    let inspect = Arc::new(harness.command(
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownCommandMode::Inspect,
    ));
    let execute_task = {
        let harness = Arc::clone(&harness);
        let execute = Arc::clone(&execute);
        tokio::spawn(async move { harness.dispatch(&execute).await })
    };
    harness.host.wait_for_drain().await;
    let inspect_task = {
        let harness = Arc::clone(&harness);
        let inspect = Arc::clone(&inspect);
        tokio::spawn(async move { harness.dispatch(&inspect).await })
    };
    sleep(Duration::from_millis(100)).await;
    assert!(
        !inspect_task.is_finished(),
        "Inspect must wait behind the exact older Execute stream lock"
    );
    harness.host.release_drain();

    let (executed, inspected) = timeout(WAIT, async {
        (execute_task.await.unwrap(), inspect_task.await.unwrap())
    })
    .await
    .expect("the Execute and its contender must converge");
    assert_execute_succeeded(&executed);
    assert!(matches!(
        inspected,
        MachineApiWorkloadTeardownObservation::Inspect(
            MachineApiWorkloadTeardownInspectObservation::Satisfied { .. }
        )
    ));
    assert_eq!(harness.host.drain_executes.load(Ordering::SeqCst), 1);
    assert_eq!(
        harness.host.drain_inspects.load(Ordering::SeqCst),
        0,
        "the joined terminal journal result must avoid a second child inspection"
    );
}

#[tokio::test]
async fn guest_workload_teardown_inspected_absence_authorizes_one_adjacent_retry() {
    let harness = AcceptanceHarness::new(ScriptedHostProvider::default());
    let drain = harness.command(
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownCommandMode::Execute,
    );
    assert_execute_succeeded(&harness.dispatch(&drain).await);

    let stop_execute = harness.command(
        WorkloadTeardownStep::StopExecution,
        WorkloadTeardownCommandMode::Execute,
    );
    harness.seed_in_progress(&stop_execute);
    let stop_inspect = harness.command(
        WorkloadTeardownStep::StopExecution,
        WorkloadTeardownCommandMode::Inspect,
    );

    let before_live_inspect = snapshot(&harness.state_root);
    let live = harness.dispatch(&stop_inspect).await;
    assert!(matches!(
        live,
        MachineApiWorkloadTeardownObservation::Inspect(
            MachineApiWorkloadTeardownInspectObservation::InProgress { .. }
        )
    ));
    assert_eq!(snapshot(&harness.state_root), before_live_inspect);

    write_terminal_runtime_receipt(&harness.state_root);
    let before_absent_inspect = snapshot(&harness.state_root);
    let absent = harness.dispatch(&stop_inspect).await;
    let absent_evidence = not_completed_evidence(&absent);
    assert_eq!(
        snapshot(&harness.state_root),
        before_absent_inspect,
        "terminal-without-fence Inspect must remain byte-stable"
    );

    let retry = harness
        .fixture
        .retry_command(&stop_inspect, absent_evidence);
    let succeeded = harness.dispatch(&retry).await;
    assert_execute_succeeded(&succeeded);
    let after_retry = snapshot(&harness.state_root);
    let replay = harness.dispatch(&retry).await;

    assert_eq!(replay, succeeded);
    assert_eq!(snapshot(&harness.state_root), after_retry);
    assert_eq!(harness.host.stop_executes.load(Ordering::SeqCst), 1);
    assert_eq!(
        record_files(&harness.state_root).len(),
        2,
        "drain and stop use two streams in the one Container-rooted journal"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn guest_workload_teardown_never_publishes_terminal_for_one_child() {
    let harness = Arc::new(AcceptanceHarness::new(
        ScriptedHostProvider::blocking_drain(),
    ));
    let command = Arc::new(harness.command(
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownCommandMode::Execute,
    ));
    let manifest_before = manifest_bytes(&harness.state_root);
    let task = {
        let harness = Arc::clone(&harness);
        let command = Arc::clone(&command);
        tokio::spawn(async move { harness.dispatch(&command).await })
    };
    harness.host.wait_for_drain().await;

    let records = record_files(&harness.state_root);
    assert_eq!(records.len(), 1);
    let envelope: Value = serde_json::from_slice(
        &fs::read(harness.state_root.join(&records[0])).expect("journal record should read"),
    )
    .unwrap();
    assert_eq!(envelope["observation"]["kind"], "claimed");
    assert_eq!(
        manifest_bytes(&harness.state_root),
        manifest_before,
        "Container must not publish its child before Systemd returns terminal success"
    );

    harness.host.release_drain();
    let completed = timeout(WAIT, task)
        .await
        .expect("the bounded composite must complete")
        .unwrap();
    assert_execute_succeeded(&completed);
    assert_eq!(harness.host.drain_executes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn guest_workload_teardown_crossed_authority_fails_before_journal_bytes() {
    let harness = AcceptanceHarness::new(ScriptedHostProvider::default());
    let command = harness.command(
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownCommandMode::Execute,
    );
    let before = snapshot(&harness.state_root);

    let crossed_forwarder = MachineForwarderAuthority::new(
        NetworkProviderHandle::new(
            NetworkProviderId::for_registration_key("crossed-forwarder"),
            "crossed-forwarder-instance",
        )
        .unwrap(),
        NetworkResourceGeneration::new(7),
    );
    let forwarder_result = timeout(
        WAIT,
        super::dispatch(&harness.service, &command, &crossed_forwarder),
    )
    .await
    .unwrap()
    .unwrap();
    assert_definite_failure(forwarder_result.observation());
    assert_eq!(snapshot(&harness.state_root), before);

    let other_node = GuestNodeWorkloadService::new_for_teardown_test(
        NodeIdentity::new("crossed-node").unwrap(),
        Arc::clone(&harness.host),
        Arc::clone(&harness.backend),
        &harness.state_root,
    );
    let node_result = timeout(
        WAIT,
        super::dispatch(&other_node, &command, &harness.fixture.forwarder),
    )
    .await
    .unwrap()
    .unwrap();
    assert_definite_failure(node_result.observation());
    assert_eq!(snapshot(&harness.state_root), before);

    let crossed_provider = TeardownFixture::new(true);
    let provider_command = crossed_provider.initial_command(
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownCommandMode::Execute,
    );
    let provider_result = timeout(
        WAIT,
        super::dispatch(
            &harness.service,
            &provider_command,
            &crossed_provider.forwarder,
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_definite_failure(provider_result.observation());
    assert_eq!(snapshot(&harness.state_root), before);
    assert!(record_files(&harness.state_root).is_empty());
    assert_eq!(harness.host.drain_executes.load(Ordering::SeqCst), 0);
}

fn manifest_bytes(state_root: &Path) -> Vec<u8> {
    snapshot(state_root)
        .into_iter()
        .find(|(path, _)| path.file_name().is_some_and(|name| name == "manifest.json"))
        .expect("one real Container manifest must exist")
        .1
}

fn assert_definite_failure(observation: &MachineApiWorkloadTeardownObservation) {
    assert!(matches!(
        observation,
        MachineApiWorkloadTeardownObservation::Execute(
            MachineApiWorkloadTeardownExecuteObservation::DefiniteFailure { .. }
        )
    ));
}

#[test]
fn guest_workload_teardown_fresh_process_recovers_claim_and_replays_terminal_result() {
    let harness = AcceptanceHarness::new(ScriptedHostProvider::default());
    let execute = harness.command(
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownCommandMode::Execute,
    );
    let inspect = harness.command(
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownCommandMode::Inspect,
    );

    let claimed = run_process_child(&harness, &execute, "claim", "claim");
    assert_eq!(claimed["outcome"], "claimed");
    let claimed_bytes = snapshot(&harness.state_root);

    let inspected = run_process_child(&harness, &inspect, "dispatch", "inspect");
    let inspected_observation: MachineApiWorkloadTeardownObservation =
        serde_json::from_value(inspected["observation"].clone()).unwrap();
    assert!(matches!(
        inspected_observation,
        MachineApiWorkloadTeardownObservation::Inspect(
            MachineApiWorkloadTeardownInspectObservation::InProgress { .. }
        )
    ));
    assert_eq!(inspected["drainExecutes"], 0);
    assert_eq!(inspected["drainInspects"], 0);
    assert_eq!(snapshot(&harness.state_root), claimed_bytes);

    let executed = run_process_child(&harness, &execute, "dispatch", "execute");
    let executed_observation: MachineApiWorkloadTeardownObservation =
        serde_json::from_value(executed["observation"].clone()).unwrap();
    assert_execute_succeeded(&executed_observation);
    assert_eq!(executed["drainExecutes"], 1);
    let terminal_bytes = snapshot(&harness.state_root);

    let replayed = run_process_child(&harness, &execute, "dispatch", "replay");
    let replayed_observation: MachineApiWorkloadTeardownObservation =
        serde_json::from_value(replayed["observation"].clone()).unwrap();
    assert_eq!(executed_observation, replayed_observation);
    assert_eq!(replayed["drainExecutes"], 0);
    assert_eq!(replayed["drainInspects"], 0);
    assert_eq!(snapshot(&harness.state_root), terminal_bytes);
}

#[test]
#[ignore = "subprocess entry point; the NNC6.5d4 parent supplies exact durable roots"]
fn guest_workload_teardown_process_child() {
    let role = std::env::var(PROCESS_ROLE_ENV).expect("process role must be supplied");
    let state_root = PathBuf::from(
        std::env::var_os(PROCESS_STATE_ROOT_ENV).expect("state root must be supplied"),
    );
    let bundles_root = PathBuf::from(
        std::env::var_os(PROCESS_BUNDLES_ROOT_ENV).expect("bundles root must be supplied"),
    );
    let runtime_path = PathBuf::from(
        std::env::var_os(PROCESS_RUNTIME_ENV).expect("runtime path must be supplied"),
    );
    let command_path = PathBuf::from(
        std::env::var_os(PROCESS_COMMAND_ENV).expect("command path must be supplied"),
    );
    let forwarder_path = PathBuf::from(
        std::env::var_os(PROCESS_FORWARDER_ENV).expect("forwarder path must be supplied"),
    );
    let result_path =
        PathBuf::from(std::env::var_os(PROCESS_RESULT_ENV).expect("result path must be supplied"));
    let node =
        NodeIdentity::new(std::env::var(PROCESS_NODE_ENV).expect("node identity must be supplied"))
            .expect("node identity must validate");
    let command: MachineApiWorkloadTeardownCommandEnvelope =
        serde_json::from_slice(&fs::read(command_path).unwrap()).unwrap();
    let forwarder: MachineForwarderAuthority =
        serde_json::from_slice(&fs::read(forwarder_path).unwrap()).unwrap();

    let mut config = ContainerSandboxBackendConfig::plan_only(bundles_root, &state_root);
    config.runtime_path = runtime_path;
    config.use_buildah_unshare = false;
    let backend = Arc::new(ContainerSandboxBackend::new(config));
    let host = Arc::new(ScriptedHostProvider::default());
    let service = GuestNodeWorkloadService::new_for_teardown_test(
        node.clone(),
        Arc::clone(&host),
        Arc::clone(&backend),
        &state_root,
    );

    let output = if role == "claim" {
        let claim = provider_claim(&command, &forwarder, &node).unwrap();
        assert!(matches!(
            backend
                .attempt_idempotency_journal()
                .unwrap()
                .claim_dispatch_epoch(&claim),
            Ok(ProviderCommandClaimDecision::ExecuteClaimed(_))
        ));
        json!({ "outcome": "claimed" })
    } else {
        assert_eq!(role, "dispatch");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let observation = runtime
            .block_on(super::dispatch(&service, &command, &forwarder))
            .unwrap();
        json!({
            "outcome": "dispatched",
            "observation": observation.observation(),
            "drainExecutes": host.drain_executes.load(Ordering::SeqCst),
            "stopExecutes": host.stop_executes.load(Ordering::SeqCst),
            "drainInspects": host.drain_inspects.load(Ordering::SeqCst),
            "stopInspects": host.stop_inspects.load(Ordering::SeqCst),
        })
    };
    fs::write(result_path, serde_json::to_vec(&output).unwrap()).unwrap();
}

fn run_process_child(
    harness: &AcceptanceHarness,
    command: &MachineApiWorkloadTeardownCommandEnvelope,
    role: &str,
    label: &str,
) -> Value {
    let command_path = harness._root.path().join(format!("{label}-command.json"));
    let forwarder_path = harness._root.path().join("forwarder.json");
    let result_path = harness._root.path().join(format!("{label}-result.json"));
    fs::write(&command_path, serde_json::to_vec(command).unwrap()).unwrap();
    fs::write(
        &forwarder_path,
        serde_json::to_vec(&harness.fixture.forwarder).unwrap(),
    )
    .unwrap();
    let output = Command::new(std::env::current_exe().expect("CLI test executable must resolve"))
        .args(["--exact", PROCESS_CHILD_TEST, "--ignored", "--nocapture"])
        .env(PROCESS_ROLE_ENV, role)
        .env(PROCESS_STATE_ROOT_ENV, &harness.state_root)
        .env(
            PROCESS_BUNDLES_ROOT_ENV,
            harness._root.path().join("bundles"),
        )
        .env(
            PROCESS_RUNTIME_ENV,
            harness._root.path().join("runtime-state-fixture"),
        )
        .env(PROCESS_COMMAND_ENV, command_path)
        .env(PROCESS_FORWARDER_ENV, forwarder_path)
        .env(PROCESS_NODE_ENV, harness.fixture.node.as_str())
        .env(PROCESS_RESULT_ENV, &result_path)
        .output()
        .expect("fresh CLI test process must launch");
    assert!(
        output.status.success(),
        "fresh CLI test process failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&fs::read(result_path).unwrap()).unwrap()
}
