//! krunkit resolution order (env override, then the bundled `libexec` copy,
//! then an error that names both), the bundled firmware argument, and the
//! `nimbus machine info` VMM probe.

use super::*;

/// The bundled krunkit candidates for a nimbus executable at `<root>/nimbus`,
/// the layout of the macOS release archive and the Homebrew Caskroom.
fn bundled_krunkit_candidates(root: &Path) -> Vec<PathBuf> {
    bundled_helper_candidates_for_executable(&root.join("nimbus"), DEFAULT_KRUNKIT_BINARY)
}

fn write_bundled_krunkit(root: &Path) -> PathBuf {
    let libexec = root.join("libexec");
    fs::create_dir_all(&libexec).expect("libexec dir should exist");
    let krunkit = libexec.join(DEFAULT_KRUNKIT_BINARY);
    write_helper_stub(&krunkit, DEFAULT_KRUNKIT_BINARY);
    krunkit
}

/// A `PATH` that holds a krunkit, as a Homebrew install would. Resolution must
/// ignore it.
fn path_dir_with_krunkit(root: &Path) -> PathBuf {
    let path_dir = root.join("path-only");
    fs::create_dir_all(&path_dir).expect("PATH dir should exist");
    write_helper_stub(
        &path_dir.join(DEFAULT_KRUNKIT_BINARY),
        DEFAULT_KRUNKIT_BINARY,
    );
    path_dir
}

#[test]
fn krunkit_resolution_prefers_env_override_over_bundled_copy() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    write_bundled_krunkit(temp_dir.path());
    let override_dir = temp_dir.path().join("override");
    fs::create_dir_all(&override_dir).expect("override dir should exist");
    let _guard = MachineHelperEnvGuard::install_stub_binaries(&override_dir);

    let resolved = resolve_helper_binary(
        KRUNKIT_ENV,
        DEFAULT_KRUNKIT_BINARY,
        &bundled_krunkit_candidates(temp_dir.path()),
    )
    .expect("the env override should resolve");

    assert_eq!(resolved, override_dir.join(DEFAULT_KRUNKIT_BINARY));
}

#[test]
fn krunkit_resolution_uses_bundled_copy_without_override() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let bundled = write_bundled_krunkit(temp_dir.path());
    let _guard = MachineHelperEnvGuard::with_path_only(&path_dir_with_krunkit(temp_dir.path()));

    let resolved = resolve_helper_binary(
        KRUNKIT_ENV,
        DEFAULT_KRUNKIT_BINARY,
        &bundled_krunkit_candidates(temp_dir.path()),
    )
    .expect("the bundled krunkit should resolve");

    assert_eq!(resolved, bundled);
}

#[test]
fn krunkit_resolution_errors_with_override_and_bundled_locations() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let _guard = MachineHelperEnvGuard::with_path_only(&path_dir_with_krunkit(temp_dir.path()));
    let candidates = bundled_krunkit_candidates(temp_dir.path());

    let error = resolve_helper_binary(KRUNKIT_ENV, DEFAULT_KRUNKIT_BINARY, &candidates)
        .expect_err("a PATH-only krunkit should not resolve")
        .to_string();

    assert!(error.contains(KRUNKIT_ENV), "{error}");
    assert!(
        error.contains(&candidates[0].display().to_string()),
        "{error}"
    );
}

#[test]
fn krunkit_backend_ignores_homebrew_and_podman_directories() {
    // The test executable has no `libexec/krunkit` beside it. Before the
    // bundled-only resolution, a host with `/opt/homebrew/bin/krunkit`
    // resolved that copy here.
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let _guard = MachineHelperEnvGuard::with_path_only(&path_dir_with_krunkit(temp_dir.path()));

    let error = KrunkitVmmBackend
        .resolve_vmm_binary()
        .expect_err("krunkit should resolve only from the override or the bundle")
        .to_string();

    assert!(error.contains(KRUNKIT_ENV), "{error}");
    assert!(error.contains("libexec/krunkit"), "{error}");
}

fn build_krunkit_launch_command(vmm_binary: &Path) -> MachineCommandLine {
    build_vmm_launch_command(&KrunkitVmmBackend, vmm_binary, false)
}

fn build_vmm_launch_command(
    backend: &dyn MachineVmmBackend,
    vmm_binary: &Path,
    nested_virtualization: bool,
) -> MachineCommandLine {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    let config = sample_config(&image_path);
    let paths = config.roots.paths("default");
    let efi_variable_store_path = paths.efi_variable_store_path.clone();
    let rest_uri = format!("unix://{}", paths.vmm_endpoint_path.display());
    let ctx = VmmLaunchContext {
        paths: &paths,
        config: &config,
        image_path: &image_path,
        efi_variable_store_path: &efi_variable_store_path,
        rest_uri: &rest_uri,
        bootstrap_mode: MachineBootstrapMode::Ignition,
        machine_config_bundle_dir: None,
        nested_virtualization,
    };
    backend
        .build_launch_command(vmm_binary, &ctx)
        .expect("launch command should build")
}

#[test]
fn krunkit_launch_passes_nested_on_a_supported_host() {
    let command = build_vmm_launch_command(&KrunkitVmmBackend, Path::new("/opt/test/vmm"), true);

    assert!(
        command.args.iter().any(|arg| arg == "--nested"),
        "{:?}",
        command.args
    );
}

#[test]
fn krunkit_launch_omits_nested_on_an_unsupported_host() {
    let command = build_vmm_launch_command(&KrunkitVmmBackend, Path::new("/opt/test/vmm"), false);

    assert!(
        !command.args.iter().any(|arg| arg == "--nested"),
        "{:?}",
        command.args
    );
}

#[test]
fn vfkit_launch_never_passes_nested() {
    // vfkit has no `--nested` flag. A supported host must not change its
    // command line.
    let command = build_vmm_launch_command(&VfkitVmmBackend, Path::new("/opt/test/vmm"), true);

    assert!(
        !command.args.iter().any(|arg| arg == "--nested"),
        "{:?}",
        command.args
    );
}

#[test]
fn krunkit_launch_passes_bundled_firmware_beside_the_binary() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let krunkit = write_bundled_krunkit(temp_dir.path());
    // The release workflow unpacks the firmware flat into `libexec`.
    let firmware = temp_dir.path().join("libexec").join("KRUN_EFI.silent.fd");
    fs::write(&firmware, b"firmware").expect("firmware should be written");

    let command = build_krunkit_launch_command(&krunkit);

    assert!(
        command
            .args
            .windows(2)
            .any(|pair| pair[0] == "--firmware-path" && pair[1] == firmware.display().to_string()),
        "{:?}",
        command.args
    );
}

#[test]
fn krunkit_launch_keeps_krunkit_firmware_lookup_without_sibling_firmware() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let krunkit = write_bundled_krunkit(temp_dir.path());

    let command = build_krunkit_launch_command(&krunkit);

    assert!(
        !command.args.iter().any(|arg| arg == "--firmware-path"),
        "{:?}",
        command.args
    );
}

#[test]
fn vmm_inspection_reports_resolved_path_and_version() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
    let krunkit = temp_dir.path().join(DEFAULT_KRUNKIT_BINARY);
    crate::test_support::write_executable_stub(&krunkit, "#!/bin/sh\necho 'krunkit 1.3.2'\n");

    let status = inspect_vmm_binary(MachineProvider::Krunkit);

    assert_eq!(status.provider, MachineProvider::Krunkit);
    assert_eq!(status.path, Some(krunkit));
    assert_eq!(status.version.as_deref(), Some("krunkit 1.3.2"));
    assert_eq!(status.error, None);
}

#[test]
fn vmm_inspection_reports_resolution_error_without_a_binary() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let _guard = MachineHelperEnvGuard::with_path_only(temp_dir.path());

    let status = inspect_vmm_binary(MachineProvider::Krunkit);

    assert_eq!(status.path, None);
    assert_eq!(status.version, None);
    let error = status
        .error
        .expect("a missing krunkit should report an error");
    assert!(error.contains(KRUNKIT_ENV), "{error}");
}
