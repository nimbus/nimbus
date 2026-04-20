use super::*;

#[test]
fn remote_shell_command_single_quotes_guest_scripts_for_ssh() {
    let script = "if [ -x '/usr/local/bin/neovex' ]; then printf '%s' ok; fi";

    assert_eq!(
        remote_shell_command(script),
        "sh -lc 'if [ -x '\"'\"'/usr/local/bin/neovex'\"'\"' ]; then printf '\"'\"'%s'\"'\"' ok; fi'"
    );
}

#[test]
fn ssh_command_requires_running_machine_and_identity() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    fs::write(&image_path, []).expect("image should write");
    let mut config = sample_config(&image_path);
    config.guest.ssh_identity_path = None;

    let mut state = MachineStateRecord::initialized();
    state.lifecycle = MachineLifecycle::Running;
    state.runtime = Some(MachineRuntimeState {
        helper_binaries: MachineHelperBinaryPaths {
            krunkit: PathBuf::from("/opt/homebrew/bin/krunkit"),
            gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
        },
        image_path,
        efi_variable_store_path: PathBuf::from("/tmp/efi"),
        machine_image_source: describe_machine_image_source(&config.guest.image_source),
        ssh_port: 2222,
        rest_uri: "unix:///tmp/krunkit.sock".to_owned(),
        ready_vsock_port: READY_VSOCK_PORT,
    });

    let error = build_ssh_command(&config, &state).expect_err("missing identity should fail");
    assert!(error.to_string().contains("no SSH identity configured"));
}

#[test]
fn ssh_command_applies_localhost_machine_safety_options() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    let identity_path = temp_dir.path().join("machine");
    fs::write(&image_path, []).expect("image should write");
    fs::write(&identity_path, "fake-private-key").expect("identity should write");

    let mut config = sample_config(&image_path);
    config.guest.ssh_identity_path = Some(identity_path.clone());

    let mut state = MachineStateRecord::initialized();
    state.lifecycle = MachineLifecycle::Running;
    state.runtime = Some(MachineRuntimeState {
        helper_binaries: MachineHelperBinaryPaths {
            krunkit: PathBuf::from("/opt/homebrew/bin/krunkit"),
            gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
        },
        image_path,
        efi_variable_store_path: PathBuf::from("/tmp/efi"),
        machine_image_source: describe_machine_image_source(&config.guest.image_source),
        ssh_port: 2222,
        rest_uri: "unix:///tmp/krunkit.sock".to_owned(),
        ready_vsock_port: READY_VSOCK_PORT,
    });

    let command = build_ssh_command(&config, &state).expect("ssh command should build");
    let args = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    assert!(
        args.windows(2)
            .any(|window| window == ["-o", "BatchMode=yes"])
    );
    assert!(
        args.windows(2)
            .any(|window| window == ["-o", "StrictHostKeyChecking=no"])
    );
    assert!(
        args.windows(2)
            .any(|window| window == ["-o", "UserKnownHostsFile=/dev/null"])
    );
    assert!(
        args.windows(2)
            .any(|window| window == ["-i", identity_path.to_string_lossy().as_ref()])
    );
    assert!(args.windows(2).any(|window| window == ["-p", "2222"]));
    assert_eq!(args.last().map(String::as_str), Some("core@127.0.0.1"));
}

#[test]
fn scp_command_applies_localhost_machine_safety_options() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    let identity_path = temp_dir.path().join("machine");
    fs::write(&image_path, []).expect("image should write");
    fs::write(&identity_path, "fake-private-key").expect("identity should write");

    let mut config = sample_config(&image_path);
    config.guest.ssh_identity_path = Some(identity_path.clone());

    let mut state = MachineStateRecord::initialized();
    state.lifecycle = MachineLifecycle::Running;
    state.runtime = Some(MachineRuntimeState {
        helper_binaries: MachineHelperBinaryPaths {
            krunkit: PathBuf::from("/opt/homebrew/bin/krunkit"),
            gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
        },
        image_path,
        efi_variable_store_path: PathBuf::from("/tmp/efi"),
        machine_image_source: describe_machine_image_source(&config.guest.image_source),
        ssh_port: 2222,
        rest_uri: "unix:///tmp/krunkit.sock".to_owned(),
        ready_vsock_port: READY_VSOCK_PORT,
    });

    let command = build_scp_command(&config, &state, false, "/tmp/remote.txt", "./local.txt")
        .expect("scp command should build");
    let args = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    assert!(
        args.windows(2)
            .any(|window| window == ["-o", "BatchMode=yes"])
    );
    assert!(
        args.windows(2)
            .any(|window| window == ["-o", "StrictHostKeyChecking=no"])
    );
    assert!(
        args.windows(2)
            .any(|window| window == ["-o", "UserKnownHostsFile=/dev/null"])
    );
    assert!(
        args.windows(2)
            .any(|window| window == ["-i", identity_path.to_string_lossy().as_ref()])
    );
    assert!(args.windows(2).any(|window| window == ["-P", "2222"]));
    assert!(args.iter().any(|arg| arg == "-r"));
    assert!(
        args.iter()
            .any(|arg| arg == "core@127.0.0.1:/tmp/remote.txt")
    );
    assert_eq!(
        args.last().map(String::as_str),
        Some("core@127.0.0.1:/tmp/remote.txt")
    );
}

#[test]
fn scp_command_formats_guest_source_for_downloads() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let image_path = temp_dir.path().join("disk.raw");
    let identity_path = temp_dir.path().join("machine");
    fs::write(&image_path, []).expect("image should write");
    fs::write(&identity_path, "fake-private-key").expect("identity should write");

    let mut config = sample_config(&image_path);
    config.guest.ssh_identity_path = Some(identity_path);

    let mut state = MachineStateRecord::initialized();
    state.lifecycle = MachineLifecycle::Running;
    state.runtime = Some(MachineRuntimeState {
        helper_binaries: MachineHelperBinaryPaths {
            krunkit: PathBuf::from("/opt/homebrew/bin/krunkit"),
            gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
        },
        image_path,
        efi_variable_store_path: PathBuf::from("/tmp/efi"),
        machine_image_source: describe_machine_image_source(&config.guest.image_source),
        ssh_port: 2222,
        rest_uri: "unix:///tmp/krunkit.sock".to_owned(),
        ready_vsock_port: READY_VSOCK_PORT,
    });

    let command = build_scp_command(&config, &state, true, "/tmp/remote.txt", "./local.txt")
        .expect("scp command should build");
    let args = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    let src_index = args
        .iter()
        .position(|arg| arg == "core@127.0.0.1:/tmp/remote.txt")
        .expect("remote source should exist");
    let dst_index = args
        .iter()
        .position(|arg| arg == "./local.txt")
        .expect("local destination should exist");
    assert!(src_index < dst_index);
}
