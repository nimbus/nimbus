use std::collections::BTreeMap;

use serde::Serialize;

use super::{
    RuntimeIsolationTier, TenantAuditRedactionPolicy, TenantIsolationDecision, TenantWorkloadKind,
};
#[cfg(test)]
use super::{TenantIsolationAuthority, TenantIsolationContext};

pub const TENANT_ISOLATION_EVENT_SCHEMA_VERSION: &str = "nimbus.tenant_isolation.event.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TenantIsolationEventKind {
    Admission,
    Rejection,
    Materialization,
    RuntimeInvocation,
    SandboxLaunch,
    StorageAccess,
    HostBridgeOperation,
    Cleanup,
    DriftViolation,
}

impl TenantIsolationEventKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Admission => "admission",
            Self::Rejection => "rejection",
            Self::Materialization => "materialization",
            Self::RuntimeInvocation => "runtime_invocation",
            Self::SandboxLaunch => "sandbox_launch",
            Self::StorageAccess => "storage_access",
            Self::HostBridgeOperation => "host_bridge_operation",
            Self::Cleanup => "cleanup",
            Self::DriftViolation => "drift_violation",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TenantIsolationEventResult {
    Allowed,
    Denied,
    Succeeded,
    Failed,
    Observed,
}

impl TenantIsolationEventResult {
    pub fn label(self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::Denied => "denied",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Observed => "observed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "value")]
pub enum TenantIsolationEventValue {
    String(String),
    U64(u64),
    Bool(bool),
    Redacted,
}

impl From<String> for TenantIsolationEventValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for TenantIsolationEventValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl From<u64> for TenantIsolationEventValue {
    fn from(value: u64) -> Self {
        Self::U64(value)
    }
}

impl From<usize> for TenantIsolationEventValue {
    fn from(value: usize) -> Self {
        Self::U64(value as u64)
    }
}

impl From<bool> for TenantIsolationEventValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantIsolationEvent {
    schema_version: &'static str,
    kind: TenantIsolationEventKind,
    decision_id: Option<String>,
    tenant_id: String,
    surface: String,
    principal_class: String,
    workload_stable_id: Option<String>,
    workload_kind: Option<TenantWorkloadKind>,
    workload_name: Option<String>,
    runtime_tier: Option<RuntimeIsolationTier>,
    sandbox_id: Option<String>,
    invocation_id: Option<String>,
    service_name: Option<String>,
    result: TenantIsolationEventResult,
    reason_code: String,
    correlation_ids: BTreeMap<String, String>,
    attributes: BTreeMap<String, TenantIsolationEventValue>,
    redacted_fields: Vec<String>,
}

impl TenantIsolationEvent {
    pub fn from_decision(
        decision: &TenantIsolationDecision,
        kind: TenantIsolationEventKind,
        result: TenantIsolationEventResult,
        reason_code: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: TENANT_ISOLATION_EVENT_SCHEMA_VERSION,
            kind,
            decision_id: Some(decision.id.as_str().to_owned()),
            tenant_id: decision.tenant_id.as_str().to_owned(),
            surface: decision.surface.to_owned(),
            principal_class: decision.authority.class().to_owned(),
            workload_stable_id: Some(decision.workload_stable_identity().stable_id()),
            workload_kind: Some(decision.workload.kind()),
            workload_name: Some(decision.workload.name().to_owned()),
            runtime_tier: decision.workload.runtime_tier(),
            sandbox_id: decision.workload.sandbox_id().map(ToOwned::to_owned),
            invocation_id: decision.workload.invocation_id().map(ToOwned::to_owned),
            service_name: None,
            result,
            reason_code: reason_code.into(),
            correlation_ids: BTreeMap::new(),
            attributes: BTreeMap::new(),
            redacted_fields: redacted_fields_from_policy(&decision.audit_redactions),
        }
    }

    pub fn without_decision(
        kind: TenantIsolationEventKind,
        tenant_id: impl Into<String>,
        surface: impl Into<String>,
        principal_class: impl Into<String>,
        result: TenantIsolationEventResult,
        reason_code: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: TENANT_ISOLATION_EVENT_SCHEMA_VERSION,
            kind,
            decision_id: None,
            tenant_id: tenant_id.into(),
            surface: surface.into(),
            principal_class: principal_class.into(),
            workload_stable_id: None,
            workload_kind: None,
            workload_name: None,
            runtime_tier: None,
            sandbox_id: None,
            invocation_id: None,
            service_name: None,
            result,
            reason_code: reason_code.into(),
            correlation_ids: BTreeMap::new(),
            attributes: BTreeMap::new(),
            redacted_fields: redacted_fields_from_policy(&TenantAuditRedactionPolicy::default()),
        }
    }

    #[cfg(test)]
    pub(crate) fn rejection_from_context(
        context: &TenantIsolationContext,
        reason_code: impl Into<String>,
    ) -> Self {
        Self::without_decision(
            TenantIsolationEventKind::Rejection,
            context.tenant_id.as_str(),
            context.surface,
            authority_class(&context.authority),
            TenantIsolationEventResult::Denied,
            reason_code,
        )
    }

    pub fn with_correlation_id(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        let name = name.into();
        if is_sensitive_event_key(&name) {
            self.record_redaction(format!("correlation_ids.{name}"));
        } else {
            self.correlation_ids.insert(name, value.into());
        }
        self
    }

    pub fn with_attribute(
        mut self,
        name: impl Into<String>,
        value: impl Into<TenantIsolationEventValue>,
    ) -> Self {
        let name = name.into();
        if is_sensitive_event_key(&name) {
            self.attributes
                .insert(name.clone(), TenantIsolationEventValue::Redacted);
            self.record_redaction(format!("attributes.{name}"));
        } else {
            self.attributes.insert(name, value.into());
        }
        self
    }

    pub fn with_redacted_attribute(mut self, name: impl Into<String>) -> Self {
        let name = name.into();
        self.attributes
            .insert(name.clone(), TenantIsolationEventValue::Redacted);
        self.record_redaction(format!("attributes.{name}"));
        self
    }

    pub fn with_service_name(mut self, service_name: impl Into<String>) -> Self {
        self.service_name = Some(service_name.into());
        self
    }

    pub fn kind(&self) -> TenantIsolationEventKind {
        self.kind
    }

    pub fn result(&self) -> TenantIsolationEventResult {
        self.result
    }

    pub fn decision_id(&self) -> Option<&str> {
        self.decision_id.as_deref()
    }

    pub fn tenant_id(&self) -> &str {
        &self.tenant_id
    }

    pub fn surface(&self) -> &str {
        &self.surface
    }

    pub fn principal_class(&self) -> &str {
        &self.principal_class
    }

    pub fn reason_code(&self) -> &str {
        &self.reason_code
    }

    pub fn correlation_ids(&self) -> &BTreeMap<String, String> {
        &self.correlation_ids
    }

    pub fn redacted_fields(&self) -> &[String] {
        &self.redacted_fields
    }

    fn record_redaction(&mut self, field: String) {
        if !self
            .redacted_fields
            .iter()
            .any(|existing| existing == &field)
        {
            self.redacted_fields.push(field);
        }
    }
}

impl TenantIsolationDecision {
    pub fn isolation_event(
        &self,
        kind: TenantIsolationEventKind,
        result: TenantIsolationEventResult,
        reason_code: impl Into<String>,
    ) -> TenantIsolationEvent {
        TenantIsolationEvent::from_decision(self, kind, result, reason_code)
    }

    pub fn admission_event(&self, reason_code: impl Into<String>) -> TenantIsolationEvent {
        self.isolation_event(
            TenantIsolationEventKind::Admission,
            TenantIsolationEventResult::Allowed,
            reason_code,
        )
    }
}

fn redacted_fields_from_policy(policy: &TenantAuditRedactionPolicy) -> Vec<String> {
    let mut fields = Vec::new();
    for field in policy.redacted_fields() {
        if !fields.contains(field) {
            fields.push(field.clone());
        }
    }
    fields
}

#[cfg(test)]
fn authority_class(authority: &TenantIsolationAuthority) -> &'static str {
    match authority {
        TenantIsolationAuthority::Operator => "operator",
        TenantIsolationAuthority::Application { .. } => "application",
        TenantIsolationAuthority::System => "system",
    }
}

fn is_sensitive_event_key(key: &str) -> bool {
    let normalized: String = key
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    matches!(
        normalized.as_str(),
        "principal_claims" | "bearer_claims" | "raw_credentials" | "secret_handles"
    ) || normalized.contains("authorization")
        || normalized.contains("bearer")
        || normalized.contains("cookie")
        || normalized.contains("credential")
        || normalized.contains("password")
        || normalized.contains("private_key")
        || normalized.contains("secret")
        || normalized.contains("token")
}

#[cfg(test)]
mod tests {
    use nimbus_core::{PrincipalContext, TenantId};
    use nimbus_runtime::{RuntimeLimits, RuntimePolicy};
    use nimbus_sandbox::{PublishedEndpointProtocol, SandboxResourceCharge};

    use super::super::{
        RuntimeIsolationTier, TenantImagePolicyDecision, TenantIsolationContext,
        TenantIsolationMode, TenantIsolationPolicyInput, TenantNetworkEndpointDecision,
        TenantNetworkPolicyDecision, TenantQuotaPolicyDecision, TenantRuntimePolicyAdmission,
        TenantSecretPolicyDecision, TenantServiceGrantPolicyDecision, TenantStoragePolicyDecision,
        TenantVolumePolicyDecision, TenantWorkloadIdentity,
    };
    use super::*;

    fn test_application_context() -> TenantIsolationContext {
        TenantIsolationContext::application(
            TenantId::new("tenant-a").expect("tenant id should parse"),
            PrincipalContext::anonymous(),
            "convex.runtime",
        )
        .with_deployment_generation(7)
    }

    fn tenant_decision() -> TenantIsolationDecision {
        let context = test_application_context();
        let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
        let input = TenantIsolationPolicyInput::new(
            TenantWorkloadIdentity::runtime_function(
                "messages:send",
                RuntimeIsolationTier::InProcessUntrusted,
            )
            .with_invocation_id("invoke-1"),
        )
        .with_runtime_policy(
            &context,
            &policy,
            RuntimeIsolationTier::InProcessUntrusted,
            TenantIsolationMode::Production,
        )
        .with_services(TenantServiceGrantPolicyDecision::new(["db"]))
        .with_network(TenantNetworkPolicyDecision::new([
            TenantNetworkEndpointDecision::new(
                "db",
                "postgres",
                PublishedEndpointProtocol::Tcp,
                "127.0.0.1",
                15432,
            )
            .with_guest_port(5432),
        ]))
        .with_storage(TenantStoragePolicyDecision::namespace("tenant-a"))
        .with_volumes(TenantVolumePolicyDecision::new(["cache"]))
        .with_image(TenantImagePolicyDecision::digest_pinned(
            "registry.example.com/app@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ))
        .with_secrets(TenantSecretPolicyDecision::handles(["prod/db/password"]))
        .with_quotas(
            TenantQuotaPolicyDecision::default()
                .with_runtime_budget(policy.tenant_budget())
                .with_sandbox_charge(SandboxResourceCharge {
                    active_sandboxes: 1,
                    vcpus: 1,
                    memory_bytes: 512 * 1024 * 1024,
                    disk_bytes: 10 * 1024 * 1024 * 1024,
                    log_bytes: 64 * 1024 * 1024,
                }),
        );
        let decision = context
            .admit_decision(input)
            .expect("decision should admit");
        assert!(matches!(
            decision.runtime().admission(),
            TenantRuntimePolicyAdmission::AdmitInProcess
        ));
        decision
    }

    #[test]
    fn tenant_isolation_event_schema_covers_required_event_kinds() {
        let decision = tenant_decision();
        let kinds = [
            TenantIsolationEventKind::Admission,
            TenantIsolationEventKind::Rejection,
            TenantIsolationEventKind::Materialization,
            TenantIsolationEventKind::RuntimeInvocation,
            TenantIsolationEventKind::SandboxLaunch,
            TenantIsolationEventKind::StorageAccess,
            TenantIsolationEventKind::HostBridgeOperation,
            TenantIsolationEventKind::Cleanup,
            TenantIsolationEventKind::DriftViolation,
        ];

        for kind in kinds {
            let event = decision
                .isolation_event(kind, TenantIsolationEventResult::Allowed, "policy_allowed")
                .with_correlation_id("request_id", "req-123")
                .with_attribute("table", "messages")
                .with_service_name("db");
            let serialized = serde_json::to_string(&event).expect("event should serialize to JSON");

            assert_eq!(event.kind(), kind);
            assert_eq!(event.decision_id(), Some(decision.id().as_str()));
            assert_eq!(event.tenant_id(), "tenant-a");
            assert_eq!(event.surface(), "convex.runtime");
            assert_eq!(event.principal_class(), "application");
            assert_eq!(event.reason_code(), "policy_allowed");
            assert_eq!(
                event
                    .correlation_ids()
                    .get("request_id")
                    .map(String::as_str),
                Some("req-123")
            );
            assert!(
                serialized.contains(TENANT_ISOLATION_EVENT_SCHEMA_VERSION),
                "event should carry schema version: {serialized}"
            );
            assert!(
                serialized.contains(kind.label()),
                "event should serialize event kind {kind:?}: {serialized}"
            );
            assert!(
                serialized.contains(decision.id().as_str()),
                "event should carry the decision id: {serialized}"
            );
            assert!(
                serialized.contains("messages%3Asend"),
                "event should carry stable workload identity: {serialized}"
            );
            assert!(
                serialized.contains("\"runtime_tier\":\"in_process_untrusted\""),
                "event should carry runtime tier: {serialized}"
            );
            assert!(
                serialized.contains("\"invocation_id\":\"invoke-1\""),
                "event should carry invocation id: {serialized}"
            );
        }
    }

    #[test]
    fn tenant_isolation_rejection_cleanup_and_drift_events_redact_by_schema() {
        let context = test_application_context();
        let rejection = TenantIsolationEvent::rejection_from_context(
            &context,
            "application_principal_tenant_mismatch",
        )
        .with_correlation_id("authorization", "Bearer do-not-log")
        .with_attribute("bearer_claims", "{\"sub\":\"do-not-log\"}")
        .with_attribute("secret_handle", "prod/db/password")
        .with_attribute("safe_reason", "tenant claim mismatch");

        let cleanup = TenantIsolationEvent::without_decision(
            TenantIsolationEventKind::Cleanup,
            "tenant-a",
            "tenant.cleanup",
            "system",
            TenantIsolationEventResult::Succeeded,
            "tenant_cleanup_complete",
        )
        .with_correlation_id("cleanup_run_id", "cleanup-1")
        .with_attribute("removed_sandbox_count", 2usize)
        .with_attribute("session_token", "do-not-log-cleanup-token");

        let drift = TenantIsolationEvent::without_decision(
            TenantIsolationEventKind::DriftViolation,
            "tenant-a",
            "tenant.drift",
            "system",
            TenantIsolationEventResult::Observed,
            "sandbox_manifest_tenant_mismatch",
        )
        .with_attribute("violation_code", "sandbox_manifest_tenant_mismatch")
        .with_attribute("raw_credentials", "do-not-log-credentials");

        for event in [rejection, cleanup, drift] {
            let serialized = serde_json::to_string(&event).expect("event should serialize to JSON");
            assert!(
                !serialized.contains("Bearer do-not-log"),
                "authorization headers must not leak: {serialized}"
            );
            assert!(
                !serialized.contains("prod/db/password"),
                "secret handles must not leak: {serialized}"
            );
            assert!(
                !serialized.contains("do-not-log-cleanup-token"),
                "token values must not leak: {serialized}"
            );
            assert!(
                !serialized.contains("do-not-log-credentials"),
                "raw credentials must not leak: {serialized}"
            );
            assert!(
                serialized.contains("\"type\":\"redacted\""),
                "sensitive attributes should be schema-level redactions: {serialized}"
            );
            assert!(
                event
                    .redacted_fields()
                    .iter()
                    .any(|field| field.starts_with("attributes.")
                        || field.starts_with("correlation_ids.")),
                "event should advertise caller-supplied redactions"
            );
        }
    }
}
