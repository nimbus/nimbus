use super::*;

#[test]
fn helper_resolution_honors_environment_overrides() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let krunkit_path = temp_dir.path().join("krunkit");
    let gvproxy_path = temp_dir.path().join("gvproxy");
    let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
    let resolved =
        resolve_machine_helper_binaries().expect("helper binaries should resolve via env");

    assert_eq!(resolved.krunkit, krunkit_path);
    assert_eq!(resolved.gvproxy, gvproxy_path);
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
fn helper_resolution_prefers_packaged_candidates_before_fallbacks() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let packaged_dir = temp_dir.path().join("libexec");
    let fallback_dir = temp_dir.path().join("fallback");
    fs::create_dir_all(&packaged_dir).expect("packaged helper dir should exist");
    fs::create_dir_all(&fallback_dir).expect("fallback helper dir should exist");
    let packaged_gvproxy = packaged_dir.join("gvproxy");
    let fallback_gvproxy = fallback_dir.join("gvproxy");
    write_helper_stub(&packaged_gvproxy, "gvproxy");
    write_helper_stub(&fallback_gvproxy, "gvproxy");

    let resolved = resolve_helper_binary(
        "NIMBUS_TEST_GVPROXY",
        "gvproxy-does-not-exist",
        std::slice::from_ref(&packaged_gvproxy),
        &[fallback_gvproxy],
    )
    .expect("packaged helper should resolve");

    assert_eq!(resolved, packaged_gvproxy);
}

#[test]
fn helper_resolution_honors_helper_binary_directory_override() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let helper_dir = temp_dir.path().join("helpers");
    fs::create_dir_all(&helper_dir).expect("helper dir should exist");
    let helper_gvproxy = helper_dir.join("gvproxy");
    write_helper_stub(&helper_gvproxy, "gvproxy");
    let _guard = MachineHelperEnvGuard::with_helper_binary_dir(&helper_dir);

    let resolved = resolve_helper_binary("NIMBUS_TEST_GVPROXY", "gvproxy", &[], &[])
        .expect("helper dir override should resolve");

    assert_eq!(resolved, helper_gvproxy);
}

#[test]
fn known_helper_candidates_mirror_podman_darwin_defaults() {
    assert_eq!(
        known_helper_candidates("gvproxy"),
        vec![
            PathBuf::from("/usr/local/opt/podman/libexec/podman/gvproxy"),
            PathBuf::from("/opt/homebrew/opt/podman/libexec/podman/gvproxy"),
            PathBuf::from("/opt/homebrew/bin/gvproxy"),
            PathBuf::from("/usr/local/bin/gvproxy"),
            PathBuf::from("/opt/homebrew/libexec/podman/gvproxy"),
            PathBuf::from("/usr/local/libexec/podman/gvproxy"),
            PathBuf::from("/usr/local/lib/podman/gvproxy"),
            PathBuf::from("/usr/libexec/podman/gvproxy"),
            PathBuf::from("/usr/lib/podman/gvproxy"),
        ]
    );
}

#[test]
fn helper_resolution_does_not_fall_back_to_path() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let helper_dir = temp_dir.path().join("path-only");
    fs::create_dir_all(&helper_dir).expect("path-only helper dir should exist");
    let helper_gvproxy = helper_dir.join("gvproxy");
    write_helper_stub(&helper_gvproxy, "gvproxy");
    let _guard = MachineHelperEnvGuard::with_path_only(&helper_dir);

    let error = resolve_helper_binary("NIMBUS_TEST_GVPROXY", "gvproxy", &[], &[])
        .expect_err("PATH-only helpers should be ignored");

    assert!(
        error
            .to_string()
            .contains("supported packaged or Homebrew helper directory"),
        "{error}"
    );
}

#[cfg(unix)]
#[test]
fn machine_command_spawn_detaches_helpers_into_new_session() {
    let command = MachineCommandLine {
        program: PathBuf::from("/bin/sh"),
        args: vec!["-c".to_owned(), "sleep 30".to_owned()],
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
