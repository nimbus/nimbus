use super::*;

#[test]
fn machine_stop_withdraws_all_publications_before_provider_and_releases_only_after_exact_absence() {
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

    let network = test_machine_network_lifecycle(temp_dir.path());
    let port_authority = network.port_leases();
    let publication_store =
        crate::machine::publication_authority::MachinePublicationIntentStore::open(
            port_authority.state_root(),
        )
        .expect("parent publication store should open");
    let forwarder_authority = test_forwarder_authority(&config);
    let first = activate_parent_publication(
        &publication_store,
        &port_authority,
        &forwarder_authority,
        "tenant-machine-stop-a",
        "api",
        42181,
    );
    let second = activate_parent_publication(
        &publication_store,
        &port_authority,
        &forwarder_authority,
        "tenant-machine-stop-b",
        "worker",
        42182,
    );

    let (vmm_pid, vmm_reaper) = spawn_reaped_process("exec sleep 30");
    let (gvproxy_pid, gvproxy_reaper) = spawn_reaped_process("exec sleep 30");
    fs::write(&paths.vmm_pid_path, vmm_pid.to_string()).expect("machine VMM pid should write");
    fs::write(&paths.gvproxy_pid_path, gvproxy_pid.to_string()).expect("gvproxy pid should write");

    let publications_withdrawn_before_provider =
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed = std::sync::Arc::clone(&publications_withdrawn_before_provider);
    let observed_authority = port_authority.clone();
    let observed_plans = [first.plan_id.clone(), second.plan_id.clone()];
    let endpoint_path = paths.vmm_endpoint_path.clone();
    let request_path = endpoint_path.clone();
    let server = thread::spawn(move || {
        let listener = UnixListener::bind(&endpoint_path).expect("endpoint listener should bind");
        let (mut stream, _) = listener.accept().expect("endpoint should accept request");
        let mut buffer = [0_u8; 1024];
        let read = stream.read(&mut buffer).expect("request should read");
        let request = String::from_utf8_lossy(&buffer[..read]);
        assert!(
            request.contains("\"Stop\""),
            "provider should receive graceful stop first: {request}"
        );
        let all_withdrawn = observed_plans.iter().all(|plan_id| {
            observed_authority.list_plan(plan_id).is_ok_and(|records| {
                !records.is_empty()
                    && records.iter().all(|record| {
                        matches!(
                            record.phase(),
                            PortLeasePhase::Withdrawing | PortLeasePhase::CleanupPending
                        )
                    })
            })
        });
        observed.store(all_withdrawn, std::sync::atomic::Ordering::SeqCst);
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
        port_authority.clone(),
        &config.name,
        &MachineStateRecord::initialized(),
    )
    .expect("machine SSH listener should prepare");
    prepared
        .activate_exact_loopback()
        .expect("exact provider observation should activate");
    let mut state = running_machine_state(&config, &paths, &image_path, &prepared);
    drop(prepared);
    write_exact_gvproxy_process_receipt(&paths, &state, gvproxy_pid);

    super::super::stop::stop_machine(&network, &paths, &config, &mut state)
        .expect("exact provider stop should converge");
    server.join().expect("endpoint server should finish");

    assert!(
        publications_withdrawn_before_provider.load(std::sync::atomic::Ordering::SeqCst),
        "every exact-incarnation parent publication must be fenced before provider stop I/O"
    );
    for intent in [&first, &second] {
        assert!(
            port_authority
                .list_plan(&intent.plan_id)
                .expect("publication plan should inspect")
                .iter()
                .all(|record| record.phase() == PortLeasePhase::Released),
            "confirmed gvproxy absence must release publication plan {}",
            intent.plan_id
        );
        assert_eq!(
            publication_store
                .load_plan(&intent.plan_id)
                .expect("publication intent should inspect")
                .expect("publication intent should remain durable")
                .phase,
            crate::machine::publication_authority::MachinePublicationIntentPhase::Terminal
        );
    }

    assert!(
        wait_for_pid_exit(vmm_pid, Duration::from_secs(2))
            .expect("machine VMM pid should become not alive")
    );
    assert!(
        wait_for_pid_exit(gvproxy_pid, Duration::from_secs(2))
            .expect("gvproxy pid should become not alive")
    );
    vmm_reaper
        .join()
        .expect("machine VMM reaper should observe process exit");
    gvproxy_reaper
        .join()
        .expect("gvproxy reaper should observe process exit");

    state.lifecycle = MachineLifecycle::Failed;
    state.manager = MachineManagerState::Stale;
    state.last_error = Some("simulated post-absence artifact cleanup interruption".to_owned());
    fs::write(&paths.gvproxy_pid_path, i32::MAX.to_string()).expect("stale retry pid should write");
    super::super::stop::stop_machine(&network, &paths, &config, &mut state)
        .expect("durable exact absence should make retry independent of stale pid evidence");
    assert_eq!(state.lifecycle, MachineLifecycle::Stopped);
    assert!(
        !paths.gvproxy_pid_path.exists(),
        "converged retry should remove the stale runtime artifact without signaling it"
    );
}

#[test]
fn exact_gvproxy_absence_settles_network_authority_despite_unrelated_stop_error() {
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

    let network = test_machine_network_lifecycle(temp_dir.path());
    let port_authority = network.port_leases();
    let publication_store =
        crate::machine::publication_authority::MachinePublicationIntentStore::open(
            port_authority.state_root(),
        )
        .expect("parent publication store should open");
    let publication = activate_parent_publication(
        &publication_store,
        &port_authority,
        &test_forwarder_authority(&config),
        "tenant-machine-stop-independent-errors",
        "api",
        42186,
    );
    let prepared = super::super::ports::PreparedMachineSshPortLease::prepare(
        port_authority.clone(),
        &config.name,
        &MachineStateRecord::initialized(),
    )
    .expect("machine SSH listener should prepare");
    prepared
        .activate_exact_loopback()
        .expect("exact provider observation should activate");
    let ssh_lease_id = nimbus_network::PortLeaseId::for_listener(prepared.listener_id());
    let mut state = running_machine_state(&config, &paths, &image_path, &prepared);
    drop(prepared);

    let (gvproxy_pid, gvproxy_reaper) = spawn_reaped_process("exec sleep 30");
    fs::write(&paths.gvproxy_pid_path, gvproxy_pid.to_string()).expect("gvproxy pid should write");
    write_exact_gvproxy_process_receipt(&paths, &state, gvproxy_pid);
    fs::write(&paths.api_forward_pid_path, "not-a-pid")
        .expect("independent API-forward diagnostic should write");

    let error = super::super::stop::stop_machine(&network, &paths, &config, &mut state)
        .expect_err("unrelated API-forward cleanup error should remain visible");
    assert!(
        error.to_string().contains("failed to parse pid file"),
        "the unrelated cleanup failure must still be reported: {error}"
    );
    assert_eq!(state.lifecycle, MachineLifecycle::Failed);
    assert!(
        wait_for_pid_exit(gvproxy_pid, Duration::from_secs(2))
            .expect("gvproxy pid should become not alive")
    );
    gvproxy_reaper
        .join()
        .expect("gvproxy reaper should observe process exit");

    let publication_records = port_authority
        .list_plan(&publication.plan_id)
        .expect("publication plan should inspect");
    assert!(
        publication_records
            .iter()
            .all(|record| record.phase() == PortLeasePhase::Released),
        "exact gvproxy absence must settle publications independently of unrelated errors: \
         records={publication_records:?}; stop_error={error}"
    );
    assert_eq!(
        publication_store
            .load_plan(&publication.plan_id)
            .expect("publication intent should inspect")
            .expect("publication intent should remain durable")
            .phase,
        crate::machine::publication_authority::MachinePublicationIntentPhase::Terminal
    );
    let ssh_lease = port_authority
        .inspect(&ssh_lease_id)
        .expect("SSH lease should inspect")
        .expect("SSH lease should remain durable");
    assert_eq!(ssh_lease.phase(), PortLeasePhase::Reserved);
    assert!(
        ssh_lease.confirmed_stopped_binding().is_some(),
        "exact gvproxy absence must be durable even while another cleanup retries"
    );
}

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

    let network = test_machine_network_lifecycle(temp_dir.path());
    let port_authority = network.port_leases();
    let prepared = super::super::ports::PreparedMachineSshPortLease::prepare(
        port_authority.clone(),
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
        forwarder_authority: test_forwarder_authority(&config),
        ssh_port: prepared.selected_port(),
        rest_uri: format!("unix://{}", paths.vmm_endpoint_path.display()),
        ready_vsock_port: READY_VSOCK_PORT,
    });
    // Launch owns the provider lifetime only until it hands the running
    // machine back to its caller. Model that handoff before stop attempts
    // dead-owner recovery.
    drop(prepared);
    write_exact_gvproxy_process_receipt(&paths, &state, gvproxy_pid);

    super::super::stop::stop_machine(&network, &paths, &config, &mut state)
        .expect("machine stop should succeed");
    server.join().expect("endpoint server should finish");

    assert_eq!(
        requests.lock().expect("request log should lock").clone(),
        vec!["Stop".to_owned()]
    );
    assert_eq!(state.lifecycle, MachineLifecycle::Stopped);
    assert_eq!(state.manager, MachineManagerState::HelpersResolved);
    assert_eq!(state.last_error, None);
    let lease = port_authority
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
    let mut config = sample_config(&image_path);
    let paths = config.roots.paths("default");
    paths
        .ensure_directories()
        .expect("machine directories should exist");
    fs::write(&paths.gvproxy_log_path, b"provider evidence")
        .expect("provider evidence should write");

    let network = test_machine_network_lifecycle(temp_dir.path());
    let port_authority = network.port_leases();
    let publication_store =
        crate::machine::publication_authority::MachinePublicationIntentStore::open(
            port_authority.state_root(),
        )
        .expect("parent publication store should open");
    let publication = activate_parent_publication(
        &publication_store,
        &port_authority,
        &test_forwarder_authority(&config),
        "tenant-machine-stop-ambiguous",
        "api",
        42183,
    );
    let prepared = super::super::ports::PreparedMachineSshPortLease::prepare(
        port_authority.clone(),
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
        forwarder_authority: test_forwarder_authority(&config),
        ssh_port: prepared.selected_port(),
        rest_uri: format!("unix://{}", paths.vmm_endpoint_path.display()),
        ready_vsock_port: READY_VSOCK_PORT,
    });
    // The launch-side lifetime must be gone before stop can prove exclusive
    // recovery authority for the provider generation.
    drop(prepared);

    let error = super::super::stop::stop_machine(&network, &paths, &config, &mut state)
        .expect_err("missing exact gvproxy stop evidence must fail closed");
    assert!(error.to_string().contains("stop is incomplete"));
    assert_eq!(state.lifecycle, MachineLifecycle::Failed);
    assert_eq!(state.manager, MachineManagerState::Stale);
    assert!(
        state
            .last_error
            .as_deref()
            .is_some_and(|message| message.contains("process identity evidence is missing"))
    );
    assert_eq!(
        fs::read(&paths.gvproxy_log_path).expect("provider evidence should remain"),
        b"provider evidence",
        "ambiguous stop must not erase evidence needed by reconciliation"
    );
    let lease = port_authority
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
    let publication_records = port_authority
        .list_plan(&publication.plan_id)
        .expect("ambiguous publication should inspect");
    assert!(
        publication_records
            .iter()
            .all(|record| record.phase() == PortLeasePhase::CleanupPending),
        "missing gvproxy identity must retain every parent publication in cleanup-pending"
    );
    assert_eq!(
        publication_store
            .load_plan(&publication.plan_id)
            .expect("publication intent should inspect")
            .expect("publication intent should remain durable")
            .phase,
        crate::machine::publication_authority::MachinePublicationIntentPhase::Committed,
        "ambiguous provider absence must not terminally settle parent intent"
    );

    let state_before_retry = state.clone();
    let publication_before_retry = publication_records;
    let retry_error =
        super::super::start_machine_with_lifecycle(&network, &paths, &mut config, &mut state)
            .expect_err("fenced prior publication must reject a new provider generation");
    assert!(
        retry_error
            .to_string()
            .contains("nonterminal parent publication plans"),
        "{retry_error}"
    );
    assert_eq!(
        state, state_before_retry,
        "restart rejection must precede machine state mutation"
    );
    assert_eq!(
        port_authority
            .list_plan(&publication.plan_id)
            .expect("publication should remain inspectable"),
        publication_before_retry,
        "restart rejection must leave the exact prior generation unchanged"
    );
}

#[test]
fn stale_gvproxy_pid_preserves_publication_and_ssh_fences() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let config = sample_config(&image_path);
    let paths = config.roots.paths("default");
    paths
        .ensure_directories()
        .expect("machine directories should exist");
    fs::write(&paths.gvproxy_pid_path, i32::MAX.to_string())
        .expect("stale gvproxy pid should write");

    let network = test_machine_network_lifecycle(temp_dir.path());
    let port_authority = network.port_leases();
    let publication_store =
        crate::machine::publication_authority::MachinePublicationIntentStore::open(
            port_authority.state_root(),
        )
        .expect("parent publication store should open");
    let publication = activate_parent_publication(
        &publication_store,
        &port_authority,
        &test_forwarder_authority(&config),
        "tenant-machine-stop-stale",
        "api",
        42184,
    );
    let prepared = super::super::ports::PreparedMachineSshPortLease::prepare(
        port_authority.clone(),
        &config.name,
        &MachineStateRecord::initialized(),
    )
    .expect("machine SSH listener should prepare");
    prepared
        .activate_exact_loopback()
        .expect("exact provider observation should activate");
    let ssh_lease_id = nimbus_network::PortLeaseId::for_listener(prepared.listener_id());
    let mut state = running_machine_state(&config, &paths, &image_path, &prepared);
    drop(prepared);

    let error = super::super::stop::stop_machine(&network, &paths, &config, &mut state)
        .expect_err("stale gvproxy identity must fail closed");

    assert!(
        error
            .to_string()
            .contains("process identity evidence is missing"),
        "{error}"
    );
    assert_eq!(state.lifecycle, MachineLifecycle::Failed);
    assert_eq!(
        port_authority
            .inspect(&ssh_lease_id)
            .expect("SSH lease should inspect")
            .expect("SSH lease should remain")
            .phase(),
        PortLeasePhase::CleanupPending
    );
    assert!(
        port_authority
            .list_plan(&publication.plan_id)
            .expect("publication plan should inspect")
            .iter()
            .all(|record| record.phase() == PortLeasePhase::CleanupPending)
    );
    assert_eq!(
        publication_store
            .load_plan(&publication.plan_id)
            .expect("publication intent should inspect")
            .expect("publication intent should remain")
            .phase,
        crate::machine::publication_authority::MachinePublicationIntentPhase::Committed
    );
}

#[test]
fn recycled_gvproxy_pid_is_never_signaled_or_used_as_provider_absence() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let config = sample_config(&image_path);
    let paths = config.roots.paths("default");
    paths
        .ensure_directories()
        .expect("machine directories should exist");

    let network = test_machine_network_lifecycle(temp_dir.path());
    let port_authority = network.port_leases();
    let publication_store =
        crate::machine::publication_authority::MachinePublicationIntentStore::open(
            port_authority.state_root(),
        )
        .expect("parent publication store should open");
    let publication = activate_parent_publication(
        &publication_store,
        &port_authority,
        &test_forwarder_authority(&config),
        "tenant-machine-stop-recycled-pid",
        "api",
        42187,
    );
    let prepared = super::super::ports::PreparedMachineSshPortLease::prepare(
        port_authority.clone(),
        &config.name,
        &MachineStateRecord::initialized(),
    )
    .expect("machine SSH listener should prepare");
    prepared
        .activate_exact_loopback()
        .expect("exact provider observation should activate");
    let ssh_lease_id = nimbus_network::PortLeaseId::for_listener(prepared.listener_id());
    let mut state = running_machine_state(&config, &paths, &image_path, &prepared);
    drop(prepared);

    let (unrelated_pid, unrelated_reaper) = spawn_reaped_process("exec sleep 30");
    fs::write(&paths.gvproxy_pid_path, unrelated_pid.to_string())
        .expect("recycled gvproxy pid should write");
    let substituted_receipt = super::super::process_identity::GvproxyProcessReceipt::capture(
        u32::try_from(unrelated_pid).expect("test pid should be positive"),
        &state
            .runtime
            .as_ref()
            .expect("running state should have runtime authority")
            .forwarder_authority,
    )
    .expect("test process identity should capture")
    .with_substituted_birth_for_test();
    super::super::write_json_file(&paths.gvproxy_process_identity_path, &substituted_receipt)
        .expect("substituted prior-incarnation receipt should write");

    super::super::stop::stop_machine(&network, &paths, &config, &mut state)
        .expect("a replaced PID proves the exact prior process absent without signaling it");

    assert!(
        super::super::stop::read_pid_if_alive(&paths.gvproxy_pid_path)
            .expect("pid observation should succeed")
            .is_none(),
        "runtime cleanup should remove the stale provider pidfile"
    );
    assert!(
        super::super::stop::wait_for_pid_exit(unrelated_pid, Duration::from_millis(50))
            .is_ok_and(|exited| !exited),
        "the unrelated process that reused the PID must remain alive"
    );
    let retained_ssh = port_authority
        .inspect(&ssh_lease_id)
        .expect("SSH lease should inspect")
        .expect("SSH lease should remain durable");
    assert_eq!(retained_ssh.phase(), PortLeasePhase::Reserved);
    assert!(
        retained_ssh.confirmed_stopped_binding().is_some(),
        "exact old-provider absence must retain only a confirmed-stopped SSH rebind claim"
    );
    assert!(
        port_authority
            .list_plan(&publication.plan_id)
            .expect("publication plan should inspect")
            .iter()
            .all(|record| record.phase() == PortLeasePhase::Released),
        "birth-token mismatch proves only the exact old provider absent"
    );

    force_stop_pid(unrelated_pid, Duration::from_secs(2))
        .expect("unrelated test process should clean up");
    unrelated_reaper
        .join()
        .expect("unrelated process reaper should observe cleanup");
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

fn activate_parent_publication(
    store: &crate::machine::publication_authority::MachinePublicationIntentStore,
    authority: &LocalPortLeaseAuthority,
    forwarder_authority: &MachineForwarderAuthority,
    tenant_name: &str,
    service_name: &str,
    host_port: u16,
) -> crate::machine::publication_authority::MachinePublicationIntent {
    let tenant = TenantId::new(tenant_name).expect("tenant fixture should validate");
    let bindings = [SandboxPortBinding::new(
        "http",
        EndpointProtocol::Http,
        host_port,
        8080,
    )];
    let staged = store
        .stage_service_attempt(&tenant, service_name, forwarder_authority, &bindings)
        .expect("publication intent should stage");
    let intent = store
        .commit_before_machine_api(&staged.plan_id)
        .expect("publication intent should cross the request barrier");
    let requests = intent
        .requests()
        .expect("publication requests should validate");
    let claims = requests
        .iter()
        .map(|request| {
            let attempt = NetworkProviderHandle::new(
                forwarder_authority
                    .provider_instance()
                    .provider_id()
                    .clone(),
                format!("machine-stop-test:{}", request.lease_id()),
            )
            .expect("provider attempt should validate");
            (request.clone(), PortBindClaim::new(attempt))
        })
        .collect::<Vec<_>>();
    let reservation = authority
        .reserve_and_claim_provider_managed_batch_with_lifetimes(&claims)
        .expect("publication batch should reserve and claim");
    let (_, lifetimes) = reservation.into_parts();
    let active = requests
        .iter()
        .zip(&claims)
        .zip(&bindings)
        .map(|((request, (_, claim)), binding)| {
            let endpoint = PortBoundEndpoint::new(
                nimbus_network::PortProtocol::Tcp,
                nimbus_network::PortBindRealm::Host,
                crate::machine::publication_authority::machine_host_bind_target(
                    binding.host_address,
                )
                .expect("binding target should validate"),
                std::num::NonZeroU16::new(binding.host_port)
                    .expect("fixture host port should be nonzero"),
            )
            .expect("bound endpoint should validate");
            (
                request.clone(),
                claim.clone(),
                PortLeaseBinding::new(
                    endpoint,
                    PortBindingProvenance::NimbusOwned,
                    forwarder_authority.provider_instance().clone(),
                ),
            )
        })
        .collect::<Vec<_>>();
    authority
        .adopt_claimed_and_activate_batch_with_lifetimes(&active, None, &lifetimes)
        .expect("publication batch should activate");
    drop(lifetimes);
    intent
}

fn running_machine_state(
    config: &MachineConfigRecord,
    paths: &MachinePaths,
    image_path: &Path,
    prepared: &super::super::ports::PreparedMachineSshPortLease,
) -> MachineStateRecord {
    let mut state = MachineStateRecord::initialized();
    state.lifecycle = MachineLifecycle::Running;
    state.manager = MachineManagerState::Ready;
    state.runtime = Some(MachineRuntimeState {
        helper_binaries: MachineHelperBinaryPaths {
            vmm: PathBuf::from("/opt/homebrew/bin/krunkit"),
            gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
        },
        image_path: image_path.to_path_buf(),
        efi_variable_store_path: paths.efi_variable_store_path.clone(),
        machine_image_source: describe_machine_image_source(&config.guest.image_source),
        ssh_listener_id: prepared.listener_id().clone(),
        forwarder_authority: test_forwarder_authority(config),
        ssh_port: prepared.selected_port(),
        rest_uri: format!("unix://{}", paths.vmm_endpoint_path.display()),
        ready_vsock_port: READY_VSOCK_PORT,
    });
    state
}

fn write_exact_gvproxy_process_receipt(paths: &MachinePaths, state: &MachineStateRecord, pid: i32) {
    let receipt = super::super::process_identity::GvproxyProcessReceipt::capture(
        u32::try_from(pid).expect("test pid should be positive"),
        &state
            .runtime
            .as_ref()
            .expect("running state should have runtime authority")
            .forwarder_authority,
    )
    .expect("exact gvproxy process identity should capture");
    super::super::write_json_file(&paths.gvproxy_process_identity_path, &receipt)
        .expect("exact gvproxy process receipt should write");
}
