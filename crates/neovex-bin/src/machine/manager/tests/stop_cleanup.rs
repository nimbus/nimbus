use super::*;

#[test]
fn stop_machine_uses_graceful_krunkit_stop_before_cleaning_up_helpers() {
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

    let (krunkit_pid, krunkit_reaper) = spawn_reaped_process("exec sleep 30");
    let (gvproxy_pid, gvproxy_reaper) = spawn_reaped_process("exec sleep 30");
    fs::write(&paths.krunkit_pid_path, krunkit_pid.to_string()).expect("krunkit pid should write");
    fs::write(&paths.gvproxy_pid_path, gvproxy_pid.to_string()).expect("gvproxy pid should write");

    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let requests_for_server = std::sync::Arc::clone(&requests);
    let endpoint_path = paths.krunkit_endpoint_path.clone();
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
        let _ = send_signal(krunkit_pid, SIGKILL);
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

    let mut state = MachineStateRecord::initialized();
    state.lifecycle = MachineLifecycle::Running;
    state.manager = MachineManagerState::Ready;
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

    stop_machine(&paths, &config, &mut state).expect("machine stop should succeed");
    server.join().expect("endpoint server should finish");

    assert_eq!(
        requests.lock().expect("request log should lock").clone(),
        vec!["Stop".to_owned()]
    );
    assert_eq!(state.lifecycle, MachineLifecycle::Stopped);
    assert_eq!(state.manager, MachineManagerState::HelpersResolved);
    assert_eq!(state.last_error, None);
    assert!(
        wait_for_pid_exit(krunkit_pid, Duration::from_secs(2))
            .expect("krunkit pid should become not alive"),
        "krunkit process should exit during graceful provider stop"
    );
    assert!(
        wait_for_pid_exit(gvproxy_pid, Duration::from_secs(2))
            .expect("gvproxy pid should become not alive"),
        "gvproxy process should be stopped during cleanup"
    );
    krunkit_reaper
        .join()
        .expect("krunkit reaper should observe process exit");
    gvproxy_reaper
        .join()
        .expect("gvproxy reaper should observe process exit");
}

#[test]
fn request_krunkit_state_change_sends_hard_stop_payload() {
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

    request_krunkit_state_change(&request_path, "HardStop")
        .expect("hard-stop request should succeed");
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
