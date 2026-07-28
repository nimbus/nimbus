//! Exact container-runner handoff and effect-receipt recovery proofs.

use super::support::*;
use super::unused_loopback_port;

use crate::backends::conmon::creator::CreatorAttemptReceipt;
use crate::backends::oci::command::CommandSpec;
use crate::backends::oci::egress::PepPreAdoptionReleaseAuthority;
use crate::backends::oci::network::default_network_attachment_id;

use tempfile::TempDir;

#[cfg(unix)]
#[path = "runner_recovery/fresh_process.rs"]
mod fresh_process;

fn prepared_runner_fixture(
    root: &std::path::Path,
    id: &str,
) -> (ContainerSandboxBackend, ContainerSandboxManifest) {
    prepared_runner_fixture_with_spec(root, id, &sample_spec())
}

fn prepared_runner_fixture_with_spec(
    root: &std::path::Path,
    id: &str,
    spec: &crate::spec::SandboxSpec,
) -> (ContainerSandboxBackend, ContainerSandboxManifest) {
    let mut config =
        ContainerSandboxBackendConfig::plan_only(root.join("bundles"), root.join("state"));
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    let pep_port = unused_loopback_port();
    config.published_port_range = pep_port..=pep_port;
    config.netavark_path = PathBuf::from("/usr/bin/true");
    let backend = ContainerSandboxBackend::new(config);
    let mut plan = backend
        .plan_start_with_id(spec, &SandboxId::new(id), None, None)
        .expect("runner fixture should plan");
    plan.manifest
        .assign_prepared_service_runner()
        .expect("fixture should assign the prepared runner exactly once");
    backend
        .attach_runner_owned_egress_proxy(&mut plan)
        .expect("runner fixture should reserve exact execution authority");
    backend
        .write_manifest(&plan.manifest)
        .expect("prepared manifest should be durable");
    backend
        .write_runner_manifest_pointer(&plan.manifest)
        .expect("prepared runner pointer should publish after its manifest");
    (backend, plan.manifest)
}

fn exact_present_command(id: &SandboxId, creator_attempt: Option<&str>) -> CommandSpec {
    let payload = creator_attempt.map_or_else(
        || serde_json::json!({"id": id.as_str(), "status": "running"}),
        |attempt| {
            serde_json::json!({
                "id": id.as_str(),
                "status": "running",
                "annotations": {
                    "com.nimbus.creator-attempt": attempt,
                },
            })
        },
    );
    CommandSpec::new("/bin/sh").args(["-c".to_owned(), format!("printf '%s\\n' '{}'", payload)])
}

fn ambiguous_runtime_command() -> CommandSpec {
    CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        "printf '%s\\n' 'provider transport failed' >&2; exit 1".to_owned(),
    ])
}

fn runtime_observed_effect_fixture(
    root: &std::path::Path,
    id: &str,
    spec: &crate::spec::SandboxSpec,
    receipt_attempt: &str,
    observed_attempt: Option<&str>,
    retire_publication_batch: bool,
) -> (
    ContainerSandboxBackend,
    ContainerSandboxManifest,
    super::super::runner::RunnerHandoffGuard,
) {
    let (backend, mut manifest) = prepared_runner_fixture_with_spec(root, id, spec);
    manifest.conmon_launch.state_command =
        exact_present_command(&manifest.handle.id, observed_attempt);
    backend
        .write_manifest(&manifest)
        .expect("present-state command should be durable before the handoff");
    let handoff = super::super::runner::persist_runner_execution_ownership(&backend, &mut manifest)
        .expect("runner should claim execution");
    super::super::runner::mark_runner_effects_started(&manifest, &handoff)
        .expect("effect boundary should become durable");

    let claim = manifest
        .launch_reservation_claim
        .clone()
        .expect("prepared runner should retain its exact launch claim");
    backend
        .segment_allocator
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            &claim,
        )
        .expect("fixture should model exact attachment adoption");
    std::fs::write(&manifest.network_layout.status_path, b"{}\n")
        .expect("exact Netavark status projection should persist");
    std::fs::write(&manifest.network_layout.netns_path, b"fixture-netns\n")
        .expect("persistent namespace projection should exist");
    backend
        .ensure_egress_proxy_running_with_release_authority(
            &manifest,
            PepPreAdoptionReleaseAuthority::FreshLaunch(&claim),
        )
        .expect("fixture should establish the exact PEP effect");
    if retire_publication_batch {
        backend
            .port_manager()
            .release_never_bound_requests(&manifest.port_leases, &claim)
            .expect("fixture should retire the exact never-bound publication batch");
    }
    manifest.creator_handoff = ContainerCreatorHandoffState::RuntimeObserved {
        receipt: CreatorAttemptReceipt::for_test(receipt_attempt),
    };
    manifest.launch_reservation_claim = None;
    backend
        .write_manifest(&manifest)
        .expect("complete post-launch manifest should become durable");
    (backend, manifest, handoff)
}

#[test]
fn effects_started_rejects_substituted_handoff_generation() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut plan = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("runner-substituted-handoff-generation"),
            None,
            None,
        )
        .expect("runner fixture should plan");
    backend
        .attach_runner_owned_egress_proxy(&mut plan)
        .expect("runner fixture should reserve exact execution authority");
    plan.manifest
        .assign_prepared_service_runner()
        .expect("fixture should assign the prepared runner exactly once");
    backend
        .write_manifest(&plan.manifest)
        .expect("prepared manifest should be durable");
    let mut manifest = plan.manifest;
    let handoff = super::super::runner::persist_runner_execution_ownership(&backend, &mut manifest)
        .expect("runner should claim execution");
    super::super::runner::mark_runner_effects_started(&manifest, &handoff)
        .expect("effect boundary should become durable");

    let decision_path = manifest
        .conmon_layout
        .container_state_dir
        .join(super::super::runner::RUNNER_HANDOFF_DECISION_FILE);
    let mut decision: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&decision_path).expect("runner decision bytes should read"),
    )
    .expect("runner decision should parse");
    let original_generation = decision["decision_id"]
        .as_str()
        .expect("runner decision should carry a generation")
        .to_owned();
    let substituted_generation = ulid::Ulid::new().to_string().to_ascii_lowercase();
    assert_ne!(substituted_generation, original_generation);
    decision["decision_id"] = serde_json::Value::String(substituted_generation);
    let mut substituted_bytes =
        serde_json::to_vec_pretty(&decision).expect("substituted decision should serialize");
    substituted_bytes.push(b'\n');
    std::fs::write(&decision_path, &substituted_bytes)
        .expect("substituted decision should become durable");
    let manifest_before = std::fs::read(&manifest.conmon_layout.manifest_path)
        .expect("execute manifest bytes should read");

    let error = super::super::runner::execute_handoff_phase(&manifest)
        .expect_err("an unanchored handoff generation must not authenticate EffectsStarted");
    assert!(
        error.to_string().contains("handoff")
            && error.to_string().contains("generation")
            && error.to_string().contains("fenced"),
        "the rejection should name the substituted authority: {error}"
    );
    assert_eq!(
        std::fs::read(&manifest.conmon_layout.manifest_path)
            .expect("execute manifest bytes should reread"),
        manifest_before,
        "generation rejection must not mutate the canonical manifest"
    );
    assert_eq!(
        std::fs::read(&decision_path).expect("runner decision bytes should reread"),
        substituted_bytes,
        "generation rejection must preserve the substituted evidence for diagnosis"
    );
    drop(handoff);
}

#[test]
fn exact_present_effect_promotes_once_without_replaying_launch() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let creator_attempt = "runner-recovery-present-creator";
    let (backend, mut manifest, handoff) = runtime_observed_effect_fixture(
        temp_dir.path(),
        "runner-recovery-present",
        &sample_spec(),
        creator_attempt,
        Some(creator_attempt),
        false,
    );

    assert_eq!(
        super::super::runner::reconcile_runner_effects_started(&backend, &mut manifest, &handoff,)
            .expect("exact present effects should promote"),
        super::super::runner::RunnerEffectOutcome::Present
    );
    assert_eq!(
        super::super::runner::execute_handoff_phase(&manifest)
            .expect("published lifecycle should authenticate"),
        None,
        "promotion must retire the ambiguous EffectsStarted phase"
    );
    let decision_path = manifest
        .conmon_layout
        .container_state_dir
        .join(super::super::runner::RUNNER_HANDOFF_DECISION_FILE);
    let before = std::fs::read(&decision_path).expect("published decision should read");
    super::super::runner::publish_runner_lifecycle_ownership(&manifest, &handoff)
        .expect("published lifecycle replay should be idempotent");
    assert_eq!(
        std::fs::read(&decision_path).expect("published decision should reread"),
        before,
        "exact present promotion replay must preserve decision bytes"
    );
}

fn assert_runtime_observed_creator_attempt_rejected(
    root: &std::path::Path,
    id: &str,
    observed_attempt: Option<&str>,
) {
    let expected_attempt = "runner-recovery-expected-creator";
    let (backend, mut manifest, handoff) = runtime_observed_effect_fixture(
        root,
        id,
        &sample_spec(),
        expected_attempt,
        observed_attempt,
        false,
    );
    let manifest_before = std::fs::read(&manifest.conmon_layout.manifest_path)
        .expect("execute manifest bytes should read");
    let decision_path = manifest
        .conmon_layout
        .container_state_dir
        .join(super::super::runner::RUNNER_HANDOFF_DECISION_FILE);
    let decision_before = std::fs::read(&decision_path).expect("effect decision should read");

    let error =
        super::super::runner::reconcile_runner_effects_started(&backend, &mut manifest, &handoff)
            .expect_err("unmatched creator attempt must keep runner effects fenced");
    assert!(
        error.to_string().contains("creator attempt")
            && error.to_string().contains(expected_attempt)
            && error.to_string().contains("remain fenced"),
        "rejection must identify the exact missing or substituted creator attempt: {error}"
    );
    assert_eq!(
        std::fs::read(&manifest.conmon_layout.manifest_path)
            .expect("execute manifest bytes should reread"),
        manifest_before
    );
    assert_eq!(
        std::fs::read(&decision_path).expect("effect decision should reread"),
        decision_before
    );
    assert_eq!(
        super::super::runner::execute_handoff_phase(&manifest)
            .expect("retained effect handoff should authenticate"),
        Some(super::super::runner::RunnerHandoffPhase::EffectsStarted)
    );
    assert!(
        !manifest
            .conmon_layout
            .container_state_dir
            .join(super::super::runner::RUNNER_RESULT_ANCHOR_FILE)
            .exists(),
        "failed creator authentication must not publish a result anchor"
    );
}

#[test]
fn runtime_observed_rejects_missing_creator_attempt_annotation() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    assert_runtime_observed_creator_attempt_rejected(
        temp_dir.path(),
        "runner-recovery-missing-creator-attempt",
        None,
    );
}

#[test]
fn runtime_observed_rejects_substituted_creator_attempt() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    assert_runtime_observed_creator_attempt_rejected(
        temp_dir.path(),
        "runner-recovery-substituted-creator-attempt",
        Some("runner-recovery-substituted-creator"),
    );
}

#[test]
fn live_promotion_rejects_nonempty_terminal_no_effect_port_batch() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let creator_attempt = "runner-recovery-terminal-port-creator";
    let spec = sample_spec().with_port_binding(SandboxPortBinding::tcp(
        "terminal-no-effect",
        unused_loopback_port(),
        8080,
    ));
    let (backend, mut manifest, handoff) = runtime_observed_effect_fixture(
        temp_dir.path(),
        "runner-recovery-terminal-port-batch",
        &spec,
        creator_attempt,
        Some(creator_attempt),
        true,
    );
    let manifest_before = std::fs::read(&manifest.conmon_layout.manifest_path)
        .expect("execute manifest bytes should read");
    let decision_path = manifest
        .conmon_layout
        .container_state_dir
        .join(super::super::runner::RUNNER_HANDOFF_DECISION_FILE);
    let decision_before = std::fs::read(&decision_path).expect("effect decision should read");

    let error =
        super::super::runner::reconcile_runner_effects_started(&backend, &mut manifest, &handoff)
            .expect_err("terminal publication authority cannot authenticate a live runtime");
    assert!(
        error.to_string().contains("TerminalNoEffect")
            && error
                .to_string()
                .contains("cannot promote live runtime effects"),
        "rejection must identify the terminal publication batch: {error}"
    );
    assert_eq!(
        std::fs::read(&manifest.conmon_layout.manifest_path)
            .expect("execute manifest bytes should reread"),
        manifest_before
    );
    assert_eq!(
        std::fs::read(&decision_path).expect("effect decision should reread"),
        decision_before
    );
    assert_eq!(
        super::super::runner::execute_handoff_phase(&manifest)
            .expect("retained effect handoff should authenticate"),
        Some(super::super::runner::RunnerHandoffPhase::EffectsStarted)
    );
}

fn published_present_effect_fixture(
    root: &std::path::Path,
    id: &str,
) -> (
    ContainerSandboxBackend,
    ContainerSandboxManifest,
    super::super::runner::RunnerHandoffGuard,
) {
    let creator_attempt = "runner-recovery-published-result-creator";
    let (backend, mut manifest, handoff) = runtime_observed_effect_fixture(
        root,
        id,
        &sample_spec(),
        creator_attempt,
        Some(creator_attempt),
        false,
    );
    assert_eq!(
        super::super::runner::reconcile_runner_effects_started(&backend, &mut manifest, &handoff,)
            .expect("exact present effects should publish"),
        super::super::runner::RunnerEffectOutcome::Present
    );
    (backend, manifest, handoff)
}

#[test]
fn lifecycle_published_rejects_substituted_result_digest() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (_backend, manifest, _handoff) =
        published_present_effect_fixture(temp_dir.path(), "runner-result-substituted-digest");
    let decision_path = manifest
        .conmon_layout
        .container_state_dir
        .join(super::super::runner::RUNNER_HANDOFF_DECISION_FILE);
    let mut decision: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&decision_path).expect("published decision should read"),
    )
    .expect("published decision should parse");
    let original = decision["effect_receipt"]["result_manifest_sha256"]
        .as_str()
        .expect("published result digest should exist");
    let substituted = if original.bytes().all(|byte| byte == b'0') {
        "1".repeat(64)
    } else {
        "0".repeat(64)
    };
    decision["effect_receipt"]["result_manifest_sha256"] = serde_json::Value::String(substituted);
    std::fs::write(
        &decision_path,
        serde_json::to_vec_pretty(&decision).expect("substituted decision should serialize"),
    )
    .expect("substituted decision should persist");

    let error = super::super::runner::execute_handoff_phase(&manifest)
        .expect_err("a substituted result digest must not authenticate LifecyclePublished");
    assert!(
        error.to_string().contains("result generation")
            && error.to_string().contains("remains fenced"),
        "rejection must identify the unanchored result generation: {error}"
    );
}

#[test]
fn lifecycle_published_rejects_substituted_effect_outcome() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (_backend, manifest, _handoff) =
        published_present_effect_fixture(temp_dir.path(), "runner-result-substituted-outcome");
    let decision_path = manifest
        .conmon_layout
        .container_state_dir
        .join(super::super::runner::RUNNER_HANDOFF_DECISION_FILE);
    let mut decision: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&decision_path).expect("published decision should read"),
    )
    .expect("published decision should parse");
    assert_eq!(decision["effect_receipt"]["outcome"], "present");
    decision["effect_receipt"]["outcome"] = serde_json::Value::String("absent".to_owned());
    std::fs::write(
        &decision_path,
        serde_json::to_vec_pretty(&decision).expect("substituted decision should serialize"),
    )
    .expect("substituted decision should persist");

    let error = super::super::runner::execute_handoff_phase(&manifest)
        .expect_err("a substituted result outcome must not authenticate LifecyclePublished");
    assert!(
        error.to_string().contains("result generation")
            && error.to_string().contains("remains fenced"),
        "rejection must identify the unanchored result outcome: {error}"
    );
}

#[test]
fn lifecycle_published_anchor_survives_mutable_lifecycle_evolution() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (backend, mut manifest, _handoff) =
        published_present_effect_fixture(temp_dir.path(), "runner-result-mutable-lifecycle");
    let anchor_path = manifest
        .conmon_layout
        .container_state_dir
        .join(super::super::runner::RUNNER_RESULT_ANCHOR_FILE);
    let anchor_before = std::fs::read(&anchor_path).expect("result anchor should read");
    let decision_path = manifest
        .conmon_layout
        .container_state_dir
        .join(super::super::runner::RUNNER_HANDOFF_DECISION_FILE);
    let decision_before = std::fs::read(&decision_path).expect("published decision should read");

    manifest.restart_count = manifest.restart_count.saturating_add(1);
    manifest.spec.egress = nimbus_egress::EgressPolicy::new([nimbus_egress::EgressRule::new(
        "mutable-desired-policy",
        nimbus_egress::EgressProtocol::Https,
        "example.com",
        443,
    )]);
    backend
        .write_manifest(&manifest)
        .expect("ordinary mutable lifecycle evolution should remain publishable");

    assert_eq!(
        super::super::runner::execute_handoff_phase(&manifest)
            .expect("immutable result anchor should authenticate evolved lifecycle state"),
        None
    );
    assert_eq!(
        std::fs::read(&anchor_path).expect("result anchor should reread"),
        anchor_before,
        "ordinary lifecycle evolution must never rewrite the create-once result anchor"
    );
    assert_eq!(
        std::fs::read(&decision_path).expect("published decision should reread"),
        decision_before,
        "ordinary lifecycle evolution must not rewrite runner result authority"
    );
}

#[test]
fn exact_absence_compensates_and_replays_with_terminal_bytes() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (backend, mut manifest) =
        prepared_runner_fixture(temp_dir.path(), "runner-recovery-absent");
    mark_runtime_absent_for_cleanup(&mut manifest);
    backend
        .write_manifest(&manifest)
        .expect("absence command should be durable before the handoff");
    let handoff = super::super::runner::persist_runner_execution_ownership(&backend, &mut manifest)
        .expect("runner should claim execution");
    super::super::runner::mark_runner_effects_started(&manifest, &handoff)
        .expect("effect boundary should become durable");

    assert_eq!(
        super::super::runner::reconcile_runner_effects_started(&backend, &mut manifest, &handoff,)
            .expect("exact provider absence should compensate"),
        super::super::runner::RunnerEffectOutcome::Absent
    );
    assert!(
        manifest.has_terminal_network_finality(),
        "absence may publish only after every retained launch authority is released"
    );
    let manifest_before = std::fs::read(&manifest.conmon_layout.manifest_path)
        .expect("terminal manifest should read");
    let decision_path = manifest
        .conmon_layout
        .container_state_dir
        .join(super::super::runner::RUNNER_HANDOFF_DECISION_FILE);
    let decision_before = std::fs::read(&decision_path).expect("terminal decision should read");
    drop(handoff);

    backend
        .stop_sync(&manifest.handle.id)
        .expect("terminal recovery replay should be idempotent");
    assert_eq!(
        std::fs::read(&manifest.conmon_layout.manifest_path)
            .expect("terminal manifest should reread"),
        manifest_before
    );
    assert_eq!(
        std::fs::read(&decision_path).expect("terminal decision should reread"),
        decision_before
    );
}

#[test]
fn ambiguous_effect_observation_preserves_exact_handoff_and_manifest() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (backend, mut manifest) =
        prepared_runner_fixture(temp_dir.path(), "runner-recovery-ambiguous");
    manifest.conmon_launch.state_command = ambiguous_runtime_command();
    backend
        .write_manifest(&manifest)
        .expect("ambiguous command should be durable before the handoff");
    let handoff = super::super::runner::persist_runner_execution_ownership(&backend, &mut manifest)
        .expect("runner should claim execution");
    super::super::runner::mark_runner_effects_started(&manifest, &handoff)
        .expect("effect boundary should become durable");
    let manifest_before =
        std::fs::read(&manifest.conmon_layout.manifest_path).expect("execute manifest should read");
    let decision_path = manifest
        .conmon_layout
        .container_state_dir
        .join(super::super::runner::RUNNER_HANDOFF_DECISION_FILE);
    let decision_before = std::fs::read(&decision_path).expect("effect decision should read");

    let error =
        super::super::runner::reconcile_runner_effects_started(&backend, &mut manifest, &handoff)
            .expect_err("generic provider failure must remain ambiguous");
    assert!(
        error
            .to_string()
            .contains("without explicit absence evidence"),
        "ambiguous provider diagnostic should name the missing evidence: {error}"
    );
    assert_eq!(
        std::fs::read(&manifest.conmon_layout.manifest_path)
            .expect("execute manifest should reread"),
        manifest_before
    );
    assert_eq!(
        std::fs::read(&decision_path).expect("effect decision should reread"),
        decision_before
    );
    assert_eq!(
        super::super::runner::execute_handoff_phase(&manifest)
            .expect("retained handoff should authenticate"),
        Some(super::super::runner::RunnerHandoffPhase::EffectsStarted)
    );
}
