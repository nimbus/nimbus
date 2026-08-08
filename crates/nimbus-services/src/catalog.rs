use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};

use nimbus_core::TenantId;
use nimbus_sandbox::{SandboxHandle, SandboxSpec};
use nimbus_tenant::TenantVolumePolicyDecision;
use sha2::{Digest, Sha256};
use ulid::Ulid;

const SERVICE_DEFINITION_RESOURCE_VERSION_DOMAIN: &[u8] =
    b"nimbus.services.service-definition.resource-version.v1";
const SANDBOX_RESOURCE_VERSION_DOMAIN: &[u8] =
    b"nimbus.services.sandbox-resource.resource-version.v1";

pub trait ServiceInstanceCatalog: Send + Sync + 'static {
    fn service_instances_for_tenant(&self, tenant_id: &TenantId)
    -> BTreeMap<String, SandboxHandle>;

    fn service_instance_for_name(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Option<SandboxHandle> {
        self.service_instances_for_tenant(tenant_id)
            .remove(service_name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceBackend {
    Sandbox(Box<SandboxSpec>),
    BuiltIn(BuiltInServiceSpec),
    External(ExternalServiceSpec),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltInServiceSpec {
    provider: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalServiceSpec {
    endpoint_url: String,
    auth: ExternalAuthPolicy,
    health: HealthCheckPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalAuthPolicy {
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthCheckPolicy {
    Http { path: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceDefinition {
    pub tenant_id: TenantId,
    pub name: String,
    pub backend: ServiceBackend,
    pub generation: u64,
    pub resource_version: String,
    pub created_at_millis: u64,
    pub updated_at_millis: u64,
    pub labels: BTreeMap<String, String>,
    pub source: ServiceDefinitionSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceDefinitionSource {
    StaticCatalog,
    Dynamic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxResource {
    pub tenant_id: TenantId,
    pub id: String,
    pub profile: String,
    pub spec: SandboxSpec,
    pub handle: SandboxHandle,
    pub generation: u64,
    pub resource_version: String,
    pub created_at_millis: u64,
    pub updated_at_millis: u64,
    pub labels: BTreeMap<String, String>,
}

/// Desired source for one standalone sandbox, stored before provider effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxResourceSource {
    pub tenant_id: TenantId,
    pub id: String,
    pub profile: String,
    pub spec: SandboxSpec,
    pub generation: u64,
    pub resource_version: String,
    pub created_at_millis: u64,
    pub updated_at_millis: u64,
    pub labels: BTreeMap<String, String>,
}

/// Optional provider observation for one exact desired sandbox generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxResourceObservation {
    pub tenant_id: TenantId,
    pub id: String,
    pub observed_generation: u64,
    pub handle: SandboxHandle,
    pub observed_at_millis: u64,
}

/// Source plus its optional observed projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxResourceSnapshot {
    pub source: SandboxResourceSource,
    pub observation: Option<SandboxResourceObservation>,
}

/// Generation-fenced observed projection for a sandbox-backed service definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceDefinitionObservation {
    pub tenant_id: TenantId,
    pub name: String,
    pub observed_generation: u64,
    pub handle: SandboxHandle,
    pub observed_at_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DurableObjectNamespace(String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DurableObjectId(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DurableObjectNamespaceError {
    Empty,
    ContainsPathSeparator,
    ContainsNul,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DurableObjectIdError {
    InvalidLength { actual: usize },
    NonHex,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DurableObjectInstanceKey {
    pub tenant_id: TenantId,
    pub namespace: DurableObjectNamespace,
    pub id: DurableObjectId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableObjectStorageHandle {
    pub instance_key: DurableObjectInstanceKey,
    pub lease_epoch: u64,
    pub storage_key_prefix: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableObjectActivationLease {
    pub instance_key: DurableObjectInstanceKey,
    pub holder_id: String,
    pub lease_epoch: u64,
    pub resource_version: String,
    pub acquired_at_millis: u64,
    pub expires_at_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableObjectInstance {
    pub key: DurableObjectInstanceKey,
    pub storage: DurableObjectStorageHandle,
    pub active_lease: Option<DurableObjectActivationLease>,
    pub generation: u64,
    pub resource_version: String,
    pub created_at_millis: u64,
    pub updated_at_millis: u64,
    pub labels: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionTarget {
    Service { name: String },
    Sandbox { id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionTargetSnapshot {
    Service {
        name: String,
        generation: u64,
        backend: String,
        provider: Option<String>,
    },
    Sandbox {
        id: String,
        generation: u64,
        profile: String,
        backend: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionLifecycleState {
    Open,
    Closed,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionResource {
    pub tenant_id: TenantId,
    pub id: String,
    pub target: SessionTarget,
    pub target_snapshot: SessionTargetSnapshot,
    pub channels: Vec<String>,
    pub lifecycle_state: SessionLifecycleState,
    pub generation: u64,
    pub resource_version: String,
    pub created_at_millis: u64,
    pub updated_at_millis: u64,
    pub expires_at_millis: u64,
    pub closed_at_millis: Option<u64>,
    pub close_reason: Option<String>,
}

impl ServiceBackend {
    pub fn sandbox(spec: SandboxSpec) -> Self {
        Self::Sandbox(Box::new(spec))
    }

    pub fn built_in(provider: impl Into<String>) -> Self {
        Self::BuiltIn(BuiltInServiceSpec::new(provider))
    }

    pub fn external(
        endpoint_url: impl Into<String>,
        auth: ExternalAuthPolicy,
        health: HealthCheckPolicy,
    ) -> Self {
        Self::External(ExternalServiceSpec::new(endpoint_url, auth, health))
    }

    pub fn sandbox_spec(&self) -> Option<&SandboxSpec> {
        match self {
            Self::Sandbox(spec) => Some(spec),
            Self::BuiltIn(_) | Self::External(_) => None,
        }
    }

    pub fn into_sandbox_spec(self) -> Option<SandboxSpec> {
        match self {
            Self::Sandbox(spec) => Some(*spec),
            Self::BuiltIn(_) | Self::External(_) => None,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::Sandbox(_) => "sandbox",
            Self::BuiltIn(_) => "built-in",
            Self::External(_) => "external",
        }
    }
}

impl BuiltInServiceSpec {
    pub fn new(provider: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
        }
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }
}

impl ExternalServiceSpec {
    pub fn new(
        endpoint_url: impl Into<String>,
        auth: ExternalAuthPolicy,
        health: HealthCheckPolicy,
    ) -> Self {
        Self {
            endpoint_url: endpoint_url.into(),
            auth,
            health,
        }
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint_url
    }

    pub fn auth(&self) -> ExternalAuthPolicy {
        self.auth
    }

    pub fn health(&self) -> &HealthCheckPolicy {
        &self.health
    }
}

impl ServiceDefinition {
    pub fn dynamic(
        tenant_id: TenantId,
        name: impl Into<String>,
        backend: ServiceBackend,
        generation: u64,
        resource_version: impl Into<String>,
        now_millis: u64,
        labels: BTreeMap<String, String>,
    ) -> Self {
        Self {
            tenant_id,
            name: name.into(),
            backend,
            generation,
            resource_version: resource_version.into(),
            created_at_millis: now_millis,
            updated_at_millis: now_millis,
            labels,
            source: ServiceDefinitionSource::Dynamic,
        }
    }

    pub fn static_catalog(
        tenant_id: TenantId,
        name: impl Into<String>,
        backend: ServiceBackend,
    ) -> Self {
        Self::static_catalog_with_labels(tenant_id, name, backend, BTreeMap::new())
    }

    /// Build one complete immutable catalog snapshot at its initial generation.
    pub fn static_catalog_with_labels(
        tenant_id: TenantId,
        name: impl Into<String>,
        backend: ServiceBackend,
        labels: BTreeMap<String, String>,
    ) -> Self {
        let name = name.into();
        let generation = 1;
        let resource_version =
            service_definition_resource_version(&tenant_id, &name, generation, &backend, &labels);
        Self {
            tenant_id,
            resource_version,
            name,
            backend,
            generation,
            created_at_millis: 0,
            updated_at_millis: 0,
            labels,
            source: ServiceDefinitionSource::StaticCatalog,
        }
    }
}

impl SandboxResourceSource {
    pub fn new(
        tenant_id: TenantId,
        id: impl Into<String>,
        profile: impl Into<String>,
        spec: SandboxSpec,
        generation: u64,
        now_millis: u64,
        labels: BTreeMap<String, String>,
    ) -> Self {
        let id = id.into();
        let profile = profile.into();
        let resource_version =
            sandbox_resource_version(&tenant_id, &id, &profile, generation, &spec, &labels);
        Self {
            tenant_id,
            id,
            profile,
            spec,
            generation,
            resource_version,
            created_at_millis: now_millis,
            updated_at_millis: now_millis,
            labels,
        }
    }
}

fn service_definition_resource_version(
    tenant_id: &TenantId,
    name: &str,
    generation: u64,
    backend: &ServiceBackend,
    labels: &BTreeMap<String, String>,
) -> String {
    let backend = canonical_service_backend_bytes(backend);
    let labels = serde_json::to_vec(labels)
        .expect("service labels must have an infallible canonical JSON representation");
    let generation = generation.to_be_bytes();
    digest_resource_version(
        SERVICE_DEFINITION_RESOURCE_VERSION_DOMAIN,
        [
            tenant_id.as_str().as_bytes(),
            name.as_bytes(),
            generation.as_slice(),
            backend.as_slice(),
            labels.as_slice(),
        ],
    )
}

fn sandbox_resource_version(
    tenant_id: &TenantId,
    id: &str,
    profile: &str,
    generation: u64,
    spec: &SandboxSpec,
    labels: &BTreeMap<String, String>,
) -> String {
    let spec = serde_json::to_vec(spec)
        .expect("sandbox specs must have an infallible canonical JSON representation");
    let labels = serde_json::to_vec(labels)
        .expect("sandbox labels must have an infallible canonical JSON representation");
    let generation = generation.to_be_bytes();
    digest_resource_version(
        SANDBOX_RESOURCE_VERSION_DOMAIN,
        [
            tenant_id.as_str().as_bytes(),
            id.as_bytes(),
            profile.as_bytes(),
            generation.as_slice(),
            spec.as_slice(),
            labels.as_slice(),
        ],
    )
}

fn canonical_service_backend_bytes(backend: &ServiceBackend) -> Vec<u8> {
    let value = match backend {
        ServiceBackend::Sandbox(spec) => serde_json::json!({
            "kind": "sandbox",
            "spec": spec,
        }),
        ServiceBackend::BuiltIn(spec) => serde_json::json!({
            "kind": "built_in",
            "provider": spec.provider(),
        }),
        ServiceBackend::External(spec) => {
            let auth = match spec.auth() {
                ExternalAuthPolicy::None => "none",
            };
            let HealthCheckPolicy::Http { path } = spec.health();
            serde_json::json!({
                "kind": "external",
                "endpoint_url": spec.endpoint(),
                "auth": auth,
                "health": {
                    "kind": "http",
                    "path": path,
                },
            })
        }
    };
    serde_json::to_vec(&value)
        .expect("service backend values must have an infallible canonical JSON representation")
}

fn digest_resource_version<'a>(
    domain: &[u8],
    frames: impl IntoIterator<Item = &'a [u8]>,
) -> String {
    let mut digest = Sha256::new();
    digest.update((domain.len() as u64).to_be_bytes());
    digest.update(domain);
    for frame in frames {
        digest.update((frame.len() as u64).to_be_bytes());
        digest.update(frame);
    }
    format!("sha256:{}", lower_hex(&digest.finalize()))
}

impl DurableObjectNamespace {
    pub fn new(value: impl Into<String>) -> Result<Self, DurableObjectNamespaceError> {
        let value = value.into();
        if value.is_empty() {
            return Err(DurableObjectNamespaceError::Empty);
        }
        if value.contains('/') {
            return Err(DurableObjectNamespaceError::ContainsPathSeparator);
        }
        if value.contains('\0') {
            return Err(DurableObjectNamespaceError::ContainsNul);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for DurableObjectNamespace {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl Display for DurableObjectNamespaceError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => f.write_str("durable object namespace must not be empty"),
            Self::ContainsPathSeparator => {
                f.write_str("durable object namespace must not contain `/`")
            }
            Self::ContainsNul => f.write_str("durable object namespace must not contain NUL"),
        }
    }
}

impl std::error::Error for DurableObjectNamespaceError {}

impl DurableObjectId {
    pub fn from_name(namespace: &DurableObjectNamespace, name: &str) -> Self {
        Self::from_digest_parts(namespace, "name", name.as_bytes())
    }

    pub fn new_unique(namespace: &DurableObjectNamespace) -> Self {
        let unique = Ulid::new().to_string();
        Self::from_digest_parts(namespace, "unique", unique.as_bytes())
    }

    pub fn from_hex_string(value: impl Into<String>) -> Result<Self, DurableObjectIdError> {
        let value = value.into();
        if value.len() != 64 {
            return Err(DurableObjectIdError::InvalidLength {
                actual: value.len(),
            });
        }
        if !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(DurableObjectIdError::NonHex);
        }
        Ok(Self(value.to_ascii_lowercase()))
    }

    pub fn as_hex(&self) -> &str {
        &self.0
    }

    fn from_digest_parts(namespace: &DurableObjectNamespace, purpose: &str, bytes: &[u8]) -> Self {
        let mut digest = Sha256::new();
        digest.update(namespace.as_str().as_bytes());
        digest.update([0]);
        digest.update(purpose.as_bytes());
        digest.update([0]);
        digest.update(bytes);
        let digest = digest.finalize();
        Self(lower_hex(&digest))
    }
}

impl Display for DurableObjectId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl Display for DurableObjectIdError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLength { actual } => write!(
                f,
                "durable object id must be exactly 64 hex characters, got {actual}"
            ),
            Self::NonHex => f.write_str("durable object id must contain only hex characters"),
        }
    }
}

impl std::error::Error for DurableObjectIdError {}

impl DurableObjectInstanceKey {
    pub fn new(
        tenant_id: TenantId,
        namespace: DurableObjectNamespace,
        id: DurableObjectId,
    ) -> Self {
        Self {
            tenant_id,
            namespace,
            id,
        }
    }
}

impl DurableObjectStorageHandle {
    pub fn for_instance(instance_key: DurableObjectInstanceKey, lease_epoch: u64) -> Self {
        let storage_key_prefix = format!(
            "cloudflare/durable-object/{}/{}/{}",
            instance_key.tenant_id.as_str(),
            instance_key.namespace.as_str(),
            instance_key.id.as_hex()
        );
        Self {
            instance_key,
            lease_epoch,
            storage_key_prefix,
        }
    }
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

pub trait ServiceDefinitionCatalog: Send + Sync + 'static {
    fn service_definition_for_tenant(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Option<ServiceDefinition>;

    fn service_definitions_for_tenant(
        &self,
        _tenant_id: &TenantId,
    ) -> BTreeMap<String, ServiceDefinition> {
        BTreeMap::new()
    }

    fn service_volume_policy_for_tenant(
        &self,
        _tenant_id: &TenantId,
        _service_name: &str,
    ) -> TenantVolumePolicyDecision {
        TenantVolumePolicyDecision::default()
    }
}

#[derive(Debug, Default)]
pub struct EmptyServiceInstanceCatalog;

impl ServiceInstanceCatalog for EmptyServiceInstanceCatalog {
    fn service_instances_for_tenant(
        &self,
        _tenant_id: &TenantId,
    ) -> BTreeMap<String, SandboxHandle> {
        BTreeMap::new()
    }
}

#[derive(Debug, Default)]
pub struct EmptyServiceDefinitionCatalog;

impl ServiceDefinitionCatalog for EmptyServiceDefinitionCatalog {
    fn service_definition_for_tenant(
        &self,
        _tenant_id: &TenantId,
        _service_name: &str,
    ) -> Option<ServiceDefinition> {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use nimbus_core::TenantId;

    use super::{
        DurableObjectId, DurableObjectIdError, DurableObjectInstanceKey, DurableObjectNamespace,
        DurableObjectNamespaceError, DurableObjectStorageHandle, EmptyServiceDefinitionCatalog,
        EmptyServiceInstanceCatalog, ServiceBackend, ServiceDefinition, ServiceDefinitionCatalog,
        ServiceInstanceCatalog,
    };

    #[test]
    fn empty_catalog_returns_none_for_unknown_service() {
        let catalog = EmptyServiceInstanceCatalog;
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");

        assert!(
            catalog
                .service_instance_for_name(&tenant_id, "db")
                .is_none(),
            "empty service instance catalog should not resolve services"
        );
    }

    #[test]
    fn empty_catalog_returns_no_tenant_sandboxes() {
        let catalog = EmptyServiceInstanceCatalog;
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");

        assert!(
            catalog.service_instances_for_tenant(&tenant_id).is_empty(),
            "empty service instance catalog should not list tenant services"
        );
    }

    #[test]
    fn empty_service_catalog_returns_none_for_unknown_service() {
        let catalog = EmptyServiceDefinitionCatalog;
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");

        assert!(
            catalog
                .service_definition_for_tenant(&tenant_id, "db")
                .is_none(),
            "empty service definition catalog should not declare services"
        );
    }

    #[test]
    fn empty_service_catalog_returns_empty_volume_policy() {
        let catalog = EmptyServiceDefinitionCatalog;
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");

        assert!(
            catalog
                .service_volume_policy_for_tenant(&tenant_id, "db")
                .named_volumes()
                .is_empty(),
            "empty service definition catalog should not authorize service volumes"
        );
    }

    #[test]
    fn static_service_definition_digest_is_complete_stable_and_initial_generation() {
        let tenant_id = TenantId::new("tenant-a").expect("tenant id should be valid");
        let mut labels = BTreeMap::new();
        labels.insert("region".to_owned(), "local".to_owned());
        let definition = ServiceDefinition::static_catalog_with_labels(
            tenant_id.clone(),
            "api",
            ServiceBackend::built_in("native-api"),
            labels.clone(),
        );
        let reconstructed = ServiceDefinition::static_catalog_with_labels(
            tenant_id.clone(),
            "api",
            ServiceBackend::built_in("native-api"),
            labels.clone(),
        );

        assert_eq!(definition.generation, 1);
        assert_eq!(definition, reconstructed);
        assert!(definition.resource_version.starts_with("sha256:"));
        assert_eq!(definition.resource_version.len(), "sha256:".len() + 64);

        for changed in [
            ServiceDefinition::static_catalog_with_labels(
                TenantId::new("tenant-b").expect("tenant id should be valid"),
                "api",
                ServiceBackend::built_in("native-api"),
                labels.clone(),
            ),
            ServiceDefinition::static_catalog_with_labels(
                tenant_id.clone(),
                "worker",
                ServiceBackend::built_in("native-api"),
                labels.clone(),
            ),
            ServiceDefinition::static_catalog_with_labels(
                tenant_id.clone(),
                "api",
                ServiceBackend::built_in("native-worker"),
                labels.clone(),
            ),
            ServiceDefinition::static_catalog_with_labels(
                tenant_id,
                "api",
                ServiceBackend::built_in("native-api"),
                BTreeMap::new(),
            ),
        ] {
            assert_ne!(definition.resource_version, changed.resource_version);
        }
    }

    #[test]
    fn durable_object_id_from_name_is_deterministic_within_namespace() {
        let namespace = DurableObjectNamespace::new("Counter").expect("namespace should be valid");

        let first = DurableObjectId::from_name(&namespace, "counter-a");
        let second = DurableObjectId::from_name(&namespace, "counter-a");
        let different_name = DurableObjectId::from_name(&namespace, "counter-b");

        assert_eq!(
            first, second,
            "idFromName should return the same id for the same namespace/name"
        );
        assert_ne!(
            first, different_name,
            "idFromName should keep distinct names on distinct object ids"
        );
        assert_eq!(
            first.as_hex().len(),
            64,
            "Durable Object ids should use the 64-hex idFromString shape"
        );
    }

    #[test]
    fn durable_object_id_from_string_requires_canonical_64_hex() {
        let upper_hex = "A".repeat(64);

        let id = DurableObjectId::from_hex_string(upper_hex).expect("hex id should parse");

        assert_eq!(
            id.as_hex(),
            "a".repeat(64),
            "idFromString should canonicalize accepted ids to lowercase hex"
        );
        assert_eq!(
            DurableObjectId::from_hex_string("abc").expect_err("short id should fail"),
            DurableObjectIdError::InvalidLength { actual: 3 }
        );
        assert_eq!(
            DurableObjectId::from_hex_string("g".repeat(64)).expect_err("non-hex id should fail"),
            DurableObjectIdError::NonHex
        );
    }

    #[test]
    fn durable_object_namespace_rejects_ambiguous_storage_components() {
        assert_eq!(
            DurableObjectNamespace::new("").expect_err("empty namespace should fail"),
            DurableObjectNamespaceError::Empty
        );
        assert_eq!(
            DurableObjectNamespace::new("a/b").expect_err("path namespace should fail"),
            DurableObjectNamespaceError::ContainsPathSeparator
        );
        assert_eq!(
            DurableObjectNamespace::new("a\0b").expect_err("nul namespace should fail"),
            DurableObjectNamespaceError::ContainsNul
        );
    }

    #[test]
    fn durable_object_key_is_tenant_and_namespace_scoped() {
        let object_id =
            DurableObjectId::from_hex_string("1".repeat(64)).expect("object id should be valid");
        let tenant_a = TenantId::new("tenant-a").expect("tenant id should be valid");
        let tenant_b = TenantId::new("tenant-b").expect("tenant id should be valid");
        let namespace_a =
            DurableObjectNamespace::new("COUNTER").expect("namespace should be valid");
        let namespace_b =
            DurableObjectNamespace::new("COUNTER_B").expect("namespace should be valid");

        let tenant_a_key =
            DurableObjectInstanceKey::new(tenant_a, namespace_a.clone(), object_id.clone());
        let tenant_b_key =
            DurableObjectInstanceKey::new(tenant_b, namespace_a.clone(), object_id.clone());
        let namespace_b_key = DurableObjectInstanceKey::new(
            TenantId::new("tenant-a").unwrap(),
            namespace_b,
            object_id,
        );

        assert_ne!(
            tenant_a_key, tenant_b_key,
            "tenant id is the lead isolation component for DO routing"
        );
        assert_ne!(
            tenant_a_key, namespace_b_key,
            "namespace is part of the single-instance directory key"
        );
    }

    #[test]
    fn durable_object_storage_handle_derives_from_typed_instance_key() {
        let tenant_id = TenantId::new("tenant-a").expect("tenant id should be valid");
        let namespace = DurableObjectNamespace::new("COUNTER").expect("namespace should be valid");
        let object_id =
            DurableObjectId::from_hex_string("2".repeat(64)).expect("object id should be valid");
        let key = DurableObjectInstanceKey::new(tenant_id, namespace, object_id);

        let handle = DurableObjectStorageHandle::for_instance(key.clone(), 7);

        assert_eq!(handle.instance_key, key);
        assert_eq!(handle.lease_epoch, 7);
        assert_eq!(
            handle.storage_key_prefix,
            format!(
                "cloudflare/durable-object/tenant-a/COUNTER/{}",
                "2".repeat(64)
            ),
            "storage handle should be derived from tenant, namespace, and object id"
        );
    }
}
