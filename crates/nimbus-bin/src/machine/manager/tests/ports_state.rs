use super::*;

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
            krunkit: PathBuf::from("/opt/homebrew/bin/krunkit"),
            gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
        },
        image_path: image_path.clone(),
        efi_variable_store_path: paths.efi_variable_store_path.clone(),
        machine_image_source: describe_machine_image_source(&config.guest.image_source),
        ssh_port: 20022,
        rest_uri: format!("unix://{}", paths.krunkit_endpoint_path.display()),
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
            krunkit: PathBuf::from("/opt/homebrew/bin/krunkit"),
            gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
        },
        image_path: image_path.clone(),
        efi_variable_store_path: paths.efi_variable_store_path.clone(),
        machine_image_source: describe_machine_image_source(&config.guest.image_source),
        ssh_port: busy_port,
        rest_uri: format!("unix://{}", paths.krunkit_endpoint_path.display()),
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
    let roots = MachineRootLayout::new(
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
    let layout = MachineRootLayout::new(
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
            krunkit: PathBuf::from("/opt/homebrew/bin/krunkit"),
            gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
        },
        image_path: PathBuf::from("/tmp/disk.raw"),
        efi_variable_store_path: paths.efi_variable_store_path.clone(),
        machine_image_source: "docker://quay.io/podman/machine-os@sha256:test".to_owned(),
        ssh_port: 2222,
        rest_uri: format!("unix://{}", paths.krunkit_endpoint_path.display()),
        ready_vsock_port: READY_VSOCK_PORT,
    });

    refresh_machine_state(&paths, &mut state).expect("refresh should succeed");

    assert_eq!(state.lifecycle, MachineLifecycle::Failed);
    assert_eq!(state.manager, MachineManagerState::Stale);
    assert!(
        state
            .last_error
            .expect("stale error should be present")
            .contains("krunkit_alive=false")
    );
}
