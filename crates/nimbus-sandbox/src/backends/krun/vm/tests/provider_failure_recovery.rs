//! Provider-launch failure recovery and durable cleanup-resumption proofs.

use super::support::*;

use std::sync::Arc;

use crate::backends::oci::network::{
    OciSegmentAllocator, RecordingSegmentAllocator, default_network_attachment_id,
};
use crate::error::SandboxError;

fn adopt_launch_attachment(backend: &KrunSandboxBackend, manifest: &mut KrunSandboxManifest) {
    let claim = manifest
        .require_reserved_claim()
        .expect("launch fixture should retain its reservation coordinator")
        .clone();
    backend
        .segment_allocator
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            &claim,
        )
        .expect("fixture should adopt the exact launch attachment");
    manifest.launch_authority = KrunLaunchAuthority::Adopted {
        reservation_claim: claim,
    };
}

fn explicit_runtime_absence(manifest: &mut KrunSandboxManifest) {
    manifest.conmon_launch.delete_command = CommandSpec::new("/usr/bin/true");
    manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "printf '%s\\n' 'container `{0}` does not exist: open \
             `/run/crun/{0}/status`: No such file or directory' >&2; exit 1",
            manifest.handle.id
        ),
    ]);
}

fn persisted_not_spawned_provider_fixture(
    temp_dir: &TempDir,
    sandbox_id: &str,
) -> (KrunSandboxBackendConfig, KrunSandboxManifest) {
    let mut config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    config.netavark_path = PathBuf::from("/usr/bin/true");
    let backend = KrunSandboxBackend::new(config.clone());
    let mut manifest = backend
        .plan_start_with_id(&sample_spec(), &SandboxId::new(sandbox_id), None, None)
        .expect("execute planning should reserve exact launch authority")
        .manifest;
    adopt_launch_attachment(&backend, &mut manifest);
    backend
        .write_manifest(&manifest)
        .expect("adopted no-spawn fixture should persist");
    (config, manifest)
}

fn assert_failed_terminal(backend: &KrunSandboxBackend, sandbox_id: &SandboxId) {
    let terminal = backend
        .read_manifest(sandbox_id)
        .expect("terminal manifest should inspect")
        .expect("terminal manifest should remain durable");
    assert_eq!(
        (
            terminal.provider_failure_cleanup,
            terminal.launch_authority,
            terminal.status,
        ),
        (
            KrunProviderFailureCleanupState::Inactive,
            KrunLaunchAuthority::Released,
            SandboxStatus::Failed,
        ),
        "provider-failure cleanup must publish Failed only with fully released authority"
    );
}

#[test]
fn not_spawned_provider_failure_skips_runtime_cli_and_releases_exact_authority() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    config.netavark_path = PathBuf::from("/usr/bin/true");
    let backend = KrunSandboxBackend::new(config);
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("krun-never-spawned-cleanup"),
            None,
            None,
        )
        .expect("execute planning should reserve exact launch authority")
        .manifest;
    adopt_launch_attachment(&backend, &mut manifest);
    assert_eq!(
        manifest.creator_handoff,
        KrunCreatorHandoffState::NotSpawned,
        "planning must durably prove that no creator effect began"
    );
    let delete_sentinel = temp_dir.path().join("unexpected-delete");
    let state_sentinel = temp_dir.path().join("unexpected-state");
    manifest.conmon_launch.delete_command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!("printf delete > '{}'; exit 1", delete_sentinel.display()),
    ]);
    manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!("printf state > '{}'; exit 1", state_sentinel.display()),
    ]);
    backend
        .write_manifest(&manifest)
        .expect("adopted no-spawn checkpoint should persist");

    let error = backend.persist_provider_launch_failure(
        &mut manifest,
        SandboxError::OperationFailed {
            message: "injected pre-creator provider failure".to_owned(),
        },
    );
    assert!(
        error
            .to_string()
            .contains("injected pre-creator provider failure"),
        "the original provider failure must remain primary: {error}"
    );
    assert!(
        !delete_sentinel.exists() && !state_sentinel.exists(),
        "exact NotSpawned authority must bypass delete and runtime-state provider effects"
    );
    let terminal = backend
        .read_manifest(&manifest.handle.id)
        .expect("terminal manifest should inspect")
        .expect("terminal manifest should remain durable");
    assert_eq!(
        (terminal.launch_authority, terminal.status),
        (KrunLaunchAuthority::Released, SandboxStatus::Failed),
        "no-spawn compensation must release exact authority before publishing Failed"
    );
}

#[test]
fn provider_failure_network_error_retry_stays_on_cleanup_coordinator_and_converges() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    config.netavark_path = PathBuf::from("/usr/bin/true");
    let recorder = Arc::new(
        RecordingSegmentAllocator::new(sample_spec().tenant_id, "10.97.0.0/24", 97)
            .with_finalize_release_failure("injected provider-failure finalization cut"),
    );
    let injected: Arc<OciSegmentAllocator> = recorder.clone();
    let backend = KrunSandboxBackend::with_segment_allocator(config, injected);
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("krun-provider-failure-resume"),
            None,
            None,
        )
        .expect("execute planning should reserve exact launch authority")
        .manifest;
    adopt_launch_attachment(&backend, &mut manifest);
    explicit_runtime_absence(&mut manifest);
    backend
        .write_manifest(&manifest)
        .expect("provider-failure fixture should persist");

    let error = backend.persist_provider_launch_failure(
        &mut manifest,
        SandboxError::OperationFailed {
            message: "injected provider launch failure".to_owned(),
        },
    );
    assert!(
        error
            .to_string()
            .contains("injected provider-failure finalization cut"),
        "the exact network convergence fault must remain observable: {error}"
    );
    let checkpoint = backend
        .read_manifest(&manifest.handle.id)
        .expect("cleanup checkpoint should inspect")
        .expect("cleanup checkpoint should remain durable");
    assert_eq!(
        checkpoint.status,
        SandboxStatus::Stopping,
        "failed provider cleanup must remain nonterminal and resumable"
    );

    recorder.clear_finalize_release_failure();
    backend
        .stop_sync(&manifest.handle.id)
        .expect("stop must resume provider-failure cleanup without requiring a PID");
    let terminal = backend
        .read_manifest(&manifest.handle.id)
        .expect("terminal manifest should inspect")
        .expect("terminal manifest should remain durable");
    assert_eq!(
        (terminal.launch_authority, terminal.status),
        (KrunLaunchAuthority::Released, SandboxStatus::Failed),
        "recovery must preserve the original failed-launch outcome"
    );
    backend
        .stop_sync(&manifest.handle.id)
        .expect("completed provider-failure cleanup should replay idempotently");
    let replay = backend
        .read_manifest(&manifest.handle.id)
        .expect("replayed manifest should inspect")
        .expect("replayed manifest should remain durable");
    assert_eq!(
        replay.status,
        SandboxStatus::Failed,
        "an idempotent stop must not rewrite a failed launch as a successful stop"
    );
}

#[test]
fn provider_failure_runtime_absence_checkpoint_replays_delete_and_inspect() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (config, mut manifest) =
        persisted_not_spawned_provider_fixture(&temp_dir, "krun-runtime-checkpoint-replay");
    manifest.creator_handoff = KrunCreatorHandoffState::Quiesced {
        proof: crate::backends::conmon::creator::CreatorQuiescenceProof::dead_contained(
            crate::backends::conmon::creator::CreatorAttemptReceipt::for_test(
                "runtime-checkpoint-attempt",
            ),
        ),
    };
    let delete_calls = temp_dir.path().join("runtime-delete-calls");
    let state_calls = temp_dir.path().join("runtime-state-calls");
    manifest.conmon_launch.delete_command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!("printf d >> '{}'", delete_calls.display()),
    ]);
    manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "printf s >> '{state_calls}'; printf '%s\\n' 'container `{sandbox_id}` does not exist: \
             open `/run/crun/{sandbox_id}/status`: No such file or directory' >&2; exit 1",
            state_calls = state_calls.display(),
            sandbox_id = manifest.handle.id,
        ),
    ]);
    let failing = KrunSandboxBackend::new(config.clone()).with_effect_barrier_test_probe(
        KrunEffectBarrierTestProbe::once(
            "provider-failure runtime-absence checkpoint",
            KrunEffectBarrierFailureStage::BeforeWrite,
        ),
    );
    failing
        .write_manifest(&manifest)
        .expect("quiesced creator fixture should persist");

    let error = failing.persist_provider_launch_failure(
        &mut manifest,
        SandboxError::OperationFailed {
            message: "injected provider launch failure".to_owned(),
        },
    );
    assert!(
        error
            .to_string()
            .contains("provider-failure runtime-absence checkpoint"),
        "the crash cut must occur after delete-and-inspect and before its checkpoint: {error}"
    );
    let checkpoint = failing
        .read_manifest(&manifest.handle.id)
        .expect("cleanup-intent checkpoint should inspect")
        .expect("cleanup-intent checkpoint should remain durable");
    assert_eq!(
        checkpoint.provider_failure_cleanup,
        KrunProviderFailureCleanupState::Requested,
        "unacknowledged absence proof must not authorize later cleanup effects"
    );

    let reopened = KrunSandboxBackend::new(config);
    reopened
        .stop_sync(&manifest.handle.id)
        .expect("absence-proof replay must re-delete and re-inspect without requiring a PID");
    assert_failed_terminal(&reopened, &manifest.handle.id);
    assert_eq!(
        std::fs::read_to_string(&delete_calls).expect("delete-call proof should remain"),
        "dd",
        "the ambiguous delete effect must replay exactly once after restart"
    );
    assert_eq!(
        std::fs::read_to_string(&state_calls).expect("state-call proof should remain"),
        "ss",
        "each delete attempt must be followed by an authenticated absence observation"
    );
}

#[test]
fn provider_failure_network_release_checkpoint_replays_without_pid() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (config, mut manifest) =
        persisted_not_spawned_provider_fixture(&temp_dir, "krun-network-checkpoint-replay");
    let failing = KrunSandboxBackend::new(config.clone()).with_effect_barrier_test_probe(
        KrunEffectBarrierTestProbe::once(
            "provider-failure network-release checkpoint",
            KrunEffectBarrierFailureStage::BeforeWrite,
        ),
    );

    let error = failing.persist_provider_launch_failure(
        &mut manifest,
        SandboxError::OperationFailed {
            message: "injected provider launch failure".to_owned(),
        },
    );
    assert!(
        error
            .to_string()
            .contains("provider-failure network-release checkpoint"),
        "the crash cut must occur after network release and before its checkpoint: {error}"
    );
    let checkpoint = failing
        .read_manifest(&manifest.handle.id)
        .expect("runtime-absence checkpoint should inspect")
        .expect("runtime-absence checkpoint should remain durable");
    assert_eq!(
        checkpoint.provider_failure_cleanup,
        KrunProviderFailureCleanupState::RuntimeAbsent {
            proof: KrunRuntimeAbsenceProof::NeverSpawned,
        },
        "failed network checkpoint publication must retain the preceding durable proof"
    );

    let reopened = KrunSandboxBackend::new(config);
    reopened
        .stop_sync(&manifest.handle.id)
        .expect("network release replay must not require a runtime PID");
    assert_failed_terminal(&reopened, &manifest.handle.id);
}

#[test]
fn provider_failure_artifact_release_checkpoint_replays_without_pid() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (config, mut manifest) =
        persisted_not_spawned_provider_fixture(&temp_dir, "krun-artifact-checkpoint-replay");
    let failing = KrunSandboxBackend::new(config.clone()).with_effect_barrier_test_probe(
        KrunEffectBarrierTestProbe::once(
            "provider-failure artifact-release checkpoint",
            KrunEffectBarrierFailureStage::BeforeWrite,
        ),
    );

    let error = failing.persist_provider_launch_failure(
        &mut manifest,
        SandboxError::OperationFailed {
            message: "injected provider launch failure".to_owned(),
        },
    );
    assert!(
        error
            .to_string()
            .contains("provider-failure artifact-release checkpoint"),
        "the crash cut must occur after artifact release and before its checkpoint: {error}"
    );
    let checkpoint = failing
        .read_manifest(&manifest.handle.id)
        .expect("network-release checkpoint should inspect")
        .expect("network-release checkpoint should remain durable");
    assert_eq!(
        checkpoint.provider_failure_cleanup,
        KrunProviderFailureCleanupState::NetworkReleased {
            runtime_absence: KrunRuntimeAbsenceProof::NeverSpawned,
        },
        "failed artifact checkpoint publication must retain the network-release checkpoint"
    );

    let reopened = KrunSandboxBackend::new(config);
    reopened
        .stop_sync(&manifest.handle.id)
        .expect("artifact release replay must not require a runtime PID");
    assert_failed_terminal(&reopened, &manifest.handle.id);
}

#[test]
fn provider_failure_terminal_publication_checkpoint_replays_without_pid() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (config, mut manifest) =
        persisted_not_spawned_provider_fixture(&temp_dir, "krun-terminal-checkpoint-replay");
    let failing = KrunSandboxBackend::new(config.clone()).with_effect_barrier_test_probe(
        KrunEffectBarrierTestProbe::once(
            "provider-owned krun cleanup result",
            KrunEffectBarrierFailureStage::BeforeWrite,
        ),
    );

    let error = failing.persist_provider_launch_failure(
        &mut manifest,
        SandboxError::OperationFailed {
            message: "injected provider launch failure".to_owned(),
        },
    );
    assert!(
        error
            .to_string()
            .contains("provider-owned krun cleanup result"),
        "the crash cut must occur after every cleanup effect and before terminal publication: {error}"
    );
    let checkpoint = failing
        .read_manifest(&manifest.handle.id)
        .expect("artifact-release checkpoint should inspect")
        .expect("artifact-release checkpoint should remain durable");
    assert_eq!(
        checkpoint.provider_failure_cleanup,
        KrunProviderFailureCleanupState::ArtifactsReleased {
            runtime_absence: KrunRuntimeAbsenceProof::NeverSpawned,
        },
        "terminal publication failure must retain the final nonterminal cleanup checkpoint"
    );
    assert_eq!(checkpoint.status, SandboxStatus::Stopping);
    assert_ne!(checkpoint.launch_authority, KrunLaunchAuthority::Released);

    let reopened = KrunSandboxBackend::new(config);
    reopened
        .stop_sync(&manifest.handle.id)
        .expect("terminal publication replay must not require a runtime PID");
    assert_failed_terminal(&reopened, &manifest.handle.id);
    reopened
        .stop_sync(&manifest.handle.id)
        .expect("terminal provider-failure replay should remain idempotent");
    assert_failed_terminal(&reopened, &manifest.handle.id);
}
