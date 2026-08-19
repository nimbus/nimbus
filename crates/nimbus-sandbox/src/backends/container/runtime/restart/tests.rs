use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nimbus_process_harness::PortWindow;

use crate::backends::conmon::creator::{CreatorAttemptReceipt, OwnedConmonCreator};
use crate::backends::oci::command::CommandSpec;
use crate::backends::oci::network::FixedOciEgressPinProvider;
use crate::provision::SandboxProvisionPhaseObservation;

use super::super::support::{
    sample_execution_attempt_id, sample_forwarder, sample_provision_network_plan, sample_spec,
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

fn reserve_loopback_port() -> TcpListener {
    TcpListener::bind("127.0.0.1:0").expect("ephemeral loopback listener should bind")
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

struct MachineRestartFixture {
    root: tempfile::TempDir,
    config: super::super::ContainerSandboxBackendConfig,
    backend: ContainerSandboxBackend,
    sandbox_id: SandboxId,
    fence: SandboxRestartAttemptFence,
    network_plan: crate::SandboxProvisionNetworkPlan,
    forwarder_port: u16,
    /// Keeps offsets 0-2 below reserved for this process. `forwarder_port` is
    /// re-bound after publication by `spawn_absent_inspection`, so the claim
    /// must outlive the fixture's constructor.
    _port_window: PortWindow,
}

impl MachineRestartFixture {
    fn published_source(name: &str) -> Self {
        let root = tempfile::tempdir().expect("temporary root should exist");
        // Offset 0 is the published binding, offset 1 the PEP range, offset 2
        // the machine forwarder endpoint. Each tripwire below still holds its
        // port until the product is ready to take it.
        let port_window = PortWindow::claim();
        let published_port = port_window.port(0);
        let pep_port = port_window.port(1);
        let forwarder_port = port_window.port(2);
        let published_reservation =
            TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, published_port))
                .expect("published tripwire should bind");
        let pep_reservation = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, pep_port))
            .expect("PEP tripwire should bind");
        let forwarder_reservation =
            TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, forwarder_port))
                .expect("forwarder tripwire should bind");
        let mut config = super::super::ContainerSandboxBackendConfig::under_root(root.path());
        config.node_network_supernet = "127.0.0.0/24".to_owned();
        config.published_port_range = pep_port..=pep_port;
        config.machine_port_forwarder = Some(sample_forwarder(forwarder_port));
        let backend = ContainerSandboxBackend::new(config.clone())
            .with_egress_pin_provider(Arc::new(FixedOciEgressPinProvider::ready()));
        let sandbox_id = SandboxId::new(format!("container-restart-machine-{name}"));
        let spec = sample_spec().with_port_binding(crate::SandboxPortBinding::tcp(
            "api",
            published_port,
            8080,
        ));
        let network_plan = sample_provision_network_plan(&spec, &sandbox_id, name);
        let source_attempt = sample_execution_attempt_id(&sandbox_id);
        backend
            .reserve_provision_network(
                spec,
                sandbox_id.clone(),
                source_attempt.clone(),
                network_plan.clone(),
            )
            .expect("machine restart fixture should reserve");
        backend
            .prepare_provision_workload(&sandbox_id, &source_attempt)
            .expect("machine restart fixture should prepare");
        drop(pep_reservation);
        backend
            .attach_provision_network_with_test_host(&sandbox_id, &source_attempt)
            .expect("machine restart fixture should attach");
        drop(published_reservation);
        let manifest = backend
            .read_manifest(&sandbox_id)
            .expect("manifest should read")
            .expect("manifest should exist");
        let assigned_ip = backend
            .ready_machine_publication_address(&manifest)
            .expect("private address should be ready");
        let reservation_claim = manifest
            .launch_reservation_claim
            .as_ref()
            .expect("fixture should retain its reservation");
        let plan_members = ContainerSandboxBackend::provision_port_plan_witness(&manifest);
        backend
            .ensure_machine_port_proxies_running_with_publication(
                &sandbox_id,
                &[assigned_ip],
                &manifest,
                MachinePortPreparationReleaseAuthority::FreshPlannedLaunch {
                    reservation_claim,
                    plan_members: &plan_members,
                },
                || backend.converge_exposed_machine_port_publication_for_test(&manifest),
            )
            .expect("source machine ingress should publish");
        drop(forwarder_reservation);
        let mut manifest = backend
            .read_manifest(&sandbox_id)
            .expect("manifest should read")
            .expect("manifest should exist");
        manifest.conmon_launch.delete_command = CommandSpec::new("/usr/bin/true");
        manifest.conmon_launch.state_command = explicitly_absent_runtime_state_command(&sandbox_id);
        manifest.creator_handoff = ContainerCreatorHandoffState::RuntimeObserved {
            receipt: dead_creator_receipt("machine-restart-source"),
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
            .expect("machine restart source should persist");
        let fence = SandboxRestartAttemptFence::new(
            source_attempt,
            crate::SandboxExecutionAttemptId::new(format!("machine-restart-target:{name}"))
                .expect("target attempt should validate"),
            1,
        )
        .expect("restart fence should validate");
        Self {
            root,
            config,
            backend,
            sandbox_id,
            fence,
            network_plan,
            forwarder_port,
            _port_window: port_window,
        }
    }

    fn provider(&self) -> crate::backends::oci::network::OciMachinePortForwarderConfig {
        self.config
            .machine_port_forwarder
            .clone()
            .expect("fixture should retain provider authority")
    }

    fn spawn_absent_inspection(&self) -> std::thread::JoinHandle<()> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, self.forwarder_port))
            .expect("forwarder inspection server should bind");
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("inspection should connect");
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .expect("inspection timeout should configure");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                let read = stream.read(&mut buffer).expect("inspection should read");
                assert_ne!(read, 0, "inspection request should be complete");
                request.extend_from_slice(&buffer[..read]);
            }
            let response =
                b"HTTP/1.0 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n[]";
            stream
                .write_all(response)
                .expect("inspection response should write");
            stream
                .shutdown(Shutdown::Write)
                .expect("inspection response should terminate");
        })
    }
}

#[test]
fn restart_machine_ingress_withdraws_before_quiescence_and_republishes_exact_target() {
    let fixture = MachineRestartFixture::published_source("lifecycle");
    let provider = fixture.provider();
    let manifest_before = fixture
        .backend
        .read_manifest(&fixture.sandbox_id)
        .expect("manifest should read")
        .expect("manifest should exist");
    let network_before = fixture
        .backend
        .read_manifest(&fixture.sandbox_id)
        .expect("manifest should reread")
        .expect("manifest should still exist")
        .network_config;
    let crossed_plan = sample_provision_network_plan(
        &manifest_before.spec,
        &fixture.sandbox_id,
        "crossed-restart-plan",
    );
    let before_crossed = snapshot_files(fixture.root.path());
    let crossed_error = fixture
        .backend
        .withdraw_restart_machine_ingress_with_test_provider(
            &fixture.sandbox_id,
            &fixture.fence,
            &crossed_plan,
            provider.provider_instance(),
            provider.provider_generation(),
        )
        .expect_err("crossed network plan must fail before withdrawal");
    assert!(crossed_error.to_string().contains("crossed its exact plan"));
    assert_eq!(snapshot_files(fixture.root.path()), before_crossed);
    let crossed_provider_reservation = reserve_loopback_port();
    let crossed_provider = sample_forwarder(
        crossed_provider_reservation
            .local_addr()
            .expect("crossed-provider reservation should expose its address")
            .port(),
    );
    let crossed_provider_error = fixture
        .backend
        .withdraw_restart_machine_ingress_with_test_provider(
            &fixture.sandbox_id,
            &fixture.fence,
            &fixture.network_plan,
            crossed_provider.provider_instance(),
            crossed_provider.provider_generation(),
        )
        .expect_err("crossed provider generation must fail before withdrawal");
    assert!(
        crossed_provider_error
            .to_string()
            .contains("crossed its machine-forwarder")
    );
    drop(crossed_provider_reservation);
    assert_eq!(snapshot_files(fixture.root.path()), before_crossed);

    assert_succeeded(
        fixture
            .backend
            .withdraw_restart_machine_ingress_with_test_provider(
                &fixture.sandbox_id,
                &fixture.fence,
                &fixture.network_plan,
                provider.provider_instance(),
                provider.provider_generation(),
            )
            .expect("source publication should withdraw"),
    );
    assert_succeeded(
        fixture
            .backend
            .withdraw_restart_machine_ingress_with_test_provider(
                &fixture.sandbox_id,
                &fixture.fence,
                &fixture.network_plan,
                provider.provider_instance(),
                provider.provider_generation(),
            )
            .expect("withdrawal should replay"),
    );
    assert_succeeded(
        fixture
            .backend
            .inspect_restart_machine_ingress_withdrawal_with_test_provider(
                &fixture.sandbox_id,
                &fixture.fence,
                &fixture.network_plan,
                provider.provider_instance(),
                provider.provider_generation(),
            )
            .expect("withdrawal should inspect"),
    );
    let fresh = ContainerSandboxBackend::new(fixture.config.clone())
        .with_egress_pin_provider(Arc::new(FixedOciEgressPinProvider::ready()));
    assert_succeeded(
        fresh
            .withdraw_restart_machine_ingress_with_test_provider(
                &fixture.sandbox_id,
                &fixture.fence,
                &fixture.network_plan,
                provider.provider_instance(),
                provider.provider_generation(),
            )
            .expect("fresh backend should replay exact withdrawal"),
    );
    assert_succeeded(
        fresh
            .inspect_restart_machine_ingress_withdrawal_with_test_provider(
                &fixture.sandbox_id,
                &fixture.fence,
                &fixture.network_plan,
                provider.provider_instance(),
                provider.provider_generation(),
            )
            .expect("fresh backend should inspect exact withdrawal"),
    );

    let server = fixture.spawn_absent_inspection();
    assert_succeeded(
        fixture
            .backend
            .quiesce_restart_source(&fixture.sandbox_id, &fixture.fence)
            .expect("withdrawn source should quiesce"),
    );
    server.join().expect("inspection server should join");
    assert_succeeded(
        fixture
            .backend
            .prepare_restart_target_attempt(&fixture.sandbox_id, &fixture.fence)
            .expect("target should prepare"),
    );
    let early = fixture
        .backend
        .publish_restart_machine_ingress_with_test_provider(
            &fixture.sandbox_id,
            &fixture.fence,
            &fixture.network_plan,
            provider.provider_instance(),
            provider.provider_generation(),
        )
        .expect_err("publication must wait for retained-network readiness");
    assert!(early.to_string().contains("retained-network readiness"));
    assert_succeeded(
        fixture
            .backend
            .attach_restart_retained_network_with_test_host(&fixture.sandbox_id, &fixture.fence)
            .expect("retained network should attach"),
    );
    let retained_manifest = fixture
        .backend
        .read_manifest(&fixture.sandbox_id)
        .expect("retained manifest should read")
        .expect("retained manifest should exist");
    let retained_listener = retained_manifest
        .port_leases
        .first()
        .expect("fixture should retain one planned listener");
    let blocker = TcpListener::bind((
        std::net::Ipv4Addr::UNSPECIFIED,
        retained_manifest.spec.port_bindings[0].host_port,
    ))
    .expect("external conflict should occupy the retained listener");
    let bind_error = fixture
        .backend
        .publish_restart_machine_ingress_with_test_provider(
            &fixture.sandbox_id,
            &fixture.fence,
            &fixture.network_plan,
            provider.provider_instance(),
            provider.provider_generation(),
        )
        .expect_err("retained listener conflict must fail before external publication");
    assert!(
        bind_error
            .to_string()
            .contains("failed to bind machine port proxy")
    );
    let retained_record =
        nimbus_network::LocalPortLeaseAuthority::open(&fixture.config.network_state_root)
            .expect("port authority should reopen after the conflict")
            .inspect(retained_listener.lease_id())
            .expect("retained listener should inspect")
            .expect("retained listener must remain durable");
    assert_eq!(
        retained_record.phase(),
        nimbus_network::PortLeasePhase::Reserved
    );
    assert!(
        retained_record.bind_claim().is_none()
            && retained_record.binding().is_none()
            && retained_record.failure().is_none(),
        "planned rebind compensation must abandon only the exact no-effect claim: \
         {retained_record:?}"
    );
    drop(blocker);
    assert_succeeded(
        fixture
            .backend
            .publish_restart_machine_ingress_with_test_provider(
                &fixture.sandbox_id,
                &fixture.fence,
                &fixture.network_plan,
                provider.provider_instance(),
                provider.provider_generation(),
            )
            .expect("target publication should converge"),
    );
    assert_succeeded(
        fixture
            .backend
            .publish_restart_machine_ingress_with_test_provider(
                &fixture.sandbox_id,
                &fixture.fence,
                &fixture.network_plan,
                provider.provider_instance(),
                provider.provider_generation(),
            )
            .expect("target publication should replay"),
    );

    let current = fixture
        .backend
        .read_manifest(&fixture.sandbox_id)
        .expect("manifest should read")
        .expect("manifest should exist");
    assert_eq!(current.network_config, network_before);
    let authority =
        nimbus_network::LocalPortLeaseAuthority::open(&fixture.config.network_state_root)
            .expect("port authority should open");
    for request in &current.port_leases {
        assert!(
            authority
                .inspect(request.lease_id())
                .expect("retained lease should inspect")
                .is_some(),
            "restart must never release or reallocate a retained listener lease"
        );
    }
    assert!(fixture.root.path().exists());
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
fn restart_retained_attach_must_retain_network_allocation_retain_port_lease_retain_attachment_identity_retain_pep_authority()
 {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let mut config = super::super::ContainerSandboxBackendConfig::under_root(root.path());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    // The window holds the PEP port until this test ends, so the attachment
    // below binds the exact port the range names.
    let port_window = PortWindow::claim();
    let pep_port = port_window.port(0);
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
