use std::collections::BTreeSet;
use std::ops::RangeInclusive;
use std::path::PathBuf;

use nimbus_core::TenantId;
use serde::Deserialize;

use super::buildah::{OciExposedPort, OciExposedPortProtocol};
use crate::artifact_paths;
use crate::error::{Result, SandboxError};
use crate::instance::SandboxStatus;
use crate::spec::SandboxPortBinding;

pub(crate) const DEFAULT_MAX_PORTS_PER_TENANT: usize = 128;

#[derive(Debug, Clone)]
pub(crate) struct PortManager {
    range: RangeInclusive<u16>,
    state_root: PathBuf,
    max_ports_per_tenant: Option<usize>,
}

impl PortManager {
    pub(crate) fn new(state_root: impl Into<PathBuf>, range: RangeInclusive<u16>) -> Self {
        Self {
            range,
            state_root: state_root.into(),
            max_ports_per_tenant: None,
        }
    }

    pub(crate) fn with_max_ports_per_tenant(mut self, max_ports_per_tenant: Option<usize>) -> Self {
        self.max_ports_per_tenant = max_ports_per_tenant;
        self
    }

    pub(crate) fn allocate_missing_bindings_for_tenant(
        &self,
        tenant_id: &TenantId,
        existing_bindings: &[SandboxPortBinding],
        exposed_ports: &[OciExposedPort],
    ) -> Result<Vec<SandboxPortBinding>> {
        let mut used_host_ports = self.read_used_host_ports()?;
        used_host_ports.extend(existing_bindings.iter().map(|binding| binding.host_port));

        let mut mapped_guest_ports: BTreeSet<u16> = existing_bindings
            .iter()
            .map(|binding| binding.guest_port)
            .collect();
        let mut unmapped_tcp_guest_ports = Vec::new();

        for exposed_port in exposed_ports {
            if exposed_port.protocol != OciExposedPortProtocol::Tcp {
                continue;
            }
            if !mapped_guest_ports.insert(exposed_port.port) {
                continue;
            }
            unmapped_tcp_guest_ports.push(exposed_port.port);
        }

        self.ensure_tenant_port_quota(
            tenant_id,
            existing_bindings
                .len()
                .saturating_add(unmapped_tcp_guest_ports.len()),
        )?;

        let mut allocated = Vec::new();
        for guest_port in unmapped_tcp_guest_ports {
            let host_port = self.next_available_host_port(&used_host_ports)?;
            used_host_ports.insert(host_port);
            allocated.push(SandboxPortBinding::tcp(
                auto_binding_name(guest_port),
                host_port,
                guest_port,
            ));
        }

        Ok(allocated)
    }

    pub(crate) fn allocate_internal_host_port(
        &self,
        existing_bindings: &[SandboxPortBinding],
    ) -> Result<u16> {
        let mut used_host_ports = self.read_used_host_ports()?;
        used_host_ports.extend(existing_bindings.iter().map(|binding| binding.host_port));
        self.next_available_host_port(&used_host_ports)
    }

    fn next_available_host_port(&self, used_host_ports: &BTreeSet<u16>) -> Result<u16> {
        self.range
            .clone()
            .find(|port| !used_host_ports.contains(port))
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "published port range {}-{} is exhausted",
                    self.range.start(),
                    self.range.end()
                ),
            })
    }

    fn read_used_host_ports(&self) -> Result<BTreeSet<u16>> {
        let mut used_host_ports = BTreeSet::new();
        for manifest_path in
            artifact_paths::all_manifest_paths(&self.state_root).map_err(|error| {
                SandboxError::OperationFailed {
                    message: format!(
                        "failed to read port-manager tenant state directory {}: {error}",
                        self.state_root.display()
                    ),
                }
            })?
        {
            let contents =
                std::fs::read(&manifest_path).map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "failed to read sandbox manifest {}: {error}",
                        manifest_path.display()
                    ),
                })?;
            let manifest: PortLeaseManifest =
                serde_json::from_slice(&contents).map_err(|error| {
                    SandboxError::OperationFailed {
                        message: format!(
                            "failed to parse sandbox manifest {} for port leasing: {error}",
                            manifest_path.display()
                        ),
                    }
                })?;

            if !manifest.status.reserves_ports() {
                continue;
            }

            used_host_ports.extend(
                manifest
                    .spec
                    .port_bindings
                    .into_iter()
                    .map(|binding| binding.host_port),
            );
            if let Some(egress_proxy) = manifest.egress_proxy {
                used_host_ports.insert(egress_proxy.port);
            }
        }

        Ok(used_host_ports)
    }

    fn ensure_tenant_port_quota(&self, tenant_id: &TenantId, launch_ports: usize) -> Result<()> {
        let Some(max_ports_per_tenant) = self.max_ports_per_tenant else {
            return Ok(());
        };
        let active_ports = self.read_reserved_port_count_for_tenant(tenant_id)?;
        let requested_ports = active_ports.saturating_add(launch_ports);
        if requested_ports <= max_ports_per_tenant {
            return Ok(());
        }
        Err(SandboxError::OperationFailed {
            message: format!(
                "published port quota exceeded for tenant {tenant_id}: {requested_ports} requested/reserved ports exceeds limit {max_ports_per_tenant}"
            ),
        })
    }

    fn read_reserved_port_count_for_tenant(&self, tenant_id: &TenantId) -> Result<usize> {
        let mut reserved_ports = 0usize;
        for manifest_path in artifact_paths::manifest_paths_for_tenant(&self.state_root, tenant_id)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to read port-manager tenant state directory {} for tenant {tenant_id}: {error}",
                    self.state_root.display()
                ),
            })?
        {
            let contents =
                std::fs::read(&manifest_path).map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "failed to read sandbox manifest {}: {error}",
                        manifest_path.display()
                    ),
                })?;
            let manifest: PortLeaseManifest =
                serde_json::from_slice(&contents).map_err(|error| {
                    SandboxError::OperationFailed {
                        message: format!(
                            "failed to parse sandbox manifest {} for tenant port quota: {error}",
                            manifest_path.display()
                        ),
                    }
                })?;

            if manifest.status.reserves_ports() {
                reserved_ports =
                    reserved_ports.saturating_add(manifest.spec.port_bindings.len());
            }
        }
        Ok(reserved_ports)
    }
}

fn auto_binding_name(guest_port: u16) -> String {
    format!("tcp-{guest_port}")
}

#[derive(Debug, Deserialize)]
struct PortLeaseManifest {
    status: SandboxStatus,
    spec: PortLeaseSpec,
    egress_proxy: Option<PortLeaseEgressProxy>,
}

#[derive(Debug, Deserialize)]
struct PortLeaseSpec {
    port_bindings: Vec<SandboxPortBinding>,
}

#[derive(Debug, Deserialize)]
struct PortLeaseEgressProxy {
    port: u16,
}

impl SandboxStatus {
    fn reserves_ports(self) -> bool {
        matches!(
            self,
            Self::Starting | Self::Ready | Self::NotReady | Self::Stopping
        )
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{self, OpenOptions};
    use std::io::{BufRead as _, Read as _, Write as _};
    use std::process::{Child, ChildStdin, Command, Stdio};
    use std::sync::{Arc, Mutex, mpsc};
    use std::thread::JoinHandle;
    use std::time::Duration;

    use serde_json::json;
    use tempfile::TempDir;

    use super::PortManager;
    use crate::artifact_paths;
    use crate::backends::oci::buildah::{OciExposedPort, OciExposedPortProtocol};
    use crate::instance::{SandboxId, SandboxStatus};
    use crate::spec::SandboxPortBinding;
    use nimbus_core::TenantId;

    const ALLOCATOR_CHILD_TEST: &str =
        "backends::oci::port_manager::tests::sandbox_and_pep_allocator_child";
    const ALLOCATOR_KIND_ENV: &str = "NIMBUS_PORT_MANAGER_ALLOCATOR_KIND";
    const ALLOCATOR_ROLE_ENV: &str = "NIMBUS_PORT_MANAGER_ALLOCATOR_ROLE";
    const ALLOCATOR_STATE_ROOT_ENV: &str = "NIMBUS_PORT_MANAGER_ALLOCATOR_STATE_ROOT";
    const ALLOCATOR_PROTOCOL_PREFIX: &str = "NIMBUS_PORT_MANAGER_ALLOCATOR/1\t";
    const CHARACTERIZATION_PORT_MIN: u16 = 41_337;
    const CHARACTERIZATION_PORT_MAX: u16 = 41_338;

    #[test]
    #[ignore = "NNC0.2 expected red until sandbox and PEP share host-port lease authority"]
    fn two_real_allocator_processes_expose_sandbox_pep_port_collision() {
        let state_root = TempDir::new().expect("shared state root should exist");
        let mut sandbox = AllocatorProcess::spawn("sandbox", "sandbox", state_root.path())
            .expect("sandbox child");
        let mut pep = AllocatorProcess::spawn("pep", "pep", state_root.path()).expect("PEP child");
        assert_ne!(
            sandbox.process_id(),
            pep.process_id(),
            "the allocators must execute in distinct OS processes"
        );

        sandbox
            .wait_ready(Duration::from_secs(5))
            .expect("sandbox allocator should reach the release barrier");
        pep.wait_ready(Duration::from_secs(5))
            .expect("PEP allocator should reach the release barrier");
        sandbox.release().expect("sandbox child should release");
        pep.release().expect("PEP child should release");
        let sandbox_reported = sandbox
            .wait_selected(Duration::from_secs(5))
            .expect("sandbox allocator should report its selected port");
        let pep_reported = pep
            .wait_selected(Duration::from_secs(5))
            .expect("PEP allocator should report its selected port");

        let sandbox_port = read_characterized_port(state_root.path(), "sandbox");
        let pep_port = read_characterized_port(state_root.path(), "pep");
        assert_eq!(sandbox_port, sandbox_reported);
        assert_eq!(pep_port, pep_reported);
        assert!((CHARACTERIZATION_PORT_MIN..=CHARACTERIZATION_PORT_MAX).contains(&sandbox_port));
        assert!((CHARACTERIZATION_PORT_MIN..=CHARACTERIZATION_PORT_MAX).contains(&pep_port));
        assert_ne!(
            sandbox_port, pep_port,
            "sandbox and PEP allocations must hold distinct host-port leases"
        );
    }

    #[test]
    #[ignore = "spawned only by the sandbox/PEP contention characterization"]
    fn sandbox_and_pep_allocator_child() {
        let state_root = std::env::var_os(ALLOCATOR_STATE_ROOT_ENV)
            .map(std::path::PathBuf::from)
            .expect("allocator child state root should be set");
        let role = std::env::var(ALLOCATOR_ROLE_ENV).expect("allocator child role should be set");
        emit_allocator_checkpoint("ready");
        let mut command = String::new();
        std::io::stdin()
            .read_line(&mut command)
            .expect("allocator child should read its release command");
        assert_eq!(
            command.trim_end(),
            format!("{ALLOCATOR_PROTOCOL_PREFIX}release")
        );

        let manager = PortManager::new(
            &state_root,
            CHARACTERIZATION_PORT_MIN..=CHARACTERIZATION_PORT_MAX,
        );
        let allocated_port = match std::env::var(ALLOCATOR_KIND_ENV).as_deref() {
            Ok("sandbox") => {
                manager
                    .allocate_missing_bindings_for_tenant(
                        &tenant_id("contention-tenant"),
                        &[],
                        &[tcp_exposed_port(8080)],
                    )
                    .expect("sandbox allocator should select its only configured port")
                    .into_iter()
                    .next()
                    .expect("sandbox allocation should return one binding")
                    .host_port
            }
            Ok("pep") => manager
                .allocate_internal_host_port(&[])
                .expect("PEP allocator should select its only configured port"),
            Ok(other) => panic!("unknown allocator kind {other:?}"),
            Err(error) => {
                panic!("missing allocator kind in {ALLOCATOR_KIND_ENV}: {error}");
            }
        };
        persist_characterized_port(&state_root, &role, allocated_port)
            .expect("child should persist its selected port");
        emit_allocator_checkpoint(&format!("selected:{allocated_port}"));
    }

    #[test]
    fn allocate_missing_bindings_uses_range_and_skips_existing_guest_ports() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let tenant_id = tenant_id("tenant-a");
        let manager = PortManager::new(temp_dir.path(), 15000..=15005);
        let existing = vec![SandboxPortBinding::tcp("http", 18080, 8080)];
        let exposed = vec![
            tcp_exposed_port(8080),
            tcp_exposed_port(5432),
            udp_exposed_port(5353),
        ];

        let allocated = manager
            .allocate_missing_bindings_for_tenant(&tenant_id, &existing, &exposed)
            .expect("port allocation should succeed");

        assert_eq!(
            allocated,
            vec![SandboxPortBinding::tcp("tcp-5432", 15000, 5432)]
        );
    }

    #[test]
    fn allocate_missing_bindings_ignores_stopped_manifests_and_reserves_active_ones() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let tenant_id = tenant_id("tenant-a");
        write_manifest(
            temp_dir.path(),
            &tenant_id,
            "active",
            SandboxStatus::Ready,
            &[(15000, 5432)],
        );
        write_manifest(
            temp_dir.path(),
            &tenant_id,
            "stopped",
            SandboxStatus::Stopped,
            &[(15001, 5432)],
        );

        let manager = PortManager::new(temp_dir.path(), 15000..=15002);
        let allocated = manager
            .allocate_missing_bindings_for_tenant(
                &tenant_id,
                &[],
                &[tcp_exposed_port(8080), tcp_exposed_port(8443)],
            )
            .expect("port allocation should succeed");

        assert_eq!(
            allocated,
            vec![
                SandboxPortBinding::tcp("tcp-8080", 15001, 8080),
                SandboxPortBinding::tcp("tcp-8443", 15002, 8443),
            ]
        );
    }

    #[test]
    fn allocate_missing_bindings_keeps_not_ready_ports_reserved() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let tenant_id = tenant_id("tenant-a");
        write_manifest(
            temp_dir.path(),
            &tenant_id,
            "not-ready",
            SandboxStatus::NotReady,
            &[(15000, 5432)],
        );

        let manager = PortManager::new(temp_dir.path(), 15000..=15001);
        let allocated = manager
            .allocate_missing_bindings_for_tenant(&tenant_id, &[], &[tcp_exposed_port(8080)])
            .expect("port allocation should succeed");

        assert_eq!(
            allocated,
            vec![SandboxPortBinding::tcp("tcp-8080", 15001, 8080)]
        );
    }

    #[test]
    fn allocate_internal_host_port_skips_active_egress_proxy_leases() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let tenant_id = tenant_id("tenant-a");
        write_manifest_with_egress_proxy(
            temp_dir.path(),
            &tenant_id,
            "active",
            SandboxStatus::Ready,
            15000,
        );

        let manager = PortManager::new(temp_dir.path(), 15000..=15001);
        let allocated = manager
            .allocate_internal_host_port(&[])
            .expect("internal port allocation should skip active proxy leases");

        assert_eq!(allocated, 15001);
    }

    #[test]
    fn allocate_internal_host_port_ignores_stopped_egress_proxy_leases() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let tenant_id = tenant_id("tenant-a");
        write_manifest_with_egress_proxy(
            temp_dir.path(),
            &tenant_id,
            "stopped",
            SandboxStatus::Stopped,
            15000,
        );

        let manager = PortManager::new(temp_dir.path(), 15000..=15001);
        let allocated = manager
            .allocate_internal_host_port(&[])
            .expect("stopped proxy lease should not reserve a host port");

        assert_eq!(allocated, 15000);
    }

    #[test]
    fn tenant_port_quota_rejects_explicit_bindings_that_exceed_same_tenant_limit() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let tenant_id = tenant_id("tenant-a");
        write_manifest(
            temp_dir.path(),
            &tenant_id,
            "active",
            SandboxStatus::Ready,
            &[(15000, 5432)],
        );

        let manager =
            PortManager::new(temp_dir.path(), 15000..=15002).with_max_ports_per_tenant(Some(1));
        let existing = vec![SandboxPortBinding::tcp("http", 18080, 8080)];
        let error = manager
            .allocate_missing_bindings_for_tenant(&tenant_id, &existing, &[])
            .expect_err("explicit bindings should still count against the tenant port quota");

        assert!(
            error.to_string().contains("published port quota exceeded")
                && error.to_string().contains("tenant-a")
                && error.to_string().contains("limit 1"),
            "expected tenant quota error, got: {error}"
        );
    }

    #[test]
    fn tenant_port_quota_counts_only_same_tenant_but_reserves_host_ports_globally() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let tenant_a = tenant_id("tenant-a");
        let tenant_b = tenant_id("tenant-b");
        write_manifest(
            temp_dir.path(),
            &tenant_a,
            "active-a",
            SandboxStatus::Ready,
            &[(15000, 5432)],
        );
        write_manifest(
            temp_dir.path(),
            &tenant_b,
            "active-b",
            SandboxStatus::Ready,
            &[(15001, 6379)],
        );

        let manager =
            PortManager::new(temp_dir.path(), 15000..=15002).with_max_ports_per_tenant(Some(2));
        let allocated = manager
            .allocate_missing_bindings_for_tenant(&tenant_a, &[], &[tcp_exposed_port(8080)])
            .expect("other tenant leases should not consume tenant-a quota");

        assert_eq!(
            allocated,
            vec![SandboxPortBinding::tcp("tcp-8080", 15002, 8080)],
            "other tenant leases should still reserve host ports globally"
        );
    }

    #[test]
    #[ignore = "NNC0.9 explicit allocation-scale characterization"]
    fn manifest_scan_port_allocation_scale_baseline() {
        const HOST_PORT_BASE: u16 = 20_000;
        const SAMPLE_COUNT: usize = 21;

        for manifest_count in [0usize, 64, 256, 1_024] {
            let temp_dir = TempDir::new().expect("temporary directory should exist");
            let tenant_id = tenant_id("nnc0-9-port-baseline");
            for index in 0..manifest_count {
                let offset = u16::try_from(index).expect("baseline manifest count fits u16");
                write_manifest(
                    temp_dir.path(),
                    &tenant_id,
                    &format!("baseline-{index:04}"),
                    SandboxStatus::Ready,
                    &[(HOST_PORT_BASE + offset, 10_000 + offset)],
                );
            }

            let manager = PortManager::new(temp_dir.path(), HOST_PORT_BASE..=40_000);
            let expected = HOST_PORT_BASE
                + u16::try_from(manifest_count).expect("baseline manifest count fits u16");
            let mut samples_ns = Vec::with_capacity(SAMPLE_COUNT);
            for _ in 0..SAMPLE_COUNT {
                let started = std::time::Instant::now();
                let selected = manager
                    .allocate_internal_host_port(&[])
                    .expect("baseline allocation should select the first unreserved port");
                samples_ns.push(started.elapsed().as_nanos());
                assert_eq!(
                    selected, expected,
                    "manifest scanning must reserve every active host port"
                );
            }
            samples_ns.sort_unstable();
            let p95_index = (SAMPLE_COUNT * 95).div_ceil(100) - 1;

            println!(
                "NNC0.9 port-allocation-baseline manifests={manifest_count} samples={SAMPLE_COUNT} median_ns={} p95_ns={} selected_port={expected}",
                samples_ns[SAMPLE_COUNT / 2],
                samples_ns[p95_index]
            );
        }
    }

    fn write_manifest(
        state_root: &std::path::Path,
        tenant_id: &TenantId,
        sandbox_id: &str,
        status: SandboxStatus,
        host_guest_ports: &[(u16, u16)],
    ) {
        let sandbox_id = SandboxId::new(sandbox_id);
        let manifest_path = artifact_paths::manifest_path(state_root, tenant_id, &sandbox_id);
        let container_dir = manifest_path
            .parent()
            .expect("manifest path should have a parent directory");
        fs::create_dir_all(container_dir).expect("container manifest directory should exist");
        let manifest = json!({
            "status": status,
            "spec": {
                "port_bindings": host_guest_ports
                    .iter()
                    .map(|(host_port, guest_port)| json!({
                        "name": format!("tcp-{guest_port}"),
                        "protocol": "tcp",
                        "host_address": "127.0.0.1",
                        "host_port": host_port,
                        "guest_port": guest_port,
                    }))
                    .collect::<Vec<_>>(),
            },
        });
        fs::write(
            manifest_path,
            serde_json::to_vec_pretty(&manifest).expect("manifest JSON should serialize"),
        )
        .expect("manifest JSON should be written");
    }

    fn write_manifest_with_egress_proxy(
        state_root: &std::path::Path,
        tenant_id: &TenantId,
        sandbox_id: &str,
        status: SandboxStatus,
        egress_proxy_port: u16,
    ) {
        let sandbox_id = SandboxId::new(sandbox_id);
        let manifest_path = artifact_paths::manifest_path(state_root, tenant_id, &sandbox_id);
        let container_dir = manifest_path
            .parent()
            .expect("manifest path should have a parent directory");
        fs::create_dir_all(container_dir).expect("container manifest directory should exist");
        let manifest = json!({
            "status": status,
            "egress_proxy": {
                "host": "10.89.0.1",
                "port": egress_proxy_port,
            },
            "spec": {
                "port_bindings": [],
            },
        });
        fs::write(
            manifest_path,
            serde_json::to_vec_pretty(&manifest).expect("manifest JSON should serialize"),
        )
        .expect("manifest JSON should be written");
    }

    fn tenant_id(value: &str) -> TenantId {
        TenantId::new(value).expect("tenant id should parse")
    }

    fn tcp_exposed_port(port: u16) -> OciExposedPort {
        OciExposedPort {
            port,
            protocol: OciExposedPortProtocol::Tcp,
            raw: format!("{port}/tcp"),
        }
    }

    fn udp_exposed_port(port: u16) -> OciExposedPort {
        OciExposedPort {
            port,
            protocol: OciExposedPortProtocol::Udp,
            raw: format!("{port}/udp"),
        }
    }

    fn persist_characterized_port(
        state_root: &std::path::Path,
        role: &str,
        port: u16,
    ) -> Result<(), String> {
        let path = state_root.join(format!("{role}.selected-port"));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| format!("failed to create {}: {error}", path.display()))?;
        writeln!(file, "{port}")
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("failed to persist {}: {error}", path.display()))
    }

    fn read_characterized_port(state_root: &std::path::Path, role: &str) -> u16 {
        let path = state_root.join(format!("{role}.selected-port"));
        fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
            .trim()
            .parse()
            .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()))
    }

    fn emit_allocator_checkpoint(checkpoint: &str) {
        let mut stdout = std::io::stdout().lock();
        writeln!(stdout, "{ALLOCATOR_PROTOCOL_PREFIX}{checkpoint}")
            .and_then(|()| stdout.flush())
            .expect("allocator child checkpoint should flush");
    }

    #[derive(Debug)]
    enum AllocatorEvent {
        Ready,
        Selected(u16),
        ProtocolError(String),
        Eof,
    }

    struct AllocatorProcess {
        role: String,
        child: Child,
        stdin: Option<ChildStdin>,
        events: mpsc::Receiver<AllocatorEvent>,
        stdout: Arc<Mutex<String>>,
        stderr: Arc<Mutex<String>>,
        stdout_reader: Option<JoinHandle<()>>,
        stderr_reader: Option<JoinHandle<()>>,
    }

    impl AllocatorProcess {
        fn spawn(
            role: &str,
            allocator_kind: &str,
            state_root: &std::path::Path,
        ) -> Result<Self, String> {
            let mut child = Command::new(
                std::env::current_exe()
                    .map_err(|error| format!("failed to resolve sandbox test binary: {error}"))?,
            )
            .arg("--exact")
            .arg(ALLOCATOR_CHILD_TEST)
            .arg("--ignored")
            .arg("--nocapture")
            .env(ALLOCATOR_KIND_ENV, allocator_kind)
            .env(ALLOCATOR_ROLE_ENV, role)
            .env(ALLOCATOR_STATE_ROOT_ENV, state_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("failed to spawn allocator role {role:?}: {error}"))?;
            let stdin = child
                .stdin
                .take()
                .expect("piped allocator stdin should be present");
            let stdout = child
                .stdout
                .take()
                .expect("piped allocator stdout should be present");
            let stderr = child
                .stderr
                .take()
                .expect("piped allocator stderr should be present");

            let stdout_capture = Arc::new(Mutex::new(String::new()));
            let stdout_target = Arc::clone(&stdout_capture);
            let (event_tx, events) = mpsc::sync_channel(4);
            let stdout_reader = std::thread::spawn(move || {
                let mut reader = std::io::BufReader::new(stdout);
                loop {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) => {
                            let _ = event_tx.send(AllocatorEvent::Eof);
                            return;
                        }
                        Ok(_) => {
                            stdout_target
                                .lock()
                                .expect("allocator stdout lock should not be poisoned")
                                .push_str(&line);
                            let Some(value) =
                                line.trim_end().strip_prefix(ALLOCATOR_PROTOCOL_PREFIX)
                            else {
                                continue;
                            };
                            let event = match value {
                                "ready" => AllocatorEvent::Ready,
                                selected if selected.starts_with("selected:") => selected
                                    .trim_start_matches("selected:")
                                    .parse::<u16>()
                                    .map(AllocatorEvent::Selected)
                                    .unwrap_or_else(|error| {
                                        AllocatorEvent::ProtocolError(format!(
                                            "invalid selected port {selected:?}: {error}"
                                        ))
                                    }),
                                other => AllocatorEvent::ProtocolError(format!(
                                    "unknown allocator checkpoint {other:?}"
                                )),
                            };
                            if event_tx.send(event).is_err() {
                                return;
                            }
                        }
                        Err(error) => {
                            let _ = event_tx.send(AllocatorEvent::ProtocolError(format!(
                                "allocator stdout read failed: {error}"
                            )));
                            return;
                        }
                    }
                }
            });

            let stderr_capture = Arc::new(Mutex::new(String::new()));
            let stderr_target = Arc::clone(&stderr_capture);
            let stderr_reader = std::thread::spawn(move || {
                let mut reader = std::io::BufReader::new(stderr);
                let mut captured = String::new();
                if let Err(error) = reader.read_to_string(&mut captured) {
                    captured.push_str(&format!("\n<stderr read failed: {error}>"));
                }
                *stderr_target
                    .lock()
                    .expect("allocator stderr lock should not be poisoned") = captured;
            });

            Ok(Self {
                role: role.to_owned(),
                child,
                stdin: Some(stdin),
                events,
                stdout: stdout_capture,
                stderr: stderr_capture,
                stdout_reader: Some(stdout_reader),
                stderr_reader: Some(stderr_reader),
            })
        }

        fn process_id(&self) -> u32 {
            self.child.id()
        }

        fn wait_ready(&mut self, timeout: Duration) -> Result<(), String> {
            match self.receive(timeout, "ready")? {
                AllocatorEvent::Ready => Ok(()),
                event => Err(self.unexpected("ready", &event)),
            }
        }

        fn release(&mut self) -> Result<(), String> {
            let stdin = self
                .stdin
                .as_mut()
                .ok_or_else(|| format!("allocator role {:?} stdin is closed", self.role))?;
            writeln!(stdin, "{ALLOCATOR_PROTOCOL_PREFIX}release")
                .and_then(|()| stdin.flush())
                .map_err(|error| {
                    format!("failed to release allocator role {:?}: {error}", self.role)
                })
        }

        fn wait_selected(&mut self, timeout: Duration) -> Result<u16, String> {
            match self.receive(timeout, "selected port")? {
                AllocatorEvent::Selected(port) => Ok(port),
                event => Err(self.unexpected("selected port", &event)),
            }
        }

        fn receive(&mut self, timeout: Duration, expected: &str) -> Result<AllocatorEvent, String> {
            match self.events.recv_timeout(timeout) {
                Ok(event) => Ok(event),
                Err(error) => {
                    let role = self.role.clone();
                    let diagnostic = self.diagnostic();
                    Err(format!(
                        "allocator role {role:?} did not reach {expected:?} within {timeout:?}: {error}; {diagnostic}"
                    ))
                }
            }
        }

        fn unexpected(&mut self, expected: &str, event: &AllocatorEvent) -> String {
            let event = match event {
                AllocatorEvent::ProtocolError(message) => message.clone(),
                other => format!("{other:?}"),
            };
            let role = self.role.clone();
            let diagnostic = self.diagnostic();
            format!("allocator role {role:?} reached {event}; expected {expected}; {diagnostic}")
        }

        fn diagnostic(&mut self) -> String {
            let status = self
                .child
                .try_wait()
                .map(|status| format!("{status:?}"))
                .unwrap_or_else(|error| format!("<status error: {error}>"));
            let stdout = self
                .stdout
                .lock()
                .expect("allocator stdout lock should not be poisoned");
            let stderr = self
                .stderr
                .lock()
                .expect("allocator stderr lock should not be poisoned");
            format!("status={status}; stdout={stdout:?}; stderr={stderr:?}")
        }
    }

    impl Drop for AllocatorProcess {
        fn drop(&mut self) {
            drop(self.stdin.take());
            let _ = self.child.kill();
            let _ = self.child.wait();
            if let Some(reader) = self.stdout_reader.take() {
                let _ = reader.join();
            }
            if let Some(reader) = self.stderr_reader.take() {
                let _ = reader.join();
            }
        }
    }
}
