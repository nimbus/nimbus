//! Deterministic contract tests for exact Krun execution teardown.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use nimbus_core::TenantId;

use crate::backends::conmon::creator::{CreatorAttemptReceipt, CreatorQuiescenceProof};
use crate::backends::conmon::runtime_process::{
    RuntimeProcessIdentity, RuntimeProcessIdentityObservation, RuntimeProcessSignal,
    RuntimeProcessSignalOutcome,
};
use crate::backends::oci::command::CommandSpec;
use crate::backends::oci::egress::EgressProxyAssignment;
use crate::backends::oci::materializer::MaterializedImageRootfs;
use crate::{
    ProviderCommandAttemptJournal, ProviderCommandClaim, ProviderCommandClaimDecision,
    ProviderCommandClaimInput, ProviderCommandObservationKind, ProviderCommandOperation,
    SandboxBackendKind, SandboxExecutionAttemptId, SandboxExecutionTeardownCommand,
    SandboxExecutionTeardownObservation, SandboxExecutionTeardownOperation, SandboxHandle,
    SandboxId, SandboxOwnerSpec, SandboxProcessSpec, SandboxRestartAttemptFence, SandboxRootSpec,
    SandboxRootfsSpec, SandboxSpec, SandboxStatus,
};

use super::super::{
    KrunBundleLayout, KrunCreatorHandoffState, KrunImageMetadata, KrunLaunchArtifact,
    KrunLaunchAuthority, KrunLifecycleLockTestProbe, KrunProviderFailureCleanupState,
    KrunSandboxBackend, KrunSandboxBackendConfig, KrunSandboxManifest, KrunStartMode,
    OciConmonLaunchPlan, OciConmonLayout, OciNetworkConfig, OciNetworkLayout,
    default_network_attachment_id,
};
use super::*;

#[path = "tests/fresh_process.rs"]
mod fresh_process;
#[path = "tests/network_teardown.rs"]
mod network_teardown;

struct TeardownFixture {
    root: tempfile::TempDir,
    backend: KrunSandboxBackend,
    runtime: Arc<ScriptedRuntime>,
    id: SandboxId,
    execution_attempt_id: SandboxExecutionAttemptId,
}

#[derive(Clone)]
struct ScriptedRuntime {
    backend: KrunSandboxBackend,
    now_unix_millis: Arc<AtomicU64>,
    terminal: Arc<AtomicBool>,
    process_observation: Arc<Mutex<Option<RuntimeProcessIdentityObservation>>>,
    signals: Arc<Mutex<Vec<(RuntimeProcessIdentity, i32)>>>,
    terminal_checks: Arc<AtomicU64>,
    capture_failure: Arc<Mutex<Option<String>>>,
}

struct BlockingTerminalRuntime {
    inner: ScriptedRuntime,
    entered: SyncSender<()>,
    release: Mutex<Receiver<()>>,
}

impl ScriptedRuntime {
    fn live(backend: KrunSandboxBackend, now_unix_millis: u64) -> Self {
        Self {
            backend,
            now_unix_millis: Arc::new(AtomicU64::new(now_unix_millis)),
            terminal: Arc::new(AtomicBool::new(false)),
            process_observation: Arc::new(Mutex::new(Some(
                RuntimeProcessIdentityObservation::ExactLive,
            ))),
            signals: Arc::new(Mutex::new(Vec::new())),
            terminal_checks: Arc::new(AtomicU64::new(0)),
            capture_failure: Arc::new(Mutex::new(None)),
        }
    }

    fn set_now(&self, value: u64) {
        self.now_unix_millis.store(value, Ordering::Release);
    }

    fn set_terminal(&self, value: bool) {
        self.terminal.store(value, Ordering::Release);
    }

    fn set_process_observation(&self, observation: RuntimeProcessIdentityObservation) {
        *self
            .process_observation
            .lock()
            .expect("scripted process observation lock should be healthy") = Some(observation);
    }

    fn set_process_observation_unknown(&self) {
        *self
            .process_observation
            .lock()
            .expect("scripted process observation lock should be healthy") = None;
    }

    fn set_capture_failure(&self, message: &str) {
        *self
            .capture_failure
            .lock()
            .expect("scripted capture failure lock should be healthy") = Some(message.to_owned());
    }

    fn signals(&self) -> Vec<(RuntimeProcessIdentity, i32)> {
        self.signals
            .lock()
            .expect("scripted signal lock should be healthy")
            .clone()
    }
}

impl KrunExecutionTeardownRuntime for ScriptedRuntime {
    fn now_unix_millis(&self) -> crate::Result<u64> {
        Ok(self.now_unix_millis.load(Ordering::Acquire))
    }

    fn observe_execution_terminal(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> crate::Result<KrunExecutionTerminalObservation> {
        self.terminal_checks.fetch_add(1, Ordering::AcqRel);
        let durable = self
            .backend
            .read_manifest(&manifest.handle.id)?
            .expect("terminal inspection requires a durable manifest");
        assert!(matches!(
            durable.execution_teardown.stop(),
            KrunStopProgress::IntentPersisted { .. }
                | KrunStopProgress::GracefulSignalMayExist { .. }
                | KrunStopProgress::KillMayExist { .. }
        ));
        Ok(if self.terminal.load(Ordering::Acquire) {
            KrunExecutionTerminalObservation::ExactExit { exit_code: 0 }
        } else {
            KrunExecutionTerminalObservation::NotObserved
        })
    }

    fn capture_process(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> crate::Result<RuntimeProcessIdentity> {
        if let Some(message) = self
            .capture_failure
            .lock()
            .expect("scripted capture failure lock should be healthy")
            .clone()
        {
            return Err(crate::SandboxError::OperationFailed { message });
        }
        let attempt_id = match &manifest.creator_handoff {
            KrunCreatorHandoffState::RuntimeObserved { receipt }
            | KrunCreatorHandoffState::Pending { receipt } => receipt.attempt_id(),
            KrunCreatorHandoffState::Quiesced { proof } => proof.attempt_id(),
            KrunCreatorHandoffState::SpawnIntent { attempt_id } => attempt_id,
            KrunCreatorHandoffState::NotSpawned => "missing-creator",
        };
        Ok(RuntimeProcessIdentity::fixture(
            manifest.handle.id.as_str(),
            attempt_id,
            42,
        ))
    }

    fn inspect_process(
        &self,
        _manifest: &KrunSandboxManifest,
        _identity: &RuntimeProcessIdentity,
    ) -> crate::Result<RuntimeProcessIdentityObservation> {
        self.process_observation
            .lock()
            .expect("scripted process observation lock should be healthy")
            .ok_or_else(|| crate::SandboxError::OperationFailed {
                message: "runtime process evidence is unknown".to_owned(),
            })
    }

    fn signal_process(
        &self,
        manifest: &KrunSandboxManifest,
        identity: &RuntimeProcessIdentity,
        signal: RuntimeProcessSignal,
    ) -> crate::Result<RuntimeProcessSignalOutcome> {
        let durable = self
            .backend
            .read_manifest(&manifest.handle.id)?
            .expect("signal requires a durable manifest");
        match signal.number() {
            libc::SIGKILL => assert!(matches!(
                durable.execution_teardown.stop(),
                KrunStopProgress::KillMayExist { .. }
            )),
            _ => assert!(matches!(
                durable.execution_teardown.stop(),
                KrunStopProgress::GracefulSignalMayExist { .. }
            )),
        }
        self.signals
            .lock()
            .expect("scripted signal lock should be healthy")
            .push((identity.clone(), signal.number()));
        Ok(RuntimeProcessSignalOutcome::Delivered)
    }
}

impl KrunExecutionTeardownRuntime for BlockingTerminalRuntime {
    fn now_unix_millis(&self) -> crate::Result<u64> {
        self.inner.now_unix_millis()
    }

    fn observe_execution_terminal(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> crate::Result<KrunExecutionTerminalObservation> {
        self.entered
            .send(())
            .expect("blocking runtime owner should await the effect boundary");
        self.release
            .lock()
            .expect("blocking runtime release lock should be healthy")
            .recv_timeout(Duration::from_secs(5))
            .expect("blocking runtime release should be bounded");
        self.inner.observe_execution_terminal(manifest)
    }

    fn capture_process(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> crate::Result<RuntimeProcessIdentity> {
        self.inner.capture_process(manifest)
    }

    fn inspect_process(
        &self,
        manifest: &KrunSandboxManifest,
        identity: &RuntimeProcessIdentity,
    ) -> crate::Result<RuntimeProcessIdentityObservation> {
        self.inner.inspect_process(manifest, identity)
    }

    fn signal_process(
        &self,
        manifest: &KrunSandboxManifest,
        identity: &RuntimeProcessIdentity,
        signal: RuntimeProcessSignal,
    ) -> crate::Result<RuntimeProcessSignalOutcome> {
        self.inner.signal_process(manifest, identity, signal)
    }
}

impl TeardownFixture {
    fn new(label: &str) -> Self {
        let root = tempfile::tempdir().expect("temporary root should exist");
        let mut config = KrunSandboxBackendConfig::under_root(root.path());
        config.stop_timeout = Duration::from_millis(100);
        let base = KrunSandboxBackend::new(config);
        let runtime = Arc::new(ScriptedRuntime::live(base.clone(), 100));
        let backend = base.with_teardown_runtime_provider(runtime.clone());
        let id = SandboxId::new(format!("krun-teardown-{label}"));
        let tenant_id =
            TenantId::new(format!("tenant-{label}")).expect("fixture tenant should validate");
        let spec = SandboxSpec::new(
            tenant_id.clone(),
            SandboxOwnerSpec::service(format!("workload-{label}")),
            SandboxBackendKind::Krun,
            SandboxRootSpec::Rootfs(SandboxRootfsSpec::new("/srv/rootfs")),
            SandboxProcessSpec::new(["/usr/bin/service"]),
        );
        let execution_attempt_id = SandboxExecutionAttemptId::new(format!("wea-{label}"))
            .expect("fixture execution attempt should validate");
        let plan = crate::provision::test_support::sandbox_provision_network_plan_fixture(
            &spec, &id, label,
        );
        let mut network_config = OciNetworkConfig::default();
        network_config.attachment_id = default_network_attachment_id(&id);
        network_config.network_plan = Some(plan.network_plan().clone());
        let published_port = EgressProxyAssignment::for_test("127.0.0.1", 15_431).port_lease;
        let egress_proxy = EgressProxyAssignment::for_test("127.0.0.1", 15_432);
        let state_root =
            crate::artifact_paths::state_root(&backend.config.workload_state_root, &tenant_id, &id);
        let manifest = KrunSandboxManifest {
            handle: SandboxHandle::new(
                tenant_id.clone(),
                id.clone(),
                format!("workload-{label}"),
                SandboxBackendKind::Krun,
                SandboxStatus::Ready,
                Vec::new(),
            ),
            execution_attempt_id: execution_attempt_id.clone(),
            spec,
            image_metadata: KrunImageMetadata::default(),
            launch_artifact: Some(KrunLaunchArtifact::Rootfs(MaterializedImageRootfs {
                image_reference: "registry.example.com/nimbus/teardown-fixture:latest".to_owned(),
                rootfs_path: root.path().join("retained-rootfs"),
            })),
            provision_prepared: true,
            bundle_layout: KrunBundleLayout::new(root.path().join("bundles").join(id.as_str())),
            conmon_layout: OciConmonLayout::new(state_root.clone(), &id),
            network_layout: OciNetworkLayout::with_roots(
                &backend.config.workload_state_root,
                &backend.config.network_state_root,
                &tenant_id,
                &id,
            ),
            provision_network_plan: Some(plan),
            network_config: Some(network_config),
            port_leases: vec![published_port],
            launch_authority: KrunLaunchAuthority::ProviderOwned,
            creator_handoff: KrunCreatorHandoffState::RuntimeObserved {
                receipt: CreatorAttemptReceipt::for_test(format!("creator-{label}")),
            },
            provider_failure_cleanup: KrunProviderFailureCleanupState::Inactive,
            execution_teardown: Default::default(),
            network_teardown: Default::default(),
            egress_proxy: Some(egress_proxy),
            conmon_launch: OciConmonLaunchPlan {
                create_command: CommandSpec::new("/bin/true"),
                state_command: CommandSpec::new("/bin/true"),
                start_command: CommandSpec::new("/bin/true"),
                delete_command: CommandSpec::new("/bin/true"),
            },
            last_exit_code: None,
            start_mode: KrunStartMode::Execute,
            shutdown_requested: false,
            status: SandboxStatus::Ready,
        };
        backend
            .write_manifest(&manifest)
            .expect("teardown fixture manifest should persist");
        drop(
            backend
                .lock_launch_lifecycle(&manifest)
                .expect("fixture lifecycle authority should initialize"),
        );
        Self {
            root,
            backend,
            runtime,
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
        self.command_for(
            &self.id,
            &self.execution_attempt_id,
            operation,
            attempt,
            epoch,
            "nimbus-sandbox.krun-execution",
        )
    }

    fn command_for(
        &self,
        sandbox_id: &SandboxId,
        execution_attempt_id: &SandboxExecutionAttemptId,
        operation: SandboxExecutionTeardownOperation,
        attempt: &str,
        epoch: u64,
        provider_key: &str,
    ) -> SandboxExecutionTeardownCommand {
        let manifest = self.manifest();
        let plan = manifest
            .provision_network_plan
            .as_ref()
            .expect("fixture has an exact provision plan");
        let claim = ProviderCommandClaim::new(ProviderCommandClaimInput {
            authority_id: "authority-krun-teardown".to_owned(),
            effect_subject: format!("{{\"sandbox\":\"{}\"}}", sandbox_id),
            source_attempt_id: None,
            attempt_id: attempt.to_owned(),
            dispatch_epoch: epoch,
            workload_generation: plan.generation().as_u64(),
            restart_ordinal: 0,
            desired_digest: "1".repeat(64),
            source_digest: "2".repeat(64),
            network_plan_digest: plan.network_plan().digest().to_string(),
            provider_target_digest: "3".repeat(64),
            operation: operation.provider_operation(),
        })
        .expect("fixture provider claim should validate");
        SandboxExecutionTeardownCommand::new(
            manifest.spec.tenant_id,
            sandbox_id.clone(),
            execution_attempt_id.clone(),
            provider_key,
            operation,
            claim,
        )
        .expect("fixture teardown command should validate")
    }

    fn manifest(&self) -> KrunSandboxManifest {
        self.backend
            .read_manifest(&self.id)
            .expect("fixture manifest should read")
            .expect("fixture manifest should exist")
    }

    fn write_manifest(&self, manifest: &KrunSandboxManifest) {
        self.backend
            .write_manifest(manifest)
            .expect("fixture manifest should persist");
    }

    fn drain(&self, attempt: &str, epoch: u64) -> SandboxExecutionTeardownCommand {
        let command = self.command(SandboxExecutionTeardownOperation::Drain, attempt, epoch);
        assert!(matches!(
            self.backend.execute_execution_teardown(&command),
            SandboxExecutionTeardownObservation::Succeeded { .. }
        ));
        command
    }

    fn network_authority(&self) -> Vec<u8> {
        let manifest = self.manifest();
        serde_json::to_vec(&(
            &manifest.provision_network_plan,
            &manifest.network_config,
            &manifest.port_leases,
            &manifest.egress_proxy,
            &manifest.network_layout,
            &manifest.launch_authority,
            &manifest.launch_artifact,
        ))
        .expect("network authority snapshot should encode")
    }
}

#[derive(Clone, Copy, Debug)]
enum ProvisionEntry {
    Preparation,
    Attachment,
    Activation,
}

fn assert_provision_entry_rereads_drain_after_lock(entry: ProvisionEntry) {
    let label = match entry {
        ProvisionEntry::Preparation => "drain-race-preparation",
        ProvisionEntry::Attachment => "drain-race-attachment",
        ProvisionEntry::Activation => "drain-race-activation",
    };
    let owner = match entry {
        ProvisionEntry::Preparation => "Krun provision preparation",
        ProvisionEntry::Attachment => "Krun provision attachment",
        ProvisionEntry::Activation => "Krun provision activation",
    };
    let fixture = TeardownFixture::new(label);
    let snapshot = fixture.manifest();
    let lifecycle = fixture
        .backend
        .lock_launch_lifecycle(&snapshot)
        .expect("test owner should acquire the lifecycle lock");
    let lock_probe = KrunLifecycleLockTestProbe::new(Duration::from_secs(1));
    let worker_backend = fixture
        .backend
        .clone()
        .with_lifecycle_lock_test_probe(lock_probe.clone());
    let worker_id = fixture.id.clone();
    let worker_attempt = fixture.execution_attempt_id.clone();
    let (completed_tx, completed_rx) = std::sync::mpsc::channel();
    let worker = thread::spawn(move || {
        let result = match entry {
            ProvisionEntry::Preparation => worker_backend
                .prepare_provision_workload(&worker_id, &worker_attempt)
                .map(|_| ()),
            ProvisionEntry::Attachment => worker_backend
                .attach_provision_network(&worker_id, &worker_attempt)
                .map(|_| ()),
            ProvisionEntry::Activation => worker_backend
                .activate_provision_workload(&worker_id, &worker_attempt)
                .map(|_| ()),
        };
        completed_tx
            .send(result)
            .expect("provision result should remain observable");
    });
    assert!(
        lock_probe.wait_until_contended(),
        "{owner} must reach the actual contended lifecycle-lock boundary"
    );

    let mut drained = fixture.manifest();
    let drain = fixture.command(SandboxExecutionTeardownOperation::Drain, "race", 1);
    drained
        .execution_teardown
        .set_drain(KrunDrainProgress::BarrierPersisted {
            fence: drain.provider_claim().clone(),
        });
    fixture.write_manifest(&drained);
    let after_drain = snapshot_files(fixture.root.path());

    drop(lifecycle);
    let error = completed_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("provision entry should finish after lifecycle-lock release")
        .expect_err("the durable drain winner must fence the stale provision snapshot");
    worker.join().expect("provision worker should join");
    assert!(
        error.to_string().contains(owner)
            && error
                .to_string()
                .contains("fenced by durable execution drain progress"),
        "case={entry:?}: {error}"
    );
    assert_eq!(
        snapshot_files(fixture.root.path()),
        after_drain,
        "case={entry:?}: the rejected stale provision entry must have zero effects"
    );
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

fn persist_observation(
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
        SandboxExecutionTeardownObservation::InProgress { .. } => {
            ProviderCommandObservationKind::InProgress
        }
        SandboxExecutionTeardownObservation::RetryAuthorized { .. } => {
            ProviderCommandObservationKind::RetryAuthorized
        }
        SandboxExecutionTeardownObservation::Absent { .. } => {
            ProviderCommandObservationKind::Absent
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
        .expect("teardown observation should become durable");
}

fn snapshot_files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(base: &Path, path: &Path, output: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let mut entries = match fs::read_dir(path) {
            Ok(entries) => entries
                .map(|entry| entry.expect("fixture directory entry should read"))
                .collect::<Vec<_>>(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => panic!("fixture directory should read: {error}"),
        };
        entries.sort_by_key(|entry| entry.path());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                visit(base, &path, output);
            } else {
                output.insert(
                    path.strip_prefix(base)
                        .expect("snapshot path should stay below root")
                        .to_path_buf(),
                    fs::read(&path).expect("snapshot file should read"),
                );
            }
        }
    }

    let mut output = BTreeMap::new();
    visit(root, root, &mut output);
    output
}

fn claim_input_from(
    claim: &ProviderCommandClaim,
    operation: ProviderCommandOperation,
) -> ProviderCommandClaimInput {
    ProviderCommandClaimInput {
        authority_id: claim.authority_id().to_owned(),
        effect_subject: claim.effect_subject().to_owned(),
        source_attempt_id: claim.source_attempt_id().map(str::to_owned),
        attempt_id: claim.attempt_id().to_owned(),
        dispatch_epoch: claim.dispatch_epoch(),
        workload_generation: claim.workload_generation(),
        restart_ordinal: claim.restart_ordinal(),
        desired_digest: claim.desired_digest().to_owned(),
        source_digest: claim.source_digest().to_owned(),
        network_plan_digest: claim.network_plan_digest().to_owned(),
        provider_target_digest: claim.provider_target_digest().to_owned(),
        operation,
    }
}

fn assert_drain_remains_pending(fixture: &TeardownFixture, label: &str) {
    let network_before = fixture.network_authority();
    let command = fixture.command(SandboxExecutionTeardownOperation::Drain, label, 1);
    assert!(matches!(
        fixture.backend.execute_execution_teardown(&command),
        SandboxExecutionTeardownObservation::InProgress { .. }
    ));
    let after_execute = snapshot_files(fixture.root.path());
    assert!(matches!(
        fixture.backend.inspect_execution_teardown(&command),
        SandboxExecutionTeardownObservation::InProgress { .. }
    ));
    assert_eq!(snapshot_files(fixture.root.path()), after_execute);
    assert_eq!(fixture.network_authority(), network_before);
}

#[test]
fn krun_execution_drain_persists_barrier_and_keeps_exact_runtime_running() {
    let fixture = TeardownFixture::new("drain-running");
    let before = fixture.network_authority();
    let command = fixture.drain("drain", 1);
    let manifest = fixture.manifest();

    assert!(matches!(
        manifest.execution_teardown.drain(),
        KrunDrainProgress::Drained { fence, .. } if fence == command.provider_claim()
    ));
    assert!(!manifest.shutdown_requested);
    assert_eq!(manifest.status, SandboxStatus::Ready);
    assert!(fixture.runtime.signals().is_empty());
    assert_eq!(fixture.network_authority(), before);
}

#[test]
fn krun_pre_activation_stop_closes_admission_without_a_drain_command() {
    let fixture = TeardownFixture::new("pre-activation-stop");
    let mut manifest = fixture.manifest();
    let reservation_claim = crate::backends::oci::port_lease::new_launch_reservation_claim()
        .expect("pre-activation reservation claim should validate");
    manifest.launch_authority = KrunLaunchAuthority::Adopted { reservation_claim };
    manifest.creator_handoff = KrunCreatorHandoffState::NotSpawned;
    manifest.status = SandboxStatus::Starting;
    manifest.handle.status = SandboxStatus::Starting;
    fixture.write_manifest(&manifest);
    let before = fixture.network_authority();
    let stop = fixture.command(SandboxExecutionTeardownOperation::Stop, "stop", 1);

    assert!(matches!(
        fixture.backend.inspect_execution_teardown(&stop),
        SandboxExecutionTeardownObservation::Absent { .. }
    ));
    assert!(matches!(
        fixture.backend.execute_execution_teardown(&stop),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));

    let durable = fixture.manifest();
    assert!(matches!(
        durable.execution_teardown.drain(),
        KrunDrainProgress::ExecutionNeverAdmitted { fence, .. }
            if fence == stop.provider_claim()
    ));
    assert!(matches!(
        durable.execution_teardown.stop(),
        KrunStopProgress::ExecutionStopped { fence, .. }
            if fence == stop.provider_claim()
    ));
    assert!(
        durable
            .require_execution_admission_open("late Krun activation")
            .is_err(),
        "the no-execution stop fence must close later activation admission"
    );
    assert!(fixture.runtime.signals().is_empty());
    assert_eq!(
        fixture.runtime.terminal_checks.load(Ordering::Acquire),
        0,
        "creator absence must not consult unauthenticated runtime state"
    );
    assert_eq!(fixture.network_authority(), before);
    assert!(matches!(
        fixture.backend.inspect_execution_teardown(&stop),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
}

#[test]
fn krun_provider_owned_pre_activation_stop_proves_execution_was_never_admitted() {
    let fixture = TeardownFixture::new("provider-owned-pre-activation-stop");
    let mut manifest = fixture.manifest();
    manifest.launch_authority = KrunLaunchAuthority::ProviderOwned;
    manifest.creator_handoff = KrunCreatorHandoffState::NotSpawned;
    manifest.status = SandboxStatus::Starting;
    manifest.handle.status = SandboxStatus::Starting;
    fixture.write_manifest(&manifest);
    let before = fixture.network_authority();
    let stop = fixture.command(SandboxExecutionTeardownOperation::Stop, "stop", 1);

    assert!(matches!(
        fixture.backend.execute_execution_teardown(&stop),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));

    let durable = fixture.manifest();
    assert!(matches!(
        durable.execution_teardown.drain(),
        KrunDrainProgress::ExecutionNeverAdmitted { fence, .. }
            if fence == stop.provider_claim()
    ));
    assert!(matches!(
        durable.execution_teardown.stop(),
        KrunStopProgress::ExecutionStopped { fence, .. }
            if fence == stop.provider_claim()
    ));
    assert!(fixture.runtime.signals().is_empty());
    assert_eq!(
        fixture.runtime.terminal_checks.load(Ordering::Acquire),
        0,
        "a never-spawned creator must not consult unauthenticated runtime state"
    );
    assert_eq!(fixture.network_authority(), before);
}

#[test]
fn krun_execution_drain_fences_creator_activation_restart_and_launch_admission() {
    let fixture = TeardownFixture::new("drain-admission");
    fixture.drain("drain", 1);
    let mut manifest = fixture.manifest();

    for owner in [
        "Krun creator spawn",
        "Krun creator release",
        "Krun provision preparation",
        "Krun provision attachment",
        "Krun provision activation",
        "Krun restart source quiescence",
        "Krun restart target preparation",
        "Krun retained restart attachment",
        "Krun legacy launch",
        "coarse Krun stop",
    ] {
        let error = manifest
            .require_execution_admission_open(owner)
            .expect_err("durable drain must fence every new provider effect");
        assert!(error.to_string().contains(owner));
    }
    let before = snapshot_files(fixture.root.path());
    let error = fixture
        .backend
        .spawn_creator_and_wait_for_runtime(&mut manifest)
        .expect_err("creator spawn must stop before its first effect");
    assert!(error.to_string().contains("Krun creator spawn"));
    assert_eq!(snapshot_files(fixture.root.path()), before);

    let target_attempt = SandboxExecutionAttemptId::new("wea-drain-admission-target")
        .expect("restart target should validate");
    let restart_fence =
        SandboxRestartAttemptFence::new(fixture.execution_attempt_id.clone(), target_attempt, 1)
            .expect("restart fence should validate");
    for (owner, result) in [
        (
            "Krun provision preparation",
            fixture
                .backend
                .prepare_provision_workload(&fixture.id, &fixture.execution_attempt_id)
                .map(|_| ()),
        ),
        (
            "Krun provision attachment",
            fixture
                .backend
                .attach_provision_network(&fixture.id, &fixture.execution_attempt_id)
                .map(|_| ()),
        ),
        (
            "Krun provision activation",
            fixture
                .backend
                .activate_provision_workload(&fixture.id, &fixture.execution_attempt_id)
                .map(|_| ()),
        ),
        (
            "Krun restart source quiescence",
            fixture
                .backend
                .quiesce_restart_source(&fixture.id, &restart_fence)
                .map(|_| ()),
        ),
        (
            "Krun restart target preparation",
            fixture
                .backend
                .prepare_restart_target(&fixture.id, &restart_fence)
                .map(|_| ()),
        ),
        (
            "Krun retained restart attachment",
            fixture
                .backend
                .attach_restart_retained_network(&fixture.id, &restart_fence)
                .map(|_| ()),
        ),
    ] {
        let error = result.expect_err("durable drain must fence the real provider entry point");
        assert!(error.to_string().contains(owner), "case={owner}: {error}");
        assert_eq!(snapshot_files(fixture.root.path()), before, "case={owner}");
    }
    let mut legacy_manifest = fixture.manifest();
    let error = fixture
        .backend
        .launch_manifest(&mut legacy_manifest, false)
        .expect_err("durable drain must fence legacy launch before reconciliation");
    assert!(error.to_string().contains("Krun legacy launch"));
    assert_eq!(snapshot_files(fixture.root.path()), before);

    for entry in [
        ProvisionEntry::Preparation,
        ProvisionEntry::Attachment,
        ProvisionEntry::Activation,
    ] {
        assert_provision_entry_rereads_drain_after_lock(entry);
    }
}

#[test]
fn krun_execution_drain_pending_owner_is_ambiguous_and_byte_stable() {
    let fixture = TeardownFixture::new("drain-pending");
    let mut manifest = fixture.manifest();
    manifest.creator_handoff = KrunCreatorHandoffState::Pending {
        receipt: CreatorAttemptReceipt::for_test("pending-creator"),
    };
    fixture.write_manifest(&manifest);
    assert_drain_remains_pending(&fixture, "pending-creator");

    let not_spawned = TeardownFixture::new("drain-not-spawned");
    let mut manifest = not_spawned.manifest();
    manifest.creator_handoff = KrunCreatorHandoffState::NotSpawned;
    not_spawned.write_manifest(&manifest);
    assert_drain_remains_pending(&not_spawned, "missing-creator");

    let cleanup = TeardownFixture::new("drain-cleanup");
    let mut manifest = cleanup.manifest();
    manifest.provider_failure_cleanup = KrunProviderFailureCleanupState::Requested;
    cleanup.write_manifest(&manifest);
    assert_drain_remains_pending(&cleanup, "provider-cleanup");

    let activation = TeardownFixture::new("drain-activation");
    let mut manifest = activation.manifest();
    manifest.provision_prepared = false;
    activation.write_manifest(&manifest);
    assert_drain_remains_pending(&activation, "activation");

    let restart = TeardownFixture::new("drain-restart");
    let manifest = restart.manifest();
    let restart_record = serde_json::json!({
        "fence": {
            "source_attempt_id": "wea-restart-source",
            "attempt_id": manifest.execution_attempt_id.as_str(),
            "restart_ordinal": 1,
        },
        "phase": "source_quiesced",
    });
    fs::write(
        manifest
            .conmon_layout
            .container_state_dir
            .join(".nimbus-krun-restart.json"),
        serde_json::to_vec_pretty(&restart_record)
            .expect("partial restart record should serialize"),
    )
    .expect("partial restart record should persist");
    assert_drain_remains_pending(&restart, "partial-restart");
}

#[test]
fn krun_execution_drain_inspection_is_read_only_and_creates_no_lock() {
    let fixture = TeardownFixture::new("drain-inspect");
    let command = fixture.drain("drain", 1);
    let manifest = fixture.manifest();
    let lock_path = manifest
        .conmon_layout
        .container_state_dir
        .join(".nimbus-krun-lifecycle.lock");
    fs::remove_file(&lock_path).expect("execution created the lifecycle lock");
    let before = snapshot_files(fixture.root.path());

    assert!(matches!(
        fixture.backend.inspect_execution_teardown(&command),
        SandboxExecutionTeardownObservation::Ambiguous { .. }
    ));
    assert_eq!(snapshot_files(fixture.root.path()), before);
    assert!(!lock_path.exists());
}

#[test]
fn krun_execution_teardown_crossed_locator_matrix_is_zero_effect() {
    let fixture = TeardownFixture::new("crossed-matrix");
    let exact = fixture.command(SandboxExecutionTeardownOperation::Drain, "drain", 1);
    let crossed_attempt = SandboxExecutionAttemptId::new("wea-crossed")
        .expect("crossed execution attempt should validate");
    let crossed_provider = fixture.command_for(
        &fixture.id,
        &fixture.execution_attempt_id,
        SandboxExecutionTeardownOperation::Drain,
        "provider",
        1,
        "nimbus-sandbox.crossed",
    );
    let crossed_execution = fixture.command_for(
        &fixture.id,
        &crossed_attempt,
        SandboxExecutionTeardownOperation::Drain,
        "execution",
        1,
        "nimbus-sandbox.krun-execution",
    );
    let crossed_tenant = SandboxExecutionTeardownCommand::new(
        TenantId::new("tenant-crossed").expect("crossed tenant should validate"),
        fixture.id.clone(),
        fixture.execution_attempt_id.clone(),
        exact.provider_registration_key(),
        exact.operation(),
        exact.provider_claim().clone(),
    )
    .expect("crossed tenant command should validate structurally");
    let mut crossed_plan_input = claim_input_from(
        exact.provider_claim(),
        ProviderCommandOperation::DrainExecution,
    );
    crossed_plan_input.network_plan_digest = "4".repeat(64);
    let crossed_plan = SandboxExecutionTeardownCommand::new(
        exact.tenant_id().clone(),
        fixture.id.clone(),
        fixture.execution_attempt_id.clone(),
        exact.provider_registration_key(),
        exact.operation(),
        ProviderCommandClaim::new(crossed_plan_input)
            .expect("crossed plan claim should validate structurally"),
    )
    .expect("crossed plan command should validate structurally");

    for command in [
        &crossed_provider,
        &crossed_execution,
        &crossed_tenant,
        &crossed_plan,
    ] {
        let before = snapshot_files(fixture.root.path());
        let observation = fixture.backend.execute_execution_teardown(command);
        assert!(matches!(
            observation,
            SandboxExecutionTeardownObservation::DefiniteFailure { ref code, .. }
                if code == "sandbox_teardown_command_crossed"
        ));
        assert_eq!(snapshot_files(fixture.root.path()), before);
    }
}

#[test]
fn krun_execution_teardown_missing_or_corrupt_manifest_is_ambiguous_and_zero_effect() {
    let fixture = TeardownFixture::new("missing-corrupt");
    let missing_id = SandboxId::new("krun-teardown-missing");
    let missing = fixture.command_for(
        &missing_id,
        &fixture.execution_attempt_id,
        SandboxExecutionTeardownOperation::Drain,
        "missing",
        1,
        "nimbus-sandbox.krun-execution",
    );
    let before_missing = snapshot_files(fixture.root.path());
    assert!(matches!(
        fixture.backend.execute_execution_teardown(&missing),
        SandboxExecutionTeardownObservation::Ambiguous { .. }
    ));
    assert_eq!(snapshot_files(fixture.root.path()), before_missing);

    let missing_plan = TeardownFixture::new("missing-plan");
    let missing_plan_command =
        missing_plan.command(SandboxExecutionTeardownOperation::Drain, "missing-plan", 1);
    let mut missing_plan_manifest = missing_plan.manifest();
    missing_plan_manifest.provision_network_plan = None;
    missing_plan.write_manifest(&missing_plan_manifest);
    let before_missing_plan = snapshot_files(missing_plan.root.path());
    assert!(matches!(
        missing_plan
            .backend
            .execute_execution_teardown(&missing_plan_command),
        SandboxExecutionTeardownObservation::Ambiguous { .. }
    ));
    assert_eq!(
        snapshot_files(missing_plan.root.path()),
        before_missing_plan
    );

    let manifest = fixture.manifest();
    let command = fixture.command_for(
        &fixture.id,
        &fixture.execution_attempt_id,
        SandboxExecutionTeardownOperation::Drain,
        "corrupt",
        1,
        "nimbus-sandbox.krun-execution",
    );
    fs::write(&manifest.conmon_layout.manifest_path, b"{corrupt")
        .expect("corrupt manifest fixture should persist");
    let before_corrupt = snapshot_files(fixture.root.path());
    assert!(matches!(
        fixture.backend.execute_execution_teardown(&command),
        SandboxExecutionTeardownObservation::Ambiguous { .. }
    ));
    assert_eq!(snapshot_files(fixture.root.path()), before_corrupt);
}

#[test]
fn krun_execution_stop_requires_the_exact_drain_fence() {
    let fixture = TeardownFixture::new("stop-needs-drain");
    let stop = fixture.command(SandboxExecutionTeardownOperation::Stop, "shared", 1);
    let before = snapshot_files(fixture.root.path());

    assert!(matches!(
        fixture.backend.execute_execution_teardown(&stop),
        SandboxExecutionTeardownObservation::DefiniteFailure { ref code, .. }
            if code == "sandbox_teardown_command_crossed"
    ));
    assert_eq!(snapshot_files(fixture.root.path()), before);

    let drain = fixture.drain("shared", 1);
    let mut crossed_input = claim_input_from(
        fixture
            .command(SandboxExecutionTeardownOperation::Stop, "shared", 1)
            .provider_claim(),
        ProviderCommandOperation::StopExecution,
    );
    crossed_input.effect_subject = "{\"sandbox\":\"crossed-subject\"}".to_owned();
    let crossed = SandboxExecutionTeardownCommand::new(
        drain.tenant_id().clone(),
        fixture.id.clone(),
        fixture.execution_attempt_id.clone(),
        "nimbus-sandbox.krun-execution",
        SandboxExecutionTeardownOperation::Stop,
        ProviderCommandClaim::new(crossed_input)
            .expect("crossed stop subject should validate structurally"),
    )
    .expect("crossed stop command should validate structurally");
    assert!(matches!(
        fixture.backend.execute_execution_teardown(&crossed),
        SandboxExecutionTeardownObservation::DefiniteFailure { .. }
    ));

    let mut crossed_workload_input = claim_input_from(
        fixture
            .command(SandboxExecutionTeardownOperation::Stop, "shared", 1)
            .provider_claim(),
        ProviderCommandOperation::StopExecution,
    );
    crossed_workload_input.desired_digest = "9".repeat(64);
    let crossed_workload = SandboxExecutionTeardownCommand::new(
        drain.tenant_id().clone(),
        fixture.id.clone(),
        fixture.execution_attempt_id.clone(),
        "nimbus-sandbox.krun-execution",
        SandboxExecutionTeardownOperation::Stop,
        ProviderCommandClaim::new(crossed_workload_input)
            .expect("crossed workload fence should validate structurally"),
    )
    .expect("crossed workload stop should validate structurally");
    let before_crossed_workload = snapshot_files(fixture.root.path());
    assert!(matches!(
        fixture
            .backend
            .execute_execution_teardown(&crossed_workload),
        SandboxExecutionTeardownObservation::DefiniteFailure { ref code, .. }
            if code == "sandbox_teardown_command_crossed"
    ));
    assert_eq!(snapshot_files(fixture.root.path()), before_crossed_workload);
    assert!(fixture.runtime.signals().is_empty());
}

#[test]
fn krun_execution_stop_accepts_a_distinct_step_attempt_for_the_same_lifecycle() {
    let fixture = TeardownFixture::new("stop-after-distinct-drain-attempt");
    fixture.drain("drain-step-attempt", 1);
    fixture.runtime.set_terminal(true);
    let stop = fixture.command(
        SandboxExecutionTeardownOperation::Stop,
        "stop-step-attempt",
        1,
    );

    assert!(matches!(
        fixture.backend.execute_execution_teardown(&stop),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
    assert!(matches!(
        fixture.manifest().execution_teardown.stop(),
        KrunStopProgress::ExecutionStopped { fence, .. } if fence == stop.provider_claim()
    ));
}

#[test]
fn krun_execution_stop_persists_intent_before_runtime_inspection() {
    let fixture = TeardownFixture::new("stop-intent");
    fixture.drain("shared", 1);
    fixture
        .runtime
        .set_capture_failure("capture blocked after intent");
    let stop = fixture.command(SandboxExecutionTeardownOperation::Stop, "shared", 1);

    assert!(matches!(
        fixture.backend.execute_execution_teardown(&stop),
        SandboxExecutionTeardownObservation::Ambiguous { .. }
    ));
    assert!(matches!(
        fixture.manifest().execution_teardown.stop(),
        KrunStopProgress::IntentPersisted { fence } if fence == stop.provider_claim()
    ));
    assert_eq!(fixture.runtime.terminal_checks.load(Ordering::Acquire), 1);
}

#[test]
fn krun_execution_stop_persists_configured_graceful_signal_before_effect() {
    let fixture = TeardownFixture::new("stop-graceful-boundary");
    let mut manifest = fixture.manifest();
    manifest.image_metadata.stop_signal = Some("SIGUSR1".to_owned());
    fixture.write_manifest(&manifest);
    fixture.drain("shared", 1);
    let stop = fixture.command(SandboxExecutionTeardownOperation::Stop, "shared", 1);

    assert!(matches!(
        fixture.backend.execute_execution_teardown(&stop),
        SandboxExecutionTeardownObservation::InProgress { .. }
    ));
    assert_eq!(fixture.runtime.signals().len(), 1);
    assert_eq!(fixture.runtime.signals()[0].1, libc::SIGUSR1);
    assert!(matches!(
        fixture.manifest().execution_teardown.stop(),
        KrunStopProgress::GracefulSignalMayExist {
            graceful_signal,
            ..
        } if graceful_signal == "SIGUSR1"
    ));
}

#[test]
fn krun_execution_stop_persists_kill_may_exist_before_effect() {
    let fixture = TeardownFixture::new("stop-kill-boundary");
    fixture.drain("shared", 1);
    let first = fixture.command(SandboxExecutionTeardownOperation::Stop, "shared", 1);
    assert!(matches!(
        fixture.backend.execute_execution_teardown(&first),
        SandboxExecutionTeardownObservation::InProgress { .. }
    ));
    fixture.runtime.set_now(1_000);
    let journal = fixture
        .backend
        .attempt_idempotency_journal()
        .expect("stop retry journal should open");
    let first_claim = claim_teardown_execution(&journal, &first);
    let first_observation = first_claim.observation().clone();
    journal
        .record_observation(
            first.provider_claim(),
            ProviderCommandObservationKind::RetryAuthorized,
            b"graceful signal deadline elapsed",
        )
        .expect("first stop progress should persist");
    let retry = fixture.command(SandboxExecutionTeardownOperation::Stop, "shared", 2);
    let retry_claim = claim_teardown_execution(&journal, &retry);
    assert!(
        retry_claim
            .observation()
            .authenticates_retry_progress(first_observation.claim())
    );
    assert!(matches!(
        fixture
            .backend
            .execute_execution_teardown_with_claim(&retry, retry_claim),
        Ok(observation) if observation.kind() == ProviderCommandObservationKind::InProgress
    ));
    assert_eq!(
        fixture
            .runtime
            .signals()
            .iter()
            .map(|(_, signal)| *signal)
            .collect::<Vec<_>>(),
        vec![libc::SIGTERM, libc::SIGKILL]
    );
    assert!(matches!(
        fixture.manifest().execution_teardown.stop(),
        KrunStopProgress::KillMayExist { fence, .. } if fence == retry.provider_claim()
    ));

    fixture.runtime.set_now(5_000);
    let retry_durable = journal
        .adopt_exact_attempt(retry.provider_claim())
        .expect("KILL retry observation should read")
        .expect("KILL retry observation should exist");
    let retry_inspection = fixture
        .backend
        .inspect_execution_teardown_with_observation(&retry, &retry_durable);
    assert!(matches!(
        retry_inspection,
        SandboxExecutionTeardownObservation::RetryAuthorized { .. }
    ));
    persist_observation(&journal, &retry, &retry_inspection);
    let redelivery = fixture.command(SandboxExecutionTeardownOperation::Stop, "shared", 3);
    let redelivery_claim = claim_teardown_execution(&journal, &redelivery);
    assert!(matches!(
        fixture
            .backend
            .execute_execution_teardown_with_claim(&redelivery, redelivery_claim),
        Ok(observation) if observation.kind() == ProviderCommandObservationKind::InProgress
    ));
    assert_eq!(
        fixture
            .runtime
            .signals()
            .iter()
            .map(|(_, signal)| *signal)
            .collect::<Vec<_>>(),
        vec![libc::SIGTERM, libc::SIGKILL, libc::SIGKILL]
    );
    assert!(matches!(
        fixture.backend.execute_execution_teardown(&redelivery),
        SandboxExecutionTeardownObservation::InProgress { .. }
    ));
    assert_eq!(fixture.runtime.signals().len(), 3);
    let skipped = fixture.command(SandboxExecutionTeardownOperation::Stop, "shared", 5);
    assert!(
        journal
            .claim_dispatch_epoch(skipped.provider_claim())
            .is_err()
    );
    assert_eq!(fixture.runtime.signals().len(), 3);

    let nonadjacent = TeardownFixture::new("stop-kill-nonadjacent");
    nonadjacent.drain("shared", 1);
    let journal = nonadjacent
        .backend
        .attempt_idempotency_journal()
        .expect("non-adjacent retry journal should open");
    let first = nonadjacent.command(SandboxExecutionTeardownOperation::Stop, "shared", 1);
    assert!(matches!(
        nonadjacent.backend.execute_execution_teardown(&first),
        SandboxExecutionTeardownObservation::InProgress { .. }
    ));
    nonadjacent.runtime.set_now(1_000);
    let first_claim = claim_teardown_execution(&journal, &first);
    drop(first_claim);
    persist_observation(
        &journal,
        &first,
        &SandboxExecutionTeardownObservation::RetryAuthorized {
            evidence: b"graceful deadline elapsed".to_vec(),
        },
    );
    let second = nonadjacent.command(SandboxExecutionTeardownOperation::Stop, "shared", 2);
    let second_claim = claim_teardown_execution(&journal, &second);
    assert!(matches!(
        nonadjacent
            .backend
            .execute_execution_teardown_with_claim(&second, second_claim),
        Ok(observation) if observation.kind() == ProviderCommandObservationKind::InProgress
    ));
    nonadjacent.runtime.set_now(5_000);
    let second_durable = journal
        .adopt_exact_attempt(second.provider_claim())
        .expect("second observation should read")
        .expect("second observation should exist");
    let second_inspection = nonadjacent
        .backend
        .inspect_execution_teardown_with_observation(&second, &second_durable);
    assert!(matches!(
        second_inspection,
        SandboxExecutionTeardownObservation::RetryAuthorized { .. }
    ));
    persist_observation(&journal, &second, &second_inspection);

    let third = nonadjacent.command(SandboxExecutionTeardownOperation::Stop, "shared", 3);
    let third_claim = claim_teardown_execution(&journal, &third);
    let third_authorization = third_claim.observation().clone();
    drop(third_claim);
    let third_inspection = nonadjacent
        .backend
        .inspect_execution_teardown_with_observation(&third, &third_authorization);
    assert!(matches!(
        third_inspection,
        SandboxExecutionTeardownObservation::RetryAuthorized { .. }
    ));
    persist_observation(&journal, &third, &third_inspection);

    let fourth = nonadjacent.command(SandboxExecutionTeardownOperation::Stop, "shared", 4);
    let fourth_claim = claim_teardown_execution(&journal, &fourth);
    let signal_count = nonadjacent.runtime.signals().len();
    let observation = nonadjacent
        .backend
        .execute_execution_teardown_with_claim(&fourth, fourth_claim)
        .expect("non-adjacent progress should publish a definite result");
    assert_eq!(
        observation.kind(),
        ProviderCommandObservationKind::DefiniteFailure
    );
    assert_eq!(
        observation.failure_code(),
        Some("sandbox_teardown_epoch_invalid")
    );
    assert_eq!(nonadjacent.runtime.signals().len(), signal_count);
}

#[test]
fn krun_execution_stop_authenticates_runtime_creator_and_process_birth_before_signal() {
    let fixture = TeardownFixture::new("stop-process-identity");
    fixture.drain("shared", 1);
    let stop = fixture.command(SandboxExecutionTeardownOperation::Stop, "shared", 1);

    assert!(matches!(
        fixture.backend.execute_execution_teardown(&stop),
        SandboxExecutionTeardownObservation::InProgress { .. }
    ));
    let signals = fixture.runtime.signals();
    assert_eq!(signals.len(), 1);
    assert_eq!(signals[0].0.runtime_id(), fixture.id.as_str());
    assert_eq!(
        signals[0].0.creator_attempt_id(),
        "creator-stop-process-identity"
    );
    assert_eq!(signals[0].0.pid(), 42);
}

#[test]
fn krun_execution_stop_rejects_raw_recycled_or_crossed_pid_before_signal() {
    let fixture = TeardownFixture::new("stop-recycled-pid");
    fixture.drain("shared", 1);
    fixture
        .runtime
        .set_capture_failure("provider PID crossed its exact creator and process birth");
    let stop = fixture.command(SandboxExecutionTeardownOperation::Stop, "shared", 1);
    let before_network = fixture.network_authority();

    let observation = fixture.backend.execute_execution_teardown(&stop);
    assert!(matches!(
        observation,
        SandboxExecutionTeardownObservation::Ambiguous { ref evidence }
            if String::from_utf8_lossy(evidence).contains("crossed")
    ));
    assert!(fixture.runtime.signals().is_empty());
    assert_eq!(fixture.network_authority(), before_network);
}

#[test]
fn krun_execution_stop_adopts_exact_exit_receipt_or_explicit_absence() {
    let exit_fixture = TeardownFixture::new("stop-exit-receipt");
    exit_fixture.drain("shared", 1);
    exit_fixture.runtime.set_terminal(true);
    let exit_stop = exit_fixture.command(SandboxExecutionTeardownOperation::Stop, "shared", 1);
    assert!(matches!(
        exit_fixture.backend.execute_execution_teardown(&exit_stop),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
    assert!(exit_fixture.runtime.signals().is_empty());

    let absent_fixture = TeardownFixture::new("stop-explicit-absence");
    absent_fixture.drain("shared", 1);
    let first = absent_fixture.command(SandboxExecutionTeardownOperation::Stop, "shared", 1);
    assert!(matches!(
        absent_fixture.backend.execute_execution_teardown(&first),
        SandboxExecutionTeardownObservation::InProgress { .. }
    ));
    absent_fixture
        .runtime
        .set_process_observation(RuntimeProcessIdentityObservation::ExplicitlyAbsent);
    let journal = absent_fixture
        .backend
        .attempt_idempotency_journal()
        .expect("absence retry journal should open");
    let first_claim = claim_teardown_execution(&journal, &first);
    persist_observation(
        &journal,
        &first,
        &SandboxExecutionTeardownObservation::RetryAuthorized {
            evidence: b"explicit absence requires durable completion".to_vec(),
        },
    );
    drop(first_claim);
    let retry = absent_fixture.command(SandboxExecutionTeardownOperation::Stop, "shared", 2);
    let retry_claim = claim_teardown_execution(&journal, &retry);
    assert!(matches!(
        absent_fixture
            .backend
            .execute_execution_teardown_with_claim(&retry, retry_claim),
        Ok(observation) if observation.kind() == ProviderCommandObservationKind::Succeeded
    ));
    assert!(matches!(
        absent_fixture.manifest().execution_teardown.stop(),
        KrunStopProgress::ExecutionStopped { .. }
    ));
}

#[test]
fn krun_execution_stop_keeps_present_unknown_and_missing_evidence_nonterminal() {
    let fixture = TeardownFixture::new("stop-unknown");
    fixture.drain("shared", 1);
    let stop = fixture.command(SandboxExecutionTeardownOperation::Stop, "shared", 1);
    assert!(matches!(
        fixture.backend.execute_execution_teardown(&stop),
        SandboxExecutionTeardownObservation::InProgress { .. }
    ));
    fixture.runtime.set_process_observation_unknown();
    let before = snapshot_files(fixture.root.path());

    assert!(matches!(
        fixture.backend.inspect_execution_teardown(&stop),
        SandboxExecutionTeardownObservation::Ambiguous { .. }
    ));
    assert_eq!(snapshot_files(fixture.root.path()), before);
    assert!(!matches!(
        fixture.manifest().execution_teardown.stop(),
        KrunStopProgress::ExecutionStopped { .. }
    ));
}

#[test]
fn stale_krun_exit_receipt_cannot_satisfy_successor_execution_stop() {
    let fixture = TeardownFixture::new("stale-exit-receipt");
    let mut manifest = fixture.manifest();
    fs::create_dir_all(&manifest.conmon_layout.exit_dir)
        .expect("exit receipt directory should exist");
    fs::write(&manifest.conmon_layout.exit_status_file, b"0\n")
        .expect("stale exit receipt should persist");
    let successor =
        SandboxExecutionAttemptId::new("wea-successor").expect("successor id should validate");
    manifest.execution_attempt_id = successor.clone();
    manifest.creator_handoff = KrunCreatorHandoffState::RuntimeObserved {
        receipt: CreatorAttemptReceipt::for_test("creator-successor"),
    };
    manifest.execution_teardown = Default::default();
    manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "printf '%s\\n' '{{\"id\":\"{}\",\"status\":\"running\",\"annotations\":{{\"com.nimbus.creator-attempt\":\"creator-successor\"}}}}'",
            fixture.id,
        ),
    ]);
    fixture.write_manifest(&manifest);
    let drain = fixture.command_for(
        &fixture.id,
        &successor,
        SandboxExecutionTeardownOperation::Drain,
        "successor",
        1,
        "nimbus-sandbox.krun-execution",
    );
    assert!(matches!(
        fixture.backend.execute_execution_teardown(&drain),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
    let stop = fixture.command_for(
        &fixture.id,
        &successor,
        SandboxExecutionTeardownOperation::Stop,
        "successor",
        1,
        "nimbus-sandbox.krun-execution",
    );
    let host_backend =
        KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(fixture.root.path()));

    assert!(matches!(
        host_backend.execute_execution_teardown(&stop),
        SandboxExecutionTeardownObservation::Ambiguous { .. }
    ));
    let durable = fixture.manifest();
    assert_eq!(durable.last_exit_code, None);
    assert!(matches!(
        durable.execution_teardown.stop(),
        KrunStopProgress::IntentPersisted { .. }
    ));

    let mut absent_successor = durable;
    absent_successor.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "printf '%s\\n' 'container `{}` does not exist: open `/run/crun/{}/status`: No such file or directory' >&2; exit 1",
            fixture.id, fixture.id,
        ),
    ]);
    fixture.write_manifest(&absent_successor);
    assert!(matches!(
        host_backend.execute_execution_teardown(&stop),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
    let stopped = fixture.manifest();
    assert_eq!(
        stopped.last_exit_code, None,
        "an unqualified predecessor receipt must never become the successor exit code"
    );
    assert!(matches!(
        stopped.execution_teardown.stop(),
        KrunStopProgress::ExecutionStopped { .. }
    ));
}

#[test]
fn krun_execution_stop_replay_never_duplicates_a_signal() {
    let fixture = TeardownFixture::new("stop-replay");
    fixture.drain("shared", 1);
    let stop = fixture.command(SandboxExecutionTeardownOperation::Stop, "shared", 1);

    assert!(matches!(
        fixture.backend.execute_execution_teardown(&stop),
        SandboxExecutionTeardownObservation::InProgress { .. }
    ));
    assert!(matches!(
        fixture.backend.execute_execution_teardown(&stop),
        SandboxExecutionTeardownObservation::InProgress { .. }
    ));
    assert_eq!(fixture.runtime.signals().len(), 1);

    let journal = fixture
        .backend
        .attempt_idempotency_journal()
        .expect("stop retry journal should open");
    let first_execution = claim_teardown_execution(&journal, &stop);
    let first_claim = first_execution.observation().claim().clone();
    journal
        .record_observation(
            stop.provider_claim(),
            ProviderCommandObservationKind::RetryAuthorized,
            b"retry adopts the exact graceful boundary",
        )
        .expect("the graceful retry should become durable");
    let retry = fixture.command(SandboxExecutionTeardownOperation::Stop, "shared", 2);
    let retry_execution = claim_teardown_execution(&journal, &retry);
    assert!(
        retry_execution
            .observation()
            .authenticates_retry_progress(&first_claim)
    );

    fixture.runtime.set_now(100);
    let retry_observation = fixture
        .backend
        .execute_execution_teardown_with_claim(&retry, retry_execution)
        .expect("adjacent retry should adopt the graceful boundary");
    assert_eq!(
        retry_observation.kind(),
        ProviderCommandObservationKind::InProgress
    );
    assert!(matches!(
        fixture.manifest().execution_teardown.stop(),
        KrunStopProgress::GracefulSignalMayExist { fence, .. }
            if fence == retry.provider_claim()
    ));
    assert_eq!(
        fixture.runtime.signals().len(),
        1,
        "adopting an adjacent graceful retry must not redeliver the signal"
    );

    fixture.runtime.set_now(1_000);
    let retry_durable = journal
        .adopt_exact_attempt(retry.provider_claim())
        .expect("graceful retry observation should read")
        .expect("graceful retry observation should exist");
    let retry_inspection = fixture
        .backend
        .inspect_execution_teardown_with_observation(&retry, &retry_durable);
    assert!(matches!(
        retry_inspection,
        SandboxExecutionTeardownObservation::RetryAuthorized { .. }
    ));
    persist_observation(&journal, &retry, &retry_inspection);

    let successor = fixture.command(SandboxExecutionTeardownOperation::Stop, "shared", 3);
    let successor_execution = claim_teardown_execution(&journal, &successor);
    let successor_observation = fixture
        .backend
        .execute_execution_teardown_with_claim(&successor, successor_execution)
        .expect("the successor of the rebased graceful retry should execute");
    assert_eq!(
        successor_observation.kind(),
        ProviderCommandObservationKind::InProgress
    );
    assert!(matches!(
        fixture.manifest().execution_teardown.stop(),
        KrunStopProgress::KillMayExist { fence, .. }
            if fence == successor.provider_claim()
    ));
    assert_eq!(
        fixture
            .runtime
            .signals()
            .iter()
            .map(|(_, signal)| *signal)
            .collect::<Vec<_>>(),
        vec![libc::SIGTERM, libc::SIGKILL]
    );
}

#[test]
fn delayed_krun_stop_claim_fails_before_manifest_or_effect_after_epoch_advances() {
    let fixture = TeardownFixture::new("delayed-stop-claim");
    fixture.drain("shared", 1);
    let journal = fixture
        .backend
        .attempt_idempotency_journal()
        .expect("delayed claim journal should open");
    let first = fixture.command(SandboxExecutionTeardownOperation::Stop, "shared", 1);
    let delayed = claim_teardown_execution(&journal, &first);
    persist_observation(
        &journal,
        &first,
        &SandboxExecutionTeardownObservation::RetryAuthorized {
            evidence: b"exact stop retry authorized".to_vec(),
        },
    );
    let next = fixture.command(SandboxExecutionTeardownOperation::Stop, "shared", 2);
    let _next = claim_teardown_execution(&journal, &next);
    let before = snapshot_files(fixture.root.path());

    assert!(
        fixture
            .backend
            .execute_execution_teardown_with_claim(&first, delayed)
            .is_err()
    );
    assert_eq!(snapshot_files(fixture.root.path()), before);
    assert!(fixture.runtime.signals().is_empty());
}

#[test]
fn krun_live_claim_publishes_result_before_releasing_provider_journal_lock() {
    let fixture = TeardownFixture::new("journal-publication");
    fixture.drain("shared", 1);
    let command = fixture.command(SandboxExecutionTeardownOperation::Stop, "shared", 1);
    let journal = fixture
        .backend
        .attempt_idempotency_journal()
        .expect("provider journal should open");
    let execution = claim_teardown_execution(&journal, &command);
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    let blocking_runtime = Arc::new(BlockingTerminalRuntime {
        inner: fixture.runtime.as_ref().clone(),
        entered: entered_tx,
        release: Mutex::new(release_rx),
    });
    let backend = fixture
        .backend
        .clone()
        .with_teardown_runtime_provider(blocking_runtime);
    let worker_command = command.clone();
    let worker = thread::spawn(move || {
        backend.execute_execution_teardown_with_claim(&worker_command, execution)
    });
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("live provider effect should reach its durable boundary");

    let mut unrelated_input = claim_input_from(
        command.provider_claim(),
        ProviderCommandOperation::StopExecution,
    );
    unrelated_input.authority_id = "unrelated-stream-authority".to_owned();
    unrelated_input.effect_subject = "{\"sandbox\":\"unrelated-stream\"}".to_owned();
    unrelated_input.attempt_id = "unrelated-stream-attempt".to_owned();
    let unrelated = ProviderCommandClaim::new(unrelated_input)
        .expect("unrelated provider stream should validate");
    assert!(matches!(
        journal
            .claim_dispatch_epoch(&unrelated)
            .expect("unrelated provider stream should progress while the first effect is live"),
        ProviderCommandClaimDecision::ExecuteClaimed(_)
    ));
    journal
        .record_observation(
            &unrelated,
            ProviderCommandObservationKind::Succeeded,
            b"unrelated stream completed",
        )
        .expect("unrelated stream result should publish independently");

    release_tx
        .send(())
        .expect("blocked provider effect should be released");
    assert!(matches!(
        worker
            .join()
            .expect("provider publication worker should join"),
        Ok(observation) if observation.kind() == ProviderCommandObservationKind::InProgress
    ));
    assert!(matches!(
        journal
            .claim_dispatch_epoch(command.provider_claim())
            .expect("completed result should reopen"),
        ProviderCommandClaimDecision::AdoptExactAttempt(observation)
            if observation.kind() == ProviderCommandObservationKind::InProgress
    ));
}

#[test]
fn krun_execution_teardown_retains_populated_network_authority_byte_stable() {
    let fixture = TeardownFixture::new("network-stable");
    let populated = fixture.manifest();
    assert!(populated.provision_network_plan.is_some());
    assert!(populated.network_config.is_some());
    assert!(!populated.port_leases.is_empty());
    assert!(populated.egress_proxy.is_some());
    assert!(populated.launch_artifact.is_some());
    let network_before = fixture.network_authority();
    fixture.drain("shared", 1);
    let stop = fixture.command(SandboxExecutionTeardownOperation::Stop, "shared", 1);
    assert!(matches!(
        fixture.backend.execute_execution_teardown(&stop),
        SandboxExecutionTeardownObservation::InProgress { .. }
    ));
    assert!(matches!(
        fixture.backend.inspect_execution_teardown(&stop),
        SandboxExecutionTeardownObservation::InProgress { .. }
    ));
    assert_eq!(fixture.network_authority(), network_before);
    let manifest = fixture.manifest();
    assert_eq!(manifest.status, SandboxStatus::Stopping);
    assert_eq!(manifest.handle.status, SandboxStatus::Stopping);
    assert!(matches!(
        manifest.launch_authority,
        KrunLaunchAuthority::ProviderOwned
    ));
}

#[test]
fn two_krun_drain_contenders_publish_one_barrier() {
    let fixture = TeardownFixture::new("thread-drain");
    let command = fixture.command(SandboxExecutionTeardownOperation::Drain, "drain", 1);
    let gate = Arc::new(Barrier::new(2));
    let mut workers = Vec::new();
    for _ in 0..2 {
        let backend = fixture.backend.clone();
        let command = command.clone();
        let gate = gate.clone();
        workers.push(thread::spawn(move || {
            gate.wait();
            let journal = backend
                .attempt_idempotency_journal()
                .expect("thread contender journal should open");
            match journal
                .claim_dispatch_epoch(command.provider_claim())
                .expect("thread contender should claim or adopt")
            {
                ProviderCommandClaimDecision::ExecuteClaimed(execution) => {
                    backend
                        .execute_execution_teardown_with_claim(&command, execution)
                        .expect("winning drain contender should publish");
                    true
                }
                ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
                    wait_for_contender_result(
                        &journal,
                        command.provider_claim(),
                        ProviderCommandObservationKind::Succeeded,
                    );
                    false
                }
            }
        }));
    }
    let executed = workers
        .into_iter()
        .map(|worker| worker.join().expect("drain contender should join"))
        .filter(|executed| *executed)
        .count();
    assert_eq!(executed, 1);
    assert!(matches!(
        fixture.manifest().execution_teardown.drain(),
        KrunDrainProgress::Drained { .. }
    ));
}

#[test]
fn two_krun_stop_contenders_dispatch_one_signal_for_one_epoch() {
    let fixture = TeardownFixture::new("thread-stop");
    fixture.drain("shared", 1);
    let command = fixture.command(SandboxExecutionTeardownOperation::Stop, "shared", 1);
    let gate = Arc::new(Barrier::new(2));
    let mut workers = Vec::new();
    for _ in 0..2 {
        let backend = fixture.backend.clone();
        let command = command.clone();
        let gate = gate.clone();
        workers.push(thread::spawn(move || {
            gate.wait();
            let journal = backend
                .attempt_idempotency_journal()
                .expect("thread contender journal should open");
            match journal
                .claim_dispatch_epoch(command.provider_claim())
                .expect("thread contender should claim or adopt")
            {
                ProviderCommandClaimDecision::ExecuteClaimed(execution) => {
                    backend
                        .execute_execution_teardown_with_claim(&command, execution)
                        .expect("winning stop contender should publish");
                    true
                }
                ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
                    wait_for_contender_result(
                        &journal,
                        command.provider_claim(),
                        ProviderCommandObservationKind::InProgress,
                    );
                    false
                }
            }
        }));
    }
    let executed = workers
        .into_iter()
        .map(|worker| worker.join().expect("stop contender should join"))
        .filter(|executed| *executed)
        .count();
    assert_eq!(executed, 1);
    assert_eq!(fixture.runtime.signals().len(), 1);
}

fn wait_for_contender_result(
    journal: &ProviderCommandAttemptJournal,
    claim: &ProviderCommandClaim,
    expected: ProviderCommandObservationKind,
) {
    let started = Instant::now();
    loop {
        let observation = journal
            .adopt_exact_attempt(claim)
            .expect("contender result should remain readable")
            .expect("contender result should remain present");
        if observation.kind() == expected {
            return;
        }
        assert_eq!(
            observation.kind(),
            ProviderCommandObservationKind::Claimed,
            "winning contender published an unexpected result"
        );
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "winning contender did not publish before timeout"
        );
        thread::yield_now();
    }
}

#[test]
fn manifest_deserialization_requires_explicit_execution_teardown() {
    let fixture = TeardownFixture::new("schema-outer");
    let mut encoded = serde_json::to_value(fixture.manifest())
        .expect("manifest should serialize for schema test");
    encoded
        .as_object_mut()
        .expect("manifest should be an object")
        .remove("execution_teardown");
    assert!(serde_json::from_value::<KrunSandboxManifest>(encoded).is_err());
}

#[test]
fn manifest_deserialization_requires_explicit_execution_drain() {
    let fixture = TeardownFixture::new("schema-drain");
    let mut encoded = serde_json::to_value(fixture.manifest())
        .expect("manifest should serialize for schema test");
    encoded["execution_teardown"]
        .as_object_mut()
        .expect("execution teardown should be an object")
        .remove("drain");
    assert!(serde_json::from_value::<KrunSandboxManifest>(encoded).is_err());
}

#[test]
fn manifest_deserialization_requires_explicit_execution_stop() {
    let fixture = TeardownFixture::new("schema-stop");
    let mut encoded = serde_json::to_value(fixture.manifest())
        .expect("manifest should serialize for schema test");
    encoded["execution_teardown"]
        .as_object_mut()
        .expect("execution teardown should be an object")
        .remove("stop");
    assert!(serde_json::from_value::<KrunSandboxManifest>(encoded).is_err());
}

#[test]
fn manifest_deserialization_rejects_unknown_execution_teardown_phase() {
    let fixture = TeardownFixture::new("schema-phase");
    let mut encoded = serde_json::to_value(fixture.manifest())
        .expect("manifest should serialize for schema test");
    encoded["execution_teardown"]["drain"]["phase"] =
        serde_json::Value::String("future_phase".to_owned());
    assert!(serde_json::from_value::<KrunSandboxManifest>(encoded).is_err());
}
