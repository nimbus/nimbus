use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nimbus_core::{Document, Error, Result, TableName, TenantId};
use nimbus_engine::Engine;
use nimbus_sandbox::{
    SandboxHandle, SandboxSpec, SandboxStatus, validate_sandbox_mounts, validate_tenant_volume_name,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use TenantIsolationDriftSurface::{
    DecisionAudit, RouteMetadata, SandboxManifest, ServiceHandle, SystemPort, TenantVolume,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TenantIsolationDriftScanConfig {
    sandbox_state_roots: Vec<PathBuf>,
    require_decision_audit_records: bool,
}

impl TenantIsolationDriftScanConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_sandbox_state_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.sandbox_state_roots.push(root.into());
        self
    }

    pub fn require_decision_audit_records(mut self, require: bool) -> Self {
        self.require_decision_audit_records = require;
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TenantIsolationDriftReport {
    violations: Vec<TenantIsolationDriftViolation>,
}

impl TenantIsolationDriftReport {
    pub fn is_clean(&self) -> bool {
        self.violations.is_empty()
    }

    pub fn violations(&self) -> &[TenantIsolationDriftViolation] {
        &self.violations
    }

    fn push(
        &mut self,
        surface: TenantIsolationDriftSurface,
        code: &'static str,
        location: impl Into<String>,
        message: impl Into<String>,
    ) {
        self.violations.push(TenantIsolationDriftViolation {
            surface,
            code: code.to_owned(),
            location: location.into(),
            message: message.into(),
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantIsolationDriftViolation {
    pub surface: TenantIsolationDriftSurface,
    pub code: String,
    pub location: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TenantIsolationDriftSurface {
    SandboxManifest,
    ServiceHandle,
    SystemPort,
    TenantVolume,
    RouteMetadata,
    DecisionAudit,
}

pub async fn scan_tenant_isolation_drift_async(
    engine: &Arc<Engine>,
    config: &TenantIsolationDriftScanConfig,
) -> Result<TenantIsolationDriftReport> {
    let mut report = TenantIsolationDriftReport::default();
    let observed = scan_sandbox_state_roots(config, &mut report);
    let services = list_system_documents(engine, "services", &mut report).await?;
    let ports = list_system_documents(engine, "ports", &mut report).await?;
    let routes = list_system_documents(engine, "routes", &mut report).await?;

    let service_records = parse_service_records(&services, config, &mut report);
    let port_records = parse_port_records(&ports, &service_records, &observed, &mut report);

    validate_observed_manifests(&observed, &service_records, &port_records, &mut report);
    validate_route_metadata(&routes, &mut report);

    report.violations.sort_by(|left, right| {
        (left.surface, &left.code, &left.location).cmp(&(
            right.surface,
            &right.code,
            &right.location,
        ))
    });
    Ok(report)
}

async fn list_system_documents(
    engine: &Arc<Engine>,
    table: &str,
    report: &mut TenantIsolationDriftReport,
) -> Result<Vec<Document>> {
    let system_tenant = crate::system_tenant::system_tenant_id()?;
    let table_name = TableName::new(table.to_owned())?;
    match engine.list_documents_async(system_tenant, table_name).await {
        Ok(documents) => Ok(documents),
        Err(Error::TenantNotFound(_)) | Err(Error::SchemaNotFound(_)) => {
            let surface = match table {
                "services" => ServiceHandle,
                "ports" => SystemPort,
                "routes" => RouteMetadata,
                _ => RouteMetadata,
            };
            report.push(
                surface,
                "system_metadata_missing",
                format!("_nimbus/{table}"),
                format!("system table `{table}` is missing; run system tenant preparation before relying on drift output"),
            );
            Ok(Vec::new())
        }
        Err(error) => Err(error),
    }
}

fn scan_sandbox_state_roots(
    config: &TenantIsolationDriftScanConfig,
    report: &mut TenantIsolationDriftReport,
) -> Vec<ObservedSandboxManifest> {
    let mut observed = Vec::new();
    for root in &config.sandbox_state_roots {
        scan_tenant_volume_roots(root, report);
        let Ok(pointers) = collect_manifest_pointers(root, report) else {
            continue;
        };
        for pointer in pointers {
            match read_sandbox_manifest(&pointer) {
                Ok(manifest) => {
                    validate_manifest_shape(&pointer, &manifest, report);
                    observed.push(ObservedSandboxManifest {
                        pointer,
                        tenant_id: manifest.spec.tenant_id.as_str().to_owned(),
                        service_name: manifest.spec.service_name().map(ToOwned::to_owned),
                        sandbox_id: manifest.handle.id.as_str().to_owned(),
                        status: manifest.status,
                        handle: manifest.handle,
                    });
                }
                Err(message) => report.push(
                    SandboxManifest,
                    "malformed_sandbox_manifest",
                    pointer.manifest_path.display().to_string(),
                    message,
                ),
            }
        }
    }
    observed
}

fn scan_tenant_volume_roots(root: &Path, report: &mut TenantIsolationDriftReport) {
    let tenants_root = root.join("tenants");
    let Ok(tenant_entries) = fs::read_dir(&tenants_root) else {
        return;
    };
    for tenant_entry in tenant_entries.flatten() {
        let tenant_name = tenant_entry.file_name().to_string_lossy().into_owned();
        if TenantId::new(tenant_name.clone()).is_err() {
            report.push(
                TenantVolume,
                "tenant_volume_root_invalid_tenant",
                tenant_entry.path().display().to_string(),
                format!("tenant volume root is under invalid tenant id `{tenant_name}`"),
            );
        }
        let volumes_root = tenant_entry.path().join("volumes");
        let Ok(volume_entries) = fs::read_dir(&volumes_root) else {
            continue;
        };
        for volume_entry in volume_entries.flatten() {
            let volume_name = volume_entry.file_name().to_string_lossy().into_owned();
            if let Err(error) = validate_tenant_volume_name(&volume_name) {
                report.push(
                    TenantVolume,
                    "tenant_volume_root_invalid_name",
                    volume_entry.path().display().to_string(),
                    error,
                );
            }
        }
    }
}

fn collect_manifest_pointers(
    root: &Path,
    report: &mut TenantIsolationDriftReport,
) -> std::result::Result<Vec<ManifestPointer>, ()> {
    let tenants_root = root.join("tenants");
    let tenant_entries = match fs::read_dir(&tenants_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            report.push(
                SandboxManifest,
                "sandbox_state_root_unreadable",
                tenants_root.display().to_string(),
                format!("failed to read sandbox tenant root: {error}"),
            );
            return Err(());
        }
    };

    let mut pointers = Vec::new();
    for tenant_entry in tenant_entries.flatten() {
        let tenant_from_path = tenant_entry.file_name().to_string_lossy().into_owned();
        let sandboxes_root = tenant_entry.path().join("sandboxes");
        let Ok(sandbox_entries) = fs::read_dir(&sandboxes_root) else {
            continue;
        };
        for sandbox_entry in sandbox_entries.flatten() {
            let sandbox_from_path = sandbox_entry.file_name().to_string_lossy().into_owned();
            let containers_root = sandbox_entry.path().join("state").join("containers");
            let Ok(container_entries) = fs::read_dir(&containers_root) else {
                continue;
            };
            for container_entry in container_entries.flatten() {
                let container_from_path =
                    container_entry.file_name().to_string_lossy().into_owned();
                let manifest_path = container_entry.path().join("manifest.json");
                if manifest_path.exists() {
                    pointers.push(ManifestPointer {
                        root: root.to_path_buf(),
                        tenant_from_path: tenant_from_path.clone(),
                        sandbox_from_path: sandbox_from_path.clone(),
                        container_from_path,
                        manifest_path,
                    });
                }
            }
        }
    }
    pointers.sort_by(|left, right| left.manifest_path.cmp(&right.manifest_path));
    Ok(pointers)
}

fn read_sandbox_manifest(
    pointer: &ManifestPointer,
) -> std::result::Result<PersistedSandboxManifest, String> {
    let bytes = fs::read(&pointer.manifest_path).map_err(|error| {
        format!(
            "failed to read sandbox manifest {}: {error}",
            pointer.manifest_path.display()
        )
    })?;
    serde_json::from_slice::<PersistedSandboxManifest>(&bytes).map_err(|error| {
        format!(
            "failed to parse sandbox manifest {}: {error}",
            pointer.manifest_path.display()
        )
    })
}

fn validate_manifest_shape(
    pointer: &ManifestPointer,
    manifest: &PersistedSandboxManifest,
    report: &mut TenantIsolationDriftReport,
) {
    let location = pointer.manifest_path.display().to_string();
    let spec_tenant = manifest.spec.tenant_id.as_str();
    let handle_tenant = manifest.handle.tenant_id.as_str();
    let handle_id = manifest.handle.id.as_str();

    if pointer.tenant_from_path != spec_tenant {
        report.push(
            SandboxManifest,
            "sandbox_manifest_tenant_path_mismatch",
            &location,
            format!(
                "manifest path tenant `{}` does not match spec tenant `{spec_tenant}`",
                pointer.tenant_from_path
            ),
        );
    }
    if pointer.tenant_from_path != handle_tenant {
        report.push(
            SandboxManifest,
            "sandbox_manifest_handle_tenant_path_mismatch",
            &location,
            format!(
                "manifest path tenant `{}` does not match handle tenant `{handle_tenant}`",
                pointer.tenant_from_path
            ),
        );
    }
    if pointer.sandbox_from_path != handle_id || pointer.container_from_path != handle_id {
        report.push(
            SandboxManifest,
            "sandbox_manifest_id_path_mismatch",
            &location,
            format!(
                "manifest path sandbox `{}`/container `{}` does not match handle sandbox `{handle_id}`",
                pointer.sandbox_from_path, pointer.container_from_path
            ),
        );
    }
    if spec_tenant != handle_tenant {
        report.push(
            SandboxManifest,
            "sandbox_manifest_handle_tenant_mismatch",
            &location,
            format!("spec tenant `{spec_tenant}` does not match handle tenant `{handle_tenant}`"),
        );
    }
    if manifest.spec.display_name() != manifest.handle.name {
        report.push(
            SandboxManifest,
            "sandbox_manifest_handle_name_mismatch",
            &location,
            format!(
                "spec display name `{}` does not match handle name `{}`",
                manifest.spec.display_name(),
                manifest.handle.name
            ),
        );
    }
    if manifest.spec.backend != manifest.handle.backend {
        report.push(
            SandboxManifest,
            "sandbox_manifest_backend_mismatch",
            &location,
            "spec backend does not match handle backend",
        );
    }
    if manifest.status != manifest.handle.status {
        report.push(
            SandboxManifest,
            "sandbox_manifest_status_mismatch",
            &location,
            "manifest status does not match handle status",
        );
    }
    if let Err(error) = validate_sandbox_mounts(&manifest.spec.mounts) {
        report.push(
            SandboxManifest,
            "sandbox_manifest_invalid_mount",
            &location,
            error,
        );
    }
    for mount in &manifest.spec.mounts {
        if let Some(volume_name) = mount.tenant_volume_name() {
            let volume_root = pointer
                .root
                .join("tenants")
                .join(spec_tenant)
                .join("volumes")
                .join(volume_name);
            if !volume_root.exists() {
                report.push(
                    TenantVolume,
                    "sandbox_manifest_volume_root_missing",
                    volume_root.display().to_string(),
                    format!(
                        "manifest {} references tenant volume `{volume_name}` but the tenant-owned root is missing",
                        pointer.manifest_path.display()
                    ),
                );
            }
        }
    }
    for binding in &manifest.spec.port_bindings {
        if !binding.host_address.is_loopback() {
            report.push(
                SandboxManifest,
                "sandbox_manifest_non_loopback_port",
                &location,
                format!(
                    "port binding `{}` exposes non-loopback host address {}",
                    binding.name, binding.host_address
                ),
            );
        }
    }
}

fn parse_service_records(
    documents: &[Document],
    config: &TenantIsolationDriftScanConfig,
    report: &mut TenantIsolationDriftReport,
) -> BTreeMap<ServiceKey, ServiceRecord> {
    let mut records = BTreeMap::new();
    for document in documents {
        let location = format!("_nimbus/services/{}", document.id);
        let Some(tenant_id) = string_field(document, "tenantId") else {
            report.push(
                ServiceHandle,
                "system_service_handle_malformed",
                &location,
                "service document is missing string field `tenantId`",
            );
            continue;
        };
        let Some(service_name) = string_field(document, "name") else {
            report.push(
                ServiceHandle,
                "system_service_handle_malformed",
                &location,
                "service document is missing string field `name`",
            );
            continue;
        };
        let state = string_field(document, "state")
            .unwrap_or("unknown")
            .to_owned();
        if TenantId::new(tenant_id.to_owned()).is_err() || tenant_id.starts_with('_') {
            report.push(
                ServiceHandle,
                "system_service_handle_invalid_tenant",
                &location,
                format!("service document references invalid or reserved tenant `{tenant_id}`"),
            );
        }

        let sandbox_id = document
            .fields
            .get("health")
            .and_then(|health| health.get("sandboxId"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        if active_state(&state) && sandbox_id.is_none() {
            report.push(
                ServiceHandle,
                "system_service_handle_missing_sandbox_id",
                &location,
                "active service document is missing health.sandboxId",
            );
        }
        let decision_id = document
            .fields
            .get("health")
            .and_then(|health| health.get("decisionId"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        if active_state(&state) && config.require_decision_audit_records && decision_id.is_none() {
            report.push(
                DecisionAudit,
                "tenant_isolation_decision_audit_missing",
                &location,
                "active service document has no tenant isolation decision/audit anchor",
            );
        }

        let key = ServiceKey::new(tenant_id, service_name);
        if records
            .insert(
                key,
                ServiceRecord {
                    document_id: document.id.to_string(),
                    state,
                    sandbox_id,
                },
            )
            .is_some()
        {
            report.push(
                ServiceHandle,
                "system_service_handle_duplicate",
                &location,
                format!(
                    "duplicate service document for tenant `{tenant_id}` service `{service_name}`"
                ),
            );
        }
    }
    records
}

fn parse_port_records(
    documents: &[Document],
    service_records: &BTreeMap<ServiceKey, ServiceRecord>,
    observed: &[ObservedSandboxManifest],
    report: &mut TenantIsolationDriftReport,
) -> BTreeMap<PortKey, PortRecord> {
    let mut records = BTreeMap::new();
    for document in documents {
        let location = format!("_nimbus/ports/{}", document.id);
        let Some(service_id) = string_field(document, "serviceId") else {
            continue;
        };
        let Some(tenant_id) = string_field(document, "tenantId") else {
            report.push(
                SystemPort,
                "system_port_record_malformed",
                &location,
                "service port document is missing string field `tenantId`",
            );
            continue;
        };
        let Some(service_name) = string_field(document, "serviceName") else {
            report.push(
                SystemPort,
                "system_port_record_malformed",
                &location,
                "service port document is missing string field `serviceName`",
            );
            continue;
        };
        let Some(endpoint_name) = string_field(document, "endpointName") else {
            report.push(
                SystemPort,
                "system_port_record_malformed",
                &location,
                "service port document is missing string field `endpointName`",
            );
            continue;
        };
        let Some(host_port) = u16_field(document, "hostPort") else {
            report.push(
                SystemPort,
                "system_port_record_malformed",
                &location,
                "service port document is missing valid hostPort",
            );
            continue;
        };
        let protocol = string_field(document, "protocol")
            .unwrap_or("unknown")
            .to_owned();
        let state = string_field(document, "state")
            .unwrap_or("unknown")
            .to_owned();
        let service_key = ServiceKey::new(tenant_id, service_name);
        if service_records
            .get(&service_key)
            .is_none_or(|service| service.document_id != service_id)
        {
            report.push(
                SystemPort,
                "system_port_record_service_mismatch",
                &location,
                format!(
                    "service port references serviceId `{service_id}` that does not match tenant `{tenant_id}` service `{service_name}`",
                ),
            );
        }
        if active_state(&state) && !manifest_has_endpoint(observed, &service_key, endpoint_name) {
            report.push(
                SystemPort,
                "system_port_record_orphaned",
                &location,
                format!(
                    "active port record `{endpoint_name}` has no active sandbox manifest endpoint for tenant `{tenant_id}` service `{service_name}`",
                ),
            );
        }
        records.insert(
            PortKey {
                tenant_id: tenant_id.to_owned(),
                service_name: service_name.to_owned(),
                endpoint_name: endpoint_name.to_owned(),
            },
            PortRecord {
                document_id: document.id.to_string(),
                host_port,
                protocol,
            },
        );
    }
    records
}

fn validate_observed_manifests(
    observed: &[ObservedSandboxManifest],
    service_records: &BTreeMap<ServiceKey, ServiceRecord>,
    port_records: &BTreeMap<PortKey, PortRecord>,
    report: &mut TenantIsolationDriftReport,
) {
    let mut active_service_keys = BTreeSet::new();
    let mut sandbox_ids = BTreeMap::<String, String>::new();
    for manifest in observed {
        let location = manifest.pointer.manifest_path.display().to_string();
        if active_sandbox_status(manifest.status)
            && let Some(service_key) = manifest.service_key()
            && !active_service_keys.insert(service_key.clone())
        {
            report.push(
                SandboxManifest,
                "duplicate_active_service_manifest",
                &location,
                format!(
                    "multiple active manifests claim tenant `{}` service `{}`",
                    service_key.tenant_id, service_key.service_name
                ),
            );
        }
        if let Some(previous) = sandbox_ids.insert(manifest.sandbox_id.clone(), location.clone())
            && previous != location
        {
            report.push(
                SandboxManifest,
                "duplicate_sandbox_id",
                &location,
                format!(
                    "sandbox id `{}` appears in multiple manifest paths, including {previous}",
                    manifest.sandbox_id
                ),
            );
        }

        let service_key = manifest.service_key();
        if active_sandbox_status(manifest.status)
            && let Some(service_key) = service_key.as_ref()
        {
            match service_records.get(service_key) {
                Some(service_record)
                    if service_record
                        .sandbox_id
                        .as_ref()
                        .is_some_and(|sandbox_id| sandbox_id == &manifest.sandbox_id) => {}
                Some(service_record) => report.push(
                    ServiceHandle,
                    "system_service_handle_sandbox_mismatch",
                    format!("_nimbus/services/{}", service_record.document_id),
                    format!(
                        "service document sandbox {:?} does not match active manifest sandbox `{}`",
                        service_record.sandbox_id, manifest.sandbox_id
                    ),
                ),
                None => report.push(
                    ServiceHandle,
                    "system_service_handle_missing",
                    &location,
                    format!(
                        "active manifest for tenant `{}` service `{}` has no _nimbus service document",
                        service_key.tenant_id, service_key.service_name
                    ),
                ),
            }
        }

        for endpoint in &manifest.handle.published_endpoints {
            let Some(service_key) = service_key.as_ref() else {
                continue;
            };
            if !active_sandbox_status(manifest.status) {
                continue;
            };
            let port_key = PortKey {
                tenant_id: service_key.tenant_id.clone(),
                service_name: service_key.service_name.clone(),
                endpoint_name: endpoint.name.clone(),
            };
            match port_records.get(&port_key) {
                Some(port_record) => {
                    let expected_protocol = crate::system_tenant::endpoint_protocol(endpoint.protocol);
                    if port_record.host_port != endpoint.address.port()
                        || port_record.protocol != expected_protocol
                    {
                        report.push(
                            SystemPort,
                            "system_port_record_endpoint_mismatch",
                            format!("_nimbus/ports/{}", port_record.document_id),
                            format!(
                                "port record does not match active manifest endpoint `{}`",
                                endpoint.name
                            ),
                        );
                    }
                }
                None => report.push(
                    SystemPort,
                    "system_port_record_missing",
                    &location,
                    format!(
                        "active manifest endpoint `{}` for tenant `{}` service `{}` has no _nimbus port document",
                        endpoint.name, service_key.tenant_id, service_key.service_name
                    ),
                ),
            }
        }
    }

    for (service_key, service_record) in service_records {
        if !active_state(&service_record.state) {
            continue;
        }
        let has_manifest = observed.iter().any(|manifest| {
            active_sandbox_status(manifest.status)
                && manifest
                    .service_key()
                    .is_some_and(|manifest_key| manifest_key == *service_key)
                && service_record
                    .sandbox_id
                    .as_ref()
                    .is_some_and(|sandbox_id| sandbox_id == &manifest.sandbox_id)
        });
        if !has_manifest {
            report.push(
                ServiceHandle,
                "system_service_handle_manifest_missing",
                format!("_nimbus/services/{}", service_record.document_id),
                format!(
                    "active service document for tenant `{}` service `{}` has no matching active manifest",
                    service_key.tenant_id, service_key.service_name
                ),
            );
        }
    }
}

fn validate_route_metadata(route_documents: &[Document], report: &mut TenantIsolationDriftReport) {
    let expected = crate::system_tenant::route_inventory()
        .into_iter()
        .map(|route| (route.document_id(), route))
        .collect::<BTreeMap<_, _>>();
    let actual = route_documents
        .iter()
        .map(|document| (document.id.to_string(), document))
        .collect::<BTreeMap<_, _>>();

    for (document_id, route) in &expected {
        let Some(document) = actual.get(document_id) else {
            report.push(
                RouteMetadata,
                "system_route_metadata_missing",
                format!("_nimbus/routes/{document_id}"),
                "expected system route metadata document is missing",
            );
            continue;
        };
        let location = format!("_nimbus/routes/{document_id}");
        let expected_auth = Value::Bool(route.auth_required);
        for (field, expected_value) in [
            ("method", Value::String(route.method.to_owned())),
            ("path", Value::String(route.path.to_owned())),
            ("adapter", Value::String(route.adapter.to_owned())),
            ("handler", Value::String(route.handler.to_owned())),
            ("authRequired", expected_auth),
        ] {
            if document.fields.get(field) != Some(&expected_value) {
                report.push(
                    RouteMetadata,
                    "system_route_metadata_mismatch",
                    &location,
                    format!(
                        "route field `{field}` is {:?}, expected {expected_value:?}",
                        document.fields.get(field)
                    ),
                );
            }
        }
    }

    for document_id in actual.keys() {
        if !expected.contains_key(document_id) {
            report.push(
                RouteMetadata,
                "system_route_metadata_unexpected",
                format!("_nimbus/routes/{document_id}"),
                "unexpected system route metadata document is present",
            );
        }
    }
}

fn manifest_has_endpoint(
    observed: &[ObservedSandboxManifest],
    service_key: &ServiceKey,
    endpoint_name: &str,
) -> bool {
    observed.iter().any(|manifest| {
        active_sandbox_status(manifest.status)
            && manifest
                .service_key()
                .is_some_and(|manifest_key| manifest_key == *service_key)
            && manifest
                .handle
                .published_endpoints
                .iter()
                .any(|endpoint| endpoint.name == endpoint_name)
    })
}

fn string_field<'a>(document: &'a Document, name: &str) -> Option<&'a str> {
    document.fields.get(name).and_then(Value::as_str)
}

fn u16_field(document: &Document, name: &str) -> Option<u16> {
    document
        .fields
        .get(name)
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .filter(|value| *value > 0)
}

fn active_state(state: &str) -> bool {
    !matches!(state, "stopped" | "failed")
}

fn active_sandbox_status(status: SandboxStatus) -> bool {
    !matches!(status, SandboxStatus::Stopped | SandboxStatus::Failed)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ManifestPointer {
    root: PathBuf,
    tenant_from_path: String,
    sandbox_from_path: String,
    container_from_path: String,
    manifest_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct PersistedSandboxManifest {
    handle: SandboxHandle,
    spec: SandboxSpec,
    status: SandboxStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ObservedSandboxManifest {
    pointer: ManifestPointer,
    tenant_id: String,
    service_name: Option<String>,
    sandbox_id: String,
    status: SandboxStatus,
    handle: SandboxHandle,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ServiceKey {
    tenant_id: String,
    service_name: String,
}

impl ServiceKey {
    fn new(tenant_id: &str, service_name: &str) -> Self {
        Self {
            tenant_id: tenant_id.to_owned(),
            service_name: service_name.to_owned(),
        }
    }
}

impl ObservedSandboxManifest {
    fn service_key(&self) -> Option<ServiceKey> {
        self.service_name.as_ref().map(|service_name| ServiceKey {
            tenant_id: self.tenant_id.clone(),
            service_name: service_name.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ServiceRecord {
    document_id: String,
    state: String,
    sandbox_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PortKey {
    tenant_id: String,
    service_name: String,
    endpoint_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PortRecord {
    document_id: String,
    host_port: u16,
    protocol: String,
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use nimbus_core::{DocumentId, TableName};
    use nimbus_sandbox::{
        PublishedEndpoint, PublishedEndpointProtocol, SandboxBackendKind, SandboxId,
        SandboxMountSpec, SandboxOwnerSpec, SandboxPortBinding, SandboxProcessSpec,
        SandboxRootSpec,
    };
    use serde_json::json;
    use tempfile::tempdir;

    use super::*;

    #[tokio::test]
    async fn tenant_isolation_drift_scanner_accepts_clean_projection() {
        let temp = tempdir().expect("tempdir should create");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
        crate::system_tenant::prepare_system_tenant_async(&engine, None)
            .await
            .expect("system tenant should prepare");
        let state_root = temp.path().join("sandbox-state");
        let tenant_id = TenantId::new("tenant-a").expect("tenant id should parse");
        let sandbox_id = SandboxId::new("sandbox-tenant-a-db");
        let endpoint = PublishedEndpoint::new(
            "postgres",
            PublishedEndpointProtocol::Tcp,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 15_432),
        )
        .with_guest_port(5432);
        let handle = SandboxHandle::new(
            tenant_id.clone(),
            sandbox_id.clone(),
            "db",
            SandboxBackendKind::Krun,
            SandboxStatus::Ready,
            vec![endpoint],
        );
        let spec = SandboxSpec::new(
            tenant_id.clone(),
            SandboxOwnerSpec::service("db"),
            SandboxBackendKind::Krun,
            SandboxRootSpec::rootfs("/rootfs"),
            SandboxProcessSpec::new(["postgres"]),
        )
        .with_port_binding(SandboxPortBinding::tcp("postgres", 15_432, 5432))
        .with_mount(SandboxMountSpec::tenant_volume(
            "data",
            "/var/lib/postgresql/data",
        ));
        create_tenant_volume_root(&state_root, "tenant-a", "data");
        write_manifest(
            &state_root,
            "tenant-a",
            "sandbox-tenant-a-db",
            &handle,
            &spec,
        );
        crate::system_tenant::record_service_handle_async(&engine, &tenant_id, &handle)
            .await
            .expect("service handle should record");

        let report = scan_tenant_isolation_drift_async(
            &engine,
            &TenantIsolationDriftScanConfig::new().with_sandbox_state_root(&state_root),
        )
        .await
        .expect("drift scan should complete");

        assert!(
            report.is_clean(),
            "clean state should not produce drift violations: {:?}",
            report.violations()
        );
    }

    #[tokio::test]
    async fn tenant_isolation_drift_scanner_does_not_treat_standalone_sandboxes_as_services() {
        let temp = tempdir().expect("tempdir should create");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
        crate::system_tenant::prepare_system_tenant_async(&engine, None)
            .await
            .expect("system tenant should prepare");
        let state_root = temp.path().join("sandbox-state");
        let tenant_id = TenantId::new("tenant-a").expect("tenant id should parse");

        for name in ["desktop-a", "desktop-b"] {
            let sandbox_id = SandboxId::new(format!("sandbox-tenant-a-{name}"));
            let handle = SandboxHandle::new(
                tenant_id.clone(),
                sandbox_id.clone(),
                name,
                SandboxBackendKind::Krun,
                SandboxStatus::Ready,
                vec![PublishedEndpoint::new(
                    "vnc",
                    PublishedEndpointProtocol::Tcp,
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 16_000),
                )],
            );
            let spec = SandboxSpec::new(
                tenant_id.clone(),
                SandboxOwnerSpec::standalone_named(name),
                SandboxBackendKind::Krun,
                SandboxRootSpec::rootfs("/rootfs"),
                SandboxProcessSpec::new(["sleep", "60"]),
            );
            write_manifest(&state_root, "tenant-a", sandbox_id.as_str(), &handle, &spec);
        }

        let report = scan_tenant_isolation_drift_async(
            &engine,
            &TenantIsolationDriftScanConfig::new().with_sandbox_state_root(&state_root),
        )
        .await
        .expect("drift scan should complete");

        assert!(
            report.is_clean(),
            "standalone sandbox manifests must not be reconciled as services: {:?}",
            report.violations()
        );
    }

    #[tokio::test]
    async fn tenant_isolation_drift_scanner_reports_malformed_state_without_mutating() {
        let temp = tempdir().expect("tempdir should create");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
        crate::system_tenant::prepare_system_tenant_async(&engine, None)
            .await
            .expect("system tenant should prepare");
        let state_root = temp.path().join("sandbox-state");

        create_tenant_volume_root(&state_root, "tenant-a", "bad name");
        write_mismatched_manifest(&state_root);
        write_bad_manifest(&state_root);
        insert_bad_service_document(&engine).await;
        insert_bad_port_document(&engine).await;
        corrupt_health_route(&engine).await;

        let report = scan_tenant_isolation_drift_async(
            &engine,
            &TenantIsolationDriftScanConfig::new()
                .with_sandbox_state_root(&state_root)
                .require_decision_audit_records(true),
        )
        .await
        .expect("drift scan should complete");
        let codes = report
            .violations()
            .iter()
            .map(|violation| violation.code.as_str())
            .collect::<BTreeSet<_>>();

        for expected in [
            "malformed_sandbox_manifest",
            "sandbox_manifest_tenant_path_mismatch",
            "sandbox_manifest_handle_tenant_mismatch",
            "sandbox_manifest_volume_root_missing",
            "tenant_volume_root_invalid_name",
            "system_service_handle_manifest_missing",
            "system_port_record_service_mismatch",
            "system_port_record_orphaned",
            "system_route_metadata_mismatch",
            "tenant_isolation_decision_audit_missing",
        ] {
            assert!(
                codes.contains(expected),
                "expected drift code {expected}; got {codes:?}"
            );
        }

        let routes = engine
            .list_documents_async(
                crate::system_tenant::system_tenant_id().expect("system id should parse"),
                TableName::new("routes").expect("table should parse"),
            )
            .await
            .expect("routes should list");
        assert!(
            routes
                .iter()
                .any(|route| route.id.as_str() == "route:get:health"
                    && route.fields.get("path") == Some(&json!("/tampered-health"))),
            "drift scan must not repair route metadata"
        );
        assert!(
            state_root
                .join("tenants/tenant-a/sandboxes/bad/state/containers/bad/manifest.json")
                .exists(),
            "drift scan must not delete malformed manifests"
        );
    }

    fn write_manifest(
        state_root: &Path,
        tenant: &str,
        sandbox: &str,
        handle: &SandboxHandle,
        spec: &SandboxSpec,
    ) {
        let manifest_path = state_root
            .join("tenants")
            .join(tenant)
            .join("sandboxes")
            .join(sandbox)
            .join("state")
            .join("containers")
            .join(sandbox)
            .join("manifest.json");
        fs::create_dir_all(manifest_path.parent().expect("manifest should have parent"))
            .expect("manifest directory should create");
        fs::write(
            manifest_path,
            serde_json::to_vec_pretty(&json!({
                "handle": handle,
                "spec": spec,
                "status": handle.status,
            }))
            .expect("manifest should serialize"),
        )
        .expect("manifest should write");
    }

    fn write_mismatched_manifest(state_root: &Path) {
        let tenant_a = TenantId::new("tenant-a").expect("tenant id should parse");
        let tenant_b = TenantId::new("tenant-b").expect("tenant id should parse");
        let sandbox_id = SandboxId::new("sandbox-tenant-a-db");
        let handle = SandboxHandle::new(
            tenant_a,
            sandbox_id,
            "db",
            SandboxBackendKind::Krun,
            SandboxStatus::Ready,
            Vec::new(),
        );
        let spec = SandboxSpec::new(
            tenant_b,
            SandboxOwnerSpec::service("db"),
            SandboxBackendKind::Krun,
            SandboxRootSpec::rootfs("/rootfs"),
            SandboxProcessSpec::new(["postgres"]),
        )
        .with_mount(SandboxMountSpec::tenant_volume(
            "data",
            "/var/lib/postgresql/data",
        ));
        write_manifest(
            state_root,
            "tenant-a",
            "sandbox-tenant-a-db",
            &handle,
            &spec,
        );
    }

    fn write_bad_manifest(state_root: &Path) {
        let manifest_path =
            state_root.join("tenants/tenant-a/sandboxes/bad/state/containers/bad/manifest.json");
        fs::create_dir_all(manifest_path.parent().expect("manifest should have parent"))
            .expect("manifest directory should create");
        fs::write(manifest_path, b"{ definitely not json").expect("manifest should write");
    }

    fn create_tenant_volume_root(state_root: &Path, tenant: &str, volume: &str) {
        fs::create_dir_all(
            state_root
                .join("tenants")
                .join(tenant)
                .join("volumes")
                .join(volume),
        )
        .expect("volume root should create");
    }

    async fn insert_bad_service_document(engine: &Arc<Engine>) {
        engine
            .insert_document_async_with_id(
                crate::system_tenant::system_tenant_id().expect("system id should parse"),
                TableName::new("services").expect("table should parse"),
                DocumentId::from_key("service:tenant-a:db").expect("document id should parse"),
                serde_json::from_value(json!({
                    "tenantId": "tenant-a",
                    "name": "db",
                    "kind": "sandbox",
                    "state": "ready",
                    "endpoints": [],
                    "health": {
                        "sandboxId": "missing-sandbox",
                        "backend": "krun",
                        "status": "ready"
                    }
                }))
                .expect("service fields should be object"),
            )
            .await
            .expect("bad service document should insert");
    }

    async fn insert_bad_port_document(engine: &Arc<Engine>) {
        engine
            .insert_document_async_with_id(
                crate::system_tenant::system_tenant_id().expect("system id should parse"),
                TableName::new("ports").expect("table should parse"),
                DocumentId::from_key("port:service:tenant-a:db:postgres")
                    .expect("document id should parse"),
                serde_json::from_value(json!({
                    "serviceId": "service:tenant-b:db",
                    "tenantId": "tenant-a",
                    "serviceName": "db",
                    "endpointName": "postgres",
                    "hostPort": 15432,
                    "protocol": "tcp",
                    "state": "ready"
                }))
                .expect("port fields should be object"),
            )
            .await
            .expect("bad port document should insert");
    }

    async fn corrupt_health_route(engine: &Arc<Engine>) {
        engine
            .update_document_async(
                crate::system_tenant::system_tenant_id().expect("system id should parse"),
                TableName::new("routes").expect("table should parse"),
                DocumentId::from_key("route:get:health").expect("document id should parse"),
                serde_json::from_value(json!({
                    "method": "GET",
                    "path": "/tampered-health",
                    "adapter": "native",
                    "handler": "health",
                    "authRequired": false,
                }))
                .expect("route fields should be object"),
            )
            .await
            .expect("route document should update");
    }
}
