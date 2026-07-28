use super::*;

#[test]
fn stop_machine_uses_graceful_vmm_stop_before_cleaning_up_helpers() {
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

    let (vmm_pid, vmm_reaper) = spawn_reaped_process("exec sleep 30");
    let (gvproxy_pid, gvproxy_reaper) = spawn_reaped_process("exec sleep 30");
    fs::write(&paths.vmm_pid_path, vmm_pid.to_string()).expect("machine VMM pid should write");
    fs::write(&paths.gvproxy_pid_path, gvproxy_pid.to_string()).expect("gvproxy pid should write");

    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let requests_for_server = std::sync::Arc::clone(&requests);
    let endpoint_path = paths.vmm_endpoint_path.clone();
    let request_path = endpoint_path.clone();
    let server = thread::spawn(move || {
        let listener = UnixListener::bind(&endpoint_path).expect("endpoint listener should bind");
        let (mut stream, _) = listener.accept().expect("endpoint should accept request");
        let mut buffer = [0_u8; 1024];
        let read = stream.read(&mut buffer).expect("request should read");
        let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
        let state = if request.contains("\"HardStop\"") {
            "HardStop"
        } else {
            "Stop"
        };
        requests_for_server
            .lock()
            .expect("request log should lock")
            .push(state.to_owned());
        let _ = send_signal(vmm_pid, SIGKILL);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .expect("response should write");
        stream.flush().expect("response should flush");
    });

    let deadline = Instant::now() + Duration::from_secs(1);
    while !request_path.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(request_path.exists(), "endpoint should appear before stop");

    let prepared = super::super::ports::PreparedMachineSshPortLease::prepare(
        &config.roots,
        &config.name,
        &MachineStateRecord::initialized(),
    )
    .expect("machine SSH listener should prepare");
    prepared
        .activate_exact_loopback()
        .expect("exact provider observation should activate");
    let mut state = MachineStateRecord::initialized();
    state.lifecycle = MachineLifecycle::Running;
    state.manager = MachineManagerState::Ready;
    state.runtime = Some(MachineRuntimeState {
        helper_binaries: MachineHelperBinaryPaths {
            vmm: PathBuf::from("/opt/homebrew/bin/krunkit"),
            gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
        },
        image_path,
        efi_variable_store_path: paths.efi_variable_store_path.clone(),
        machine_image_source: describe_machine_image_source(&config.guest.image_source),
        ssh_listener_id: prepared.listener_id().clone(),
        ssh_port: prepared.selected_port(),
        rest_uri: format!("unix://{}", paths.vmm_endpoint_path.display()),
        ready_vsock_port: READY_VSOCK_PORT,
    });
    // Launch owns the provider lifetime only until it hands the running
    // machine back to its caller. Model that handoff before stop attempts
    // dead-owner recovery.
    drop(prepared);

    stop_machine(&paths, &config, &mut state).expect("machine stop should succeed");
    server.join().expect("endpoint server should finish");

    assert_eq!(
        requests.lock().expect("request log should lock").clone(),
        vec!["Stop".to_owned()]
    );
    assert_eq!(state.lifecycle, MachineLifecycle::Stopped);
    assert_eq!(state.manager, MachineManagerState::HelpersResolved);
    assert_eq!(state.last_error, None);
    let lease = nimbus_network::LocalPortLeaseAuthority::open(&config.roots.network_state_root)
        .expect("shared authority should reopen")
        .inspect(&nimbus_network::PortLeaseId::for_listener(
            &state
                .runtime
                .as_ref()
                .expect("stopped runtime should remain")
                .ssh_listener_id,
        ))
        .expect("machine SSH lease should inspect")
        .expect("machine SSH lease should remain durable");
    assert_eq!(lease.phase(), nimbus_network::PortLeasePhase::Reserved);
    assert!(
        lease.confirmed_stopped_binding().is_some(),
        "confirmed gvproxy stop should retain exact absence evidence"
    );
    assert!(
        wait_for_pid_exit(vmm_pid, Duration::from_secs(2))
            .expect("machine VMM pid should become not alive"),
        "machine VMM process should exit during graceful provider stop"
    );
    assert!(
        wait_for_pid_exit(gvproxy_pid, Duration::from_secs(2))
            .expect("gvproxy pid should become not alive"),
        "gvproxy process should be stopped during cleanup"
    );
    vmm_reaper
        .join()
        .expect("machine VMM reaper should observe process exit");
    gvproxy_reaper
        .join()
        .expect("gvproxy reaper should observe process exit");
}

#[test]
fn ambiguous_gvproxy_stop_preserves_runtime_evidence_and_port_fence() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let config = sample_config(&image_path);
    let paths = config.roots.paths("default");
    paths
        .ensure_directories()
        .expect("machine directories should exist");
    fs::write(&paths.gvproxy_log_path, b"provider evidence")
        .expect("provider evidence should write");

    let prepared = super::super::ports::PreparedMachineSshPortLease::prepare(
        &config.roots,
        &config.name,
        &MachineStateRecord::initialized(),
    )
    .expect("machine SSH listener should prepare");
    prepared
        .activate_exact_loopback()
        .expect("exact provider observation should activate");
    let mut state = MachineStateRecord::initialized();
    state.lifecycle = MachineLifecycle::Running;
    state.manager = MachineManagerState::Ready;
    state.runtime = Some(MachineRuntimeState {
        helper_binaries: MachineHelperBinaryPaths {
            vmm: PathBuf::from("/opt/homebrew/bin/krunkit"),
            gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
        },
        image_path,
        efi_variable_store_path: paths.efi_variable_store_path.clone(),
        machine_image_source: describe_machine_image_source(&config.guest.image_source),
        ssh_listener_id: prepared.listener_id().clone(),
        ssh_port: prepared.selected_port(),
        rest_uri: format!("unix://{}", paths.vmm_endpoint_path.display()),
        ready_vsock_port: READY_VSOCK_PORT,
    });
    // The launch-side lifetime must be gone before stop can prove exclusive
    // recovery authority for the provider generation.
    drop(prepared);

    let error = stop_machine(&paths, &config, &mut state)
        .expect_err("missing exact gvproxy stop evidence must fail closed");
    assert!(error.to_string().contains("stop is incomplete"));
    assert_eq!(state.lifecycle, MachineLifecycle::Failed);
    assert_eq!(state.manager, MachineManagerState::Stale);
    assert!(
        state
            .last_error
            .as_deref()
            .is_some_and(|message| message.contains("gvproxy pid evidence is missing"))
    );
    assert_eq!(
        fs::read(&paths.gvproxy_log_path).expect("provider evidence should remain"),
        b"provider evidence",
        "ambiguous stop must not erase evidence needed by reconciliation"
    );
    let lease = nimbus_network::LocalPortLeaseAuthority::open(&config.roots.network_state_root)
        .expect("shared authority should reopen")
        .inspect(&nimbus_network::PortLeaseId::for_listener(
            &state
                .runtime
                .as_ref()
                .expect("failed runtime should remain")
                .ssh_listener_id,
        ))
        .expect("machine SSH lease should inspect")
        .expect("machine SSH lease should remain durable");
    assert_eq!(
        lease.phase(),
        nimbus_network::PortLeasePhase::CleanupPending,
        "dead-owner recovery must quarantine the exact provider generation before stop"
    );
    assert!(
        lease.binding().is_some(),
        "ambiguous stop must retain exact prior provider evidence"
    );
}

#[test]
fn request_vmm_state_change_sends_hard_stop_payload() {
    let _guard = machine_lifecycle_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let endpoint_path = temp_dir.path().join("krunkit.sock");
    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let requests_for_server = std::sync::Arc::clone(&requests);
    let request_path = endpoint_path.clone();
    let server = thread::spawn(move || {
        let listener = UnixListener::bind(&endpoint_path).expect("endpoint listener should bind");
        let (mut stream, _) = listener.accept().expect("endpoint should accept request");
        let mut buffer = [0_u8; 1024];
        let read = stream.read(&mut buffer).expect("request should read");
        let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
        let state = if request.contains("\"HardStop\"") {
            "HardStop"
        } else {
            "Stop"
        };
        requests_for_server
            .lock()
            .expect("request log should lock")
            .push(state.to_owned());
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .expect("response should write");
        stream.flush().expect("response should flush");
    });

    let deadline = Instant::now() + Duration::from_secs(1);
    while !request_path.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        request_path.exists(),
        "endpoint should appear before request"
    );

    request_vmm_state_change(&request_path, "HardStop").expect("hard-stop request should succeed");
    server.join().expect("endpoint server should finish");

    assert_eq!(
        requests.lock().expect("request log should lock").clone(),
        vec!["HardStop".to_owned()]
    );
}

#[test]
fn wait_for_pid_exit_reports_timeout_while_process_is_still_running() {
    let _guard = machine_lifecycle_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let (pid, reaper) = spawn_reaped_process("exec sleep 30");

    assert!(
        !wait_for_pid_exit(pid, Duration::from_millis(50))
            .expect("wait should report timeout for a running process")
    );

    force_stop_pid(pid, Duration::from_secs(2)).expect("force stop should succeed");
    reaper
        .join()
        .expect("process reaper should observe process exit");
}
