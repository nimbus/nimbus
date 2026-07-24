use super::super::ports::allocate_machine_ssh_port;
use super::*;
use std::io::{self, BufRead as _};

const EXTERNAL_PORT_OWNER_CHILD_TEST: &str =
    "machine::manager::tests::ports_state::external_machine_port_owner_child";
const EXTERNAL_PORT_OWNER_ENV: &str = "NIMBUS_MACHINE_EXTERNAL_PORT_OWNER";
const EXTERNAL_PORT_OWNER_PREFIX: &str = "NIMBUS_MACHINE_EXTERNAL_PORT_OWNER/1\tbound:";

#[test]
#[ignore = "NNC0.2 expected red until machine listeners consume host-global port leases"]
fn external_binder_after_probe_blocks_provider_while_machine_state_claims_port() {
    let _lifecycle_guard = machine_lifecycle_test_lock()
        .lock()
        .expect("machine lifecycle test lock should not be poisoned");
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let roots = MachineRootLayout::test_sibling_roots(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );

    let allocated_port =
        allocate_machine_ssh_port(&roots, "race-window", &MachineStateRecord::initialized())
            .expect("Nimbus should probe, drop, and persist a machine port");
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

    let persisted = load_machine_port_allocation_state(&roots)
        .expect("machine allocation state should remain readable");
    assert_ne!(
        persisted.machine_ports.get("race-window"),
        Some(&allocated_port),
        "machine state must not retain a port claim after an external owner wins the bind"
    );
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
fn launch_plan_reuses_recorded_managed_ssh_port_when_available() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let config = sample_config(&image_path);
    let paths = config.roots.paths("default");
    let mut state = MachineStateRecord::initialized();
    state.runtime = Some(MachineRuntimeState {
        helper_binaries: MachineHelperBinaryPaths {
            vmm: PathBuf::from("/opt/homebrew/bin/krunkit"),
            gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
        },
        image_path: image_path.clone(),
        efi_variable_store_path: paths.efi_variable_store_path.clone(),
        machine_image_source: describe_machine_image_source(&config.guest.image_source),
        ssh_port: 20022,
        rest_uri: format!("unix://{}", paths.vmm_endpoint_path.display()),
        ready_vsock_port: READY_VSOCK_PORT,
    });

    let plan = MachineLaunchPlan::build(&paths, &config, &state).expect("launch plan should build");
    let allocation_state = load_machine_port_allocation_state(&config.roots)
        .expect("port allocation state should load");

    assert_eq!(plan.runtime.ssh_port, 20022);
    assert_eq!(allocation_state.machine_ports.get("default"), Some(&20022));
}

#[test]
fn launch_plan_reassigns_recorded_ssh_port_when_it_is_busy() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let config = sample_config(&image_path);
    let paths = config.roots.paths("default");
    let listener = TcpListener::bind("127.0.0.1:20023")
        .or_else(|_| TcpListener::bind("127.0.0.1:0"))
        .expect("listener should bind");
    let busy_port = listener
        .local_addr()
        .expect("listener address should resolve")
        .port();
    let mut state = MachineStateRecord::initialized();
    state.runtime = Some(MachineRuntimeState {
        helper_binaries: MachineHelperBinaryPaths {
            vmm: PathBuf::from("/opt/homebrew/bin/krunkit"),
            gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
        },
        image_path: image_path.clone(),
        efi_variable_store_path: paths.efi_variable_store_path.clone(),
        machine_image_source: describe_machine_image_source(&config.guest.image_source),
        ssh_port: busy_port,
        rest_uri: format!("unix://{}", paths.vmm_endpoint_path.display()),
        ready_vsock_port: READY_VSOCK_PORT,
    });

    let plan = MachineLaunchPlan::build(&paths, &config, &state).expect("launch plan should build");
    let allocation_state = load_machine_port_allocation_state(&config.roots)
        .expect("port allocation state should load");

    assert_ne!(plan.runtime.ssh_port, busy_port);
    assert!(managed_machine_port_range_contains(plan.runtime.ssh_port));
    assert_eq!(
        allocation_state.machine_ports.get("default"),
        Some(&plan.runtime.ssh_port)
    );
}

#[test]
fn release_machine_ssh_port_removes_reserved_port() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let roots = MachineRootLayout::test_sibling_roots(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    with_port_allocation_lock(&roots, || {
        let mut state = load_machine_port_allocation_state(&roots)?;
        state.machine_ports.insert("default".to_owned(), 20024);
        write_machine_port_allocation_state(&roots, &state)
    })
    .expect("reserved machine port should write");

    release_machine_ssh_port(&roots, "default").expect("port release should succeed");

    let allocation_state =
        load_machine_port_allocation_state(&roots).expect("allocation state should load");
    assert!(allocation_state.machine_ports.is_empty());
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
