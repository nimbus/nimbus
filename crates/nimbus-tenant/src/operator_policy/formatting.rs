use nimbus_egress::EgressProtocol;
use nimbus_network::EndpointProtocol;
use nimbus_sandbox::{SandboxBackendKind, SandboxResourceCharge};

use crate::TenantRuntimePolicyAdmission;

use super::{
    OperatorImageProvenancePolicy, OperatorImageSignaturePolicy, OperatorPolicyImageSummary,
};

pub(super) fn normalized_strings(values: &[String]) -> Vec<String> {
    let mut normalized = values.to_vec();
    normalized.sort();
    normalized.dedup();
    normalized
}

pub(super) fn optional_backend_label(backend: Option<SandboxBackendKind>) -> String {
    backend
        .map(|backend| format!("{backend:?}"))
        .unwrap_or_else(|| "none".to_string())
}

pub(super) fn bool_label(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

pub(super) fn image_policy_summary(policy: &OperatorPolicyImageSummary) -> String {
    let mut parts = vec![format!(
        "digest_required={}",
        bool_label(policy.digest_required)
    )];
    if let Some(reference) = &policy.reference {
        parts.push(format!("reference={reference}"));
    }
    if !policy.allowed_registries.is_empty() {
        parts.push(format!(
            "allowed_registries={}",
            policy.allowed_registries.join(",")
        ));
    }
    if let Some(signature) = &policy.signature {
        parts.push(format!("signature={}", signature_summary(Some(signature))));
    }
    if let Some(provenance) = &policy.provenance {
        parts.push(format!(
            "provenance={}",
            provenance_summary(Some(provenance))
        ));
    }
    if policy.sbom_required {
        parts.push("sbom_required=true".to_string());
    }
    if policy.allow_local_build {
        parts.push("allow_local_build=true".to_string());
    }
    parts.join("; ")
}

pub(super) fn signature_summary(signature: Option<&OperatorImageSignaturePolicy>) -> String {
    signature
        .map(|signature| format!("issuer={}, subject={}", signature.issuer, signature.subject))
        .unwrap_or_else(|| "none".to_string())
}

pub(super) fn provenance_summary(provenance: Option<&OperatorImageProvenancePolicy>) -> String {
    provenance
        .map(|provenance| {
            let predicates = join_or_none(&provenance.predicates);
            let source_uri = provenance.source_uri.as_deref().unwrap_or("none");
            format!(
                "builder_id={}, source_uri={source_uri}, predicates={predicates}",
                provenance.builder_id
            )
        })
        .unwrap_or_else(|| "none".to_string())
}

pub(super) fn quota_summary(charge: Option<SandboxResourceCharge>) -> String {
    charge
        .map(|charge| {
            format!(
                "active_sandboxes={}, vcpus={}, memory_bytes={}, disk_bytes={}, log_bytes={}",
                charge.active_sandboxes,
                charge.vcpus,
                charge.memory_bytes,
                charge.disk_bytes,
                charge.log_bytes
            )
        })
        .unwrap_or_else(|| "none".to_string())
}

pub(super) fn join_or_none(values: &[String]) -> String {
    if values.is_empty() {
        "none".to_string()
    } else {
        values.join(", ")
    }
}

pub(super) fn admission_label(admission: &TenantRuntimePolicyAdmission) -> String {
    match admission {
        TenantRuntimePolicyAdmission::AdmitInProcess => "admit_in_process".to_string(),
        TenantRuntimePolicyAdmission::Route {
            recommended_tier,
            reason,
        } => format!("route_to_{} ({reason})", recommended_tier.label()),
    }
}

pub(super) fn protocol_label(protocol: EndpointProtocol) -> &'static str {
    match protocol {
        EndpointProtocol::Tcp => "tcp",
        EndpointProtocol::Http => "http",
        EndpointProtocol::Https => "https",
    }
}

pub(super) fn egress_protocol_label(protocol: EgressProtocol) -> &'static str {
    match protocol {
        EgressProtocol::Tcp => "tcp",
        EgressProtocol::Http => "http",
        EgressProtocol::Https => "https",
    }
}
