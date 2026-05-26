use std::collections::BTreeSet;
use std::net::IpAddr;

use nimbus_core::{Error, Result, TenantId};
use nimbus_sandbox::validate_tenant_volume_name;

use super::super::image_admission::{has_sha256_digest, parse_oci_image_reference};
use super::prove::validate_accepted_risks;
use super::{
    DEFAULT_REDACTED_FIELDS, OPERATOR_POLICY_SCHEMA_VERSION, OperatorAuditPolicy,
    OperatorImagePolicy, OperatorImageProvenancePolicy, OperatorImageSignaturePolicy,
    OperatorNetworkEndpointPolicy, OperatorNetworkPolicy, OperatorPolicyDocument,
    OperatorPolicyWorkload, OperatorQuotaPolicy, OperatorRuntimePolicy, OperatorRuntimeProfile,
    OperatorSandboxPolicy, OperatorSecretPolicy, OperatorServicePolicy, OperatorStoragePolicy,
    OperatorVolumePolicy,
};
use crate::tenant_isolation::{RuntimeIsolationTier, TenantIsolationMode, TenantWorkloadKind};

impl OperatorPolicyDocument {
    pub fn validate(&self) -> Result<()> {
        self.evaluate().map(|_| ())
    }

    pub(super) fn validate_shape(&self) -> Result<()> {
        if self.schema_version != OPERATOR_POLICY_SCHEMA_VERSION {
            return invalid_policy(format!(
                "schema_version must be {OPERATOR_POLICY_SCHEMA_VERSION}, got {}",
                self.schema_version
            ));
        }
        if self.workloads.is_empty() {
            return invalid_policy("workloads must contain at least one workload");
        }
        TenantId::new(self.tenant.clone()).map_err(|error| {
            Error::InvalidInput(format!("operator policy tenant is invalid: {error}"))
        })?;
        validate_accepted_risks(&self.accepted_risks)?;
        validate_storage_namespace(
            &self.defaults.storage_namespace,
            "defaults.storage_namespace",
        )?;
        validate_redactions(&self.defaults.audit_redactions, "defaults.audit_redactions")?;

        let mut seen = BTreeSet::new();
        for workload in &self.workloads {
            let key = workload.key();
            if !seen.insert(key.clone()) {
                return invalid_policy(format!("workload `{key}` is declared more than once"));
            }
            workload.validate(self.defaults.tenant_isolation_mode)?;
        }
        Ok(())
    }
}

impl OperatorPolicyWorkload {
    pub(super) fn validate(&self, default_mode: TenantIsolationMode) -> Result<()> {
        if self.name.trim().is_empty() {
            return invalid_policy("workload.name cannot be empty");
        }
        self.runtime.validate(&self.key())?;
        self.sandbox.validate(&self.key(), self.kind)?;
        self.services.validate(&self.key())?;
        self.network.validate(&self.key(), &self.services)?;
        self.storage.validate(&self.key())?;
        self.volumes.validate(&self.key())?;
        self.image.validate(
            &self.key(),
            self.runtime.tenant_isolation_mode.unwrap_or(default_mode),
        )?;
        self.secrets.validate(&self.key())?;
        self.quotas.validate(&self.key())?;
        self.audit.validate(&self.key())?;
        Ok(())
    }
}

impl OperatorRuntimePolicy {
    fn validate(&self, workload_key: &str) -> Result<()> {
        if matches!(
            self.profile,
            OperatorRuntimeProfile::Node20
                | OperatorRuntimeProfile::Node22
                | OperatorRuntimeProfile::Node24
        ) && matches!(self.tier, RuntimeIsolationTier::InProcessUntrusted)
        {
            // This is allowed, but it should be visible in explain output because
            // production admission routes broad Node localhost/listen grants away
            // from in-process untrusted execution.
            return Ok(());
        }
        let _ = workload_key;
        Ok(())
    }
}

impl OperatorSandboxPolicy {
    fn validate(&self, workload_key: &str, kind: TenantWorkloadKind) -> Result<()> {
        if matches!(kind, TenantWorkloadKind::SandboxService) && self.sandbox_id.is_none() {
            return invalid_policy(format!(
                "workload `{workload_key}` is a sandbox_service and must set sandbox.sandbox_id"
            ));
        }
        if let Some(sandbox_id) = &self.sandbox_id
            && sandbox_id.trim().is_empty()
        {
            return invalid_policy(format!(
                "workload `{workload_key}` sandbox_id cannot be empty"
            ));
        }
        Ok(())
    }
}

impl OperatorServicePolicy {
    fn validate(&self, workload_key: &str) -> Result<()> {
        validate_name_list(
            &self.allow,
            &format!("workload `{workload_key}` services.allow"),
            "service",
        )
    }
}

impl OperatorNetworkPolicy {
    fn validate(&self, workload_key: &str, services: &OperatorServicePolicy) -> Result<()> {
        let allowed_services: BTreeSet<_> = services.allow.iter().map(String::as_str).collect();
        let mut seen = BTreeSet::new();
        for endpoint in &self.endpoints {
            endpoint.validate(workload_key)?;
            if !allowed_services.contains(endpoint.service.as_str()) {
                return invalid_policy(format!(
                    "workload `{workload_key}` network endpoint `{}` references service `{}` that is not in services.allow",
                    endpoint.name, endpoint.service
                ));
            }
            let key = format!("{}/{}", endpoint.service, endpoint.name);
            if !seen.insert(key.clone()) {
                return invalid_policy(format!(
                    "workload `{workload_key}` network endpoint `{key}` is declared more than once"
                ));
            }
        }
        self.egress.validate(workload_key)?;
        Ok(())
    }
}

impl OperatorNetworkEndpointPolicy {
    fn validate(&self, workload_key: &str) -> Result<()> {
        validate_required_name(&self.service, "service", workload_key)?;
        validate_required_name(&self.name, "network endpoint", workload_key)?;
        validate_host(&self.host, workload_key)?;
        validate_port(self.host_port, "host_port", workload_key)?;
        if let Some(guest_port) = self.guest_port {
            validate_port(guest_port, "guest_port", workload_key)?;
        }
        Ok(())
    }
}

impl OperatorStoragePolicy {
    fn validate(&self, workload_key: &str) -> Result<()> {
        if let Some(namespace) = &self.namespace {
            validate_storage_namespace(
                namespace,
                &format!("workload `{workload_key}` storage.namespace"),
            )?;
        }
        Ok(())
    }
}

impl OperatorVolumePolicy {
    fn validate(&self, workload_key: &str) -> Result<()> {
        validate_name_list(
            &self.named,
            &format!("workload `{workload_key}` volumes.named"),
            "volume",
        )?;
        for name in &self.named {
            validate_tenant_volume_name(name).map_err(|error| {
                Error::InvalidInput(format!(
                    "operator policy invalid: workload `{workload_key}` volume `{name}` is invalid: {error}"
                ))
            })?;
        }
        Ok(())
    }
}

impl OperatorImagePolicy {
    fn validate(&self, workload_key: &str, mode: TenantIsolationMode) -> Result<()> {
        if !self.digest_required {
            return invalid_policy(format!(
                "workload `{workload_key}` image.digest_required=false is unsafe; use immutable sha256 digest references"
            ));
        }
        if matches!(mode, TenantIsolationMode::Production) && self.allow_local_build {
            return invalid_policy(format!(
                "workload `{workload_key}` image.allow_local_build=true is not allowed in production policy"
            ));
        }
        if let Some(reference) = &self.reference {
            let parsed = parse_oci_image_reference(reference).map_err(|error| {
                Error::InvalidInput(format!(
                    "operator policy invalid: workload `{workload_key}` image.reference is invalid: {error}"
                ))
            })?;
            if !has_sha256_digest(&parsed) {
                return invalid_policy(format!(
                    "workload `{workload_key}` image.reference must be pinned with @sha256:<64 hex chars>"
                ));
            }
        }
        validate_name_list(
            &self.allowed_registries,
            &format!("workload `{workload_key}` image.allowed_registries"),
            "registry",
        )?;
        if let Some(signature) = &self.signature {
            signature.validate(workload_key)?;
        }
        if let Some(provenance) = &self.provenance {
            provenance.validate(workload_key)?;
        }
        Ok(())
    }
}

impl OperatorImageSignaturePolicy {
    fn validate(&self, workload_key: &str) -> Result<()> {
        validate_required_name(&self.issuer, "signature issuer", workload_key)?;
        validate_required_name(&self.subject, "signature subject", workload_key)
    }
}

impl OperatorImageProvenancePolicy {
    fn validate(&self, workload_key: &str) -> Result<()> {
        validate_required_name(&self.builder_id, "provenance builder_id", workload_key)?;
        if let Some(source_uri) = &self.source_uri {
            validate_required_name(source_uri, "provenance source_uri", workload_key)?;
        }
        validate_name_list(
            &self.predicates,
            &format!("workload `{workload_key}` provenance.predicates"),
            "predicate",
        )
    }
}

impl OperatorSecretPolicy {
    fn validate(&self, workload_key: &str) -> Result<()> {
        let mut seen = BTreeSet::new();
        for handle in &self.handles {
            if handle.trim().is_empty() || handle.contains(char::is_whitespace) {
                return invalid_policy(format!(
                    "workload `{workload_key}` secret handles must be non-empty references without whitespace"
                ));
            }
            if handle.starts_with("raw:") || handle.contains('=') {
                return invalid_policy(format!(
                    "workload `{workload_key}` secret handle `{handle}` looks like inline secret material"
                ));
            }
            if !seen.insert(handle) {
                return invalid_policy(format!(
                    "workload `{workload_key}` secret handle `{handle}` is declared more than once"
                ));
            }
        }
        Ok(())
    }
}

impl OperatorQuotaPolicy {
    fn validate(&self, workload_key: &str) -> Result<()> {
        if let Some(charge) = self.sandbox_charge
            && (charge.active_sandboxes == 0
                || charge.vcpus == 0
                || charge.memory_bytes == 0
                || charge.disk_bytes == 0)
        {
            return invalid_policy(format!(
                "workload `{workload_key}` sandbox_charge must set non-zero active_sandboxes, vcpus, memory_bytes, and disk_bytes"
            ));
        }
        Ok(())
    }
}

impl OperatorAuditPolicy {
    fn validate(&self, workload_key: &str) -> Result<()> {
        if let Some(fields) = &self.redacted_fields {
            validate_redactions(
                fields,
                &format!("workload `{workload_key}` audit.redacted_fields"),
            )?;
        }
        Ok(())
    }
}

pub(super) fn invalid_policy<T>(message: impl Into<String>) -> Result<T> {
    Err(Error::InvalidInput(format!(
        "operator policy invalid: {}",
        message.into()
    )))
}

fn validate_required_name(value: &str, label: &str, workload_key: &str) -> Result<()> {
    if value.trim().is_empty() || value == "*" {
        return invalid_policy(format!(
            "workload `{workload_key}` {label} must be a concrete non-empty value"
        ));
    }
    Ok(())
}

fn validate_name_list(values: &[String], field: &str, item_label: &str) -> Result<()> {
    let mut seen = BTreeSet::new();
    for value in values {
        if value.trim().is_empty() || value == "*" {
            return invalid_policy(format!(
                "{field} contains an unsafe {item_label} value `{value}`"
            ));
        }
        if value.contains(char::is_whitespace) {
            return invalid_policy(format!(
                "{field} value `{value}` must not contain whitespace"
            ));
        }
        if !seen.insert(value) {
            return invalid_policy(format!("{field} value `{value}` is duplicated"));
        }
    }
    Ok(())
}

fn validate_storage_namespace(namespace: &str, field: &str) -> Result<()> {
    if namespace != "tenant" {
        return invalid_policy(format!(
            "{field} must be `tenant`; custom storage namespaces are deferred until the storage PEP consumes namespace decisions"
        ));
    }
    Ok(())
}

fn validate_redactions(fields: &[String], field: &str) -> Result<()> {
    validate_name_list(fields, field, "redaction field")?;
    for required in DEFAULT_REDACTED_FIELDS {
        if !fields.iter().any(|field| field == required) {
            return invalid_policy(format!("{field} must include `{required}`"));
        }
    }
    Ok(())
}

fn validate_host(host: &str, workload_key: &str) -> Result<()> {
    let host = host.trim();
    if host.is_empty() || matches!(host, "*" | "0.0.0.0" | "::" | "[::]") {
        return invalid_policy(format!(
            "workload `{workload_key}` network host `{host}` is a wildcard bind, not an admitted egress endpoint"
        ));
    }
    if let Ok(ip) = host.parse::<IpAddr>()
        && ip.is_unspecified()
    {
        return invalid_policy(format!(
            "workload `{workload_key}` network host `{host}` is unspecified"
        ));
    }
    Ok(())
}

fn validate_port(port: u16, field: &str, workload_key: &str) -> Result<()> {
    if port == 0 {
        return invalid_policy(format!(
            "workload `{workload_key}` network {field} must not be 0"
        ));
    }
    Ok(())
}
