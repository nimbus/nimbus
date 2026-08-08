use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::backends::conmon::creator::{CreatorAttemptReceipt, OwnedConmonCreator};
use crate::backends::oci::command::CommandSpec;
use crate::backends::oci::network::FixedOciEgressPinProvider;
use crate::provision::SandboxProvisionPhaseObservation;

use super::super::support::{
    sample_execution_attempt_id, sample_provision_network_plan, sample_spec,
};
use super::*;

struct RestartFixture {
    root: tempfile::TempDir,
    config: super::super::ContainerSandboxBackendConfig,
    backend: ContainerSandboxBackend,
    sandbox_id: SandboxId,
    fence: SandboxRestartAttemptFence,
}

impl RestartFixture {
    fn natural_exit(name: &str) -> Self {
        let root = tempfile::tempdir().expect("temporary root should exist");
        let config = super::super::ContainerSandboxBackendConfig::under_root(root.path());
        let backend = ContainerSandboxBackend::new(config.clone());
        let sandbox_id = SandboxId::new(format!("container-restart-{name}"));
        let mut manifest = backend
            .plan_start_with_id(&sample_spec(), &sandbox_id, None, None)
            .expect("restart fixture should plan")
            .manifest;
        let source_attempt = manifest.execution_attempt_id.clone();
        let target_attempt =
            crate::SandboxExecutionAttemptId::new(format!("test-restart-target:{sandbox_id}"))
                .expect("target attempt should validate");
        let fence = SandboxRestartAttemptFence::new(source_attempt, target_attempt, 1)
            .expect("restart fence should validate");
        manifest.conmon_launch.delete_command = CommandSpec::new("/usr/bin/true");
        manifest.conmon_launch.state_command = explicitly_absent_runtime_state_command(&sandbox_id);
        let creator_receipt = dead_creator_receipt("natural-exit-creator");
        manifest.creator_handoff = ContainerCreatorHandoffState::RuntimeObserved {
            receipt: creator_receipt,
        };
        std::fs::create_dir_all(&manifest.conmon_layout.container_state_dir)
            .expect("container state should exist");
        std::fs::write(
            &manifest.conmon_layout.conmon_pidfile,
            format!("{}\n", i32::MAX),
        )
        .expect("dead conmon receipt should persist");
        std::fs::write(&manifest.conmon_layout.exit_status_file, b"42\n")
            .expect("natural exit receipt should persist");
        backend
            .write_manifest(&manifest)
            .expect("restart fixture should persist");
        Self {
            root,
            config,
            backend,
            sandbox_id,
            fence,
        }
    }

    fn manifest(&self) -> ContainerSandboxManifest {
        self.backend
            .read_manifest(&self.sandbox_id)
            .expect("manifest should read")
            .expect("manifest should exist")
    }
}

fn explicitly_absent_runtime_state_command(runtime_id: &SandboxId) -> CommandSpec {
    CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "printf '%s\\n' 'container `{0}` does not exist: open `/run/crun/{0}/status`: No such file or directory' >&2; exit 1",
            runtime_id.as_str()
        ),
    ])
}

fn unused_loopback_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("ephemeral loopback listener should bind")
        .local_addr()
        .expect("ephemeral listener should expose its address")
        .port()
}

fn dead_creator_receipt(attempt_id: &str) -> CreatorAttemptReceipt {
    let command = CommandSpec::new("/bin/sh").args(["-c".to_owned(), "exec sleep 60".to_owned()]);
    let mut creator = OwnedConmonCreator::spawn(&command).expect("creator should spawn");
    let receipt = creator
        .attempt_receipt(attempt_id)
        .expect("creator receipt should capture");
    creator
        .cancel_containment_and_reap()
        .expect("creator should become exactly quiescent");
    receipt
}

fn snapshot_files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(base: &Path, current: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let mut entries = std::fs::read_dir(current)
            .expect("snapshot directory should read")
            .map(|entry| entry.expect("snapshot entry should read"))
            .collect::<Vec<_>>();
        entries.sort_by_key(std::fs::DirEntry::path);
        for entry in entries {
            let path = entry.path();
            let metadata = entry.metadata().expect("snapshot metadata should read");
            if metadata.is_dir() {
                visit(base, &path, out);
            } else if metadata.is_file() && path.extension().is_none_or(|ext| ext != "lock") {
                out.insert(
                    path.strip_prefix(base)
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

fn assert_succeeded(observation: SandboxProvisionPhaseObservation) {
    assert!(
        matches!(
            observation,
            SandboxProvisionPhaseObservation::Succeeded { .. }
        ),
        "expected succeeded restart phase, got {observation:?}"
    );
}

#[test]
fn crossed_source_attempt_fails_before_runtime_or_network_mutation() {
    let fixture = RestartFixture::natural_exit("crossed-source");
    let crossed = SandboxRestartAttemptFence::new(
        crate::SandboxExecutionAttemptId::new("crossed-source-attempt")
            .expect("crossed source should validate"),
        fixture.fence.attempt_id().clone(),
        fixture.fence.restart_ordinal(),
    )
    .expect("crossed fence should validate");
    let before = snapshot_files(fixture.root.path());

    let error = fixture
        .backend
        .quiesce_restart_source(&fixture.sandbox_id, &crossed)
        .expect_err("crossed source must fail closed");

    assert!(error.to_string().contains("crossed execution attempt"));
    assert_eq!(snapshot_files(fixture.root.path()), before);
}

#[test]
fn target_switch_before_durable_quiescence_is_byte_stable() {
    let fixture = RestartFixture::natural_exit("early-target");
    let before = snapshot_files(fixture.root.path());

    let error = fixture
        .backend
        .prepare_restart_target_attempt(&fixture.sandbox_id, &fixture.fence)
        .expect_err("target switch must require durable source quiescence");

    assert!(error.to_string().contains("durable source quiescence"));
    assert_eq!(snapshot_files(fixture.root.path()), before);
    assert_eq!(
        fixture.manifest().execution_attempt_id,
        *fixture.fence.source_attempt_id()
    );
}

#[test]
fn natural_exit_restart_replays_and_fresh_backend_inspects_each_durable_phase() {
    let fixture = RestartFixture::natural_exit("natural-exit-replay");

    assert_succeeded(
        fixture
            .backend
            .quiesce_restart_source(&fixture.sandbox_id, &fixture.fence)
            .expect("natural exit should quiesce"),
    );
    assert_eq!(
        fixture.manifest().execution_attempt_id,
        *fixture.fence.source_attempt_id(),
        "source attempt must remain authoritative through durable quiescence"
    );
    assert_succeeded(
        fixture
            .backend
            .quiesce_restart_source(&fixture.sandbox_id, &fixture.fence)
            .expect("same quiescence command should replay"),
    );
    let fresh = ContainerSandboxBackend::new(fixture.config.clone());
    assert_succeeded(
        fresh
            .inspect_restart_source_quiescence(&fixture.sandbox_id, &fixture.fence)
            .expect("fresh backend should inspect source quiescence"),
    );

    assert_succeeded(
        fixture
            .backend
            .prepare_restart_target_attempt(&fixture.sandbox_id, &fixture.fence)
            .expect("target preparation should succeed"),
    );
    assert_eq!(
        fixture.manifest().execution_attempt_id,
        *fixture.fence.attempt_id()
    );
    assert_succeeded(
        fixture
            .backend
            .prepare_restart_target_attempt(&fixture.sandbox_id, &fixture.fence)
            .expect("same target preparation should replay"),
    );
    let fresh = ContainerSandboxBackend::new(fixture.config.clone());
    assert_succeeded(
        fresh
            .inspect_restart_source_quiescence(&fixture.sandbox_id, &fixture.fence)
            .expect("fresh backend should retain source proof after target switch"),
    );
    assert_succeeded(
        fresh
            .inspect_restart_target_preparation(&fixture.sandbox_id, &fixture.fence)
            .expect("fresh backend should inspect target preparation"),
    );

    let successor = SandboxRestartAttemptFence::new(
        fixture.fence.attempt_id().clone(),
        crate::SandboxExecutionAttemptId::new("natural-exit-replay-successor")
            .expect("successor target should validate"),
        2,
    )
    .expect("successor fence should validate");
    assert_succeeded(
        fixture
            .backend
            .quiesce_restart_source(&fixture.sandbox_id, &successor)
            .expect("a completed predecessor must admit the next exact restart fence"),
    );
    assert_eq!(
        fixture.manifest().execution_attempt_id,
        *successor.source_attempt_id()
    );
}

#[test]
fn running_restart_stops_and_deletes_source_exactly_once() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let mut config = super::super::ContainerSandboxBackendConfig::under_root(root.path());
    config.stop_timeout = std::time::Duration::from_secs(2);
    let backend = ContainerSandboxBackend::new(config);
    let sandbox_id = SandboxId::new("container-restart-running-source");
    let mut manifest = backend
        .plan_start_with_id(&sample_spec(), &sandbox_id, None, None)
        .expect("running restart fixture should plan")
        .manifest;
    let fence = SandboxRestartAttemptFence::new(
        manifest.execution_attempt_id.clone(),
        crate::SandboxExecutionAttemptId::new("running-source-target")
            .expect("target should validate"),
        1,
    )
    .expect("restart fence should validate");
    let creator_attempt = "running-source-creator";
    manifest.creator_handoff = ContainerCreatorHandoffState::RuntimeObserved {
        receipt: dead_creator_receipt(creator_attempt),
    };
    std::fs::write(
        &manifest.conmon_layout.conmon_pidfile,
        format!("{}\n", i32::MAX),
    )
    .expect("dead conmon receipt should persist");
    let exit_path = manifest
        .conmon_layout
        .exit_status_file
        .display()
        .to_string();
    let mut runtime = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(format!(
            r#"trap 'printf "42\n" > "{exit_path}"; exit 0' TERM; while :; do sleep 0.05; done"#
        ))
        .spawn()
        .expect("runtime fixture should spawn");
    std::fs::write(
        &manifest.conmon_layout.pidfile,
        format!("{}\n", runtime.id()),
    )
    .expect("runtime pidfile should persist");
    let delete_marker = manifest
        .conmon_layout
        .container_state_dir
        .join("restart-delete-count");
    let delete_marker_path = delete_marker.display().to_string();
    manifest.conmon_launch.delete_command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(r#"printf 'delete\n' >> "{delete_marker_path}""#),
    ]);
    manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            r#"if [ -f "{exit_path}" ]; then printf '%s\n' 'container `{0}` does not exist: open `/run/crun/{0}/status`: No such file or directory' >&2; exit 1; else printf '%s\n' '{{"id":"{0}","status":"running","annotations":{{"com.nimbus.creator-attempt":"{creator_attempt}"}}}}'; fi"#,
            sandbox_id.as_str()
        ),
    ]);
    backend
        .write_manifest(&manifest)
        .expect("running restart fixture should persist");

    assert_succeeded(
        backend
            .quiesce_restart_source(&sandbox_id, &fence)
            .expect("running source should stop and delete"),
    );
    runtime.wait().expect("runtime fixture should reap");
    assert_eq!(
        std::fs::read_to_string(&delete_marker).expect("delete marker should read"),
        "delete\n"
    );
    assert_succeeded(
        backend
            .quiesce_restart_source(&sandbox_id, &fence)
            .expect("quiescence replay should not delete twice"),
    );
    assert_eq!(
        std::fs::read_to_string(delete_marker).expect("delete marker should reread"),
        "delete\n"
    );
}

#[test]
fn live_and_substituted_creator_receipts_fence_restart_progress() {
    for (name, substitute_birth) in [("live", false), ("substituted", true)] {
        let fixture = RestartFixture::natural_exit(name);
        let mut manifest = fixture.manifest();
        let command =
            CommandSpec::new("/bin/sh").args(["-c".to_owned(), "exec sleep 60".to_owned()]);
        let mut creator = OwnedConmonCreator::spawn(&command).expect("creator should spawn");
        let mut receipt = creator
            .attempt_receipt(&format!("{name}-creator"))
            .expect("creator receipt should capture");
        if substitute_birth {
            receipt = receipt.with_substituted_birth_for_test();
        }
        manifest.creator_handoff = ContainerCreatorHandoffState::Pending { receipt };
        fixture
            .backend
            .write_manifest(&manifest)
            .expect("pending creator should persist");
        let before = snapshot_files(fixture.root.path());

        let error = fixture
            .backend
            .quiesce_restart_source(&fixture.sandbox_id, &fixture.fence)
            .expect_err("untrusted creator must fence source quiescence");

        if substitute_birth {
            assert!(error.to_string().contains("cannot be authenticated"));
        } else {
            assert!(error.to_string().contains("remains live"));
        }
        assert_eq!(snapshot_files(fixture.root.path()), before);
        creator
            .cancel_containment_and_reap()
            .expect("creator fixture should terminate");
    }
}

#[test]
fn retained_attach_preserves_exact_network_and_lease_authority() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let mut config = super::super::ContainerSandboxBackendConfig::under_root(root.path());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    let pep_port = unused_loopback_port();
    config.published_port_range = pep_port..=pep_port;
    let backend = ContainerSandboxBackend::new(config.clone())
        .with_egress_pin_provider(std::sync::Arc::new(FixedOciEgressPinProvider::ready()));
    let sandbox_id = SandboxId::new("container-restart-retained-network");
    let spec = sample_spec();
    let network_plan = sample_provision_network_plan(&spec, &sandbox_id, "restart-retained");
    let source_attempt = sample_execution_attempt_id(&sandbox_id);
    backend
        .reserve_provision_network(
            spec,
            sandbox_id.clone(),
            source_attempt.clone(),
            network_plan,
        )
        .expect("retained-network fixture should reserve exact network authority");
    backend
        .prepare_provision_workload(&sandbox_id, &source_attempt)
        .expect("retained-network fixture should prepare");
    backend
        .attach_provision_network_with_test_host(&sandbox_id, &source_attempt)
        .expect("initial private attachment should converge");
    let mut manifest = backend
        .read_manifest(&sandbox_id)
        .expect("manifest should read")
        .expect("manifest should exist");
    manifest.conmon_launch.delete_command = CommandSpec::new("/usr/bin/true");
    manifest.conmon_launch.state_command = explicitly_absent_runtime_state_command(&sandbox_id);
    manifest.creator_handoff = ContainerCreatorHandoffState::RuntimeObserved {
        receipt: dead_creator_receipt("retained-network-source"),
    };
    std::fs::write(
        &manifest.conmon_layout.conmon_pidfile,
        format!("{}\n", i32::MAX),
    )
    .expect("dead conmon receipt should persist");
    std::fs::write(&manifest.conmon_layout.exit_status_file, b"42\n")
        .expect("source exit should persist");
    backend
        .write_manifest(&manifest)
        .expect("retained-network source should persist");
    let fence = SandboxRestartAttemptFence::new(
        source_attempt,
        crate::SandboxExecutionAttemptId::new("retained-network-target")
            .expect("target should validate"),
        1,
    )
    .expect("restart fence should validate");
    let authority = nimbus_network::LocalPortLeaseAuthority::open(&config.network_state_root)
        .expect("port authority should open");
    let pep_lease = manifest
        .egress_proxy
        .as_ref()
        .expect("fixture should retain its PEP")
        .port_lease
        .clone();
    let lease_before = authority
        .inspect(pep_lease.lease_id())
        .expect("PEP lease should inspect")
        .expect("PEP lease should exist");
    let network_before = manifest.network_config.clone();
    let segments_before = backend
        .segment_allocator
        .inspect_segments(&manifest.spec.tenant_id)
        .expect("segments should inspect");

    assert_succeeded(
        backend
            .quiesce_restart_source(&sandbox_id, &fence)
            .expect("source should quiesce"),
    );
    assert_succeeded(
        backend
            .prepare_restart_target_attempt(&sandbox_id, &fence)
            .expect("target should prepare"),
    );
    assert_succeeded(
        backend
            .attach_restart_retained_network_with_test_host(&sandbox_id, &fence)
            .expect("retained private network should attach"),
    );

    let current = backend
        .read_manifest(&sandbox_id)
        .expect("manifest should read")
        .expect("manifest should exist");
    assert_eq!(current.network_config, network_before);
    assert_eq!(
        authority
            .inspect(pep_lease.lease_id())
            .expect("PEP lease should reinspect")
            .expect("PEP lease should remain"),
        lease_before,
        "restart attachment must not release or reallocate the retained PEP lease"
    );
    assert_eq!(
        backend
            .segment_allocator
            .inspect_segments(&manifest.spec.tenant_id)
            .expect("segments should reinspect"),
        segments_before,
        "restart attachment must not allocate another tenant segment"
    );
    assert_succeeded(
        backend
            .attach_restart_retained_network_with_test_host(&sandbox_id, &fence)
            .expect("same retained attachment command should replay"),
    );
    drop(backend);
    let fresh = ContainerSandboxBackend::new(config)
        .with_egress_pin_provider(std::sync::Arc::new(FixedOciEgressPinProvider::ready()));
    assert!(matches!(
        fresh
            .inspect_restart_retained_network(&sandbox_id, &fence)
            .expect("fresh backend should report missing process-local PEP evidence"),
        SandboxProvisionPhaseObservation::InProgress { .. }
    ));
}
