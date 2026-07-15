use super::*;

#[test]
fn ensure_machine_can_start_rejects_external_vmm_pid_collision() {
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
    fs::write(&paths.vmm_pid_path, vmm_pid.to_string()).expect("machine VMM pidfile should write");

    let state = MachineStateRecord::initialized();
    let error = ensure_machine_can_start(&paths, &config, &state)
        .expect_err("an external machine VMM owner should block start");

    let rendered = error.to_string();
    assert!(
        matches!(error, Error::Conflict { .. }),
        "external collision must surface as Conflict: {rendered}"
    );
    assert!(
        rendered.contains(&format!("machine-vmm pid {vmm_pid}")),
        "error should name the live machine VMM owner: {rendered}"
    );
    assert!(
        rendered.contains("NIMBUS_MACHINE_RUNTIME_ROOT"),
        "error should explain the runtime-root escape hatch: {rendered}"
    );

    force_stop_pid(vmm_pid, Duration::from_secs(2)).expect("force stop should succeed");
    vmm_reaper
        .join()
        .expect("machine VMM reaper should observe process exit");
}

#[test]
fn ensure_machine_can_start_rejects_external_gvproxy_pid_collision() {
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

    let (gvproxy_pid, gvproxy_reaper) = spawn_reaped_process("exec sleep 30");
    fs::write(&paths.gvproxy_pid_path, gvproxy_pid.to_string())
        .expect("gvproxy pidfile should write");

    let state = MachineStateRecord::initialized();
    let error = ensure_machine_can_start(&paths, &config, &state)
        .expect_err("an external gvproxy owner should block start");

    let rendered = error.to_string();
    assert!(
        matches!(error, Error::Conflict { .. }),
        "external collision must surface as Conflict: {rendered}"
    );
    assert!(
        rendered.contains(&format!("gvproxy pid {gvproxy_pid}")),
        "error should name the live gvproxy owner: {rendered}"
    );

    force_stop_pid(gvproxy_pid, Duration::from_secs(2)).expect("force stop should succeed");
    gvproxy_reaper
        .join()
        .expect("gvproxy reaper should observe process exit");
}

#[test]
fn ensure_machine_can_start_ignores_stale_pid_files_with_no_live_process() {
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

    let (vmm_pid, vmm_reaper) = spawn_reaped_process("exit 0");
    vmm_reaper
        .join()
        .expect("machine VMM reaper should observe immediate exit");
    fs::write(&paths.vmm_pid_path, vmm_pid.to_string())
        .expect("stale machine VMM pidfile should write");

    let state = MachineStateRecord::initialized();
    ensure_machine_can_start(&paths, &config, &state)
        .expect("a stale pid file must not block a fresh start");
}
