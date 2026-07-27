use super::*;
use nimbus::SandboxBackend;

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

pub(super) use crate::test_support::with_current_dir;

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

pub(super) fn write_manifest(
    state_root: &Path,
    sandbox_id: &str,
    tenant_id: &str,
    service_name: &str,
    status: SandboxStatus,
) {
    let container_dir = state_root
        .join("tenants")
        .join(tenant_id)
        .join("sandboxes")
        .join(sandbox_id)
        .join("state")
        .join("containers")
        .join(sandbox_id);
    fs::create_dir_all(&container_dir).expect("container directory should build");

    let handle = nimbus::SandboxHandle::new(
        nimbus::TenantId::new(tenant_id).expect("tenant id should parse"),
        nimbus::SandboxId::new(sandbox_id),
        service_name,
        nimbus::SandboxBackendKind::Krun,
        status,
        vec![nimbus::PublishedEndpoint::new(
            "http",
            nimbus::EndpointProtocol::Tcp,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 18080),
        )],
    );
    let manifest = json!({
        "handle": handle,
        "spec": {
            "tenant_id": tenant_id,
            "owner": {
                "kind": "service",
                "name": service_name
            },
            "backend": "krun",
            "root": {
                "kind": "rootfs",
                "rootfs": "/tmp/rootfs",
                "readonly": true
            },
            "process": {
                "args": ["/bin/server"],
                "env": ["PATH=/usr/bin"],
                "cwd": "/",
                "terminal": false
            },
            "resources": nimbus::SandboxResourceLimits::default(),
            "lifecycle": {
                "restart_policy": "never"
            },
            "port_bindings": [nimbus::SandboxPortBinding::tcp("http", 18080, 8080)]
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
        SandboxOwnerSpec::service(service_name),
        SandboxBackendKind::Krun,
        SandboxRootSpec::rootfs("/tmp/rootfs"),
        SandboxProcessSpec::new(["/bin/server"]),
    )
}

pub(super) fn stub_handle(
    id: &SandboxId,
    service_name: &str,
    status: SandboxStatus,
) -> SandboxHandle {
    SandboxHandle::new(
        nimbus::TenantId::new("tenant").expect("tenant id should parse"),
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
        let service_name = spec.display_name().to_owned();
        let handle = SandboxHandle::new(
            spec.tenant_id.clone(),
            SandboxId::new(format!("{service_name}-01stub")),
            service_name,
            SandboxBackendKind::Container,
            SandboxStatus::Ready,
            Vec::new(),
        );
        Box::pin(async move { Ok(handle) })
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
        let service_name = spec.display_name().to_owned();
        let handle = stub_handle(
            &SandboxId::new(format!("{service_name}-01stub")),
            &service_name,
            SandboxStatus::Starting,
        );
        self.started_services
            .lock()
            .expect("started services lock should hold")
            .push(service_name);
        self.handles
            .lock()
            .expect("handles lock should hold")
            .insert(handle.id.as_str().to_owned(), handle.clone());
        Box::pin(async move { Ok(handle) })
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
