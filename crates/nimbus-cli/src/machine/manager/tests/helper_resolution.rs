use super::*;

#[test]
fn helper_resolution_honors_environment_overrides() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let krunkit_path = temp_dir.path().join("krunkit");
    let gvproxy_path = temp_dir.path().join("gvproxy");
    let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());

    // VMM binary resolution is owned by the per-provider backend; gvproxy
    // resolution is the shared helper path. Both honor their env overrides.
    let resolved_vmm = KrunkitVmmBackend
        .resolve_vmm_binary()
        .expect("krunkit binary should resolve via env");
    let resolved_gvproxy = resolve_gvproxy_binary().expect("gvproxy should resolve via env");

    assert_eq!(resolved_vmm, krunkit_path);
    assert_eq!(resolved_gvproxy, gvproxy_path);
}

#[test]
fn bundled_helper_candidates_cover_root_and_bin_layouts() {
    let root_layout = bundled_helper_candidates_for_executable(
        Path::new("/opt/homebrew/Caskroom/nimbus/0.1.10/nimbus"),
        "gvproxy",
    );
    assert_eq!(
        root_layout,
        vec![PathBuf::from(
            "/opt/homebrew/Caskroom/nimbus/0.1.10/libexec/gvproxy"
        )]
    );

    let bin_layout =
        bundled_helper_candidates_for_executable(Path::new("/opt/homebrew/bin/nimbus"), "gvproxy");
    assert_eq!(
        bin_layout,
        vec![
            PathBuf::from("/opt/homebrew/bin/libexec/gvproxy"),
            PathBuf::from("/opt/homebrew/libexec/gvproxy"),
        ]
    );
}

#[test]
fn helper_resolution_honors_helper_binary_directory_override() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let helper_dir = temp_dir.path().join("helpers");
    fs::create_dir_all(&helper_dir).expect("helper dir should exist");
    let helper_gvproxy = helper_dir.join("gvproxy");
    write_helper_stub(&helper_gvproxy, "gvproxy");
    let _guard = MachineHelperEnvGuard::with_helper_binary_dir(&helper_dir);

    let resolved = resolve_helper_binary("NIMBUS_TEST_GVPROXY", "gvproxy", &[])
        .expect("helper dir override should resolve");

    assert_eq!(resolved, helper_gvproxy);
}

#[test]
fn helper_resolution_does_not_fall_back_to_path() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let helper_dir = temp_dir.path().join("path-only");
    fs::create_dir_all(&helper_dir).expect("path-only helper dir should exist");
    let helper_gvproxy = helper_dir.join("gvproxy");
    write_helper_stub(&helper_gvproxy, "gvproxy");
    let _guard = MachineHelperEnvGuard::with_path_only(&helper_dir);

    let error = resolve_helper_binary("NIMBUS_TEST_GVPROXY", "gvproxy", &[])
        .expect_err("PATH-only helpers should be ignored");

    assert!(
        error
            .to_string()
            .contains("required helper 'gvproxy' was not found; set NIMBUS_TEST_GVPROXY"),
        "{error}"
    );
}

#[cfg(unix)]
#[test]
fn machine_command_spawn_detaches_helpers_into_new_session() {
    let command = MachineCommandLine {
        program: PathBuf::from("/bin/sh"),
        args: vec!["-c".to_owned(), "sleep 30".to_owned()],
        capture_log_path: None,
    };
    let mut child = command.spawn().expect("helper process should spawn");
    let child_pid = child.id() as i32;
    let parent_sid = unsafe { libc::getsid(0) };
    let child_sid = unsafe { libc::getsid(child_pid) };

    assert!(parent_sid > 0, "parent sid should resolve");
    assert_eq!(child_sid, child_pid, "child should lead its own session");
    assert_ne!(
        child_sid, parent_sid,
        "child session should differ from parent"
    );

    cleanup_process(&mut child).expect("child should clean up");
}
