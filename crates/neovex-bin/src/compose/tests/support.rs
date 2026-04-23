use super::*;

#[derive(Debug, Parser)]
pub(super) struct RootCli {
    #[command(subcommand)]
    pub(super) command: Option<RootCommand>,
}

#[derive(Debug, Subcommand)]
pub(super) enum RootCommand {
    #[command(name = "compose")]
    Compose(ComposeCommand),
}

pub(super) fn write_compose_fixture(root: &Path) -> PathBuf {
    let compose_path = root.join("compose.yaml");
    fs::write(
        &compose_path,
        r#"
name: Demo App
services:
  db:
    image: busybox:latest
"#,
    )
    .expect("compose fixture should write");
    compose_path
}

pub(super) fn write_compose_fixture_with_body(root: &Path, body: &str) -> PathBuf {
    let compose_path = root.join("compose.yaml");
    fs::write(&compose_path, body).expect("compose fixture should write");
    compose_path
}

pub(super) fn wait_for_machine_api_health(client: &MachineApiClient) {
    let start = std::time::Instant::now();
    loop {
        match client.health() {
            Ok(_) => return,
            Err(_) if start.elapsed() < Duration::from_secs(5) => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => panic!("machine API never became reachable: {error}"),
        }
    }
}

pub(super) fn write_fake_runtime_binaries(dir: &Path) {
    for binary in [
        "buildah",
        "conmon",
        "crun",
        "netavark",
        "aardvark-dns",
        "fuse-overlayfs",
    ] {
        let path = dir.join(binary);
        crate::test_support::write_executable_stub(&path, "#!/bin/sh\nexit 0\n");
    }
}

pub(super) fn write_container_machine_manifest(
    state_root: &Path,
    sandbox_id: &str,
    tenant_id: &str,
    service_name: &str,
    status: SandboxStatus,
) -> PathBuf {
    let container_dir = state_root.join("containers").join(sandbox_id);
    let exit_dir = state_root.join("exits");
    let persist_dir = state_root.join("persist").join(sandbox_id);
    let bundle_dir = state_root.join("bundles").join(sandbox_id);
    let network_root = state_root.join("networks");
    let run_root = network_root.join("run");
    let netns_root = network_root.join("netns");
    let container_network_dir = network_root.join("containers").join(sandbox_id);
    fs::create_dir_all(&container_dir).expect("container directory should build");
    fs::create_dir_all(&exit_dir).expect("exit directory should build");
    fs::create_dir_all(&persist_dir).expect("persist directory should build");
    fs::create_dir_all(&bundle_dir).expect("bundle directory should build");
    fs::create_dir_all(&container_network_dir).expect("container network directory should build");

    let handle = neovex::SandboxHandle::new(
        neovex::SandboxId::new(sandbox_id),
        service_name,
        neovex::SandboxBackendKind::Container,
        status,
        vec![neovex::PublishedEndpoint::new(
            "http",
            neovex::PublishedEndpointProtocol::Tcp,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 18080),
        )],
    );
    let manifest = json!({
        "handle": handle,
        "spec": {
            "tenant_id": tenant_id,
            "name": service_name,
            "backend": "container",
            "filesystem": {
                "rootfs": "/tmp/rootfs",
                "readonly": true
            },
            "process": {
                "args": ["/bin/server"],
                "env": ["PATH=/usr/bin"],
                "cwd": "/",
                "terminal": false
            },
            "resources": neovex::SandboxResourceLimits::default(),
            "lifecycle": {
                "restart_policy": "never"
            },
            "port_bindings": [neovex::SandboxPortBinding::tcp("http", 18080, 8080)]
        },
        "image_metadata": {},
        "launch_artifact": null,
        "bundle_layout": {
            "bundle_dir": bundle_dir,
            "config_path": bundle_dir.join("config.json")
        },
        "conmon_layout": {
            "state_root": state_root,
            "container_state_dir": container_dir,
            "exit_dir": exit_dir,
            "persist_dir": persist_dir,
            "ctr_log": container_dir.join("ctr.log"),
            "oci_log": container_dir.join("oci.log"),
            "pidfile": container_dir.join("pidfile"),
            "conmon_pidfile": container_dir.join("conmon.pid"),
            "exit_status_file": exit_dir.join(sandbox_id),
            "manifest_path": container_dir.join("manifest.json")
        },
        "network_layout": {
            "network_root": network_root,
            "run_root": run_root,
            "netns_root": netns_root,
            "container_network_dir": container_network_dir,
            "netns_path": netns_root.join(sandbox_id),
            "status_path": container_network_dir.join("status.json"),
            "ipam_state_path": run_root.join("ipam-state.json"),
            "ipam_lock_path": run_root.join("ipam.lock")
        },
        "conmon_launch": {
            "create_command": {
                "program": "/bin/true",
                "args": []
            },
            "state_command": {
                "program": "/bin/true",
                "args": []
            },
            "start_command": {
                "program": "/bin/true",
                "args": []
            },
            "delete_command": {
                "program": "/bin/true",
                "args": []
            }
        },
        "last_exit_code": null,
        "launch_mode": "plan_only",
        "shutdown_requested": matches!(status, SandboxStatus::Stopped),
        "status": status
    });
    fs::write(
        container_dir.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).expect("manifest should serialize"),
    )
    .expect("manifest should write");
    container_dir
}

pub(super) fn write_manifest(
    state_root: &Path,
    sandbox_id: &str,
    tenant_id: &str,
    service_name: &str,
    status: SandboxStatus,
) {
    let container_dir = state_root.join("containers").join(sandbox_id);
    fs::create_dir_all(&container_dir).expect("container directory should build");

    let handle = neovex::SandboxHandle::new(
        neovex::SandboxId::new(sandbox_id),
        service_name,
        neovex::SandboxBackendKind::Krun,
        status,
        vec![neovex::PublishedEndpoint::new(
            "http",
            neovex::PublishedEndpointProtocol::Tcp,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 18080),
        )],
    );
    let manifest = json!({
        "handle": handle,
        "spec": {
            "tenant_id": tenant_id,
            "name": service_name,
            "backend": "krun",
            "filesystem": {
                "rootfs": "/tmp/rootfs",
                "readonly": true
            },
            "process": {
                "args": ["/bin/server"],
                "env": ["PATH=/usr/bin"],
                "cwd": "/",
                "terminal": false
            },
            "resources": neovex::SandboxResourceLimits::default(),
            "lifecycle": {
                "restart_policy": "never"
            },
            "port_bindings": [neovex::SandboxPortBinding::tcp("http", 18080, 8080)]
        },
        "conmon_layout": {
            "container_state_dir": container_dir,
            "ctr_log": container_dir.join("ctr.log"),
            "oci_log": container_dir.join("oci.log")
        },
        "last_exit_code": null,
        "restart_count": 0,
        "shutdown_requested": matches!(status, SandboxStatus::Stopped),
        "status": status
    });
    fs::write(
        container_dir.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).expect("manifest should serialize"),
    )
    .expect("manifest should write");
}

pub(super) fn sample_spec(tenant: &TenantId, service_name: &str) -> SandboxSpec {
    SandboxSpec::new(
        tenant.clone(),
        service_name,
        SandboxBackendKind::Krun,
        SandboxFilesystemSpec::new("/tmp/rootfs"),
        SandboxProcessSpec::new(["/bin/server"]),
    )
}

pub(super) fn stub_handle(
    id: &SandboxId,
    service_name: &str,
    status: SandboxStatus,
) -> SandboxHandle {
    SandboxHandle::new(
        id.clone(),
        service_name,
        SandboxBackendKind::Krun,
        status,
        Vec::new(),
    )
}

#[derive(Default)]
pub(super) struct StubBackend {
    pub(super) handles: Mutex<BTreeMap<String, SandboxHandle>>,
    pub(super) started_services: Mutex<Vec<String>>,
    pub(super) stopped_ids: Mutex<Vec<String>>,
}

impl StubBackend {
    pub(super) fn with_handles(handles: impl IntoIterator<Item = SandboxHandle>) -> Self {
        let backend = Self::default();
        for handle in handles {
            backend
                .handles
                .lock()
                .expect("handles lock should hold")
                .insert(handle.id.as_str().to_owned(), handle);
        }
        backend
    }
}

#[derive(Default)]
pub(super) struct StubMachineApiSandboxBackend;

impl SandboxBackend for StubMachineApiSandboxBackend {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Container
    }

    fn start(&self, spec: SandboxSpec) -> SandboxFuture<SandboxHandle> {
        let handle = SandboxHandle::new(
            SandboxId::new(format!("{}-01stub", spec.name)),
            &spec.name,
            SandboxBackendKind::Container,
            SandboxStatus::Ready,
            Vec::new(),
        );
        Box::pin(async move { Ok(handle) })
    }

    fn start_from_image(&self, launch: SandboxImageLaunchSpec) -> SandboxFuture<SandboxHandle> {
        self.start(launch.spec)
    }

    fn start_from_build(&self, launch: SandboxBuildLaunchSpec) -> SandboxFuture<SandboxHandle> {
        self.start(launch.spec)
    }

    fn inspect(&self, _id: &SandboxId) -> SandboxFuture<Option<SandboxHandle>> {
        Box::pin(async move { Ok(None) })
    }

    fn stop(&self, _id: &SandboxId) -> SandboxFuture<()> {
        Box::pin(async move { Ok(()) })
    }
}

impl SandboxBackend for StubBackend {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Krun
    }

    fn start(&self, spec: SandboxSpec) -> SandboxFuture<SandboxHandle> {
        let handle = stub_handle(
            &SandboxId::new(format!("{}-01stub", spec.name)),
            &spec.name,
            SandboxStatus::Starting,
        );
        self.handles
            .lock()
            .expect("handles lock should hold")
            .insert(handle.id.as_str().to_owned(), handle.clone());
        Box::pin(async move { Ok(handle) })
    }

    fn start_from_image(&self, launch: SandboxImageLaunchSpec) -> SandboxFuture<SandboxHandle> {
        self.started_services
            .lock()
            .expect("started services lock should hold")
            .push(launch.spec.name.clone());
        self.start(launch.spec)
    }

    fn start_from_build(&self, launch: SandboxBuildLaunchSpec) -> SandboxFuture<SandboxHandle> {
        self.started_services
            .lock()
            .expect("started services lock should hold")
            .push(launch.spec.name.clone());
        self.start(launch.spec)
    }

    fn inspect(&self, id: &SandboxId) -> SandboxFuture<Option<SandboxHandle>> {
        let handle = self
            .handles
            .lock()
            .expect("handles lock should hold")
            .get(id.as_str())
            .cloned();
        Box::pin(async move { Ok(handle) })
    }

    fn stop(&self, id: &SandboxId) -> SandboxFuture<()> {
        self.stopped_ids
            .lock()
            .expect("stopped ids lock should hold")
            .push(id.as_str().to_owned());
        if let Some(handle) = self
            .handles
            .lock()
            .expect("handles lock should hold")
            .get_mut(id.as_str())
        {
            handle.status = SandboxStatus::Stopped;
        }
        Box::pin(async move { Ok(()) })
    }
}
