use std::collections::BTreeMap;

use serde::Serialize;

use super::{
    RuntimeIsolationTier, TenantAuditRedactionPolicy, TenantIsolationDecision, TenantWorkloadKind,
    evidence::{canonical_evidence_reason_code, tenant_isolation_event_name},
};
#[cfg(test)]
use super::{TenantIsolationContext, authority::TenantIsolationAuthority};

pub const TENANT_ISOLATION_EVENT_SCHEMA_VERSION: &str = "nimbus.tenant_isolation.event.v1";
pub const TENANT_ISOLATION_OCSF_SCHEMA_VERSION: &str = "1.8.0";

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
    StringList(Vec<String>),
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
    workload_audit_projection_id: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantIsolationOcsfEvent {
    pub time: u64,
    pub metadata: TenantIsolationOcsfMetadata,
    pub category_uid: u16,
    pub category_name: &'static str,
    pub class_uid: u16,
    pub class_name: &'static str,
    pub activity_id: u16,
    pub activity_name: String,
    pub type_uid: u32,
    pub type_name: String,
    pub severity_id: u8,
    pub severity: &'static str,
    pub status_id: u8,
    pub status: &'static str,
    pub status_code: String,
    pub status_detail: String,
    pub message: String,
    pub unmapped: BTreeMap<String, TenantIsolationEventValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantIsolationOcsfMetadata {
    pub version: &'static str,
    pub product: TenantIsolationOcsfProduct,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantIsolationOcsfProduct {
    pub name: &'static str,
    pub vendor_name: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantIsolationOtelLogRecord {
    pub time_unix_nano: u64,
    pub observed_time_unix_nano: u64,
    pub severity_text: &'static str,
    pub severity_number: u8,
    pub event_name: String,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub body: String,
    pub attributes: BTreeMap<String, TenantIsolationEventValue>,
}

impl TenantIsolationEvent {
    pub fn from_decision(
        decision: &TenantIsolationDecision,
        kind: TenantIsolationEventKind,
        result: TenantIsolationEventResult,
        reason_code: impl Into<String>,
    ) -> Self {
        let reason_code = canonical_evidence_reason_code(reason_code.into());
        let mut redacted_fields = redacted_fields_from_policy(&decision.audit_redactions);
        if reason_code.was_redacted() {
            record_redaction_field(&mut redacted_fields, "reason_code".to_owned());
        }
        Self {
            schema_version: TENANT_ISOLATION_EVENT_SCHEMA_VERSION,
            kind,
            decision_id: Some(decision.id.as_str().to_owned()),
            tenant_id: decision.tenant_id.as_str().to_owned(),
            surface: decision.surface.to_owned(),
            principal_class: decision.authority.class().to_owned(),
            workload_stable_id: Some(decision.workload_stable_identity().stable_id()),
            workload_audit_projection_id: Some(
                decision.workload_stable_identity().audit_projection_id(),
            ),
            workload_kind: Some(decision.workload.kind()),
            workload_name: Some(decision.workload.name().to_owned()),
            runtime_tier: decision.workload.runtime_tier(),
            sandbox_id: decision.workload.sandbox_id().map(ToOwned::to_owned),
            invocation_id: decision.workload.invocation_id().map(ToOwned::to_owned),
            service_name: None,
            result,
            reason_code: reason_code.into_value(),
            correlation_ids: BTreeMap::new(),
            attributes: BTreeMap::new(),
            redacted_fields,
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
        let reason_code = canonical_evidence_reason_code(reason_code.into());
        let mut redacted_fields =
            redacted_fields_from_policy(&TenantAuditRedactionPolicy::default());
        if reason_code.was_redacted() {
            record_redaction_field(&mut redacted_fields, "reason_code".to_owned());
        }
        Self {
            schema_version: TENANT_ISOLATION_EVENT_SCHEMA_VERSION,
            kind,
            decision_id: None,
            tenant_id: tenant_id.into(),
            surface: surface.into(),
            principal_class: principal_class.into(),
            workload_stable_id: None,
            workload_audit_projection_id: None,
            workload_kind: None,
            workload_name: None,
            runtime_tier: None,
            sandbox_id: None,
            invocation_id: None,
            service_name: None,
            result,
            reason_code: reason_code.into_value(),
            correlation_ids: BTreeMap::new(),
            attributes: BTreeMap::new(),
            redacted_fields,
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

    pub fn to_ocsf_event(&self, time_millis: u64) -> TenantIsolationOcsfEvent {
        let severity = ocsf_severity(self.kind, self.result);
        let status = ocsf_status(self.result);
        let event_name = self.event_name();
        TenantIsolationOcsfEvent {
            time: time_millis,
            metadata: TenantIsolationOcsfMetadata {
                version: TENANT_ISOLATION_OCSF_SCHEMA_VERSION,
                product: TenantIsolationOcsfProduct {
                    name: "Nimbus",
                    vendor_name: "Nimbus",
                },
            },
            category_uid: 0,
            category_name: "Uncategorized",
            class_uid: 0,
            class_name: "Base Event",
            activity_id: 99,
            activity_name: self.kind.label().to_owned(),
            type_uid: 99,
            type_name: event_name,
            severity_id: severity.id,
            severity: severity.label,
            status_id: status.id,
            status: status.label,
            status_code: self.reason_code.clone(),
            status_detail: self.summary_message(),
            message: self.summary_message(),
            unmapped: self.export_attributes(),
        }
    }

    pub fn to_otel_log_record(
        &self,
        time_unix_nano: u64,
        observed_time_unix_nano: u64,
    ) -> TenantIsolationOtelLogRecord {
        let severity = otel_severity(self.kind, self.result);
        TenantIsolationOtelLogRecord {
            time_unix_nano,
            observed_time_unix_nano,
            severity_text: severity.text,
            severity_number: severity.number,
            event_name: self.event_name(),
            trace_id: self.correlation_ids.get("trace_id").cloned(),
            span_id: self.correlation_ids.get("span_id").cloned(),
            body: self.summary_message(),
            attributes: self.export_attributes(),
        }
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

    pub fn event_name(&self) -> String {
        tenant_isolation_event_name(self.kind.label(), self.result.label())
    }

    pub fn correlation_ids(&self) -> &BTreeMap<String, String> {
        &self.correlation_ids
    }

    pub fn redacted_fields(&self) -> &[String] {
        &self.redacted_fields
    }

    fn record_redaction(&mut self, field: String) {
        record_redaction_field(&mut self.redacted_fields, field);
    }

    fn summary_message(&self) -> String {
        format!(
            "Nimbus tenant isolation {} {}: {}",
            self.kind.label(),
            self.result.label(),
            self.reason_code
        )
    }

    fn export_attributes(&self) -> BTreeMap<String, TenantIsolationEventValue> {
        let mut attributes = BTreeMap::from([
            (
                "nimbus.schema_version".to_owned(),
                TenantIsolationEventValue::String(self.schema_version.to_owned()),
            ),
            (
                "nimbus.tenant_id".to_owned(),
                TenantIsolationEventValue::String(self.tenant_id.clone()),
            ),
            (
                "nimbus.surface".to_owned(),
                TenantIsolationEventValue::String(self.surface.clone()),
            ),
            (
                "nimbus.principal_class".to_owned(),
                TenantIsolationEventValue::String(self.principal_class.clone()),
            ),
            (
                "nimbus.event.kind".to_owned(),
                TenantIsolationEventValue::String(self.kind.label().to_owned()),
            ),
            (
                "nimbus.event.name".to_owned(),
                TenantIsolationEventValue::String(self.event_name()),
            ),
            (
                "nimbus.event.result".to_owned(),
                TenantIsolationEventValue::String(self.result.label().to_owned()),
            ),
            (
                "nimbus.event.reason_code".to_owned(),
                TenantIsolationEventValue::String(self.reason_code.clone()),
            ),
            (
                "nimbus.redacted_fields".to_owned(),
                TenantIsolationEventValue::StringList(self.redacted_fields.clone()),
            ),
        ]);
        insert_optional_string(&mut attributes, "nimbus.decision_id", &self.decision_id);
        insert_optional_string(
            &mut attributes,
            "nimbus.workload_stable_id",
            &self.workload_stable_id,
        );
        insert_optional_string(
            &mut attributes,
            "nimbus.workload_audit_projection_id",
            &self.workload_audit_projection_id,
        );
        insert_optional_string(&mut attributes, "nimbus.workload_name", &self.workload_name);
        insert_optional_string(&mut attributes, "nimbus.sandbox_id", &self.sandbox_id);
        insert_optional_string(&mut attributes, "nimbus.invocation_id", &self.invocation_id);
        insert_optional_string(&mut attributes, "nimbus.service_name", &self.service_name);
        if let Some(workload_kind) = self.workload_kind {
            attributes.insert(
                "nimbus.workload_kind".to_owned(),
                TenantIsolationEventValue::String(workload_kind.label().to_owned()),
            );
        }
        if let Some(runtime_tier) = self.runtime_tier {
            attributes.insert(
                "nimbus.runtime_tier".to_owned(),
                TenantIsolationEventValue::String(runtime_tier.label().to_owned()),
            );
        }
        for (key, value) in &self.correlation_ids {
            attributes.insert(
                format!("nimbus.correlation.{key}"),
                TenantIsolationEventValue::String(value.clone()),
            );
        }
        for (key, value) in &self.attributes {
            attributes.insert(format!("nimbus.attribute.{key}"), value.clone());
        }
        attributes
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
        record_redaction_field(&mut fields, field.clone());
    }
    fields
}

fn record_redaction_field(fields: &mut Vec<String>, field: String) {
    if !fields.iter().any(|existing| existing == &field) {
        fields.push(field);
    }
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
        || normalized.contains("query_param")
        || normalized.contains("query_string")
        || normalized.contains("raw_query")
        || normalized.contains("url_query")
        || normalized.contains("secret")
        || normalized.contains("token")
}

fn insert_optional_string(
    attributes: &mut BTreeMap<String, TenantIsolationEventValue>,
    key: &str,
    value: &Option<String>,
) {
    if let Some(value) = value {
        attributes.insert(
            key.to_owned(),
            TenantIsolationEventValue::String(value.clone()),
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OcsfSeverity {
    id: u8,
    label: &'static str,
}

fn ocsf_severity(
    kind: TenantIsolationEventKind,
    result: TenantIsolationEventResult,
) -> OcsfSeverity {
    match (kind, result) {
        (TenantIsolationEventKind::DriftViolation, _) => OcsfSeverity {
            id: 4,
            label: "High",
        },
        (_, TenantIsolationEventResult::Failed) => OcsfSeverity {
            id: 4,
            label: "High",
        },
        (_, TenantIsolationEventResult::Denied) => OcsfSeverity {
            id: 3,
            label: "Medium",
        },
        _ => OcsfSeverity {
            id: 1,
            label: "Informational",
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OcsfStatus {
    id: u8,
    label: &'static str,
}

fn ocsf_status(result: TenantIsolationEventResult) -> OcsfStatus {
    match result {
        TenantIsolationEventResult::Denied | TenantIsolationEventResult::Failed => OcsfStatus {
            id: 2,
            label: "Failure",
        },
        TenantIsolationEventResult::Allowed
        | TenantIsolationEventResult::Succeeded
        | TenantIsolationEventResult::Observed => OcsfStatus {
            id: 1,
            label: "Success",
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OtelSeverity {
    number: u8,
    text: &'static str,
}

fn otel_severity(
    kind: TenantIsolationEventKind,
    result: TenantIsolationEventResult,
) -> OtelSeverity {
    match (kind, result) {
        (TenantIsolationEventKind::DriftViolation, _) | (_, TenantIsolationEventResult::Failed) => {
            OtelSeverity {
                number: 17,
                text: "ERROR",
            }
        }
        (_, TenantIsolationEventResult::Denied) => OtelSeverity {
            number: 13,
            text: "WARN",
        },
        _ => OtelSeverity {
            number: 9,
            text: "INFO",
        },
    }
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
                event.event_name(),
                format!("nimbus.tenant_isolation.{}.allowed", event.kind().label())
            );
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
    fn tenant_isolation_event_taxonomy_names_are_stable() {
        let cases = [
            (
                TenantIsolationEventKind::Admission,
                TenantIsolationEventResult::Allowed,
                "nimbus.tenant_isolation.admission.allowed",
            ),
            (
                TenantIsolationEventKind::Rejection,
                TenantIsolationEventResult::Denied,
                "nimbus.tenant_isolation.rejection.denied",
            ),
            (
                TenantIsolationEventKind::Materialization,
                TenantIsolationEventResult::Succeeded,
                "nimbus.tenant_isolation.materialization.succeeded",
            ),
            (
                TenantIsolationEventKind::RuntimeInvocation,
                TenantIsolationEventResult::Failed,
                "nimbus.tenant_isolation.runtime_invocation.failed",
            ),
            (
                TenantIsolationEventKind::SandboxLaunch,
                TenantIsolationEventResult::Succeeded,
                "nimbus.tenant_isolation.sandbox_launch.succeeded",
            ),
            (
                TenantIsolationEventKind::StorageAccess,
                TenantIsolationEventResult::Allowed,
                "nimbus.tenant_isolation.storage_access.allowed",
            ),
            (
                TenantIsolationEventKind::HostBridgeOperation,
                TenantIsolationEventResult::Denied,
                "nimbus.tenant_isolation.host_bridge_operation.denied",
            ),
            (
                TenantIsolationEventKind::Cleanup,
                TenantIsolationEventResult::Succeeded,
                "nimbus.tenant_isolation.cleanup.succeeded",
            ),
            (
                TenantIsolationEventKind::DriftViolation,
                TenantIsolationEventResult::Observed,
                "nimbus.tenant_isolation.drift_violation.observed",
            ),
        ];

        for (kind, result, expected) in cases {
            let event = TenantIsolationEvent::without_decision(
                kind,
                "tenant-a",
                "taxonomy.test",
                "system",
                result,
                "policy_allowed",
            );
            assert_eq!(event.event_name(), expected);
            assert_eq!(
                event
                    .to_otel_log_record(1, 2)
                    .attributes
                    .get("nimbus.event.name"),
                Some(&TenantIsolationEventValue::String(expected.to_owned()))
            );
        }
    }

    #[test]
    fn tenant_isolation_event_exports_ocsf_and_otel_records() {
        let decision = tenant_decision();
        let event = decision
            .admission_event("policy_allowed")
            .with_correlation_id("trace_id", "0af7651916cd43dd8448eb211c80319c")
            .with_correlation_id("span_id", "b7ad6b7169203331")
            .with_attribute("operation", "db.query")
            .with_attribute("table", "messages")
            .with_service_name("db");

        let ocsf = event.to_ocsf_event(1_772_000_000_000);
        assert_eq!(ocsf.time, 1_772_000_000_000);
        assert_eq!(ocsf.metadata.version, TENANT_ISOLATION_OCSF_SCHEMA_VERSION);
        assert_eq!(ocsf.metadata.product.name, "Nimbus");
        assert_eq!(ocsf.category_uid, 0);
        assert_eq!(ocsf.category_name, "Uncategorized");
        assert_eq!(ocsf.class_uid, 0);
        assert_eq!(ocsf.class_name, "Base Event");
        assert_eq!(ocsf.activity_id, 99);
        assert_eq!(ocsf.activity_name, "admission");
        assert_eq!(ocsf.type_uid, 99);
        assert_eq!(ocsf.type_name, "nimbus.tenant_isolation.admission.allowed");
        assert_eq!(ocsf.severity_id, 1);
        assert_eq!(ocsf.severity, "Informational");
        assert_eq!(ocsf.status_id, 1);
        assert_eq!(ocsf.status, "Success");
        assert_eq!(ocsf.status_code, "policy_allowed");
        assert!(ocsf.status_detail.contains("policy_allowed"));
        assert_eq!(
            ocsf.unmapped.get("nimbus.decision_id"),
            Some(&TenantIsolationEventValue::String(
                decision.id().as_str().to_owned()
            ))
        );
        assert_eq!(
            ocsf.unmapped.get("nimbus.tenant_id"),
            Some(&TenantIsolationEventValue::String("tenant-a".to_owned()))
        );
        assert_eq!(
            ocsf.unmapped.get("nimbus.event.name"),
            Some(&TenantIsolationEventValue::String(
                "nimbus.tenant_isolation.admission.allowed".to_owned()
            ))
        );
        assert_eq!(
            ocsf.unmapped.get("nimbus.attribute.table"),
            Some(&TenantIsolationEventValue::String("messages".to_owned()))
        );
        assert_eq!(
            ocsf.unmapped.get("nimbus.service_name"),
            Some(&TenantIsolationEventValue::String("db".to_owned()))
        );
        match ocsf.unmapped.get("nimbus.workload_stable_id") {
            Some(TenantIsolationEventValue::String(value)) => {
                assert!(value.contains("messages%3Asend"), "{value}");
                assert!(
                    !value.contains("/invocation/invoke-1"),
                    "stable subject must not include per-invocation fields: {value}"
                );
            }
            other => panic!("expected workload stable id, got {other:?}"),
        }
        match ocsf.unmapped.get("nimbus.workload_audit_projection_id") {
            Some(TenantIsolationEventValue::String(value)) => {
                assert!(value.contains("messages%3Asend"), "{value}");
                assert!(value.contains("/invocation/invoke-1"), "{value}");
            }
            other => panic!("expected workload audit projection id, got {other:?}"),
        }
        serde_json::to_string(&ocsf).expect("OCSF event should serialize");

        let otel = event.to_otel_log_record(1_772_000_000_000_000_000, 1_772_000_000_000_001_000);
        assert_eq!(otel.time_unix_nano, 1_772_000_000_000_000_000);
        assert_eq!(otel.observed_time_unix_nano, 1_772_000_000_000_001_000);
        assert_eq!(otel.severity_text, "INFO");
        assert_eq!(otel.severity_number, 9);
        assert_eq!(otel.event_name, "nimbus.tenant_isolation.admission.allowed");
        assert_eq!(
            otel.trace_id.as_deref(),
            Some("0af7651916cd43dd8448eb211c80319c")
        );
        assert_eq!(otel.span_id.as_deref(), Some("b7ad6b7169203331"));
        assert!(otel.body.contains("policy_allowed"));
        assert_eq!(
            otel.attributes.get("nimbus.decision_id"),
            Some(&TenantIsolationEventValue::String(
                decision.id().as_str().to_owned()
            ))
        );
        assert_eq!(
            otel.attributes.get("nimbus.correlation.trace_id"),
            Some(&TenantIsolationEventValue::String(
                "0af7651916cd43dd8448eb211c80319c".to_owned()
            ))
        );
        assert_eq!(
            otel.attributes.get("nimbus.event.reason_code"),
            Some(&TenantIsolationEventValue::String(
                "policy_allowed".to_owned()
            ))
        );
        serde_json::to_string(&otel).expect("OpenTelemetry log record should serialize");
    }

    #[test]
    fn tenant_isolation_event_reason_code_is_canonical_and_redacted() {
        let event = TenantIsolationEvent::without_decision(
            TenantIsolationEventKind::HostBridgeOperation,
            "tenant-a",
            "runtime.host_bridge",
            "application",
            TenantIsolationEventResult::Denied,
            "Authorization: Bearer do-not-log-token",
        );

        assert_eq!(event.reason_code(), "non_canonical_reason_code");
        assert!(
            event.redacted_fields().contains(&"reason_code".to_owned()),
            "non-canonical reason codes should be advertised as redacted"
        );
        let serialized = serde_json::to_string(&event).expect("event should serialize");
        assert!(
            !serialized.contains("do-not-log-token"),
            "raw reason-code text must not leak: {serialized}"
        );
    }

    #[test]
    fn tenant_isolation_event_exports_redact_sensitive_fields() {
        let event = TenantIsolationEvent::without_decision(
            TenantIsolationEventKind::HostBridgeOperation,
            "tenant-a",
            "runtime.host_bridge",
            "application",
            TenantIsolationEventResult::Denied,
            "secret_grant_denied",
        )
        .with_correlation_id("authorization", "Bearer do-not-log-authorization")
        .with_attribute("query_params", "token=do-not-log-query")
        .with_attribute("raw_bearer_claims", "{\"sub\":\"do-not-log-claims\"}")
        .with_attribute("secret_handle", "prod/db/password")
        .with_attribute("safe_reason", "secret grant denied");

        let ocsf = event.to_ocsf_event(1_772_000_000_000);
        let otel = event.to_otel_log_record(1_772_000_000_000_000_000, 1_772_000_000_000_001_000);
        let ocsf_json = serde_json::to_string(&ocsf).expect("OCSF event should serialize");
        let otel_json = serde_json::to_string(&otel).expect("OTel record should serialize");
        for serialized in [&ocsf_json, &otel_json] {
            for secret in [
                "do-not-log-authorization",
                "do-not-log-query",
                "do-not-log-claims",
                "prod/db/password",
            ] {
                assert!(
                    !serialized.contains(secret),
                    "sensitive value leaked into export: {serialized}"
                );
            }
            assert!(
                serialized.contains("\"type\":\"redacted\""),
                "sensitive fields should serialize as typed redactions: {serialized}"
            );
            assert!(
                serialized.contains("attributes.query_params"),
                "query parameter redaction should be advertised: {serialized}"
            );
            assert!(
                serialized.contains("attributes.raw_bearer_claims"),
                "raw bearer claim redaction should be advertised: {serialized}"
            );
            assert!(
                serialized.contains("correlation_ids.authorization"),
                "authorization correlation redaction should be advertised: {serialized}"
            );
        }
        assert_eq!(
            ocsf.unmapped.get("nimbus.attribute.query_params"),
            Some(&TenantIsolationEventValue::Redacted)
        );
        assert_eq!(
            otel.attributes.get("nimbus.attribute.secret_handle"),
            Some(&TenantIsolationEventValue::Redacted)
        );
        assert!(
            event.correlation_ids().get("authorization").is_none(),
            "sensitive correlation IDs should be removed before export"
        );
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
        .with_attribute("query_params", "session=do-not-log-query")
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
                !serialized.contains("do-not-log-query"),
                "query parameter values must not leak: {serialized}"
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
