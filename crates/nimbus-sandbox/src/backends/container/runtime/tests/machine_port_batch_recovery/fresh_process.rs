//! Real-process NNC5.4a recovery proofs with one provider surviving every child.

use super::*;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::{Duration, Instant};

use nimbus_network::{LocalPortLeaseAuthority, PortLeasePhase};

use crate::backends::container::runtime::machine_port_publication::{
    MachinePortPublicationAction, MachinePortPublicationCheckpoint, MachinePortPublicationObserver,
};

const ROOT_ENV: &str = "NIMBUS_NNC54A_ROOT";
const FORWARDER_PORT_ENV: &str = "NIMBUS_NNC54A_FORWARDER_PORT";
const SANDBOX_ID_ENV: &str = "NIMBUS_NNC54A_SANDBOX_ID";
const CHILD_ROLE_ENV: &str = "NIMBUS_NNC54A_CHILD_ROLE";
const CUT_LABEL_ENV: &str = "NIMBUS_NNC54A_CUT_LABEL";
const CUT_MARKER_ENV: &str = "NIMBUS_NNC54A_CUT_MARKER";
const CUT_MARKER_FILE: &str = ".nimbus-nnc5-4a-child-cut";
const CHILD_TIMEOUT: Duration = Duration::from_secs(15);
const CHILD_POLL: Duration = Duration::from_millis(10);
const CHILD_TEST: &str = concat!(
    "backends::container::runtime::tests::lifecycle::machine_port_batch_recovery::",
    "fresh_process::nnc5_4a_fresh_process_child"
);
const EXPOSURE_CUTS: [&str; 7] = [
    "machine.expose.local_provider_ready",
    "machine.expose.batch_prepared",
    "machine.expose.slot_effect_prepared",
    "machine.expose.slot_effect_returned",
    "machine.expose.slot_observed",
    "machine.expose.batch_exposed",
    "machine.expose.attachment_active",
];
const WITHDRAWAL_CUTS: [&str; 7] = [
    "machine.withdraw.batch_prepared",
    "machine.withdraw.local_provider_stopped",
    "machine.withdraw.slot_effect_prepared",
    "machine.withdraw.slot_effect_returned",
    "machine.withdraw.slot_observed_absent",
    "machine.withdraw.batch_absent",
    "machine.withdraw.listener_settled",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScriptedMutation {
    Expose,
    Withdraw,
}

#[derive(Debug)]
struct ProviderResult {
    expose_mutations: BTreeMap<String, usize>,
    withdraw_mutations: BTreeMap<String, usize>,
    requests: Vec<String>,
    routes: BTreeSet<(String, String, String)>,
}

struct SurvivingProvider {
    address: SocketAddr,
    gate_reached: Receiver<()>,
    gate_release: SyncSender<()>,
    server: thread::JoinHandle<ProviderResult>,
}

impl SurvivingProvider {
    fn spawn(
        listener: TcpListener,
        initial_routes: BTreeSet<(String, String, String)>,
        lost_response: Option<(ScriptedMutation, String)>,
        block_first_inspection: bool,
    ) -> Self {
        let address = listener
            .local_addr()
            .expect("surviving provider address should resolve");
        let (gate_reached_tx, gate_reached) = mpsc::sync_channel(1);
        let (gate_release, gate_release_rx) = mpsc::sync_channel(1);
        let server = thread::spawn(move || {
            let mut routes = initial_routes;
            let mut expose_mutations = BTreeMap::<String, usize>::new();
            let mut withdraw_mutations = BTreeMap::<String, usize>::new();
            let mut requests = Vec::new();
            let mut response_was_lost = false;
            let mut lose_next_inspection = false;
            let mut first_inspection_was_blocked = false;
            loop {
                let (mut stream, _) = listener.accept().expect("provider request should arrive");
                let request = read_complete_request(&mut stream);
                if request.starts_with("POST /__nimbus_nnc5_4a_complete ") {
                    write_response(&mut stream, b"[]");
                    return ProviderResult {
                        expose_mutations,
                        withdraw_mutations,
                        requests,
                        routes,
                    };
                }
                if request.starts_with("GET /services/forwarder/all ") {
                    requests.push(request);
                    if block_first_inspection && !first_inspection_was_blocked {
                        first_inspection_was_blocked = true;
                        gate_reached_tx
                            .send(())
                            .expect("provider inspection gate should report readiness");
                        gate_release_rx
                            .recv_timeout(CHILD_TIMEOUT)
                            .expect("provider inspection gate should be released");
                    }
                    if lose_next_inspection {
                        lose_next_inspection = false;
                        stream
                            .shutdown(Shutdown::Write)
                            .expect("lost inspection response should close");
                        continue;
                    }
                    let body = serde_json::to_vec(
                        &routes
                            .iter()
                            .map(|(local, remote, protocol)| {
                                serde_json::json!({
                                    "local": local,
                                    "remote": remote,
                                    "protocol": protocol,
                                })
                            })
                            .collect::<Vec<_>>(),
                    )
                    .expect("route list should encode");
                    write_response(&mut stream, &body);
                    continue;
                }

                let (action, local) = if request.starts_with("POST /services/forwarder/expose ") {
                    let (local, remote, protocol) = parse_route(&request);
                    *expose_mutations.entry(local.clone()).or_default() += 1;
                    routes.replace((local.clone(), remote, protocol));
                    (ScriptedMutation::Expose, local)
                } else {
                    assert!(
                        request.starts_with("POST /services/forwarder/unexpose "),
                        "unexpected provider request: {request}"
                    );
                    let (local, protocol) = parse_withdraw_route(&request);
                    *withdraw_mutations.entry(local.clone()).or_default() += 1;
                    routes.retain(|(candidate, _, candidate_protocol)| {
                        candidate != &local || candidate_protocol != &protocol
                    });
                    (ScriptedMutation::Withdraw, local)
                };
                requests.push(request);
                if !response_was_lost
                    && lost_response
                        .as_ref()
                        .is_some_and(|(expected_action, expected_local)| {
                            *expected_action == action && expected_local == &local
                        })
                {
                    response_was_lost = true;
                    lose_next_inspection = true;
                    stream
                        .shutdown(Shutdown::Write)
                        .expect("lost mutation response should close");
                } else {
                    write_response(&mut stream, &[]);
                }
            }
        });
        Self {
            address,
            gate_reached,
            gate_release,
            server,
        }
    }

    fn wait_for_blocked_inspection(&self) {
        self.gate_reached
            .recv_timeout(CHILD_TIMEOUT)
            .expect("first provider inspection should reach its bounded gate");
    }

    fn release_blocked_inspection(&self) {
        self.gate_release
            .send(())
            .expect("blocked provider inspection should release");
    }

    fn finish(self) -> ProviderResult {
        let _ = self.gate_release.try_send(());
        let mut stream =
            TcpStream::connect(self.address).expect("provider completion request should connect");
        stream
            .write_all(b"POST /__nimbus_nnc5_4a_complete HTTP/1.0\r\nContent-Length: 0\r\n\r\n")
            .expect("provider completion request should write");
        stream
            .shutdown(Shutdown::Write)
            .expect("provider completion request should finish");
        self.server.join().expect("surviving provider should join")
    }
}

struct CrashAtPublicationCheckpoint {
    target: String,
}

impl CrashAtPublicationCheckpoint {
    fn from_environment() -> Self {
        Self {
            target: std::env::var(CUT_LABEL_ENV)
                .expect("named NNC5.4a crash cut should be selected"),
        }
    }
}

impl MachinePortPublicationObserver for CrashAtPublicationCheckpoint {
    fn checkpoint(&mut self, checkpoint: MachinePortPublicationCheckpoint) -> Result<()> {
        let label = match checkpoint {
            MachinePortPublicationCheckpoint::BatchPrepared {
                action: MachinePortPublicationAction::Expose,
                ..
            } => "machine.expose.batch_prepared",
            MachinePortPublicationCheckpoint::SlotEffectPrepared {
                action: MachinePortPublicationAction::Expose,
                index: 0,
                ..
            } => "machine.expose.slot_effect_prepared",
            MachinePortPublicationCheckpoint::SlotEffectReturned {
                action: MachinePortPublicationAction::Expose,
                index: 0,
                ..
            } => "machine.expose.slot_effect_returned",
            MachinePortPublicationCheckpoint::SlotObserved {
                action: MachinePortPublicationAction::Expose,
                index: 0,
                ..
            } => "machine.expose.slot_observed",
            MachinePortPublicationCheckpoint::BatchTerminal {
                action: MachinePortPublicationAction::Expose,
                ..
            } => "machine.expose.batch_exposed",
            MachinePortPublicationCheckpoint::BatchPrepared {
                action: MachinePortPublicationAction::Withdraw,
                ..
            } => "machine.withdraw.batch_prepared",
            MachinePortPublicationCheckpoint::SlotEffectPrepared {
                action: MachinePortPublicationAction::Withdraw,
                index: 0,
                ..
            } => "machine.withdraw.slot_effect_prepared",
            MachinePortPublicationCheckpoint::SlotEffectReturned {
                action: MachinePortPublicationAction::Withdraw,
                index: 0,
                ..
            } => "machine.withdraw.slot_effect_returned",
            MachinePortPublicationCheckpoint::SlotObserved {
                action: MachinePortPublicationAction::Withdraw,
                index: 0,
                ..
            } => "machine.withdraw.slot_observed_absent",
            MachinePortPublicationCheckpoint::BatchTerminal {
                action: MachinePortPublicationAction::Withdraw,
                ..
            } => "machine.withdraw.batch_absent",
            MachinePortPublicationCheckpoint::SlotEffectPrepared { .. }
            | MachinePortPublicationCheckpoint::SlotEffectReturned { .. }
            | MachinePortPublicationCheckpoint::SlotObserved { .. } => return Ok(()),
        };
        if self.target == label {
            signal_crash_cut(label);
        }
        Ok(())
    }
}

#[test]
fn nnc5_4a_fresh_process_exposure_response_loss_recovers_each_slot() {
    for lost_index in 0..2 {
        let root = tempfile::tempdir().expect("fresh-process root should exist");
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .expect("surviving exposure provider should bind");
        let forwarder_port = listener
            .local_addr()
            .expect("surviving exposure provider address should resolve")
            .port();
        let sandbox_id = format!("machine-exposure-process-loss-{lost_index}");
        let (_port_window, _setup, manifest) =
            prepare_manifest(root.path(), forwarder_port, &sandbox_id);
        let lost_local = binding_local(&manifest.spec.port_bindings[lost_index]);
        let provider = SurvivingProvider::spawn(
            listener,
            BTreeSet::new(),
            Some((ScriptedMutation::Expose, lost_local)),
            false,
        );

        kill_child_at_marker(
            spawn_child("exposure-crash", root.path(), forwarder_port, &sandbox_id),
            root.path(),
            "exposure-effect-may-exist",
        );
        let partial = publication_record(&manifest);
        assert_eq!(partial["phase"], "exposing");
        assert_eq!(partial["batch_generation"], 1);
        assert_eq!(partial["slots"][lost_index]["state"], "effect_may_exist");

        assert_child_success(
            run_child(
                "exposure-recovery",
                root.path(),
                forwarder_port,
                &sandbox_id,
            ),
            "exposure-recovered",
        );
        let terminal_bytes = evidence_bytes(&manifest);
        assert_child_success(
            run_child("exposure-replay", root.path(), forwarder_port, &sandbox_id),
            "exposure-replayed",
        );
        assert_eq!(
            evidence_bytes(&manifest),
            terminal_bytes,
            "second fresh-process exposure replay must preserve terminal bytes"
        );

        let result = provider.finish();
        assert!(result.withdraw_mutations.is_empty());
        assert_eq!(result.routes.len(), manifest.spec.port_bindings.len());
        for binding in &manifest.spec.port_bindings {
            assert_eq!(
                result.expose_mutations.get(&binding_local(binding)),
                Some(&1),
                "each exposure effect must occur once after owner death; requests: {:?}",
                result.requests
            );
        }
        let terminal = publication_record(&manifest);
        assert_eq!(terminal["phase"], "exposed");
        assert_eq!(terminal["batch_generation"], 1);
    }
}

#[test]
fn nnc5_4a_fresh_process_withdrawal_response_loss_recovers_each_slot() {
    for lost_index in 0..2 {
        let root = tempfile::tempdir().expect("fresh-process root should exist");
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .expect("surviving withdrawal provider should bind");
        let forwarder_port = listener
            .local_addr()
            .expect("surviving withdrawal provider address should resolve")
            .port();
        let sandbox_id = format!("machine-withdrawal-process-loss-{lost_index}");
        let (_port_window, _setup, manifest) =
            prepare_manifest(root.path(), forwarder_port, &sandbox_id);
        let initial_routes = desired_routes(&manifest);
        let lost_local = binding_local(&manifest.spec.port_bindings[lost_index]);
        let provider = SurvivingProvider::spawn(
            listener,
            initial_routes,
            Some((ScriptedMutation::Withdraw, lost_local)),
            false,
        );

        kill_child_at_marker(
            spawn_child("withdrawal-crash", root.path(), forwarder_port, &sandbox_id),
            root.path(),
            "withdrawal-effect-may-exist",
        );
        let partial = publication_record(&manifest);
        assert_eq!(partial["phase"], "withdrawing");
        assert_eq!(partial["batch_generation"], 2);
        assert_eq!(partial["slots"][lost_index]["state"], "effect_may_exist");

        assert_child_success(
            run_child(
                "withdrawal-recovery",
                root.path(),
                forwarder_port,
                &sandbox_id,
            ),
            "withdrawal-recovered",
        );
        let terminal_bytes = evidence_bytes(&manifest);
        assert_child_success(
            run_child(
                "withdrawal-replay",
                root.path(),
                forwarder_port,
                &sandbox_id,
            ),
            "withdrawal-replayed",
        );
        assert_eq!(
            evidence_bytes(&manifest),
            terminal_bytes,
            "second fresh-process withdrawal replay must preserve terminal bytes"
        );

        let result = provider.finish();
        assert!(result.expose_mutations.is_empty());
        assert!(result.routes.is_empty());
        for binding in &manifest.spec.port_bindings {
            assert_eq!(
                result.withdraw_mutations.get(&binding_local(binding)),
                Some(&1),
                "each withdrawal effect must occur once after owner death; requests: {:?}",
                result.requests
            );
        }
        let terminal = publication_record(&manifest);
        assert_eq!(terminal["phase"], "absent");
        assert_eq!(terminal["batch_generation"], 2);
    }
}

#[test]
fn nnc5_4a_every_named_exposure_cut_recovers_in_a_fresh_process() {
    for cut in EXPOSURE_CUTS {
        let root = tempfile::tempdir().expect("named exposure root should exist");
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .expect("named exposure provider should bind");
        let forwarder_port = listener
            .local_addr()
            .expect("named exposure provider address should resolve")
            .port();
        let sandbox_id = format!("machine-exposure-cut-{}", cut.replace('.', "-"));
        let (_port_window, _setup, manifest) =
            prepare_manifest(root.path(), forwarder_port, &sandbox_id);
        let provider = SurvivingProvider::spawn(listener, BTreeSet::new(), None, false);

        kill_child_at_marker(
            spawn_cut_child(
                "exposure-cut",
                cut,
                root.path(),
                forwarder_port,
                &sandbox_id,
            ),
            root.path(),
            cut,
        );
        assert_child_success(
            run_child(
                "exposure-cut-recovery",
                root.path(),
                forwarder_port,
                &sandbox_id,
            ),
            "exposure-recovered",
        );
        let terminal_bytes = evidence_bytes(&manifest);
        assert_child_success(
            run_child("exposure-replay", root.path(), forwarder_port, &sandbox_id),
            "exposure-replayed",
        );
        assert_eq!(
            evidence_bytes(&manifest),
            terminal_bytes,
            "named exposure cut {cut} must converge to a byte-stable terminal replay"
        );

        let result = provider.finish();
        assert!(result.withdraw_mutations.is_empty());
        assert_eq!(result.routes.len(), manifest.spec.port_bindings.len());
        for binding in &manifest.spec.port_bindings {
            assert_eq!(
                result.expose_mutations.get(&binding_local(binding)),
                Some(&1),
                "named exposure cut {cut} duplicated or skipped a provider mutation; requests: {:?}",
                result.requests
            );
        }
    }
}

#[test]
fn nnc5_4a_every_named_withdrawal_cut_recovers_in_a_fresh_process() {
    for cut in WITHDRAWAL_CUTS {
        let root = tempfile::tempdir().expect("named withdrawal root should exist");
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .expect("named withdrawal provider should bind");
        let forwarder_port = listener
            .local_addr()
            .expect("named withdrawal provider address should resolve")
            .port();
        let sandbox_id = format!("machine-withdrawal-cut-{}", cut.replace('.', "-"));
        let (_port_window, _setup, manifest) =
            prepare_manifest(root.path(), forwarder_port, &sandbox_id);
        let provider = SurvivingProvider::spawn(listener, desired_routes(&manifest), None, false);

        kill_child_at_marker(
            spawn_cut_child(
                "withdrawal-cut",
                cut,
                root.path(),
                forwarder_port,
                &sandbox_id,
            ),
            root.path(),
            cut,
        );
        assert_child_success(
            run_child(
                "withdrawal-recovery",
                root.path(),
                forwarder_port,
                &sandbox_id,
            ),
            "withdrawal-recovered",
        );
        let terminal_bytes = evidence_bytes(&manifest);
        assert_child_success(
            run_child(
                "withdrawal-replay",
                root.path(),
                forwarder_port,
                &sandbox_id,
            ),
            "withdrawal-replayed",
        );
        assert_eq!(
            evidence_bytes(&manifest),
            terminal_bytes,
            "named withdrawal cut {cut} must converge to a byte-stable terminal replay"
        );

        let result = provider.finish();
        assert!(result.expose_mutations.is_empty());
        assert!(result.routes.is_empty());
        for binding in &manifest.spec.port_bindings {
            assert_eq!(
                result.withdraw_mutations.get(&binding_local(binding)),
                Some(&1),
                "named withdrawal cut {cut} duplicated or skipped a provider mutation; requests: {:?}",
                result.requests
            );
        }
    }
}

#[test]
fn nnc5_4a_two_process_contenders_share_one_generation_and_effect_sequence() {
    let root = tempfile::tempdir().expect("contention root should exist");
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .expect("contended surviving provider should bind");
    let forwarder_port = listener
        .local_addr()
        .expect("contended surviving provider address should resolve")
        .port();
    let sandbox_id = "machine-publication-process-contention";
    let (_port_window, setup, manifest) = prepare_manifest(root.path(), forwarder_port, sandbox_id);
    setup
        .ensure_machine_port_proxies_running_with_publication(
            &manifest.handle.id,
            &[Ipv4Addr::LOCALHOST],
            &manifest,
            MachinePortPreparationReleaseAuthority::FreshLaunch(
                manifest
                    .launch_reservation_claim
                    .as_ref()
                    .expect("contention fixture should retain launch authority"),
            ),
            || Ok(()),
        )
        .expect("contention fixture should activate exact local listeners");
    let provider = SurvivingProvider::spawn(listener, BTreeSet::new(), None, true);

    let owner = spawn_child("contention-owner", root.path(), forwarder_port, sandbox_id);
    provider.wait_for_blocked_inspection();
    assert_child_success(
        run_child("contention-waiter", root.path(), forwarder_port, sandbox_id),
        "contention-timeout",
    );
    provider.release_blocked_inspection();
    assert_child_success(
        wait_for_child(owner, "contention owner"),
        "contention-owner-complete",
    );
    let terminal_bytes = evidence_bytes(&manifest);
    assert_child_success(
        run_child("exposure-replay", root.path(), forwarder_port, sandbox_id),
        "exposure-replayed",
    );
    assert_eq!(evidence_bytes(&manifest), terminal_bytes);

    let result = provider.finish();
    assert!(result.withdraw_mutations.is_empty());
    for binding in &manifest.spec.port_bindings {
        assert_eq!(
            result.expose_mutations.get(&binding_local(binding)),
            Some(&1),
            "contenders must share one provider effect sequence; requests: {:?}",
            result.requests
        );
    }
    let terminal = publication_record(&manifest);
    assert_eq!(terminal["phase"], "exposed");
    assert_eq!(terminal["batch_generation"], 1);
}

#[test]
#[ignore = "spawned only by the NNC5.4a real-process parent tests"]
fn nnc5_4a_fresh_process_child() {
    match std::env::var(CHILD_ROLE_ENV)
        .expect("NNC5.4a child role should be set")
        .as_str()
    {
        "exposure-crash" => exposure_crash_child(),
        "exposure-cut" => exposure_named_cut_child(),
        "exposure-cut-recovery" => exposure_named_cut_recovery_child(),
        "exposure-recovery" => exposure_recovery_child(false),
        "exposure-replay" => exposure_recovery_child(true),
        "withdrawal-crash" => withdrawal_crash_child(),
        "withdrawal-cut" => withdrawal_named_cut_child(),
        "withdrawal-recovery" => withdrawal_recovery_child(false),
        "withdrawal-replay" => withdrawal_recovery_child(true),
        "contention-owner" => contention_owner_child(),
        "contention-waiter" => contention_waiter_child(),
        role => panic!("unknown NNC5.4a child role {role:?}"),
    }
}

fn exposure_named_cut_child() {
    let cut = std::env::var(CUT_LABEL_ENV).expect("named exposure cut should be selected");
    assert!(
        EXPOSURE_CUTS.contains(&cut.as_str()),
        "unknown named exposure cut {cut:?}"
    );
    let (backend, manifest) = child_backend_and_manifest();
    let launch_claim = manifest
        .launch_reservation_claim
        .as_ref()
        .expect("named exposure child should retain exact launch authority");

    if cut == "machine.expose.attachment_active" {
        backend
            .segment_allocator
            .adopt_reserved_attachment(
                &manifest.spec.tenant_id,
                &default_network_attachment_id(&manifest.handle.id),
                launch_claim,
            )
            .expect("named Active cut should adopt its exact attachment reservation");
        let network_config = manifest
            .require_network_config()
            .expect("named Active cut should retain network config")
            .clone();
        let ports = backend
            .port_lease_coordinator_for_manifest(&manifest)
            .expect("named Active cut should retain exact port authority");
        let hostname = hostname_for(&manifest.spec);
        backend
            .attachment_adapter(
                &manifest,
                &network_config,
                &hostname,
                manifest.runner_config.machine_port_forwarder.as_ref(),
            )
            .attach_with_test_host(
                &backend.attachment_lifecycle(&ports),
                crate::backends::oci::network::AttachmentAttachAuthority::FreshLaunch(launch_claim),
                |assigned_ips| {
                    let mut observer = CrashAtPublicationCheckpoint::from_environment();
                    backend.ensure_machine_port_proxies_running_with_publication(
                        &manifest.handle.id,
                        assigned_ips,
                        &manifest,
                        MachinePortPreparationReleaseAuthority::FreshLaunch(launch_claim),
                        || {
                            let forwarder = manifest
                                .runner_config
                                .machine_port_forwarder
                                .as_ref()
                                .expect("named exposure cut should retain provider authority");
                            backend
                                .converge_machine_port_publication_for_test_with_observer(
                                    &manifest,
                                    forwarder,
                                    MachinePortPublicationAction::Expose,
                                    &mut observer,
                                )
                                .map(|_| ())
                        },
                    )
                },
            )
            .expect("named Active cut should complete the portable attachment");
        signal_crash_cut("machine.expose.attachment_active");
    }

    let mut observer = CrashAtPublicationCheckpoint::from_environment();
    backend
        .ensure_machine_port_proxies_running_with_publication(
            &manifest.handle.id,
            &[Ipv4Addr::LOCALHOST],
            &manifest,
            MachinePortPreparationReleaseAuthority::FreshLaunch(launch_claim),
            || {
                if cut == "machine.expose.local_provider_ready" {
                    signal_crash_cut("machine.expose.local_provider_ready");
                }
                let forwarder = manifest
                    .runner_config
                    .machine_port_forwarder
                    .as_ref()
                    .expect("named exposure cut should retain provider authority");
                backend
                    .converge_machine_port_publication_for_test_with_observer(
                        &manifest,
                        forwarder,
                        MachinePortPublicationAction::Expose,
                        &mut observer,
                    )
                    .map(|_| ())
            },
        )
        .unwrap_or_else(|error| panic!("named exposure cut {cut} was not reached: {error}"));
    panic!("named exposure cut {cut} completed without signaling its boundary");
}

fn withdrawal_named_cut_child() {
    let cut = std::env::var(CUT_LABEL_ENV).expect("named withdrawal cut should be selected");
    assert!(
        WITHDRAWAL_CUTS.contains(&cut.as_str()),
        "unknown named withdrawal cut {cut:?}"
    );
    let (backend, manifest) = child_backend_and_manifest();
    backend
        .ensure_machine_port_proxies_running(&manifest.handle.id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("named withdrawal child should own exact local listeners and Exposed evidence");

    let mut observer = CrashAtPublicationCheckpoint::from_environment();
    if cut == "machine.withdraw.batch_prepared" {
        backend
            .prepare_machine_port_publication_withdrawal_for_test_with_observer(
                &manifest,
                &mut observer,
            )
            .expect("named withdrawal batch cut should prepare");
        panic!("named withdrawal batch-prepared cut did not signal");
    }

    let cleanup = backend
        .begin_machine_port_proxy_release(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("named withdrawal child should begin exact release")
        .expect("named withdrawal child should retain provider cleanup");
    backend
        .stop_machine_port_proxy_provider_for_test(&cleanup)
        .expect("named withdrawal child should stop its exact local provider");
    if cut == "machine.withdraw.local_provider_stopped" {
        signal_crash_cut("machine.withdraw.local_provider_stopped");
    }

    let forwarder = manifest
        .runner_config
        .machine_port_forwarder
        .as_ref()
        .expect("named withdrawal child should retain provider authority");
    backend
        .converge_machine_port_publication_for_test_with_observer(
            &manifest,
            forwarder,
            MachinePortPublicationAction::Withdraw,
            &mut observer,
        )
        .unwrap_or_else(|error| panic!("named withdrawal cut {cut} was not reached: {error}"));
    backend
        .complete_machine_port_proxy_cleanup(&cleanup)
        .expect("named withdrawal child should settle exact listener authority");
    if cut == "machine.withdraw.listener_settled" {
        signal_crash_cut("machine.withdraw.listener_settled");
    }
    panic!("named withdrawal cut {cut} completed without signaling its boundary");
}

fn exposure_named_cut_recovery_child() {
    let (backend, manifest) = child_backend_and_manifest();
    backend
        .ensure_machine_port_proxies_running_with_publication(
            &manifest.handle.id,
            &[Ipv4Addr::LOCALHOST],
            &manifest,
            MachinePortPreparationReleaseAuthority::Retain,
            || backend.converge_exposed_machine_port_publication(&manifest),
        )
        .expect(
            "fresh named-cut recovery should reclaim only dead local lifetimes, rebuild the exact \
             route set, and converge publication from current provider evidence",
        );
    let record = publication_record(&manifest);
    assert_eq!(record["phase"], "exposed");
    assert_eq!(record["batch_generation"], 1);
    emit_child_event("exposure-recovered");
}

fn exposure_crash_child() {
    let (backend, manifest) = child_backend_and_manifest();
    let publish = || backend.converge_exposed_machine_port_publication(&manifest);
    let error = backend
        .ensure_machine_port_proxies_running_with_publication(
            &manifest.handle.id,
            &[Ipv4Addr::LOCALHOST],
            &manifest,
            MachinePortPreparationReleaseAuthority::FreshLaunch(
                manifest
                    .launch_reservation_claim
                    .as_ref()
                    .expect("exposure child should retain launch authority"),
            ),
            publish,
        )
        .expect_err("lost mutation and inspection responses must remain ambiguous");
    assert!(
        error
            .to_string()
            .contains("exact post-effect inspection failed"),
        "exposure ambiguity should retain its exact diagnostic: {error}"
    );
    signal_crash_cut("exposure-effect-may-exist");
}

fn exposure_recovery_child(replay: bool) {
    let (backend, manifest) = child_backend_and_manifest();
    let before = replay.then(|| evidence_bytes(&manifest));
    backend
        .converge_exposed_machine_port_publication(&manifest)
        .expect("fresh exposure owner should converge from durable batch plus provider inspection");
    let record = publication_record(&manifest);
    assert_eq!(record["phase"], "exposed");
    assert_eq!(record["batch_generation"], 1);
    if let Some(before) = before {
        assert_eq!(
            evidence_bytes(&manifest),
            before,
            "terminal exposure replay must be byte-stable"
        );
        emit_child_event("exposure-replayed");
    } else {
        emit_child_event("exposure-recovered");
    }
}

fn withdrawal_crash_child() {
    let (backend, manifest) = child_backend_and_manifest();
    backend
        .ensure_machine_port_proxies_running(&manifest.handle.id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("withdrawal child should own exact local listeners");
    let cleanup = backend
        .begin_machine_port_proxy_release(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("withdrawal child should begin exact release")
        .expect("withdrawal child should retain provider cleanup");
    let forwarder = manifest
        .runner_config
        .machine_port_forwarder
        .as_ref()
        .expect("withdrawal child should retain provider authority");
    let error = backend
        .unexpose_machine_port_proxy_publications(&cleanup, forwarder)
        .expect_err("lost mutation and inspection responses must retain withdrawal ambiguity");
    assert!(
        error
            .to_string()
            .contains("exact post-effect inspection failed"),
        "withdrawal ambiguity should retain its exact diagnostic: {error}"
    );
    signal_crash_cut("withdrawal-effect-may-exist");
}

fn withdrawal_recovery_child(replay: bool) {
    let (backend, manifest) = child_backend_and_manifest();
    if replay {
        let before = evidence_bytes(&manifest);
        let forwarder = manifest
            .runner_config
            .machine_port_forwarder
            .as_ref()
            .expect("withdrawal replay should retain provider authority");
        backend
            .converge_absent_machine_port_publication(
                &manifest.spec.tenant_id,
                &manifest.handle.id,
                &manifest.spec.port_bindings,
                forwarder,
            )
            .expect("fresh terminal replay should inspect exact provider absence");
        assert_eq!(
            evidence_bytes(&manifest),
            before,
            "terminal withdrawal replay must be byte-stable"
        );
        assert_listener_released(&backend, &manifest);
        emit_child_event("withdrawal-replayed");
        return;
    }
    let cleanup = backend
        .begin_machine_port_proxy_release(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("fresh withdrawal owner should recover exact authority");
    if cleanup.is_some() {
        assert_cleanup_pending(&backend, &manifest);
    }
    let authority_path = nimbus_network::LocalNetworkStateStore::authority_path_for(
        &backend.config.network_state_root,
    );
    let authority_before = fs::read(&authority_path)
        .expect("combined network authority should remain readable before provider inspection");
    let forwarder = manifest
        .runner_config
        .machine_port_forwarder
        .as_ref()
        .expect("withdrawal recovery should retain provider authority");
    backend
        .converge_absent_machine_port_publication(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            forwarder,
        )
        .expect("fresh withdrawal owner should converge from current provider inspection");
    assert_eq!(
        fs::read(&authority_path)
            .expect("combined network authority should remain readable after provider effects"),
        authority_before,
        "provider inspection and withdrawal must not alter shared network authority"
    );
    let record = publication_record(&manifest);
    assert_eq!(record["phase"], "absent");
    assert_eq!(record["batch_generation"], 2);
    if let Some(cleanup) = cleanup {
        backend
            .stop_machine_port_proxy_provider_for_test(&cleanup)
            .expect("fresh withdrawal owner should confirm exact local provider absence");
        backend
            .complete_machine_port_proxy_cleanup(&cleanup)
            .expect("fresh withdrawal owner should release exact listener authority");
    }
    assert_listener_released(&backend, &manifest);
    emit_child_event("withdrawal-recovered");
}

fn contention_owner_child() {
    let (backend, manifest) = child_backend_and_manifest();
    backend
        .converge_exposed_machine_port_publication(&manifest)
        .expect("contention owner should converge one exact publication generation");
    emit_child_event("contention-owner-complete");
}

fn contention_waiter_child() {
    let (backend, manifest) = child_backend_and_manifest();
    let error = backend
        .converge_exposed_machine_port_publication(&manifest)
        .expect_err("second process must receive the bounded publication-lock timeout");
    let diagnostic = error.to_string();
    assert!(
        diagnostic.contains("timed out acquiring machine port evidence lock")
            && diagnostic.contains("canonical observation remains unchanged"),
        "bounded contender diagnostic must retain unchanged canonical state: {error}"
    );
    emit_child_event("contention-timeout");
}

/// Returns the claimed window alongside the fixture. The child processes below
/// bind both published ports, so the caller must keep the claim alive until the
/// last child exits rather than letting it end with this constructor.
fn prepare_manifest(
    root: &Path,
    forwarder_port: u16,
    sandbox_id: &str,
) -> (
    PortWindow,
    ContainerSandboxBackend,
    ContainerSandboxManifest,
) {
    let port_window = PortWindow::claim();
    let first_port = port_window.port(0);
    let second_port = port_window.port(1);
    let mut config = ContainerSandboxBackendConfig::under_root(root);
    config.machine_port_forwarder = Some(sample_forwarder(forwarder_port));
    let backend = ContainerSandboxBackend::new(config);
    let manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_bindings([
                SandboxPortBinding::tcp("first", first_port, 8_080),
                SandboxPortBinding::tcp("second", second_port, 8_081),
            ]),
            &SandboxId::new(sandbox_id),
            None,
            None,
        )
        .expect("fresh-process manifest should reserve exact authority")
        .manifest;
    seed_publication_attachment(&backend, &manifest);
    backend
        .write_manifest(&manifest)
        .expect("fresh-process manifest should be durable before child launch");
    (port_window, backend, manifest)
}

fn child_backend_and_manifest() -> (ContainerSandboxBackend, ContainerSandboxManifest) {
    let root = child_root();
    let forwarder_port = std::env::var(FORWARDER_PORT_ENV)
        .expect("NNC5.4a child forwarder port should be set")
        .parse::<u16>()
        .expect("NNC5.4a child forwarder port should parse");
    let sandbox_id = SandboxId::new(
        std::env::var(SANDBOX_ID_ENV).expect("NNC5.4a child sandbox ID should be set"),
    );
    let mut config = ContainerSandboxBackendConfig::under_root(&root);
    config.machine_port_forwarder = Some(sample_forwarder(forwarder_port));
    let backend = ContainerSandboxBackend::new(config);
    let manifest = backend
        .read_manifest(&sandbox_id)
        .expect("NNC5.4a child manifest should read")
        .expect("NNC5.4a child manifest should remain durable");
    (backend, manifest)
}

fn assert_cleanup_pending(backend: &ContainerSandboxBackend, manifest: &ContainerSandboxManifest) {
    let authority = LocalPortLeaseAuthority::open(&backend.config.network_state_root)
        .expect("port authority should reopen in a fresh process");
    for lease in &manifest.port_leases {
        let record = authority
            .inspect(lease.lease_id())
            .expect("port lease should inspect")
            .expect("port lease should remain durable");
        assert_eq!(
            record.phase(),
            PortLeasePhase::CleanupPending,
            "fresh withdrawal must retain every exact listener fence"
        );
    }
}

fn assert_listener_released(
    backend: &ContainerSandboxBackend,
    manifest: &ContainerSandboxManifest,
) {
    let authority = LocalPortLeaseAuthority::open(&backend.config.network_state_root)
        .expect("port authority should reopen in a fresh process");
    for lease in &manifest.port_leases {
        let record = authority
            .inspect(lease.lease_id())
            .expect("port lease should inspect")
            .expect("port lease should remain durable");
        assert_eq!(
            record.phase(),
            PortLeasePhase::Released,
            "terminal withdrawal recovery must release every exact listener fence once"
        );
    }
}

fn desired_routes(manifest: &ContainerSandboxManifest) -> BTreeSet<(String, String, String)> {
    manifest
        .spec
        .port_bindings
        .iter()
        .map(|binding| {
            (
                binding_local(binding),
                format!(":{}", binding.host_port),
                "tcp".to_owned(),
            )
        })
        .collect()
}

fn binding_local(binding: &SandboxPortBinding) -> String {
    format!("{}:{}", binding.host_address, binding.host_port)
}

fn evidence_bytes(manifest: &ContainerSandboxManifest) -> Vec<u8> {
    let path = manifest
        .conmon_layout
        .container_state_dir
        .join(".nimbus-machine-port-evidence.json");
    fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "machine publication evidence {} should read: {error}",
            path.display()
        )
    })
}

fn child_root() -> PathBuf {
    PathBuf::from(std::env::var_os(ROOT_ENV).expect("NNC5.4a child root should be set"))
}

fn signal_crash_cut(label: &str) -> ! {
    let marker = PathBuf::from(
        std::env::var_os(CUT_MARKER_ENV).expect("NNC5.4a child cut marker should be set"),
    );
    let stage = marker.with_extension("stage");
    emit_child_event(label);
    let mut file = fs::File::create(&stage).expect("NNC5.4a child cut marker should create");
    file.write_all(label.as_bytes())
        .expect("NNC5.4a child cut marker should write");
    file.sync_all()
        .expect("NNC5.4a child cut marker should be observable");
    fs::rename(&stage, &marker).expect("NNC5.4a child cut marker should publish atomically");
    thread::park_timeout(CHILD_TIMEOUT);
    panic!("NNC5.4a child was not killed at {label} within {CHILD_TIMEOUT:?}");
}

fn emit_child_event(label: &str) {
    println!("NIMBUS_NNC54A/1\t{label}");
    std::io::stdout()
        .flush()
        .expect("NNC5.4a child event should flush");
}

fn spawn_child(role: &str, root: &Path, forwarder_port: u16, sandbox_id: &str) -> Child {
    Command::new(std::env::current_exe().expect("NNC5.4a test executable should resolve"))
        .args(["--exact", CHILD_TEST, "--ignored", "--nocapture"])
        .env(ROOT_ENV, root)
        .env(FORWARDER_PORT_ENV, forwarder_port.to_string())
        .env(SANDBOX_ID_ENV, sandbox_id)
        .env(CHILD_ROLE_ENV, role)
        .env(CUT_MARKER_ENV, root.join(CUT_MARKER_FILE))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("failed to spawn NNC5.4a child role {role}: {error}"))
}

fn spawn_cut_child(
    role: &str,
    cut: &str,
    root: &Path,
    forwarder_port: u16,
    sandbox_id: &str,
) -> Child {
    let mut command =
        Command::new(std::env::current_exe().expect("NNC5.4a test executable should resolve"));
    command
        .args(["--exact", CHILD_TEST, "--ignored", "--nocapture"])
        .env(ROOT_ENV, root)
        .env(FORWARDER_PORT_ENV, forwarder_port.to_string())
        .env(SANDBOX_ID_ENV, sandbox_id)
        .env(CHILD_ROLE_ENV, role)
        .env(CUT_LABEL_ENV, cut)
        .env(CUT_MARKER_ENV, root.join(CUT_MARKER_FILE))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
        .spawn()
        .unwrap_or_else(|error| panic!("failed to spawn NNC5.4a child role {role}: {error}"))
}

fn run_child(role: &str, root: &Path, forwarder_port: u16, sandbox_id: &str) -> ChildOutput {
    wait_for_child(spawn_child(role, root, forwarder_port, sandbox_id), role)
}

fn wait_for_child(mut child: Child, role: &str) -> ChildOutput {
    let deadline = Instant::now() + CHILD_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return collect_child(child),
            Ok(None) if Instant::now() < deadline => thread::sleep(CHILD_POLL),
            Ok(None) => {
                terminate_child(&mut child);
                let output = collect_child(child);
                panic!(
                    "NNC5.4a child {role} exceeded {CHILD_TIMEOUT:?}\nstdout:\n{}\nstderr:\n{}",
                    output.stdout, output.stderr
                );
            }
            Err(error) => {
                terminate_child(&mut child);
                let output = collect_child(child);
                panic!(
                    "failed waiting for NNC5.4a child {role}: {error}\nstdout:\n{}\nstderr:\n{}",
                    output.stdout, output.stderr
                );
            }
        }
    }
}

fn kill_child_at_marker(mut child: Child, root: &Path, expected_label: &str) {
    let marker = root.join(CUT_MARKER_FILE);
    let deadline = Instant::now() + CHILD_TIMEOUT;
    loop {
        if marker.exists() {
            let label = fs::read_to_string(&marker)
                .expect("NNC5.4a crash-cut marker should remain readable");
            assert_eq!(
                label, expected_label,
                "NNC5.4a child reached a different crash cut"
            );
            terminate_child(&mut child);
            let output = collect_child(child);
            assert!(
                !output.status.success() && output.stdout.contains(&label),
                "NNC5.4a child must be killed after reporting {label}\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                output.stdout,
                output.stderr
            );
            return;
        }
        match child.try_wait() {
            Ok(Some(_)) => {
                let output = collect_child(child);
                panic!(
                    "NNC5.4a child exited before its crash cut\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
                    output.status, output.stdout, output.stderr
                );
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(CHILD_POLL),
            Ok(None) => {
                terminate_child(&mut child);
                let output = collect_child(child);
                panic!(
                    "NNC5.4a child did not reach its crash cut within {CHILD_TIMEOUT:?}\nstdout:\n{}\nstderr:\n{}",
                    output.stdout, output.stderr
                );
            }
            Err(error) => {
                terminate_child(&mut child);
                let output = collect_child(child);
                panic!(
                    "failed waiting for NNC5.4a crash-cut child: {error}\nstdout:\n{}\nstderr:\n{}",
                    output.stdout, output.stderr
                );
            }
        }
    }
}

fn terminate_child(child: &mut Child) {
    let _ = child.kill();
}

struct ChildOutput {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

fn collect_child(mut child: Child) -> ChildOutput {
    let status = child.wait().expect("NNC5.4a child should reap");
    let mut stdout = String::new();
    child
        .stdout
        .take()
        .expect("NNC5.4a child stdout should be piped")
        .read_to_string(&mut stdout)
        .expect("NNC5.4a child stdout should read");
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("NNC5.4a child stderr should be piped")
        .read_to_string(&mut stderr)
        .expect("NNC5.4a child stderr should read");
    ChildOutput {
        status,
        stdout,
        stderr,
    }
}

fn assert_child_success(output: ChildOutput, expected: &str) {
    assert!(
        output.status.success() && output.stdout.contains(expected),
        "NNC5.4a child did not report {expected:?}\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        output.stdout,
        output.stderr
    );
}
