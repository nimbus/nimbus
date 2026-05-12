use super::*;

#[test]
fn machine_status_marks_missing_machine_api_socket_as_unreachable() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths(DEFAULT_MACHINE_NAME);

    let api = machine_api_status_view(&paths, None);

    assert_eq!(api.socket_path, paths.api_socket_path);
    assert_eq!(api.guest_socket_path, None);
    assert_eq!(api.transport, None);
    assert_eq!(api.forward_user, None);
    assert_eq!(api.identity_path, None);
    assert!(!api.exists);
    assert!(!api.reachable);
    assert!(api.capabilities.is_none());
    assert!(api.error.is_none());
}

#[test]
fn machine_status_renders_release_asset_guest_binary_contract() {
    let _env_lock = lock_machine_guest_binary_override_env();
    let _env_guard = GuestBinaryOverrideEnvGuard::clear();

    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths(DEFAULT_MACHINE_NAME);
    paths
        .ensure_directories()
        .expect("machine directories should exist");
    let config = MachineConfigRecord {
        version: CURRENT_MACHINE_CONFIG_VERSION,
        name: DEFAULT_MACHINE_NAME.to_owned(),
        provider: MachineProvider::Krunkit,
        guest: MachineGuestConfig {
            image_source: MachineImageSource::parse(&default_machine_image())
                .expect("default image should parse"),
            ssh_user: DEFAULT_MACHINE_SSH_USER.to_owned(),
            ssh_identity_path: Some(temp_dir.path().join("nimbus-test-ed25519")),
            ignition_file_path: None,
            efi_variable_store_path: None,
        },
        resources: MachineResources {
            cpus: DEFAULT_MACHINE_CPUS,
            memory_mib: DEFAULT_MACHINE_MEMORY_MIB,
            disk_gib: DEFAULT_MACHINE_DISK_GIB,
        },
        volumes: Vec::new(),
        roots: layout,
    };
    let desired = inspect_desired_guest_nimbus_binary(&paths);
    fs::write(&desired.desired_path, b"release guest binary").expect("guest binary should write");

    let rendered = render_machine_status_view(
        MachineCommandResult::Status,
        &paths,
        Some(&config),
        Some(&MachineStateRecord::initialized()),
        MachineStatusOutputFormat::Yaml,
        false,
        false,
    )
    .expect("machine view should render");
    let desired = inspect_desired_guest_nimbus_binary(&paths);

    if !cfg!(target_os = "macos") {
        assert!(rendered.contains("guest_binary_contract: null"));
        return;
    }

    assert!(rendered.contains("guest_binary_contract:"));
    assert!(rendered.contains("source: release-asset"));
    assert!(rendered.contains(&format!(
        "source_detail: GitHub release asset {}",
        current_machine_release_tag()
    )));
    assert!(rendered.contains(&format!(
        "desired_version: {}",
        current_machine_release_tag()
    )));
    assert!(rendered.contains(&format!("desired_path: {}", desired.desired_path.display())));
    assert!(rendered.contains("desired_exists: true"));
    assert!(rendered.contains(&format!(
            "desired_hash: {}",
            desired
                .desired_hash
                .as_deref()
                .expect("desired hash should exist for cached release asset")
        )));
}

#[test]
fn machine_status_renders_explicit_override_guest_binary_contract() {
    let _env_lock = lock_machine_guest_binary_override_env();

    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths(DEFAULT_MACHINE_NAME);
    paths
        .ensure_directories()
        .expect("machine directories should exist");
    let override_binary = temp_dir.path().join("override-nimbus");
    fs::write(&override_binary, b"override guest binary").expect("override binary should write");
    let _env_guard = GuestBinaryOverrideEnvGuard::set(&override_binary);

    let config = MachineConfigRecord {
        version: CURRENT_MACHINE_CONFIG_VERSION,
        name: DEFAULT_MACHINE_NAME.to_owned(),
        provider: MachineProvider::Krunkit,
        guest: MachineGuestConfig {
            image_source: MachineImageSource::parse(&default_machine_image())
                .expect("default image should parse"),
            ssh_user: DEFAULT_MACHINE_SSH_USER.to_owned(),
            ssh_identity_path: Some(temp_dir.path().join("nimbus-test-ed25519")),
            ignition_file_path: None,
            efi_variable_store_path: None,
        },
        resources: MachineResources {
            cpus: DEFAULT_MACHINE_CPUS,
            memory_mib: DEFAULT_MACHINE_MEMORY_MIB,
            disk_gib: DEFAULT_MACHINE_DISK_GIB,
        },
        volumes: Vec::new(),
        roots: layout,
    };

    let rendered = render_machine_status_view(
        MachineCommandResult::Status,
        &paths,
        Some(&config),
        Some(&MachineStateRecord::initialized()),
        MachineStatusOutputFormat::Yaml,
        false,
        false,
    )
    .expect("machine view should render");
    let desired = inspect_desired_guest_nimbus_binary(&paths);

    if !cfg!(target_os = "macos") {
        assert!(rendered.contains("guest_binary_contract: null"));
        return;
    }

    assert!(rendered.contains("guest_binary_contract:"));
    assert!(rendered.contains("source: explicit-override"));
    assert!(rendered.contains(&format!(
        "source_detail: $NIMBUS_MACHINE_GUEST_BINARY={}",
        override_binary.display()
    )));
    assert!(rendered.contains(&format!("desired_path: {}", override_binary.display())));
    assert!(rendered.contains("desired_exists: true"));
    assert!(rendered.contains(&format!(
            "desired_hash: {}",
            desired
                .desired_hash
                .as_deref()
                .expect("desired hash should exist for explicit override")
        )));
}

#[test]
fn machine_status_detects_reachable_machine_api_socket() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths(DEFAULT_MACHINE_NAME);

    std::fs::create_dir_all(
        paths
            .api_socket_path
            .parent()
            .expect("machine api socket should have a parent"),
    )
    .expect("socket parent should exist");
    let listener =
        StdUnixListener::bind(&paths.api_socket_path).expect("listener should bind cleanly");
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("server should accept");
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let body = serde_json::json!({
            "status": "ok",
            "role": "guest-machine-api",
            "protocol_version": "v1alpha2",
            "listen_mode": "direct-socket",
            "control_data_dir": temp_dir.path().join("control").display().to_string(),
        })
        .to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("server should write response");

        let (mut stream, _) = listener
            .accept()
            .expect("server should accept capabilities");
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let body = serde_json::json!({
                "protocol_version": "v1alpha2",
                "service_execution_ready": false,
                "service_execution_mode": "standard_containers",
                "supported_service_backends": ["container"],
                "supported_operations": ["healthz", "capabilities"],
                "binary_statuses": [
                    {
                        "name": "buildah",
                        "present": true,
                        "resolved_path": "/usr/bin/buildah",
                        "required_for_operations": ["service-sandboxes.build-start"]
                    }
                ],
                "operation_statuses": [
                    {
                        "name": "service-sandboxes.build-start",
                        "available": false,
                        "blockers": ["guest machine API does not yet expose service lifecycle operations"]
                    }
                ],
                "service_execution_blockers": [
                    "guest machine API does not yet expose service lifecycle operations"
                ]
            })
            .to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("server should write capabilities response");
    });

    std::thread::sleep(std::time::Duration::from_millis(100));
    let api = machine_api_status_view(&paths, None);
    server
        .join()
        .expect("machine API server thread should join cleanly");

    assert_eq!(api.socket_path, paths.api_socket_path);
    assert_eq!(api.guest_socket_path, None);
    assert_eq!(api.transport, None);
    assert_eq!(api.forward_user, None);
    assert_eq!(api.identity_path, None);
    assert!(api.exists);
    assert!(api.reachable);
    assert_eq!(api.role.as_deref(), Some("guest-machine-api"));
    assert_eq!(api.protocol_version.as_deref(), Some("v1alpha2"));
    assert_eq!(api.listen_mode.as_deref(), Some("direct-socket"));
    assert_eq!(
        api.capabilities
            .as_ref()
            .map(|capabilities| capabilities.service_execution_mode),
        Some(protocol::MachineApiServiceExecutionMode::StandardContainers)
    );
    assert_eq!(
        api.capabilities
            .as_ref()
            .map(|capabilities| capabilities.supported_service_backends.clone()),
        Some(vec![nimbus::SandboxBackendKind::Container])
    );
    assert!(api.error.is_none());
}

#[test]
fn machine_status_reports_forwarding_contract_when_machine_identity_exists() {
    let temp_dir = TempDir::new().expect("temp dir should exist");
    let layout = MachineRootLayout::new(
        temp_dir.path().join("config"),
        temp_dir.path().join("state"),
        temp_dir.path().join("runtime"),
    );
    let paths = layout.paths(DEFAULT_MACHINE_NAME);
    let config = MachineConfigRecord {
        version: CURRENT_MACHINE_CONFIG_VERSION,
        name: DEFAULT_MACHINE_NAME.to_owned(),
        provider: MachineProvider::Krunkit,
        guest: MachineGuestConfig {
            image_source: MachineImageSource::parse(&default_machine_image())
                .expect("default image should parse"),
            ssh_user: DEFAULT_MACHINE_SSH_USER.to_owned(),
            ssh_identity_path: Some(PathBuf::from("/tmp/nimbus-test-ed25519")),
            ignition_file_path: None,
            efi_variable_store_path: None,
        },
        resources: MachineResources {
            cpus: DEFAULT_MACHINE_CPUS,
            memory_mib: DEFAULT_MACHINE_MEMORY_MIB,
            disk_gib: DEFAULT_MACHINE_DISK_GIB,
        },
        volumes: Vec::new(),
        roots: layout,
    };

    let api = machine_api_status_view(&paths, Some(&config));

    assert_eq!(api.socket_path, paths.api_socket_path);
    assert_eq!(
        api.guest_socket_path,
        Some(PathBuf::from("/run/nimbus/nimbus.sock"))
    );
    assert_eq!(
        api.transport.as_deref(),
        Some("gvproxy-ssh-forwarded-unix-socket")
    );
    assert_eq!(api.forward_user.as_deref(), Some("root"));
    assert_eq!(
        api.identity_path,
        Some(PathBuf::from("/tmp/nimbus-test-ed25519"))
    );
    assert!(!api.exists);
    assert!(!api.reachable);
    assert!(api.capabilities.is_none());
    assert!(api.error.is_none());
}
