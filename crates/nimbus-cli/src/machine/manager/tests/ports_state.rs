use super::*;
use nimbus_engine::Engine;
use nimbus_network::{
    LocalPortLeaseAuthority, NetworkManagementMode, PortLeaseEffectScope, PortLeaseId,
    PortLeasePhase,
};
use std::io::{self, BufRead as _};
use std::sync::Arc;

const EXTERNAL_PORT_OWNER_CHILD_TEST: &str =
    "machine::manager::tests::ports_state::external_machine_port_owner_child";
const EXTERNAL_PORT_OWNER_ENV: &str = "NIMBUS_MACHINE_EXTERNAL_PORT_OWNER";
const EXTERNAL_PORT_OWNER_PREFIX: &str = "NIMBUS_MACHINE_EXTERNAL_PORT_OWNER/1\tbound:";

#[test]
fn external_binder_after_probe_blocks_provider_while_machine_state_claims_port() {
    let _lifecycle_guard = machine_lifecycle_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let roots = MachineRootLayout::test_sibling_roots(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let engine = Arc::new(
        Engine::new(temp_dir.path().join("engine"))
            .expect("collision fixture engine should initialize"),
    );
    let server = nimbus_server::ServeOptions::new(engine)
        .with_network_state_root(roots.network_state_root.clone());
    let first_kernel_free = fence_preoccupied_range_prefix(&server);

    let prepared = super::super::ports::PreparedMachineSshPortLease::prepare(
        &roots,
        "race-window",
        &MachineStateRecord::initialized(),
    )
    .expect("Nimbus should durably claim a machine port before provider bind");
    let listener_id = prepared.listener_id().clone();
    let allocated_port = prepared.selected_port();
    assert_eq!(
        allocated_port, first_kernel_free,
        "the range authority should select the first candidate not fenced by the harness"
    );
    let mut external_owner = ExternalMachinePortOwner::spawn(allocated_port)
        .expect("external process should bind the probed-and-dropped port");
    assert_eq!(
        external_owner
            .wait_until_bound(Duration::from_secs(5))
            .expect("external owner should acknowledge its exact bound port"),
        allocated_port
    );

    let provider_error = TcpListener::bind(("127.0.0.1", allocated_port))
        .expect_err("a faithful provider socket bind must lose to the external owner");
    assert_eq!(
        provider_error.kind(),
        io::ErrorKind::AddrInUse,
        "provider failure must be the kernel's address-in-use result"
    );

    prepared
        .record_bind_failure(provider_error)
        .expect("the exact no-effect provider failure should become durable");
    let authority = LocalPortLeaseAuthority::open(&roots.network_state_root)
        .expect("shared authority should reopen");
    let record = authority
        .inspect(&PortLeaseId::for_listener(&listener_id))
        .expect("machine lease should inspect")
        .expect("machine lease should remain auditable");
    assert_eq!(record.phase(), PortLeasePhase::Failed);
    assert_eq!(
        record
            .failure()
            .expect("failed bind must retain its receipt")
            .kind(),
        nimbus_network::PortBindFailureKind::AddrInUse
    );
    assert!(
        record.bind_claim().is_none(),
        "a proven no-effect failure must not leave ambiguous bind authority"
    );
}

#[test]
fn machine_ssh_reservation_conflicts_with_server_listener_authority() {
    let _lifecycle_guard = machine_lifecycle_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let roots = MachineRootLayout::test_sibling_roots(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let prepared = super::super::ports::PreparedMachineSshPortLease::prepare(
        &roots,
        "server-conflict",
        &MachineStateRecord::initialized(),
    )
    .expect("machine should select and claim its desired SSH port");
    let allocated_port = prepared.selected_port();
    let engine = Arc::new(
        Engine::new(temp_dir.path().join("engine"))
            .expect("conflict fixture engine should initialize"),
    );
    let server = nimbus_server::ServeOptions::new(engine)
        .with_network_state_root(roots.network_state_root.clone());
    let requested_addr =
        std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, allocated_port));

    let error = match server.prepare_main_listener(requested_addr) {
        Ok(_) => {
            panic!("server listener authority must reject machine-owned SSH port {allocated_port}")
        }
        Err(error) => error,
    };
    let rendered = error.to_string();
    assert!(
        rendered.contains("conflicts with lease") && rendered.contains("owner"),
        "conflict must name both stable authorities: {rendered}"
    );
}

fn fence_preoccupied_range_prefix(server: &nimbus_server::ServeOptions) -> u16 {
    for port in MACHINE_PORT_MIN..=MACHINE_PORT_MAX {
        match TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)) {
            Ok(listener) => {
                drop(listener);
                return port;
            }
            Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
                let requested_addr =
                    std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, port));
                let _durable_blocker =
                    server
                        .prepare_main_listener(requested_addr)
                        .unwrap_or_else(|reserve_error| {
                            panic!(
                                "test harness failed to fence preoccupied candidate {port}: \
                             {reserve_error}"
                            )
                        });
            }
            Err(error) => {
                panic!("test harness failed to probe candidate {port}: {error}");
            }
        }
    }
    panic!("test host has no free managed machine port");
}

#[test]
#[ignore = "spawned only by the machine probe/bind race characterization"]
fn external_machine_port_owner_child() {
    let port = std::env::var(EXTERNAL_PORT_OWNER_ENV)
        .expect("external owner port should be set")
        .parse::<u16>()
        .expect("external owner port should parse");
    let _listener = TcpListener::bind(("127.0.0.1", port))
        .unwrap_or_else(|error| panic!("external owner failed to bind port {port}: {error}"));
    println!("{EXTERNAL_PORT_OWNER_PREFIX}{port}");
    io::stdout()
        .flush()
        .expect("external owner acknowledgement should flush");

    let mut parent_input = Vec::new();
    io::stdin()
        .read_to_end(&mut parent_input)
        .expect("external owner should wait for parent stdin to close");
}

#[test]
fn launch_plan_reuses_confirmed_stopped_managed_ssh_port() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let config = sample_config(&image_path);
    let paths = config.roots.paths("default");
    let first = MachineLaunchPlan::build(&paths, &config, &MachineStateRecord::initialized())
        .expect("first launch plan should build");
    first
        .ssh_port_lease()
        .activate_exact_loopback()
        .expect("exact provider observation should activate");
    let first_runtime = first.runtime().clone();
    drop(first);
    let cleanup = super::super::ports::withdraw_machine_ssh_port(&config.roots, &first_runtime)
        .expect("stop must fence before provider teardown")
        .expect("dead provider owner should yield exact cleanup authority");
    super::super::ports::retain_machine_ssh_port_after_confirmed_stop(cleanup)
        .expect("confirmed stop should retain the selected port");
    let mut state = MachineStateRecord::initialized();
    state.runtime = Some(first_runtime.clone());

    let restarted =
        MachineLaunchPlan::build(&paths, &config, &state).expect("restart plan should build");
    assert_eq!(restarted.runtime.ssh_port, first_runtime.ssh_port);
    assert_eq!(
        restarted.runtime.ssh_listener_id,
        first_runtime.ssh_listener_id
    );
}

#[test]
fn concurrent_machine_listener_reservations_receive_distinct_ports() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let roots = MachineRootLayout::test_sibling_roots(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let first = super::super::ports::PreparedMachineSshPortLease::prepare(
        &roots,
        "first",
        &MachineStateRecord::initialized(),
    )
    .expect("first machine listener should reserve");
    let second = super::super::ports::PreparedMachineSshPortLease::prepare(
        &roots,
        "second",
        &MachineStateRecord::initialized(),
    )
    .expect("second machine listener should reserve");

    assert_ne!(first.selected_port(), second.selected_port());
    assert!(managed_machine_port_range_contains(first.selected_port()));
    assert!(managed_machine_port_range_contains(second.selected_port()));
}

#[test]
fn machine_ssh_claim_precedes_provider_and_exact_observation_activates() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let roots = MachineRootLayout::test_sibling_roots(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let prepared = super::super::ports::PreparedMachineSshPortLease::prepare(
        &roots,
        "claim-before-provider",
        &MachineStateRecord::initialized(),
    )
    .expect("machine listener should prepare");
    assert_eq!(
        MachineProvider::Krunkit.network_management_mode(),
        NetworkManagementMode::NimbusHostManaged,
        "Nimbus owns the machine topology even though gvproxy owns the socket effect"
    );
    assert_eq!(
        prepared.effect_scope(),
        PortLeaseEffectScope::ProviderManaged,
        "the out-of-process gvproxy bind must retain provider-effect fencing"
    );
    let listener_id = prepared.listener_id().clone();
    let selected_port = prepared.selected_port();
    let authority =
        LocalPortLeaseAuthority::open(&roots.network_state_root).expect("authority should open");
    let claimed = authority
        .inspect(&PortLeaseId::for_listener(&listener_id))
        .expect("claimed lease should inspect")
        .expect("claimed lease should exist");
    assert_eq!(claimed.phase(), PortLeasePhase::Reserved);
    assert_eq!(
        claimed.reserved_port().map(|port| port.get()),
        Some(selected_port)
    );
    assert!(
        claimed.bind_claim().is_some(),
        "durable bind ownership must precede the provider effect"
    );

    prepared
        .activate_exact_loopback()
        .expect("exact readiness evidence should activate");
    let active = authority
        .inspect(&PortLeaseId::for_listener(&listener_id))
        .expect("active lease should inspect")
        .expect("active lease should exist");
    assert_eq!(active.phase(), PortLeasePhase::Active);
    let binding = active
        .binding()
        .expect("active lease must retain exact provider evidence");
    assert_eq!(binding.actual_port().get(), selected_port);
    assert_eq!(
        binding.endpoint().target().specific_address(),
        Some(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST))
    );
}

#[test]
fn pre_provider_failure_releases_claim_without_creating_an_effect() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let roots = MachineRootLayout::test_sibling_roots(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let prepared = super::super::ports::PreparedMachineSshPortLease::prepare(
        &roots,
        "pre-provider-failure",
        &MachineStateRecord::initialized(),
    )
    .expect("machine listener should prepare");
    let listener_id = prepared.listener_id().clone();

    prepared
        .abandon_before_provider_start()
        .expect("proven no-effect preparation should release");
    let released = LocalPortLeaseAuthority::open(&roots.network_state_root)
        .expect("authority should reopen")
        .inspect(&PortLeaseId::for_listener(&listener_id))
        .expect("released lease should inspect")
        .expect("released lease should remain as audit evidence");
    assert_eq!(released.phase(), PortLeasePhase::Released);
    assert!(released.bind_claim().is_none());
    assert!(released.binding().is_none());
}

#[test]
fn release_machine_ssh_port_releases_confirmed_stopped_authority() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let roots = MachineRootLayout::test_sibling_roots(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let prepared = super::super::ports::PreparedMachineSshPortLease::prepare(
        &roots,
        "default",
        &MachineStateRecord::initialized(),
    )
    .expect("machine listener should prepare");
    prepared
        .activate_exact_loopback()
        .expect("provider observation should activate");
    let runtime = MachineRuntimeState {
        helper_binaries: MachineHelperBinaryPaths {
            vmm: PathBuf::from("/opt/homebrew/bin/krunkit"),
            gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
        },
        image_path: PathBuf::from("/tmp/disk.raw"),
        efi_variable_store_path: PathBuf::from("/tmp/efi"),
        machine_image_source: "fixture".to_owned(),
        ssh_listener_id: prepared.listener_id().clone(),
        ssh_port: prepared.selected_port(),
        rest_uri: "unix:///tmp/vmm.sock".to_owned(),
        ready_vsock_port: READY_VSOCK_PORT,
    };
    drop(prepared);
    let cleanup = super::super::ports::withdraw_machine_ssh_port(&roots, &runtime)
        .expect("stop should fence the listener")
        .expect("dead provider owner should yield exact cleanup authority");
    super::super::ports::retain_machine_ssh_port_after_confirmed_stop(cleanup)
        .expect("confirmed stop should retain the lease");
    let mut state = MachineStateRecord::initialized();
    state.runtime = Some(runtime.clone());

    release_machine_ssh_port(&roots, &state).expect("port release should succeed");
    let authority =
        LocalPortLeaseAuthority::open(&roots.network_state_root).expect("authority should open");
    let record = authority
        .inspect(&PortLeaseId::for_listener(&runtime.ssh_listener_id))
        .expect("lease should inspect")
        .expect("lease should remain as terminal audit evidence");
    assert_eq!(record.phase(), PortLeasePhase::Released);
}

#[test]
fn refresh_machine_state_marks_missing_pids_as_stale() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::test_sibling_roots(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths("default");
    paths
        .ensure_runtime_directories()
        .expect("runtime directories should exist");

    let mut state = MachineStateRecord::initialized();
    state.lifecycle = MachineLifecycle::Running;
    state.manager = MachineManagerState::Ready;
    state.runtime = Some(MachineRuntimeState {
        helper_binaries: MachineHelperBinaryPaths {
            vmm: PathBuf::from("/opt/homebrew/bin/krunkit"),
            gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
        },
        image_path: PathBuf::from("/tmp/disk.raw"),
        efi_variable_store_path: paths.efi_variable_store_path.clone(),
        machine_image_source: "docker://quay.io/podman/machine-os@sha256:test".to_owned(),
        ssh_listener_id: fixture_machine_ssh_listener_id("refresh-stale"),
        ssh_port: 2222,
        rest_uri: format!("unix://{}", paths.vmm_endpoint_path.display()),
        ready_vsock_port: READY_VSOCK_PORT,
    });

    refresh_machine_state(&paths, &mut state).expect("refresh should succeed");

    assert_eq!(state.lifecycle, MachineLifecycle::Failed);
    assert_eq!(state.manager, MachineManagerState::Stale);
    assert!(
        state
            .last_error
            .expect("stale error should be present")
            .contains("vmm_alive=false")
    );
}

struct ExternalMachinePortOwner {
    child: std::process::Child,
    acknowledgement: std::sync::mpsc::Receiver<Result<u16, String>>,
    stdout_reader: Option<std::thread::JoinHandle<()>>,
    stderr: std::sync::Arc<std::sync::Mutex<String>>,
    stderr_reader: Option<std::thread::JoinHandle<()>>,
}

impl ExternalMachinePortOwner {
    fn spawn(port: u16) -> Result<Self, String> {
        let mut child = std::process::Command::new(
            std::env::current_exe()
                .map_err(|error| format!("failed to resolve current test executable: {error}"))?,
        )
        .arg("--exact")
        .arg(EXTERNAL_PORT_OWNER_CHILD_TEST)
        .arg("--ignored")
        .arg("--nocapture")
        .env(EXTERNAL_PORT_OWNER_ENV, port.to_string())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to spawn external port owner: {error}"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "external port owner stdout was not piped".to_owned())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "external port owner stderr was not piped".to_owned())?;

        let (acknowledgement_tx, acknowledgement) = std::sync::mpsc::sync_channel(1);
        let stdout_reader = std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(stdout);
            let mut captured = String::new();
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => {
                        let _ = acknowledgement_tx.send(Err(format!(
                            "external owner exited before acknowledgement; stdout={captured:?}"
                        )));
                        return;
                    }
                    Ok(_) => {
                        captured.push_str(&line);
                        if let Some(value) =
                            line.trim_end().strip_prefix(EXTERNAL_PORT_OWNER_PREFIX)
                        {
                            let result = value.parse::<u16>().map_err(|error| {
                                format!(
                                    "external owner emitted invalid bound port {value:?}: {error}"
                                )
                            });
                            let _ = acknowledgement_tx.send(result);
                            return;
                        }
                    }
                    Err(error) => {
                        let _ = acknowledgement_tx.send(Err(format!(
                            "failed to read external owner acknowledgement: {error}; stdout={captured:?}"
                        )));
                        return;
                    }
                }
            }
        });

        let stderr_capture = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let stderr_target = std::sync::Arc::clone(&stderr_capture);
        let stderr_reader = std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(stderr);
            let mut captured = String::new();
            if let Err(error) = reader.read_to_string(&mut captured) {
                captured.push_str(&format!("\n<stderr read failed: {error}>"));
            }
            *stderr_target
                .lock()
                .expect("external owner stderr lock should not be poisoned") = captured;
        });

        Ok(Self {
            child,
            acknowledgement,
            stdout_reader: Some(stdout_reader),
            stderr: stderr_capture,
            stderr_reader: Some(stderr_reader),
        })
    }

    fn wait_until_bound(&mut self, timeout: Duration) -> Result<u16, String> {
        match self.acknowledgement.recv_timeout(timeout) {
            Ok(result) => result,
            Err(error) => Err(format!(
                "external owner did not acknowledge within {timeout:?}: {error}; stderr={:?}",
                self.stderr
                    .lock()
                    .expect("external owner stderr lock should not be poisoned")
                    .as_str()
            )),
        }
    }
}

impl Drop for ExternalMachinePortOwner {
    fn drop(&mut self) {
        drop(self.child.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.stdout_reader.take() {
            let _ = reader.join();
        }
        if let Some(reader) = self.stderr_reader.take() {
            let _ = reader.join();
        }
    }
}
