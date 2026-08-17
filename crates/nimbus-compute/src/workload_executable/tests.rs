use std::time::Duration;

use nimbus_core::TenantId;
use nimbus_network::EndpointProtocol;
use nimbus_sandbox::{
    SandboxBackendKind, SandboxLifecycleSpec, SandboxMountSpec, SandboxOwnerSpec,
    SandboxPortBinding, SandboxProcessSpec, SandboxResourceLimits, SandboxRestartPolicy,
    SandboxRootSpec, SandboxSpec,
};
use nimbus_workloads::{WorkloadExecutableEncoding, WorkloadExecutableIntent};
use serde_json::Value;

use super::*;

const SECRET: &str = "NNC63A_SECRET=must-not-leak";

fn complete_spec() -> SandboxSpec {
    let spec = SandboxSpec::new(
        TenantId::new("tenant-executable-codec").expect("fixture tenant should validate"),
        SandboxOwnerSpec::service("executable-service"),
        SandboxBackendKind::Container,
        SandboxRootSpec::rootfs("/fixtures/rootfs"),
        SandboxProcessSpec::new(["/bin/serve", "--port", "8080"])
            .with_entrypoint(["/usr/bin/env"])
            .with_command(["/bin/serve", "--port", "8080"])
            .with_env([SECRET, "MODE=production"])
            .with_cwd("/workspace")
            .with_user("1000:1000")
            .with_terminal(true),
    )
    .with_resource_limits(
        SandboxResourceLimits::default()
            .with_cpu_count(4)
            .with_memory_limit_bytes(512 * 1024 * 1024)
            .with_disk_limit_bytes(4 * 1024 * 1024 * 1024)
            .with_log_limit_bytes(16 * 1024 * 1024),
    )
    .with_lifecycle(
        SandboxLifecycleSpec::default()
            .with_restart_policy(SandboxRestartPolicy::OnFailure { max_restarts: 3 })
            .with_stop_timeout(Duration::from_millis(2_500)),
    )
    .with_port_binding(SandboxPortBinding::new(
        "http",
        EndpointProtocol::Http,
        32_808,
        8_080,
    ))
    .with_mount(SandboxMountSpec::tenant_volume("state", "/data").read_only(true));
    let mut value = serde_json::to_value(spec).expect("complete spec should serialize");
    value["egress"] = serde_json::json!({
        "allow": [{
            "name": "artifact-registry",
            "protocol": "https",
            "host": "registry.example.com",
            "port": 443,
            "methods": ["GET"],
            "path_prefixes": ["/v2"]
        }]
    });
    serde_json::from_value(value).expect("non-default egress fixture should deserialize")
}

#[test]
fn sandbox_spec_round_trip_is_exact() {
    let spec = complete_spec();
    let carrier = encode_sandbox_spec(&spec).expect("complete spec should encode");
    let decoded = decode_sandbox_spec(&carrier).expect("canonical spec should decode");

    assert_eq!(decoded, spec);
    assert_eq!(
        carrier.encoding(),
        WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1
    );
    assert_eq!(
        carrier.canonical_content().as_bytes(),
        serde_json::to_vec(&spec).unwrap()
    );
    let value: Value = serde_json::from_str(carrier.canonical_content()).unwrap();
    for field in [
        "tenant_id",
        "owner",
        "backend",
        "root",
        "process",
        "resources",
        "lifecycle",
        "port_bindings",
        "mounts",
        "egress",
    ] {
        assert!(value.get(field).is_some(), "complete codec lost {field}");
    }
    assert_eq!(decoded.egress.rules().len(), 1);
    assert_eq!(decoded.egress.rules()[0].name, "artifact-registry");
    assert_eq!(decoded.egress.rules()[0].host, "registry.example.com");
    assert_eq!(decoded.egress.rules()[0].methods, ["GET"]);
    assert_eq!(decoded.egress.rules()[0].path_prefixes, ["/v2"]);
    let rendered = format!("{carrier:?}");
    assert!(!rendered.contains(SECRET));
    assert!(!rendered.contains("must-not-leak"));
}

#[test]
fn noncanonical_sandbox_spec_is_rejected() {
    let canonical = encode_sandbox_spec(&complete_spec()).unwrap();
    let content = canonical.canonical_content();
    let variants = [
        ("whitespace", format!(" {content}\n"), false),
        (
            "unknown field",
            format!(r#"{{"unknown":true,{}"#, &content[1..]),
            false,
        ),
        (
            "duplicate field",
            format!(
                r#"{{"tenant_id":"tenant-executable-codec",{}"#,
                &content[1..]
            ),
            true,
        ),
        (
            "secret-bearing invalid enum",
            content.replace(
                r#""backend":"container""#,
                r#""backend":"NNC63A_SECRET=must-not-leak""#,
            ),
            true,
        ),
        (
            "default-expanded field",
            content.replace(
                r#""path_prefixes":["/v2"]"#,
                r#""path_prefixes":["/v2"],"allow_internal_ips":false"#,
            ),
            false,
        ),
    ];

    for (label, variant, expects_decode_error) in variants {
        assert_ne!(variant, content, "fixture must alter canonical bytes");
        let carrier = WorkloadExecutableIntent::new(
            WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1,
            variant,
        )
        .expect("portable carrier should not interpret inner content");
        let error = decode_sandbox_spec(&carrier).expect_err("alias must be rejected");
        if expects_decode_error {
            assert!(
                matches!(error, WorkloadExecutableCodecError::Decode),
                "{label} should be rejected by serde's duplicate-field gate: {error:?}"
            );
        } else {
            assert!(
                matches!(error, WorkloadExecutableCodecError::NonCanonical),
                "{label} should be rejected by exact re-encoding: {error:?}"
            );
        }
        assert!(!format!("{error:?}").contains(SECRET));
        assert!(!error.to_string().contains("must-not-leak"));
    }
}
