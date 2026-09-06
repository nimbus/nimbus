use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use nimbus_process_harness::PortWindow;

use crate::backends::conmon::runtime_process::{
    RuntimeProcessIdentity, RuntimeProcessIdentityObservation, RuntimeProcessSignal,
    RuntimeProcessSignalOutcome,
};
use crate::backends::container::runtime::support::{
    sample_execution_attempt_id, sample_provision_network_plan, sample_spec_for_tenant,
};
use crate::backends::oci::command::CommandSpec;
use crate::backends::oci::network::FixedOciEgressPinProvider;
use crate::{
    ProviderCommandClaimDecision, ProviderCommandClaimInput, ProviderCommandObservationKind,
    ProviderCommandOperation, SandboxExecutionTeardownCommand, SandboxExecutionTeardownObservation,
    SandboxExecutionTeardownOperation,
};

use super::*;

#[path = "tests/composite_substep.rs"]
mod composite_substep;
#[path = "tests/external_stop_bridge.rs"]
mod external_stop_bridge;
#[path = "tests/fresh_process.rs"]
mod fresh_process;
#[path = "tests/network_teardown.rs"]
mod network_teardown;
#[path = "tests/retry_recovery.rs"]
mod retry_recovery;

struct TeardownFixture {
    root: tempfile::TempDir,
    /// Holds the host ports this fixture handed to the backend. The claim must
    /// outlive every bind those ports feed, so it lives in the fixture rather
    /// than in the constructor that took it. Fixtures that never publish a
    /// port carry `None`.
    port_window: Option<PortWindow>,
    backend: ContainerSandboxBackend,
    id: crate::SandboxId,
    execution_attempt_id: crate::SandboxExecutionAttemptId,
}

#[derive(Clone)]
struct ScriptedRuntime {
    backend: ContainerSandboxBackend,
    now_unix_millis: Arc<AtomicU64>,
    terminal: Arc<AtomicBool>,
    terminal_unknown: Arc<AtomicBool>,
    terminal_inspections: Arc<AtomicU64>,
    process_observation: Arc<Mutex<RuntimeProcessIdentityObservation>>,
    signals: Arc<Mutex<Vec<i32>>>,
}

impl ScriptedRuntime {
    fn live(backend: ContainerSandboxBackend, now_unix_millis: u64) -> Self {
        Self {
            backend,
            now_unix_millis: Arc::new(AtomicU64::new(now_unix_millis)),
            terminal: Arc::new(AtomicBool::new(false)),
            terminal_unknown: Arc::new(AtomicBool::new(false)),
            terminal_inspections: Arc::new(AtomicU64::new(0)),
            process_observation: Arc::new(Mutex::new(RuntimeProcessIdentityObservation::ExactLive)),
            signals: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn set_now(&self, value: u64) {
        self.now_unix_millis.store(value, Ordering::Release);
    }

    fn set_process_observation(&self, observation: RuntimeProcessIdentityObservation) {
        *self
            .process_observation
            .lock()
            .expect("scripted process observation lock should be healthy") = observation;
    }

    fn set_terminal_unknown(&self, value: bool) {
        self.terminal_unknown.store(value, Ordering::Release);
    }

    fn terminal_inspections(&self) -> u64 {
        self.terminal_inspections.load(Ordering::Acquire)
    }

    fn signals(&self) -> Vec<i32> {
        self.signals
            .lock()
            .expect("scripted signal lock should be healthy")
            .clone()
    }
}

impl effects::ContainerExecutionTeardownRuntime for ScriptedRuntime {
    fn now_unix_millis(&self) -> crate::Result<u64> {
        Ok(self.now_unix_millis.load(Ordering::Acquire))
    }

    fn execution_is_terminal(&self, _manifest: &ContainerSandboxManifest) -> crate::Result<bool> {
        self.terminal_inspections.fetch_add(1, Ordering::AcqRel);
        if self.terminal_unknown.load(Ordering::Acquire) {
            return Err(SandboxError::OperationFailed {
                message: "scripted Container runtime terminality is unknown".to_owned(),
            });
        }
        Ok(self.terminal.load(Ordering::Acquire))
    }

    fn capture_process(
        &self,
        manifest: &ContainerSandboxManifest,
    ) -> crate::Result<RuntimeProcessIdentity> {
        Ok(RuntimeProcessIdentity::fixture(
            manifest.handle.id.as_str(),
            "creator-attempt-fixture",
            42,
        ))
    }

    fn inspect_process(
        &self,
        _manifest: &ContainerSandboxManifest,
        _identity: &RuntimeProcessIdentity,
    ) -> crate::Result<RuntimeProcessIdentityObservation> {
        Ok(*self
            .process_observation
            .lock()
            .expect("scripted process observation lock should be healthy"))
    }

    fn signal_process(
        &self,
        manifest: &ContainerSandboxManifest,
        _identity: &RuntimeProcessIdentity,
        signal: RuntimeProcessSignal,
    ) -> crate::Result<RuntimeProcessSignalOutcome> {
        let durable = self
            .backend
            .read_manifest(&manifest.handle.id)?
            .expect("signal requires a durable manifest");
        match signal.number() {
            libc::SIGKILL => assert!(matches!(
                durable.execution_teardown.stop(),
                ContainerStopProgress::KillMayExist { .. }
            )),
            _ => assert!(matches!(
                durable.execution_teardown.stop(),
                ContainerStopProgress::TermMayExist { .. }
            )),
        }
        self.signals
            .lock()
            .expect("scripted signal lock should be healthy")
            .push(signal.number());
        Ok(RuntimeProcessSignalOutcome::Delivered)
    }
}

impl TeardownFixture {
    fn reserved(label: &str) -> Self {
        let root = tempfile::tempdir().expect("temporary root should exist");
        let backend = ContainerSandboxBackend::new(
            super::super::ContainerSandboxBackendConfig::under_root(root.path()),
        );
        let id = crate::SandboxId::new(format!("container-teardown-{label}"));
        let spec = sample_spec_for_tenant(
            &format!("container-teardown-{label}"),
            &format!("workload-{label}"),
        );
        let execution_attempt_id = sample_execution_attempt_id(&id);
        let plan = sample_provision_network_plan(&spec, &id, label);
        backend
            .reserve_provision_network(spec, id.clone(), execution_attempt_id.clone(), plan)
            .expect("teardown fixture should reserve its exact plan");
        Self {
            root,
            port_window: None,
            backend,
            id,
            execution_attempt_id,
        }
    }

    fn materialized_plan_only(label: &str) -> Self {
        let root = tempfile::tempdir().expect("temporary root should exist");
        let backend =
            ContainerSandboxBackend::new(super::super::ContainerSandboxBackendConfig::plan_only(
                root.path().join("bundles"),
                root.path().join("state"),
            ));
        let id = crate::SandboxId::new(format!("container-teardown-{label}"));
        let spec = sample_spec_for_tenant(
            &format!("container-teardown-{label}"),
            &format!("workload-{label}"),
        );
        let execution_attempt_id = sample_execution_attempt_id(&id);
        let plan = sample_provision_network_plan(&spec, &id, label);
        backend
            .reserve_provision_network(spec, id.clone(), execution_attempt_id.clone(), plan)
            .expect("PlanOnly teardown fixture should reserve its exact plan");
        backend
            .prepare_provision_workload(&id, &execution_attempt_id)
            .expect("PlanOnly teardown fixture should materialize its workload");
        let manifest = backend
            .read_manifest(&id)
            .expect("PlanOnly manifest should read")
            .expect("PlanOnly manifest should exist");
        assert_eq!(manifest.start_mode, ContainerStartMode::PlanOnly);
        assert!(manifest.provision_prepared);
        Self {
            root,
            port_window: None,
            backend,
            id,
            execution_attempt_id,
        }
    }

    fn attached(label: &str) -> Self {
        let root = tempfile::tempdir().expect("temporary root should exist");
        // One claimed window owns both host ports this fixture publishes: the
        // spec's published binding and the one-port range the egress proxy is
        // allocated from. The claim excludes every other test process, so the
        // proxy bind inside the attachment below cannot lose the port.
        let port_window = PortWindow::claim();
        let published_port = port_window.port(0);
        let pep_port = port_window.port(1);
        let mut config = super::super::ContainerSandboxBackendConfig::under_root(root.path());
        config.node_network_supernet = "127.0.0.0/24".to_owned();
        config.published_port_range = pep_port..=pep_port;
        config.netavark_path = PathBuf::from("/usr/bin/true");
        let backend = ContainerSandboxBackend::new(config)
            .with_egress_pin_provider(Arc::new(FixedOciEgressPinProvider::ready()));
        let id = crate::SandboxId::new(format!("container-teardown-{label}"));
        let spec = sample_spec_for_tenant(
            &format!("container-teardown-{label}"),
            &format!("workload-{label}"),
        )
        .with_port_binding(crate::SandboxPortBinding::tcp("api", published_port, 8080));
        let execution_attempt_id = sample_execution_attempt_id(&id);
        let plan = sample_provision_network_plan(&spec, &id, label);
        backend
            .reserve_provision_network(spec, id.clone(), execution_attempt_id.clone(), plan)
            .expect("attached teardown fixture should reserve its exact plan");
        backend
            .prepare_provision_workload(&id, &execution_attempt_id)
            .expect("attached teardown fixture should prepare its workload");
        backend
            .attach_provision_network_with_test_host(&id, &execution_attempt_id)
            .expect("attached teardown fixture should realize its private network");
        let manifest = backend
            .read_manifest(&id)
            .expect("attached manifest should read")
            .expect("attached manifest should exist");
        assert!(manifest.network_config.is_some());
        assert!(manifest.network_layout.netns_path.is_file());
        assert!(manifest.network_layout.status_path.is_file());
        assert!(!manifest.port_leases.is_empty());
        assert!(manifest.egress_proxy.is_some());
        Self {
            root,
            port_window: Some(port_window),
            backend,
            id,
            execution_attempt_id,
        }
    }

    fn command(
        &self,
        operation: SandboxExecutionTeardownOperation,
        attempt: &str,
        epoch: u64,
    ) -> SandboxExecutionTeardownCommand {
        self.command_for_execution(&self.execution_attempt_id, operation, attempt, epoch)
    }

    fn command_for_execution(
        &self,
        execution_attempt_id: &crate::SandboxExecutionAttemptId,
        operation: SandboxExecutionTeardownOperation,
        attempt: &str,
        epoch: u64,
    ) -> SandboxExecutionTeardownCommand {
        let manifest = self.manifest();
        let plan = manifest
            .provision_network_plan
            .as_ref()
            .expect("fixture has an exact provision plan");
        self.command_with_execution_and_plan(
            execution_attempt_id,
            operation,
            attempt,
            epoch,
            plan.generation().as_u64(),
            plan.network_plan().digest().to_string(),
        )
    }

    fn command_with_execution_and_plan(
        &self,
        execution_attempt_id: &crate::SandboxExecutionAttemptId,
        operation: SandboxExecutionTeardownOperation,
        attempt: &str,
        epoch: u64,
        workload_generation: u64,
        network_plan_digest: String,
    ) -> SandboxExecutionTeardownCommand {
        let manifest = self.manifest();
        let claim = crate::ProviderCommandClaim::new(ProviderCommandClaimInput {
            authority_id: "authority-container-teardown".to_owned(),
            effect_subject: format!("{{\"sandbox\":\"{}\"}}", self.id),
            source_attempt_id: None,
            attempt_id: attempt.to_owned(),
            dispatch_epoch: epoch,
            workload_generation,
            restart_ordinal: 0,
            desired_digest: "1".repeat(64),
            source_digest: "2".repeat(64),
            network_plan_digest,
            provider_target_digest: "3".repeat(64),
            operation: match operation {
                SandboxExecutionTeardownOperation::Drain => {
                    ProviderCommandOperation::DrainExecution
                }
                SandboxExecutionTeardownOperation::Stop => ProviderCommandOperation::StopExecution,
            },
        })
        .expect("fixture provider claim should validate");
        SandboxExecutionTeardownCommand::new(
            manifest.spec.tenant_id,
            self.id.clone(),
            execution_attempt_id.clone(),
            CONTAINER_EXECUTION_TEARDOWN_PROVIDER_KEY,
            operation,
            claim,
        )
        .expect("fixture teardown command should validate")
    }

    fn manifest(&self) -> ContainerSandboxManifest {
        self.backend
            .read_manifest(&self.id)
            .expect("fixture manifest should read")
            .expect("fixture manifest should exist")
    }

    fn set_explicit_runtime_absence(&self) {
        let mut manifest = self.manifest();
        manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
            "-c".to_owned(),
            format!(
                "printf '%s\\n' 'container `{0}` does not exist: open `/run/crun/{0}/status`: No such file or directory' >&2; exit 1",
                self.id.as_str()
            ),
        ]);
        self.backend
            .write_manifest(&manifest)
            .expect("runtime absence fixture should persist");
    }

    fn network_authority(&self) -> Vec<u8> {
        let manifest = self.manifest();
        serde_json::to_vec(&(
            &manifest.provision_network_plan,
            &manifest.network_config,
            manifest.network_cleanup_complete,
            &manifest.port_leases,
            &manifest.egress_proxy,
            &manifest.network_layout,
            &manifest.launch_reservation_claim,
        ))
        .expect("network authority snapshot should encode")
    }

    fn durable_network_files(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        let manifest = self.manifest();
        let manifest_path = crate::artifact_paths::manifest_path(
            &self.backend.config.workload_state_root,
            &manifest.spec.tenant_id,
            &self.id,
        );
        let manifest_relative = manifest_path
            .strip_prefix(&self.backend.config.network_state_root)
            .expect("fixture manifest stays below the shared state root")
            .to_path_buf();
        snapshot_files(&self.backend.config.network_state_root)
            .into_iter()
            .filter(|(path, _)| {
                path != &manifest_relative
                    && path.components().all(|component| {
                        component.as_os_str() != ".nimbus-provider-command-attempts"
                    })
            })
            .collect()
    }

    fn reopen_backend(&self) -> ContainerSandboxBackend {
        ContainerSandboxBackend::new(super::super::ContainerSandboxBackendConfig::under_root(
            self.root.path(),
        ))
    }
}

fn claim_teardown_execution(
    journal: &crate::ProviderCommandAttemptJournal,
    command: &SandboxExecutionTeardownCommand,
) -> crate::ProviderCommandExecutionClaim {
    match journal
        .claim_dispatch_epoch(command.provider_claim())
        .expect("the exact teardown epoch should claim")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("a new teardown epoch must receive execute authority")
        }
    }
}

fn persist_teardown_observation(
    journal: &crate::ProviderCommandAttemptJournal,
    command: &SandboxExecutionTeardownCommand,
    observation: &SandboxExecutionTeardownObservation,
) {
    let kind = match observation {
        SandboxExecutionTeardownObservation::Succeeded { .. } => {
            ProviderCommandObservationKind::Succeeded
        }
        SandboxExecutionTeardownObservation::DefiniteFailure { .. } => {
            ProviderCommandObservationKind::DefiniteFailure
        }
        SandboxExecutionTeardownObservation::Absent { .. } => {
            ProviderCommandObservationKind::Absent
        }
        SandboxExecutionTeardownObservation::RetryAuthorized { .. } => {
            ProviderCommandObservationKind::RetryAuthorized
        }
        SandboxExecutionTeardownObservation::InProgress { .. } => {
            ProviderCommandObservationKind::InProgress
        }
        SandboxExecutionTeardownObservation::Ambiguous { .. } => {
            ProviderCommandObservationKind::Ambiguous
        }
    };
    journal
        .record_observation_with_failure_code(
            command.provider_claim(),
            kind,
            observation.failure_code(),
            observation.evidence(),
        )
        .expect("the exact teardown observation should become durable");
}

#[test]
fn an_unpublished_exit_receipt_is_not_terminality() {
    let fixture = TeardownFixture::reserved("unpublished-exit-receipt");
    let manifest = fixture.manifest();
    let host_runtime = effects::HostContainerExecutionTeardownRuntime;

    // Publish the way conmon does: the receipt appears first, and the exit code
    // arrives afterwards.
    std::fs::write(&manifest.conmon_layout.exit_status_file, b"")
        .expect("an empty exit receipt should persist");
    assert!(
        !host_runtime
            .execution_is_terminal(&manifest)
            .expect("an unfinished publication must not fail the predicate"),
        "a receipt without an exit code is not an observation of terminality"
    );

    std::fs::write(&manifest.conmon_layout.exit_status_file, b"0\n")
        .expect("an exact exit receipt should persist");
    assert!(
        host_runtime
            .execution_is_terminal(&manifest)
            .expect("a published receipt should be observable"),
        "once the code lands the receipt proves terminality"
    );
}

#[test]
fn container_drain_persists_barrier_and_retains_runtime_and_network() {
    let fixture = TeardownFixture::reserved("drain-retains-network");
    let command = fixture.command(SandboxExecutionTeardownOperation::Drain, "drain-a", 1);
    let before = fixture.network_authority();

    assert!(matches!(
        fixture.backend.execute_execution_teardown(&command),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));

    let manifest = fixture.manifest();
    assert!(matches!(
        manifest.execution_teardown.drain(),
        ContainerDrainProgress::Drained { fence, .. }
            if fence == command.provider_claim()
    ));
    assert!(!manifest.shutdown_requested);
    assert_eq!(fixture.network_authority(), before);
}

#[test]
fn container_drain_inspection_is_byte_stable() {
    let fixture = TeardownFixture::reserved("drain-inspect");
    let command = fixture.command(SandboxExecutionTeardownOperation::Drain, "drain-b", 1);
    assert!(matches!(
        fixture.backend.execute_execution_teardown(&command),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
    let before = snapshot_files(fixture.root.path());

    assert!(matches!(
        fixture.backend.inspect_execution_teardown(&command),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
    assert_eq!(snapshot_files(fixture.root.path()), before);
}

#[test]
fn container_stop_records_execution_terminality_without_network_release() {
    let fixture = TeardownFixture::reserved("stop-retains-network");
    fixture.set_explicit_runtime_absence();
    let drain = fixture.command(SandboxExecutionTeardownOperation::Drain, "shared", 1);
    assert!(matches!(
        fixture.backend.execute_execution_teardown(&drain),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
    let before = fixture.network_authority();
    let stop = fixture.command(SandboxExecutionTeardownOperation::Stop, "shared", 1);

    assert!(matches!(
        fixture.backend.execute_execution_teardown(&stop),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));

    let manifest = fixture.manifest();
    assert!(matches!(
        manifest.execution_teardown.stop(),
        ContainerStopProgress::ExecutionStopped { fence, .. }
            if fence == stop.provider_claim()
    ));
    assert!(manifest.shutdown_requested);
    assert_eq!(manifest.status, crate::SandboxStatus::Stopping);
    assert!(!manifest.network_cleanup_complete);
    assert_eq!(fixture.network_authority(), before);
}

#[test]
fn container_pre_activation_stop_closes_admission_without_a_drain_command() {
    let fixture = TeardownFixture::attached("pre-activation-stop");
    let runtime = ScriptedRuntime::live(fixture.backend.clone(), 100);
    runtime.terminal.store(true, Ordering::Release);
    let before = fixture.network_authority();
    let stop = fixture.command(SandboxExecutionTeardownOperation::Stop, "stop", 1);

    assert!(matches!(
        fixture
            .backend
            .inspect_execution_teardown_inner_with_runtime(&stop, &runtime)
            .expect("pre-activation stop inspection should classify"),
        SandboxExecutionTeardownObservation::Absent { .. }
    ));
    assert!(matches!(
        fixture
            .backend
            .execute_execution_teardown_inner_with_runtime(&stop, &runtime)
            .expect("pre-activation stop should persist"),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));

    let durable = fixture.manifest();
    assert!(matches!(
        durable.execution_teardown.drain(),
        ContainerDrainProgress::ExecutionNeverAdmitted { fence, .. }
            if fence == stop.provider_claim()
    ));
    assert!(matches!(
        durable.execution_teardown.stop(),
        ContainerStopProgress::ExecutionStopped { fence, .. }
            if fence == stop.provider_claim()
    ));
    assert!(
        durable
            .require_execution_admission_open("late Container activation")
            .is_err(),
        "the no-execution stop fence must close later activation admission"
    );
    assert!(runtime.signals().is_empty());
    assert_eq!(fixture.network_authority(), before);
    assert!(matches!(
        fixture
            .backend
            .inspect_execution_teardown_inner_with_runtime(&stop, &runtime)
            .expect("persisted pre-activation stop should inspect"),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
}

#[test]
fn container_stop_rejects_a_crossed_drain_subject_without_mutation() {
    let fixture = TeardownFixture::reserved("stop-crossed-subject");
    fixture.set_explicit_runtime_absence();
    let drain = fixture.command(SandboxExecutionTeardownOperation::Drain, "drain-subject", 1);
    assert!(matches!(
        fixture.backend.execute_execution_teardown(&drain),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));

    let exact = fixture.command(SandboxExecutionTeardownOperation::Stop, "stop-subject", 1);
    let claim_input = ProviderCommandClaimInput {
        authority_id: exact.provider_claim().authority_id().to_owned(),
        effect_subject: "{\"sandbox\":\"crossed-execution\"}".to_owned(),
        source_attempt_id: None,
        attempt_id: exact.provider_claim().attempt_id().to_owned(),
        dispatch_epoch: exact.provider_claim().dispatch_epoch(),
        workload_generation: exact.provider_claim().workload_generation(),
        restart_ordinal: exact.provider_claim().restart_ordinal(),
        desired_digest: exact.provider_claim().desired_digest().to_owned(),
        source_digest: exact.provider_claim().source_digest().to_owned(),
        network_plan_digest: exact.provider_claim().network_plan_digest().to_owned(),
        provider_target_digest: exact.provider_claim().provider_target_digest().to_owned(),
        operation: ProviderCommandOperation::StopExecution,
    };
    let crossed_claim = crate::ProviderCommandClaim::new(claim_input)
        .expect("crossed subject remains a structurally valid claim");
    let crossed = SandboxExecutionTeardownCommand::new(
        exact.tenant_id().clone(),
        exact.sandbox_id().clone(),
        exact.execution_attempt_id().clone(),
        exact.provider_registration_key(),
        exact.operation(),
        crossed_claim,
    )
    .expect("crossed lower command remains structurally valid");
    let before = snapshot_files(fixture.root.path());

    assert!(matches!(
        fixture.backend.execute_execution_teardown(&crossed),
        SandboxExecutionTeardownObservation::DefiniteFailure { .. }
    ));
    assert_eq!(snapshot_files(fixture.root.path()), before);
}

#[test]
fn crossed_teardown_command_changes_no_durable_byte() {
    let fixture = TeardownFixture::reserved("crossed-command");
    let exact = fixture.command(SandboxExecutionTeardownOperation::Drain, "drain-c", 1);
    let crossed = SandboxExecutionTeardownCommand::new(
        nimbus_core::TenantId::new("crossed-tenant").expect("tenant should validate"),
        exact.sandbox_id().clone(),
        exact.execution_attempt_id().clone(),
        exact.provider_registration_key(),
        exact.operation(),
        exact.provider_claim().clone(),
    )
    .expect("crossed lower command remains structurally valid");
    let before = snapshot_files(fixture.root.path());

    assert!(matches!(
        fixture.backend.execute_execution_teardown(&crossed),
        SandboxExecutionTeardownObservation::DefiniteFailure { .. }
    ));
    assert_eq!(snapshot_files(fixture.root.path()), before);
}

#[test]
fn drain_barrier_recovers_only_at_the_exact_next_epoch() {
    let fixture = TeardownFixture::reserved("drain-barrier-recovery");
    let journal = fixture
        .backend
        .attempt_idempotency_journal()
        .expect("the drain-recovery journal should open");
    let first = fixture.command(
        SandboxExecutionTeardownOperation::Drain,
        "drain-recovery",
        1,
    );
    let _first_execution = claim_teardown_execution(&journal, &first);
    let mut manifest = fixture.manifest();
    manifest
        .execution_teardown
        .set_drain(ContainerDrainProgress::BarrierPersisted {
            fence: first.provider_claim().clone(),
        });
    fixture
        .backend
        .write_existing_workload_manifest(&manifest)
        .expect("barrier crash cut should persist");
    let before_inspect = snapshot_files(fixture.root.path());

    let inspected = fixture.backend.inspect_execution_teardown(&first);
    assert!(matches!(
        inspected,
        SandboxExecutionTeardownObservation::Absent { .. }
    ));
    assert_eq!(snapshot_files(fixture.root.path()), before_inspect);
    persist_teardown_observation(&journal, &first, &inspected);

    let next = fixture.command(
        SandboxExecutionTeardownOperation::Drain,
        "drain-recovery",
        2,
    );
    let next_execution = claim_teardown_execution(&journal, &next);
    assert!(matches!(
        fixture
            .backend
            .execute_execution_teardown_with_claim(&next, next_execution),
        Ok(observation) if observation.kind() == ProviderCommandObservationKind::Succeeded
    ));
    assert!(matches!(
        fixture.manifest().execution_teardown.drain(),
        ContainerDrainProgress::Drained { fence, .. } if fence == next.provider_claim()
    ));

    let skipped = fixture.command(
        SandboxExecutionTeardownOperation::Drain,
        "drain-recovery",
        4,
    );
    assert!(matches!(
        fixture.backend.execute_execution_teardown(&skipped),
        SandboxExecutionTeardownObservation::DefiniteFailure { .. }
    ));
}

#[test]
fn stop_persists_each_may_exist_boundary_and_never_duplicates_a_signal() {
    let fixture = TeardownFixture::reserved("stop-signal-boundaries");
    let drain = fixture.command(SandboxExecutionTeardownOperation::Drain, "drain-stop", 1);
    assert!(matches!(
        fixture.backend.execute_execution_teardown(&drain),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
    let journal = fixture
        .backend
        .attempt_idempotency_journal()
        .expect("the stop-recovery journal should open");
    let network_before = fixture.network_authority();
    let runtime = ScriptedRuntime::live(fixture.backend.clone(), 100);
    let first = fixture.command(SandboxExecutionTeardownOperation::Stop, "stop-recovery", 1);
    let first_execution = claim_teardown_execution(&journal, &first);

    let first_outcome = fixture
        .backend
        .execute_execution_teardown_inner_with_runtime_and_authorization(
            &first,
            &runtime,
            Some(first_execution.observation()),
        )
        .expect("first stop dispatch should complete its durable transition");
    assert!(matches!(
        first_outcome,
        SandboxExecutionTeardownObservation::InProgress { .. }
    ));
    persist_teardown_observation(&journal, &first, &first_outcome);
    assert_eq!(runtime.signals(), vec![libc::SIGTERM]);
    assert!(matches!(
        fixture.manifest().execution_teardown.stop(),
        ContainerStopProgress::TermMayExist { fence, .. } if fence == first.provider_claim()
    ));

    assert!(matches!(
        fixture
            .backend
            .execute_execution_teardown_inner_with_runtime(&first, &runtime)
            .expect("same-epoch replay should adopt the may-exist state"),
        SandboxExecutionTeardownObservation::InProgress { .. }
    ));
    assert_eq!(runtime.signals(), vec![libc::SIGTERM]);

    runtime.set_now(10_000);
    let before_inspect = snapshot_files(fixture.root.path());
    let first_durable = journal
        .adopt_exact_attempt(first.provider_claim())
        .expect("the first stop observation should read")
        .expect("the first stop observation should exist");
    let first_inspection = fixture
        .reopen_backend()
        .inspect_execution_teardown_inner_with_runtime_and_authorization(
            &first,
            &runtime,
            Some(&first_durable),
        )
        .expect("fresh-process inspection should classify the elapsed deadline");
    assert!(matches!(
        first_inspection,
        SandboxExecutionTeardownObservation::RetryAuthorized { .. }
    ));
    assert_eq!(snapshot_files(fixture.root.path()), before_inspect);
    persist_teardown_observation(&journal, &first, &first_inspection);

    let second = fixture.command(SandboxExecutionTeardownOperation::Stop, "stop-recovery", 2);
    let second_execution = claim_teardown_execution(&journal, &second);
    let second_outcome = fixture
        .reopen_backend()
        .execute_execution_teardown_inner_with_runtime_and_authorization(
            &second,
            &runtime,
            Some(second_execution.observation()),
        )
        .expect("exact next epoch should dispatch the forced stop");
    assert!(matches!(
        second_outcome,
        SandboxExecutionTeardownObservation::InProgress { .. }
    ));
    persist_teardown_observation(&journal, &second, &second_outcome);
    assert_eq!(runtime.signals(), vec![libc::SIGTERM, libc::SIGKILL]);
    assert!(matches!(
        fixture.manifest().execution_teardown.stop(),
        ContainerStopProgress::KillMayExist { fence, .. } if fence == second.provider_claim()
    ));

    assert!(matches!(
        fixture
            .backend
            .execute_execution_teardown_inner_with_runtime(&second, &runtime)
            .expect("same forced-stop epoch should not signal again"),
        SandboxExecutionTeardownObservation::InProgress { .. }
    ));
    assert_eq!(runtime.signals(), vec![libc::SIGTERM, libc::SIGKILL]);
    let third = fixture.command(SandboxExecutionTeardownOperation::Stop, "stop-recovery", 3);
    assert_eq!(
        journal
            .claim_dispatch_epoch(third.provider_claim())
            .expect_err("KILL response ambiguity alone must not authorize another epoch"),
        crate::ProviderCommandJournalError::RetryWithoutAuthority
    );

    runtime.set_process_observation(RuntimeProcessIdentityObservation::ExplicitlyAbsent);
    let second_durable = journal
        .adopt_exact_attempt(second.provider_claim())
        .expect("the second stop observation should read")
        .expect("the second stop observation should exist");
    let second_inspection = fixture
        .reopen_backend()
        .inspect_execution_teardown_inner_with_runtime_and_authorization(
            &second,
            &runtime,
            Some(&second_durable),
        )
        .expect("fresh-process inspection should prove exact absence");
    assert!(matches!(
        second_inspection,
        SandboxExecutionTeardownObservation::RetryAuthorized { .. }
    ));
    persist_teardown_observation(&journal, &second, &second_inspection);
    let third_execution = claim_teardown_execution(&journal, &third);
    assert!(matches!(
        fixture
            .reopen_backend()
            .execute_execution_teardown_inner_with_runtime_and_authorization(
                &third,
                &runtime,
                Some(third_execution.observation()),
            )
            .expect("exact absence should authorize terminal persistence"),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
    assert_eq!(runtime.signals(), vec![libc::SIGTERM, libc::SIGKILL]);
    assert!(matches!(
        fixture.manifest().execution_teardown.stop(),
        ContainerStopProgress::ExecutionStopped { fence, .. } if fence == third.provider_claim()
    ));
    assert_eq!(fixture.network_authority(), network_before);
}

#[test]
fn populated_network_authority_is_byte_stable_through_stop_ambiguity_and_replay() {
    let fixture = TeardownFixture::attached("populated-network-retention");
    let manifest = fixture.manifest();
    let config = manifest
        .network_config
        .as_ref()
        .expect("attached fixture retains exact attachment configuration");
    let plan = manifest
        .provision_network_plan
        .as_ref()
        .expect("attached fixture retains its compiled network plan");
    assert_eq!(config.attachment_id, *plan.attachment_id());
    assert!(manifest.network_layout.netns_path.is_file());
    assert!(manifest.network_layout.status_path.is_file());
    assert!(manifest.egress_proxy.is_some());
    assert!(!manifest.port_leases.is_empty());
    let durable_before = fixture.durable_network_files();
    let durable_paths = durable_before.keys().collect::<Vec<_>>();
    assert!(
        durable_paths
            .iter()
            .any(|path| path.ends_with("networks/control-plane/state.json")),
        "the unified authority store must retain attachment, provider, listener, port, IPAM, and segment records: {durable_paths:#?}"
    );
    assert!(
        durable_paths
            .iter()
            .any(|path| path.ends_with("status.json"))
            && durable_paths
                .iter()
                .any(|path| path.to_string_lossy().contains("/netns/"))
            && durable_paths
                .iter()
                .any(|path| path.to_string_lossy().contains("egress-decision-logs")),
        "the attached fixture must retain provider status, netns, and live PEP artifacts: {durable_paths:#?}"
    );
    let manifest_before = fixture.network_authority();

    let drain = fixture.command(SandboxExecutionTeardownOperation::Drain, "populated", 1);
    assert!(matches!(
        fixture.backend.execute_execution_teardown(&drain),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
    assert_eq!(fixture.durable_network_files(), durable_before);
    assert_eq!(fixture.network_authority(), manifest_before);

    let journal = fixture
        .backend
        .attempt_idempotency_journal()
        .expect("the populated-network stop journal should open");
    let runtime = ScriptedRuntime::live(fixture.backend.clone(), 100);
    let term = fixture.command(SandboxExecutionTeardownOperation::Stop, "populated", 1);
    let term_execution = claim_teardown_execution(&journal, &term);
    let term_outcome = fixture
        .backend
        .execute_execution_teardown_inner_with_runtime_and_authorization(
            &term,
            &runtime,
            Some(term_execution.observation()),
        )
        .expect("TERM boundary should persist");
    assert!(matches!(
        term_outcome,
        SandboxExecutionTeardownObservation::InProgress { .. }
    ));
    persist_teardown_observation(&journal, &term, &term_outcome);
    assert_eq!(runtime.signals(), vec![libc::SIGTERM]);
    assert_eq!(fixture.durable_network_files(), durable_before);
    assert_eq!(fixture.network_authority(), manifest_before);

    runtime.set_now(10_000);
    let term_durable = journal
        .adopt_exact_attempt(term.provider_claim())
        .expect("the TERM observation should read")
        .expect("the TERM observation should exist");
    let term_inspection = fixture
        .backend
        .inspect_execution_teardown_inner_with_runtime_and_authorization(
            &term,
            &runtime,
            Some(&term_durable),
        )
        .expect("elapsed TERM should authorize the forced-stop epoch");
    assert!(matches!(
        term_inspection,
        SandboxExecutionTeardownObservation::RetryAuthorized { .. }
    ));
    persist_teardown_observation(&journal, &term, &term_inspection);
    let kill = fixture.command(SandboxExecutionTeardownOperation::Stop, "populated", 2);
    let kill_execution = claim_teardown_execution(&journal, &kill);
    let kill_outcome = fixture
        .backend
        .execute_execution_teardown_inner_with_runtime_and_authorization(
            &kill,
            &runtime,
            Some(kill_execution.observation()),
        )
        .expect("KILL boundary should persist");
    assert!(matches!(
        kill_outcome,
        SandboxExecutionTeardownObservation::InProgress { .. }
    ));
    persist_teardown_observation(&journal, &kill, &kill_outcome);
    assert_eq!(runtime.signals(), vec![libc::SIGTERM, libc::SIGKILL]);
    assert_eq!(fixture.durable_network_files(), durable_before);
    assert_eq!(fixture.network_authority(), manifest_before);

    runtime.set_process_observation(RuntimeProcessIdentityObservation::ExplicitlyAbsent);
    let kill_durable = journal
        .adopt_exact_attempt(kill.provider_claim())
        .expect("the KILL observation should read")
        .expect("the KILL observation should exist");
    let kill_inspection = fixture
        .backend
        .inspect_execution_teardown_inner_with_runtime_and_authorization(
            &kill,
            &runtime,
            Some(&kill_durable),
        )
        .expect("exact process absence should authorize terminal persistence");
    assert!(matches!(
        kill_inspection,
        SandboxExecutionTeardownObservation::RetryAuthorized { .. }
    ));
    persist_teardown_observation(&journal, &kill, &kill_inspection);
    let stopped = fixture.command(SandboxExecutionTeardownOperation::Stop, "populated", 3);
    let stopped_execution = claim_teardown_execution(&journal, &stopped);
    assert!(matches!(
        fixture
            .backend
            .execute_execution_teardown_inner_with_runtime_and_authorization(
                &stopped,
                &runtime,
                Some(stopped_execution.observation()),
            )
            .expect("exact absence should complete execution stop"),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
    assert!(matches!(
        fixture
            .backend
            .execute_execution_teardown_inner_with_runtime(&stopped, &runtime)
            .expect("exact terminal replay should adopt success"),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
    assert_eq!(runtime.signals(), vec![libc::SIGTERM, libc::SIGKILL]);
    assert_eq!(fixture.durable_network_files(), durable_before);
    assert_eq!(fixture.network_authority(), manifest_before);
}

#[test]
fn exact_exit_receipt_converges_before_term_or_kill_pidfile_inspection() {
    for receipt_phase in ["term", "kill"] {
        let fixture = TeardownFixture::reserved(&format!("stop-{receipt_phase}-exit-receipt"));
        let drain = fixture.command(
            SandboxExecutionTeardownOperation::Drain,
            &format!("drain-{receipt_phase}-exit-receipt"),
            1,
        );
        assert!(matches!(
            fixture.backend.execute_execution_teardown(&drain),
            SandboxExecutionTeardownObservation::Succeeded { .. }
        ));
        let journal = fixture
            .backend
            .attempt_idempotency_journal()
            .expect("the exit-receipt stop journal should open");
        let network_before = fixture.network_authority();
        let runtime = ScriptedRuntime::live(fixture.backend.clone(), 100);
        let first = fixture.command(
            SandboxExecutionTeardownOperation::Stop,
            &format!("stop-{receipt_phase}-exit-receipt"),
            1,
        );
        let first_execution = claim_teardown_execution(&journal, &first);
        let first_outcome = fixture
            .backend
            .execute_execution_teardown_inner_with_runtime_and_authorization(
                &first,
                &runtime,
                Some(first_execution.observation()),
            )
            .expect("the first stop dispatch should persist TERM may-exist");
        assert!(matches!(
            first_outcome,
            SandboxExecutionTeardownObservation::InProgress { .. }
        ));
        persist_teardown_observation(&journal, &first, &first_outcome);
        let (may_exist, next_epoch) = if receipt_phase == "kill" {
            runtime.set_now(10_000);
            let first_durable = journal
                .adopt_exact_attempt(first.provider_claim())
                .expect("the TERM observation should read")
                .expect("the TERM observation should exist");
            let first_inspection = fixture
                .backend
                .inspect_execution_teardown_inner_with_runtime_and_authorization(
                    &first,
                    &runtime,
                    Some(&first_durable),
                )
                .expect("elapsed TERM should authorize KILL");
            assert!(matches!(
                first_inspection,
                SandboxExecutionTeardownObservation::RetryAuthorized { .. }
            ));
            persist_teardown_observation(&journal, &first, &first_inspection);
            let kill = fixture.command(
                SandboxExecutionTeardownOperation::Stop,
                "stop-kill-exit-receipt",
                2,
            );
            let kill_execution = claim_teardown_execution(&journal, &kill);
            let kill_outcome = fixture
                .backend
                .execute_execution_teardown_inner_with_runtime_and_authorization(
                    &kill,
                    &runtime,
                    Some(kill_execution.observation()),
                )
                .expect("the next stop epoch should persist KILL may-exist");
            assert!(matches!(
                kill_outcome,
                SandboxExecutionTeardownObservation::InProgress { .. }
            ));
            persist_teardown_observation(&journal, &kill, &kill_outcome);
            assert_eq!(runtime.signals(), vec![libc::SIGTERM, libc::SIGKILL]);
            (kill, 3)
        } else {
            assert_eq!(runtime.signals(), vec![libc::SIGTERM]);
            (first, 2)
        };

        let manifest = fixture.manifest();
        std::fs::write(&manifest.conmon_layout.exit_status_file, "0\n")
            .expect("an exact exit receipt should persist");
        let _ = std::fs::remove_file(&manifest.conmon_layout.pidfile);
        let host_runtime = effects::HostContainerExecutionTeardownRuntime;
        let before_inspect = snapshot_files(fixture.root.path());
        let may_exist_durable = journal
            .adopt_exact_attempt(may_exist.provider_claim())
            .expect("the may-exist observation should read")
            .expect("the may-exist observation should exist");
        let terminal_inspection = fixture
            .reopen_backend()
            .inspect_execution_teardown_inner_with_runtime_and_authorization(
                &may_exist,
                &host_runtime,
                Some(&may_exist_durable),
            )
            .expect("inspection should prefer the exact exit receipt over a missing pidfile");
        assert!(matches!(
            terminal_inspection,
            SandboxExecutionTeardownObservation::RetryAuthorized { .. }
        ));
        assert_eq!(snapshot_files(fixture.root.path()), before_inspect);
        persist_teardown_observation(&journal, &may_exist, &terminal_inspection);

        let terminal = fixture.command(
            SandboxExecutionTeardownOperation::Stop,
            &format!("stop-{receipt_phase}-exit-receipt"),
            next_epoch,
        );
        let terminal_execution = claim_teardown_execution(&journal, &terminal);
        assert!(matches!(
            fixture
                .reopen_backend()
                .execute_execution_teardown_inner_with_runtime_and_authorization(
                    &terminal,
                    &host_runtime,
                    Some(terminal_execution.observation()),
                )
                .expect("the exact next epoch should persist terminal execution evidence"),
            SandboxExecutionTeardownObservation::Succeeded { .. }
        ));
        let expected_signals = if receipt_phase == "kill" {
            vec![libc::SIGTERM, libc::SIGKILL]
        } else {
            vec![libc::SIGTERM]
        };
        assert_eq!(runtime.signals(), expected_signals);
        assert!(matches!(
            fixture.manifest().execution_teardown.stop(),
            ContainerStopProgress::ExecutionStopped { fence, .. }
                if fence == terminal.provider_claim()
        ));
        assert_eq!(fixture.network_authority(), network_before);
    }
}

#[test]
fn two_thread_contenders_publish_one_barrier_and_one_signal() {
    let fixture = TeardownFixture::reserved("thread-contention");
    let backend = Arc::new(fixture.backend.clone());
    let drain =
        Arc::new(fixture.command(SandboxExecutionTeardownOperation::Drain, "thread-drain", 1));
    std::thread::scope(|scope| {
        let mut joins = Vec::new();
        for _ in 0..2 {
            let backend = Arc::clone(&backend);
            let drain = Arc::clone(&drain);
            joins.push(scope.spawn(move || backend.execute_execution_teardown(&drain)));
        }
        let outcomes = joins
            .into_iter()
            .map(|join| join.join().expect("drain contender should not panic"))
            .collect::<Vec<_>>();
        assert!(outcomes.iter().any(|outcome| matches!(
            outcome,
            SandboxExecutionTeardownObservation::Succeeded { .. }
        )));
        assert!(outcomes.iter().all(|outcome| matches!(
            outcome,
            SandboxExecutionTeardownObservation::Succeeded { .. }
                | SandboxExecutionTeardownObservation::Ambiguous { .. }
        )));
    });

    let runtime = Arc::new(ScriptedRuntime::live(fixture.backend.clone(), 100));
    let stop = Arc::new(fixture.command(SandboxExecutionTeardownOperation::Stop, "thread-stop", 1));
    std::thread::scope(|scope| {
        let mut joins = Vec::new();
        for _ in 0..2 {
            let backend = Arc::clone(&backend);
            let runtime = Arc::clone(&runtime);
            let stop = Arc::clone(&stop);
            joins.push(scope.spawn(move || {
                backend
                    .execute_execution_teardown_inner_with_runtime(&stop, runtime.as_ref())
                    .unwrap_or_else(|error| ambiguous(error.to_string()))
            }));
        }
        for join in joins {
            assert!(matches!(
                join.join().expect("stop contender should not panic"),
                SandboxExecutionTeardownObservation::InProgress { .. }
                    | SandboxExecutionTeardownObservation::Ambiguous { .. }
            ));
        }
    });
    assert_eq!(runtime.signals(), vec![libc::SIGTERM]);
}

#[test]
fn durable_drain_barrier_blocks_creator_activation_restart_and_legacy_launch() {
    let fixture = TeardownFixture::reserved("execution-admission-barrier");
    let drain = fixture.command(SandboxExecutionTeardownOperation::Drain, "drain-launch", 1);
    assert!(matches!(
        fixture.backend.execute_execution_teardown(&drain),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
    let before = snapshot_files(fixture.root.path());

    let mut manifest = fixture.manifest();
    let creator_error = fixture
        .backend
        .spawn_creator_and_wait_for_runtime(&mut manifest)
        .expect_err("a durable drain must reject creator admission");
    assert_barrier_error(&creator_error);

    let activation_error = fixture
        .backend
        .activate_provision_workload(&fixture.id, &fixture.execution_attempt_id)
        .expect_err("a durable drain must reject direct activation");
    assert_barrier_error(&activation_error);

    let restart_fence = crate::SandboxRestartAttemptFence::new(
        fixture.execution_attempt_id.clone(),
        crate::SandboxExecutionAttemptId::new("execution-admission-restart")
            .expect("target attempt should validate"),
        1,
    )
    .expect("restart fence should validate");
    let restart_error = fixture
        .backend
        .quiesce_restart_source(&fixture.id, &restart_fence)
        .expect_err("a durable drain must reject restart admission");
    assert_barrier_error(&restart_error);

    let legacy_error = fixture
        .backend
        .launch_manifest(&mut manifest, true)
        .expect_err("a durable drain must reject legacy launch admission");
    assert_barrier_error(&legacy_error);
    assert_eq!(snapshot_files(fixture.root.path()), before);
}

#[test]
fn durable_drain_barrier_blocks_an_admitted_runner_before_effects() {
    let fixture = TeardownFixture::reserved("runner-effect-barrier");
    let mut admitted = fixture.manifest();
    admitted.lifecycle_coordinator =
        super::super::manifest::ContainerLifecycleCoordinator::PreparedServiceRunner;
    fixture
        .backend
        .write_existing_workload_manifest(&admitted)
        .expect("runner coordinator fixture should become durable");
    let handoff =
        super::super::runner::persist_runner_execution_ownership(&fixture.backend, &mut admitted)
            .expect("runner admission should become durable before drain");
    drop(handoff);

    let drain = fixture.command(SandboxExecutionTeardownOperation::Drain, "drain-runner", 1);
    let drain_outcome = fixture.backend.execute_execution_teardown(&drain);
    assert!(
        matches!(
            drain_outcome,
            SandboxExecutionTeardownObservation::Succeeded { .. }
        ),
        "the owner-dead pre-effect claim is a settled no-effect admission: {drain_outcome:?}"
    );
    assert!(matches!(
        fixture.manifest().execution_teardown.drain(),
        ContainerDrainProgress::Drained { fence, .. } if fence == drain.provider_claim()
    ));

    let mut replay = fixture.manifest();
    let handoff =
        super::super::runner::persist_runner_execution_ownership(&fixture.backend, &mut replay)
            .expect("the admitted runner should recover its exact handoff");
    let before = snapshot_files(fixture.root.path());
    let error = super::super::runner::mark_runner_effects_started(&replay, &handoff)
        .expect_err("the durable drain must reject runner effects");
    assert_barrier_error(&error);
    drop(handoff);
    assert_eq!(snapshot_files(fixture.root.path()), before);
    let replay_outcome = fixture.backend.execute_execution_teardown(&drain);
    assert!(
        matches!(
            replay_outcome,
            SandboxExecutionTeardownObservation::Succeeded { .. }
        ),
        "exact drain replay must retain terminal success: {replay_outcome:?}"
    );
}

#[test]
fn every_durable_restart_checkpoint_is_a_settled_drain_boundary() {
    use super::super::manifest::ContainerRestartTransition;

    for phase in [
        "source_quiesced",
        "target_preparing",
        "target_prepared",
        "retained_network_attached",
    ] {
        let fixture = TeardownFixture::reserved(&format!("restart-checkpoint-{phase}"));
        let mut manifest = fixture.manifest();
        let target_attempt =
            crate::SandboxExecutionAttemptId::new(format!("restart-checkpoint-target-{phase}"))
                .expect("target attempt should validate");
        let restart_fence = crate::SandboxRestartAttemptFence::new(
            fixture.execution_attempt_id.clone(),
            target_attempt.clone(),
            1,
        )
        .expect("restart fence should validate");
        let creator_quiescence =
            crate::backends::conmon::creator::CreatorQuiescenceProof::never_spawned(format!(
                "restart-checkpoint-creator-{phase}"
            ));
        manifest.restart_transition = Some(match phase {
            "source_quiesced" => ContainerRestartTransition::SourceQuiesced {
                fence: restart_fence.clone(),
                creator_quiescence,
            },
            "target_preparing" => ContainerRestartTransition::TargetPreparing {
                fence: restart_fence.clone(),
                creator_quiescence,
            },
            "target_prepared" => ContainerRestartTransition::TargetPrepared {
                fence: restart_fence.clone(),
                creator_quiescence,
            },
            "retained_network_attached" => ContainerRestartTransition::RetainedNetworkAttached {
                fence: restart_fence.clone(),
                creator_quiescence,
            },
            _ => unreachable!("restart phase table is closed"),
        });
        let execution_attempt = if phase == "source_quiesced" {
            fixture.execution_attempt_id.clone()
        } else {
            manifest.execution_attempt_id = target_attempt.clone();
            target_attempt
        };
        fixture
            .backend
            .write_existing_workload_manifest(&manifest)
            .expect("stable restart checkpoint should persist");

        let drain = fixture.command_for_execution(
            &execution_attempt,
            SandboxExecutionTeardownOperation::Drain,
            &format!("drain-restart-{phase}"),
            1,
        );
        let network_before = fixture.network_authority();
        assert!(matches!(
            fixture.backend.execute_execution_teardown(&drain),
            SandboxExecutionTeardownObservation::Succeeded { .. }
        ));
        assert!(matches!(
            fixture.manifest().execution_teardown.drain(),
            ContainerDrainProgress::Drained { fence, .. } if fence == drain.provider_claim()
        ));
        let error = fixture
            .backend
            .prepare_restart_target_attempt(&fixture.id, &restart_fence)
            .expect_err("the drained execution must reject restart progression");
        assert_barrier_error(&error);
        assert_eq!(fixture.network_authority(), network_before);
    }
}

fn assert_barrier_error(error: &crate::SandboxError) {
    assert!(
        error
            .to_string()
            .contains("after the durable execution drain barrier"),
        "unexpected admission failure: {error}"
    );
}

fn snapshot_files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, current: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let mut entries = std::fs::read_dir(current)
            .expect("snapshot directory should read")
            .map(|entry| entry.expect("snapshot entry should read"))
            .collect::<Vec<_>>();
        entries.sort_by_key(std::fs::DirEntry::path);
        for entry in entries {
            let path = entry.path();
            let metadata = entry.metadata().expect("snapshot metadata should read");
            if metadata.is_dir() {
                visit(root, &path, out);
            } else if metadata.is_file() && path.extension().is_none_or(|ext| ext != "lock") {
                out.insert(
                    path.strip_prefix(root)
                        .expect("snapshot path should stay below root")
                        .to_path_buf(),
                    std::fs::read(&path).expect("snapshot file should read"),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

fn provider_journal_files(fixture: &TeardownFixture) -> BTreeMap<PathBuf, Vec<u8>> {
    snapshot_files(&fixture.backend.config.workload_state_root)
        .into_iter()
        .filter(|(path, _)| {
            path.components()
                .any(|component| component.as_os_str() == ".nimbus-provider-command-attempts")
        })
        .collect()
}
