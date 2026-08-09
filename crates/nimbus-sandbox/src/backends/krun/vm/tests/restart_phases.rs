//! Exact-attempt proofs for the krun restart provider phases.

use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use crate::backends::conmon::creator::OwnedConmonCreator;
use crate::backends::oci::command::CommandSpec;
use crate::backends::oci::conmon::OciConmonLayout;
use crate::backends::oci::network::{
    AttachmentAttachAuthority, FixedOciEgressPinProvider, OciNetworkLayout,
};
use crate::{SandboxExecutionAttemptId, SandboxRestartAttemptFence};

use super::support::*;

struct RestartFixture {
    backend: KrunSandboxBackend,
    config: KrunSandboxBackendConfig,
    manifest: KrunSandboxManifest,
    runtime_marker: PathBuf,
    delete_log: PathBuf,
}

fn attempt(value: &str) -> SandboxExecutionAttemptId {
    SandboxExecutionAttemptId::new(value).expect("test execution attempt should validate")
}

fn restart_fence() -> SandboxRestartAttemptFence {
    SandboxRestartAttemptFence::new(attempt("wea_source"), attempt("wea_target"), 1)
        .expect("restart fence should validate")
}

fn explicit_runtime_state_command(id: &SandboxId, marker: &Path) -> CommandSpec {
    CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "if [ -f \"$1\" ]; then printf '%s\\n' '{{\"id\":\"{}\",\"status\":\"running\"}}'; else printf '%s\\n' 'container `{}` does not exist: open `/run/crun/{}/status`: No such file or directory' >&2; exit 1; fi",
            id.as_str(),
            id.as_str(),
            id.as_str(),
        ),
        "sh".to_owned(),
        marker.display().to_string(),
    ])
}

fn restart_fixture(root: &Path, id_value: &str, runtime_present: bool) -> RestartFixture {
    let config = KrunSandboxBackendConfig::under_root(root);
    let backend = KrunSandboxBackend::new(config.clone());
    let spec = sample_spec_for_tenant("krun-restart-phases", id_value);
    let id = SandboxId::new(id_value);
    let mut manifest = sample_manifest(spec.clone(), KrunStartMode::Execute);
    manifest.handle.id = id.clone();
    manifest.execution_attempt_id = attempt("wea_source");
    manifest.conmon_layout =
        OciConmonLayout::new_for_tenant(&config.workload_state_root, &spec.tenant_id, &id);
    manifest.network_layout =
        OciNetworkLayout::under_root(&config.workload_state_root, &spec.tenant_id, &id);
    let runtime_marker = root.join(format!("{id_value}-runtime"));
    let delete_log = root.join(format!("{id_value}-delete-log"));
    manifest.conmon_launch.state_command = explicit_runtime_state_command(&id, &runtime_marker);
    manifest.conmon_launch.delete_command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        "rm -f \"$1\"; printf '%s\\n' delete >> \"$2\"".to_owned(),
        "sh".to_owned(),
        runtime_marker.display().to_string(),
        delete_log.display().to_string(),
    ]);
    if runtime_present {
        fs::write(&runtime_marker, b"running\n").expect("runtime marker should persist");
    }

    let mut creator = OwnedConmonCreator::spawn(&CommandSpec::new("/usr/bin/true"))
        .expect("creator fixture should spawn");
    let receipt = creator
        .attempt_receipt(&format!("{id_value}-creator"))
        .expect("creator receipt should capture");
    creator
        .reap_after_runtime_observed(Duration::from_secs(1))
        .expect("creator fixture should quiesce");
    manifest.creator_handoff = KrunCreatorHandoffState::RuntimeObserved { receipt };
    backend
        .write_manifest(&manifest)
        .expect("restart fixture manifest should persist");
    fs::write(
        &manifest.conmon_layout.conmon_pidfile,
        format!("{}\n", i32::MAX),
    )
    .expect("dead conmon receipt should persist");

    RestartFixture {
        backend,
        config,
        manifest,
        runtime_marker,
        delete_log,
    }
}

fn manifest_bytes(fixture: &RestartFixture) -> Vec<u8> {
    fs::read(&fixture.manifest.conmon_layout.manifest_path)
        .expect("restart fixture manifest bytes should read")
}

fn restart_record_path(fixture: &RestartFixture) -> PathBuf {
    fixture
        .manifest
        .conmon_layout
        .container_state_dir
        .join(".nimbus-krun-restart.json")
}

#[test]
fn crossed_source_attempt_has_zero_runtime_or_provider_state_effects() {
    let root = TempDir::new().expect("temporary root should exist");
    let fixture = restart_fixture(root.path(), "krun-crossed-source", true);
    let before = manifest_bytes(&fixture);
    let crossed =
        SandboxRestartAttemptFence::new(attempt("wea_crossed_source"), attempt("wea_target"), 1)
            .expect("crossed restart fence should validate");

    let error = fixture
        .backend
        .quiesce_restart_source(&fixture.manifest.handle.id, &crossed)
        .expect_err("crossed source attempt must fail before effects");

    assert!(error.to_string().contains("crossed execution attempt"));
    assert!(fixture.runtime_marker.is_file());
    assert!(!fixture.delete_log.exists());
    assert!(!restart_record_path(&fixture).exists());
    assert_eq!(manifest_bytes(&fixture), before);
}

#[test]
fn target_switch_before_quiescence_fails_closed_and_preserves_source() {
    let root = TempDir::new().expect("temporary root should exist");
    let fixture = restart_fixture(root.path(), "krun-target-before-quiescence", true);
    let before = manifest_bytes(&fixture);

    let error = fixture
        .backend
        .prepare_restart_target(&fixture.manifest.handle.id, &restart_fence())
        .expect_err("target switch must require durable source quiescence");

    assert!(
        error
            .to_string()
            .contains("requires durable source quiescence")
    );
    assert!(fixture.runtime_marker.is_file());
    assert!(!fixture.delete_log.exists());
    assert_eq!(manifest_bytes(&fixture), before);
    assert_eq!(
        fixture
            .backend
            .read_manifest(&fixture.manifest.handle.id)
            .expect("source manifest should read")
            .expect("source manifest should remain")
            .execution_attempt_id,
        attempt("wea_source")
    );
}

#[test]
fn live_creator_fences_quiescence_before_runtime_delete() {
    let root = TempDir::new().expect("temporary root should exist");
    let mut fixture = restart_fixture(root.path(), "krun-live-creator-restart", true);
    let mut creator = OwnedConmonCreator::spawn(
        &CommandSpec::new("/bin/sh").args(["-c".to_owned(), "exec sleep 60".to_owned()]),
    )
    .expect("live creator should spawn");
    let receipt = creator
        .attempt_receipt("krun-live-restart-creator")
        .expect("live creator receipt should capture");
    fixture.manifest.creator_handoff = KrunCreatorHandoffState::Pending { receipt };
    fixture
        .backend
        .write_manifest(&fixture.manifest)
        .expect("live creator fence should persist");
    let before = manifest_bytes(&fixture);

    let error = fixture
        .backend
        .quiesce_restart_source(&fixture.manifest.handle.id, &restart_fence())
        .expect_err("live creator must fence restart quiescence");

    assert!(error.to_string().contains("remains live"));
    assert!(fixture.runtime_marker.is_file());
    assert!(!fixture.delete_log.exists());
    assert!(!restart_record_path(&fixture).exists());
    assert_eq!(manifest_bytes(&fixture), before);
    creator
        .cancel_containment_and_reap()
        .expect("live creator fixture should clean up");
}

#[test]
fn unknown_creator_identity_fences_quiescence_before_runtime_delete() {
    let root = TempDir::new().expect("temporary root should exist");
    let mut fixture = restart_fixture(root.path(), "krun-unknown-creator-restart", true);
    let mut creator = OwnedConmonCreator::spawn(
        &CommandSpec::new("/bin/sh").args(["-c".to_owned(), "exec sleep 60".to_owned()]),
    )
    .expect("creator should spawn");
    let crossed_receipt = creator
        .attempt_receipt("krun-unknown-restart-creator")
        .expect("creator receipt should capture")
        .with_substituted_birth_for_test();
    fixture.manifest.creator_handoff = KrunCreatorHandoffState::Pending {
        receipt: crossed_receipt,
    };
    fixture
        .backend
        .write_manifest(&fixture.manifest)
        .expect("unknown creator fence should persist");
    let before = manifest_bytes(&fixture);

    let error = fixture
        .backend
        .quiesce_restart_source(&fixture.manifest.handle.id, &restart_fence())
        .expect_err("unknown creator identity must fence restart quiescence");

    assert!(
        error.to_string().contains("cannot be authenticated")
            || error
                .to_string()
                .contains("escaped its authenticated containment"),
        "unknown or escaped exact containment must be explicit: {error}"
    );
    assert!(fixture.runtime_marker.is_file());
    assert!(!fixture.delete_log.exists());
    assert!(!restart_record_path(&fixture).exists());
    assert_eq!(manifest_bytes(&fixture), before);
    creator
        .cancel_containment_and_reap()
        .expect("creator fixture should clean up");
}

#[test]
fn exact_restart_quiescence_and_target_switch_replay_across_fresh_backends() {
    let root = TempDir::new().expect("temporary root should exist");
    let fixture = restart_fixture(root.path(), "krun-restart-fresh-process", true);
    let fence = restart_fence();

    assert!(matches!(
        fixture
            .backend
            .quiesce_restart_source(&fixture.manifest.handle.id, &fence)
            .expect("exact source should quiesce"),
        crate::SandboxProvisionPhaseObservation::Succeeded { .. }
    ));
    assert!(!fixture.runtime_marker.exists());
    assert_eq!(
        fs::read_to_string(&fixture.delete_log)
            .expect("one runtime delete should be observable")
            .lines()
            .count(),
        1
    );

    let fresh = KrunSandboxBackend::new(fixture.config.clone());
    assert!(matches!(
        fresh
            .inspect_restart_source_quiescence(&fixture.manifest.handle.id, &fence)
            .expect("fresh backend should inspect source quiescence"),
        crate::SandboxProvisionPhaseObservation::Succeeded { .. }
    ));
    assert!(matches!(
        fresh
            .quiesce_restart_source(&fixture.manifest.handle.id, &fence)
            .expect("source quiescence replay should be idempotent"),
        crate::SandboxProvisionPhaseObservation::Succeeded { .. }
    ));
    assert_eq!(
        fs::read_to_string(&fixture.delete_log)
            .expect("runtime delete log should remain")
            .lines()
            .count(),
        1,
        "quiescence replay must not repeat the provider delete"
    );

    assert!(matches!(
        fresh
            .prepare_restart_target(&fixture.manifest.handle.id, &fence)
            .expect("quiesced source should switch to exact target"),
        crate::SandboxProvisionPhaseObservation::Succeeded { .. }
    ));
    let switched = fresh
        .read_manifest(&fixture.manifest.handle.id)
        .expect("target manifest should read")
        .expect("target manifest should exist");
    assert_eq!(switched.execution_attempt_id, attempt("wea_target"));
    assert!(matches!(
        switched.launch_authority,
        KrunLaunchAuthority::Adopted { .. }
    ));
    assert_eq!(
        switched.creator_handoff,
        KrunCreatorHandoffState::NotSpawned
    );
    assert!(!switched.conmon_layout.conmon_pidfile.exists());

    let target_manifest_before =
        fs::read(&switched.conmon_layout.manifest_path).expect("target manifest bytes should read");
    let target_record_before =
        fs::read(restart_record_path(&fixture)).expect("target restart record should read");
    let crossed_target =
        SandboxRestartAttemptFence::new(attempt("wea_source"), attempt("wea_crossed_target"), 1)
            .expect("crossed target fence should validate");
    let crossed_error = fresh
        .attach_restart_retained_network(&fixture.manifest.handle.id, &crossed_target)
        .expect_err("retained attachment must authenticate the exact target attempt");
    assert!(
        crossed_error
            .to_string()
            .contains("crossed execution attempt")
    );
    assert_eq!(
        fs::read(&switched.conmon_layout.manifest_path)
            .expect("crossed target manifest should remain readable"),
        target_manifest_before
    );
    assert_eq!(
        fs::read(restart_record_path(&fixture))
            .expect("crossed target restart record should remain readable"),
        target_record_before
    );
    assert!(!switched.network_layout.netns_path.exists());
    assert!(!switched.network_layout.status_path.exists());

    let second_fresh = KrunSandboxBackend::new(fixture.config);
    assert!(matches!(
        second_fresh
            .inspect_restart_target_preparation(&fixture.manifest.handle.id, &fence)
            .expect("fresh backend should inspect exact target preparation"),
        crate::SandboxProvisionPhaseObservation::Succeeded { .. }
    ));
    let target_before =
        fs::read(&switched.conmon_layout.manifest_path).expect("target manifest bytes should read");
    assert!(matches!(
        second_fresh
            .prepare_restart_target(&fixture.manifest.handle.id, &fence)
            .expect("target preparation replay should be idempotent"),
        crate::SandboxProvisionPhaseObservation::Succeeded { .. }
    ));
    assert_eq!(
        fs::read(&switched.conmon_layout.manifest_path)
            .expect("target manifest should remain readable"),
        target_before
    );
}

#[test]
fn fresh_backend_execute_repairs_process_local_retained_network_state() {
    let root = TempDir::new().expect("temporary root should exist");
    let pep_reservation = TcpListener::bind("127.0.0.1:0").expect("PEP tripwire should bind");
    let pep_port = pep_reservation
        .local_addr()
        .expect("PEP tripwire should expose its address")
        .port();
    let mut config = KrunSandboxBackendConfig::under_root(root.path().to_path_buf());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    config.published_port_range = pep_port..=pep_port;
    let backend = KrunSandboxBackend::new(config.clone())
        .with_egress_pin_provider(Arc::new(FixedOciEgressPinProvider::ready()));
    let sandbox_id = SandboxId::new("krun-restart-process-local-network");
    let spec = sample_spec_for_tenant("krun-restart-process-local-network", "api");
    let source_attempt = attempt("wea_process_local_source");
    let network_plan =
        sample_provision_network_plan(&spec, &sandbox_id, "krun-process-local-restart");
    backend
        .reserve_provision_network(
            spec,
            sandbox_id.clone(),
            source_attempt.clone(),
            network_plan,
        )
        .expect("fixture should reserve exact network authority");
    backend
        .prepare_provision_workload(&sandbox_id, &source_attempt)
        .expect("fixture should prepare before private attachment");
    let mut manifest = backend
        .read_manifest(&sandbox_id)
        .expect("prepared manifest should read")
        .expect("prepared manifest should exist");
    let reservation_claim = manifest
        .require_reserved_claim()
        .expect("prepared manifest should retain its reservation")
        .clone();
    manifest
        .mark_adopting()
        .expect("fixture should persist attachment-adoption intent");
    backend
        .persist_effect_barrier(&manifest, "test restart attachment-adoption intent")
        .expect("attachment-adoption intent should persist");
    let network_config = manifest
        .require_network_config()
        .expect("prepared manifest should retain network config")
        .clone();
    backend
        .segment_allocator
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &network_config.attachment_id,
            &reservation_claim,
        )
        .expect("fixture should adopt the exact attachment");
    manifest
        .mark_adopted()
        .expect("fixture should retain adopted authority");
    backend
        .persist_effect_barrier(&manifest, "test restart adopted attachment")
        .expect("adopted attachment should persist");
    {
        let ports = backend.port_lease_coordinator();
        let hostname = super::super::start::hostname_for(&manifest.spec);
        backend
            .non_routable_attachment_adapter(&manifest, &network_config, &hostname)
            .attach_with_test_host(
                &backend.attachment_lifecycle(&ports),
                AttachmentAttachAuthority::FreshLaunch(&reservation_claim),
                |_| {
                    backend.egress_pin_provider.apply(
                        &manifest.network_layout,
                        manifest
                            .egress_proxy
                            .as_ref()
                            .expect("PEP assignment should persist"),
                    )
                },
            )
            .expect("fixture should realize the private attachment");
    }
    drop(pep_reservation);
    backend
        .start_planned_provision_pep(&manifest, &reservation_claim)
        .expect("fixture should start its process-local PEP");

    let mut creator = OwnedConmonCreator::spawn(&CommandSpec::new("/usr/bin/true"))
        .expect("creator fixture should spawn");
    let receipt = creator
        .attempt_receipt("krun-process-local-restart-creator")
        .expect("creator receipt should capture");
    creator
        .reap_after_runtime_observed(Duration::from_secs(1))
        .expect("creator fixture should quiesce");
    manifest.creator_handoff = KrunCreatorHandoffState::RuntimeObserved { receipt };
    manifest.launch_authority = KrunLaunchAuthority::ProviderOwned;
    manifest.conmon_launch.state_command =
        explicit_runtime_state_command(&sandbox_id, &root.path().join("absent-runtime-marker"));
    fs::write(
        &manifest.conmon_layout.conmon_pidfile,
        format!("{}\n", i32::MAX),
    )
    .expect("dead conmon receipt should persist");
    backend
        .write_manifest(&manifest)
        .expect("provider-owned source fixture should persist");

    let fence =
        SandboxRestartAttemptFence::new(source_attempt, attempt("wea_process_local_target"), 1)
            .expect("restart fence should validate");
    assert!(matches!(
        backend
            .quiesce_restart_source(&sandbox_id, &fence)
            .expect("source should quiesce"),
        crate::SandboxProvisionPhaseObservation::Succeeded { .. }
    ));
    assert!(matches!(
        backend
            .prepare_restart_target(&sandbox_id, &fence)
            .expect("target should prepare"),
        crate::SandboxProvisionPhaseObservation::Succeeded { .. }
    ));
    assert!(matches!(
        backend
            .attach_restart_retained_network_with_test_host(&sandbox_id, &fence)
            .expect("first process should attach retained network state"),
        crate::SandboxProvisionPhaseObservation::Succeeded { .. }
    ));
    let before = backend
        .read_manifest(&sandbox_id)
        .expect("attached manifest should read")
        .expect("attached manifest should exist");
    let segments_before = backend
        .segment_allocator
        .inspect_segments(&before.spec.tenant_id)
        .expect("segment authority should inspect");
    let pep_lease_before = before
        .egress_proxy
        .as_ref()
        .expect("PEP assignment should remain")
        .port_lease
        .lease_id()
        .clone();
    drop(backend);

    let fresh = KrunSandboxBackend::new(config)
        .with_egress_pin_provider(Arc::new(FixedOciEgressPinProvider::ready()));
    assert!(matches!(
        fresh
            .inspect_restart_retained_network(&sandbox_id, &fence)
            .expect("inspection should report missing process-local PEP state"),
        crate::SandboxProvisionPhaseObservation::InProgress { .. }
    ));
    assert!(matches!(
        fresh
            .attach_restart_retained_network_with_test_host(&sandbox_id, &fence)
            .expect("execute should repair retained process-local state"),
        crate::SandboxProvisionPhaseObservation::Succeeded { .. }
    ));
    let repaired = fresh
        .read_manifest(&sandbox_id)
        .expect("repaired manifest should read")
        .expect("repaired manifest should exist");
    assert_eq!(repaired.network_config, before.network_config);
    assert_eq!(
        repaired
            .egress_proxy
            .as_ref()
            .expect("repaired PEP assignment should remain")
            .port_lease
            .lease_id(),
        &pep_lease_before,
        "repair must retain the stable PEP lease identity"
    );
    assert_eq!(
        fresh
            .segment_allocator
            .inspect_segments(&repaired.spec.tenant_id)
            .expect("segments should reinspect"),
        segments_before,
        "repair must not release or reallocate the retained segment"
    );
}

#[test]
fn natural_exit_quiescence_consumes_no_runtime_delete() {
    let root = TempDir::new().expect("temporary root should exist");
    let fixture = restart_fixture(root.path(), "krun-natural-exit-restart", false);
    fs::create_dir_all(&fixture.manifest.conmon_layout.exit_dir)
        .expect("natural-exit receipt directory should exist");
    fs::write(&fixture.manifest.conmon_layout.exit_status_file, b"17\n")
        .expect("natural-exit receipt should persist");

    assert!(matches!(
        fixture
            .backend
            .quiesce_restart_source(&fixture.manifest.handle.id, &restart_fence())
            .expect("natural exit should quiesce without a delete effect"),
        crate::SandboxProvisionPhaseObservation::Succeeded { .. }
    ));
    assert!(!fixture.delete_log.exists());
    assert!(
        fixture.manifest.conmon_layout.exit_status_file.is_file(),
        "source exit evidence must remain until the authenticated target switch"
    );
}
