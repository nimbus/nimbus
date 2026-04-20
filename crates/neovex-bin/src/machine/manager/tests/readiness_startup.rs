use super::*;

#[test]
fn ensure_guest_neovex_socket_shell_repairs_first_boot_failures() {
    let script = ensure_guest_neovex_socket_shell_script();

    assert!(script.contains("systemctl daemon-reload"), "{script}");
    assert!(
        script.contains("systemctl stop neovex.service neovex.socket"),
        "{script}"
    );
    assert!(
        script.contains("systemctl reset-failed neovex.service neovex.socket"),
        "{script}"
    );
    assert!(script.contains("systemctl start neovex.socket"), "{script}");
    assert!(script.contains(GUEST_NEOVEX_SOCKET), "{script}");
}

#[test]
fn annotate_machine_start_error_hints_when_guest_reaches_login_prompt() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let config = sample_config(&image_path);
    let paths = config.roots.paths("default");
    paths
        .ensure_directories()
        .expect("machine directories should exist");
    fs::write(&paths.machine_log_path, "Fedora Linux 42\nfedora login:\n")
        .expect("machine log should write");

    let error = annotate_machine_start_error(
        &paths,
        &config,
        Error::Internal(
            "gvproxy exited before machine readiness with status exit status: 0".to_owned(),
        ),
    );

    let message = error.to_string();
    assert!(message.contains("gvproxy exited before machine readiness"));
    assert!(message.contains("guest reached a console login prompt"));
    assert!(message.contains("generic fedora-bootc raw images"));
}

#[test]
fn annotate_machine_start_error_leaves_unrelated_failures_unchanged() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let config = sample_config(&image_path);
    let paths = config.roots.paths("default");
    paths
        .ensure_directories()
        .expect("machine directories should exist");
    fs::write(&paths.machine_log_path, "Fedora Linux 42\nfedora login:\n")
        .expect("machine log should write");

    let error = annotate_machine_start_error(
        &paths,
        &config,
        Error::Internal("failed to resolve machine guest OCI reference".to_owned()),
    );

    assert_eq!(
        error.to_string(),
        "internal error: failed to resolve machine guest OCI reference"
    );
}

#[test]
fn ssh_port_is_listening_detects_local_listener() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let port = listener
        .local_addr()
        .expect("listener address should resolve")
        .port();

    assert!(ssh_port_is_listening(port));
}

#[test]
fn wait_for_ssh_ready_accepts_listening_port_without_identity_probe() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let port = listener
        .local_addr()
        .expect("listener address should resolve")
        .port();
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let config = sample_config(&image_path);
    let mut gvproxy_child = MachineCommandLine {
        program: PathBuf::from("/bin/sh"),
        args: vec!["-c".to_owned(), "sleep 30".to_owned()],
    }
    .spawn()
    .expect("gvproxy probe child should spawn");
    let mut krunkit_child = MachineCommandLine {
        program: PathBuf::from("/bin/sh"),
        args: vec!["-c".to_owned(), "sleep 30".to_owned()],
    }
    .spawn()
    .expect("krunkit probe child should spawn");

    let result = wait_for_ssh_ready(
        &config,
        port,
        Duration::from_secs(1),
        &mut krunkit_child,
        &mut gvproxy_child,
        &StartupSignalMonitor::inactive_for_test(),
    );

    cleanup_process(&mut krunkit_child).expect("krunkit probe child should clean up");
    cleanup_process(&mut gvproxy_child).expect("gvproxy probe child should clean up");
    drop(listener);

    assert!(result.is_ok(), "listener-backed SSH readiness should pass");
}

#[test]
fn wait_for_path_returns_cancelled_when_startup_signal_is_set() {
    let _guard = machine_lifecycle_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let path = temp_dir.path().join("gvproxy.sock");
    let mut child = MachineCommandLine {
        program: PathBuf::from("/bin/sh"),
        args: vec!["-c".to_owned(), "sleep 30".to_owned()],
    }
    .spawn()
    .expect("probe child should spawn");

    let result = wait_for_path(
        &path,
        Duration::from_secs(1),
        &mut child,
        &StartupSignalMonitor::interrupted_for_test(),
    );

    cleanup_process(&mut child).expect("probe child should clean up");

    assert!(matches!(result, Err(Error::Cancelled)));
}

#[test]
fn interrupted_start_transitions_to_stopped_and_cleans_runtime_artifacts() {
    let _guard = machine_lifecycle_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let config = sample_config(&image_path);
    let paths = config.roots.paths("default");
    paths
        .ensure_directories()
        .expect("machine directories should exist");

    for path in [
        &paths.ready_socket_path,
        &paths.ignition_socket_path,
        &paths.api_socket_path,
        &paths.gvproxy_socket_path,
        &paths.krunkit_endpoint_path,
        &paths.gvproxy_pid_path,
        &paths.krunkit_pid_path,
    ] {
        fs::write(path, b"artifact").expect("runtime artifact should write");
    }
    for path in [
        &paths.machine_log_path,
        &paths.krunkit_log_path,
        &paths.gvproxy_log_path,
    ] {
        fs::write(path, b"non-empty").expect("log artifact should write");
    }

    let mut krunkit_child = MachineCommandLine {
        program: PathBuf::from("/bin/sh"),
        args: vec!["-c".to_owned(), "sleep 30".to_owned()],
    }
    .spawn()
    .expect("krunkit child should spawn");
    let mut gvproxy_child = MachineCommandLine {
        program: PathBuf::from("/bin/sh"),
        args: vec!["-c".to_owned(), "sleep 30".to_owned()],
    }
    .spawn()
    .expect("gvproxy child should spawn");

    let mut state = MachineStateRecord::initialized();
    state.lifecycle = MachineLifecycle::Starting;
    state.manager = MachineManagerState::Launching;
    state.runtime = Some(MachineRuntimeState {
        helper_binaries: MachineHelperBinaryPaths {
            krunkit: PathBuf::from("/opt/homebrew/bin/krunkit"),
            gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
        },
        image_path,
        efi_variable_store_path: paths.efi_variable_store_path.clone(),
        machine_image_source: describe_machine_image_source(&config.guest.image_source),
        ssh_port: 20022,
        rest_uri: format!("unix://{}", paths.krunkit_endpoint_path.display()),
        ready_vsock_port: READY_VSOCK_PORT,
    });

    let result = handle_start_machine_error(
        &paths,
        &config,
        &mut state,
        Error::Cancelled,
        Some(&mut krunkit_child),
        Some(&mut gvproxy_child),
    );

    assert!(matches!(result, Err(Error::Cancelled)));
    assert_eq!(state.lifecycle, MachineLifecycle::Stopped);
    assert_eq!(state.manager, MachineManagerState::HelpersResolved);
    assert_eq!(state.last_error, None);
    assert!(
        krunkit_child
            .try_wait()
            .expect("krunkit child status should resolve")
            .is_some(),
        "krunkit child should be reaped on interrupted startup"
    );
    assert!(
        gvproxy_child
            .try_wait()
            .expect("gvproxy child status should resolve")
            .is_some(),
        "gvproxy child should be reaped on interrupted startup"
    );
    for path in [
        &paths.ready_socket_path,
        &paths.ignition_socket_path,
        &paths.api_socket_path,
        &paths.gvproxy_socket_path,
        &paths.krunkit_endpoint_path,
        &paths.gvproxy_pid_path,
        &paths.krunkit_pid_path,
    ] {
        assert!(
            !path.exists(),
            "runtime artifact {} should be removed",
            path.display()
        );
    }
    for path in [
        &paths.machine_log_path,
        &paths.krunkit_log_path,
        &paths.gvproxy_log_path,
    ] {
        assert_eq!(
            fs::read(path).expect("log artifact should remain readable"),
            Vec::<u8>::new(),
            "log artifact {} should be truncated",
            path.display()
        );
    }
}
