use super::*;

#[test]
fn manifest_deserialization_requires_explicit_launch_authority_phase() {
    let mut wire = serde_json::to_value(sample_manifest(sample_spec(), KrunStartMode::Execute))
        .expect("fixture manifest should serialize");
    wire.as_object_mut()
        .expect("manifest wire should be an object")
        .remove("launch_authority");

    let error = serde_json::from_value::<KrunSandboxManifest>(wire)
        .expect_err("omitted launch authority must not infer provider ownership");
    assert!(
        error.to_string().contains("launch_authority"),
        "the missing required authority field must be explicit: {error}"
    );
}

#[test]
fn manifest_deserialization_requires_explicit_creator_handoff_phase() {
    let mut wire = serde_json::to_value(sample_manifest(sample_spec(), KrunStartMode::Execute))
        .expect("fixture manifest should serialize");
    wire.as_object_mut()
        .expect("manifest wire should be an object")
        .remove("creator_handoff");

    let error = serde_json::from_value::<KrunSandboxManifest>(wire)
        .expect_err("omitted creator handoff must not infer quiescence");
    assert!(
        error.to_string().contains("creator_handoff"),
        "the missing creator authority field must be explicit: {error}"
    );
}

#[test]
fn manifest_deserialization_requires_explicit_provider_failure_cleanup_phase() {
    let mut wire = serde_json::to_value(sample_manifest(sample_spec(), KrunStartMode::Execute))
        .expect("fixture manifest should serialize");
    wire.as_object_mut()
        .expect("manifest wire should be an object")
        .remove("provider_failure_cleanup");

    let error = serde_json::from_value::<KrunSandboxManifest>(wire)
        .expect_err("omitted provider-failure progress must not infer inactive cleanup");
    assert!(
        error.to_string().contains("provider_failure_cleanup"),
        "the missing required cleanup-progress field must be explicit: {error}"
    );
}

#[test]
fn manifest_deserialization_rejects_unknown_launch_authority_phase() {
    let mut wire = serde_json::to_value(sample_manifest(sample_spec(), KrunStartMode::Execute))
        .expect("fixture manifest should serialize");
    wire.as_object_mut()
        .expect("manifest wire should be an object")
        .insert(
            "launch_authority".to_owned(),
            serde_json::json!({"phase": "guessed_from_provider_state"}),
        );

    let error = serde_json::from_value::<KrunSandboxManifest>(wire)
        .expect_err("unknown authority phases must fail closed");
    assert!(
        error
            .to_string()
            .contains("unknown variant `guessed_from_provider_state`"),
        "the invalid phase must be explicit: {error}"
    );
}

#[test]
fn manifest_deserialization_defaults_lifecycle_for_pre_restart_manifests() {
    let manifest: KrunSandboxManifest = serde_json::from_value(json!({
        "handle": {
            "tenant_id": "tenant",
            "id": "sandbox-01",
            "name": "legacy",
            "backend": "krun",
            "status": "starting",
            "published_endpoints": [],
        },
        "execution_attempt_id": "test-execution-attempt:sandbox-01",
        "spec": {
            "tenant_id": "tenant",
            "owner": {
                "kind": "standalone",
                "display_name": "legacy",
            },
            "backend": "krun",
            "root": {
                "kind": "rootfs",
                "rootfs": "/srv/rootfs",
                "readonly": false,
            },
            "process": {
                "args": ["/bin/service"],
                "env": ["PATH=/usr/bin"],
                "cwd": "/",
                "terminal": false,
            },
            "resources": {
                "cpu_count": null,
                "memory_limit_bytes": null,
            },
            "port_bindings": [],
        },
        "image_metadata": {},
        "launch_artifact": null,
        "bundle_layout": {
            "bundle_dir": "/tmp/bundle",
            "config_path": "/tmp/bundle/config.json",
        },
        "conmon_layout": {
            "state_root": "/tmp/state",
            "container_state_dir": "/tmp/state/containers/sandbox-01",
            "exit_dir": "/tmp/state/exits",
            "persist_dir": "/tmp/state/persist/sandbox-01",
            "ctr_log": "/tmp/state/containers/sandbox-01/ctr.log",
            "oci_log": "/tmp/state/containers/sandbox-01/oci.log",
            "pidfile": "/tmp/state/containers/sandbox-01/pidfile",
            "conmon_pidfile": "/tmp/state/containers/sandbox-01/conmon.pid",
            "exit_status_file": "/tmp/state/exits/sandbox-01",
            "manifest_path": "/tmp/state/containers/sandbox-01/manifest.json",
        },
        "network_layout": {
            "workload_state_root": "/tmp/state",
            "network_state_root": "/tmp/state",
            "tenant_id": "tenant",
            "network_root": "/tmp/state/tenants/tenant/networks",
            "run_root": "/tmp/state/tenants/tenant/networks/run",
            "netns_root": "/tmp/state/tenants/tenant/networks/netns",
            "container_network_dir": "/tmp/state/tenants/tenant/networks/containers/sandbox-01",
            "netns_path": "/tmp/state/tenants/tenant/networks/netns/sandbox-01",
            "status_path": "/tmp/state/tenants/tenant/networks/containers/sandbox-01/status.json",
        },
        "port_leases": [],
        "launch_authority": {
            "phase": "provider_owned"
        },
        "creator_handoff": {
            "phase": "runtime_observed",
            "receipt": {
                "attempt_id": "fixture-attempt",
                "process": {
                    "pid": 42,
                    "process_group": 42,
                    "birth": {
                        "kind": "linux_proc_start_ticks",
                        "ticks": 1234
                    }
                }
            }
        },
        "provider_failure_cleanup": {
            "phase": "inactive"
        },
        "execution_teardown": {
            "drain": {
                "phase": "open"
            },
            "stop": {
                "phase": "not_requested"
            }
        },
        "network_teardown": crate::backends::oci::network::HostManagedAttachmentTeardownState::initial(),
        "egress_proxy": null,
        "conmon_launch": {
            "create_command": {
                "program": "/usr/bin/conmon",
                "args": [],
            },
            "state_command": {
                "program": "/usr/libexec/nimbus/crun",
                "args": ["state", "sandbox-01"],
            },
            "start_command": {
                "program": "/usr/libexec/nimbus/crun",
                "args": ["start", "sandbox-01"],
            },
        },
        "last_exit_code": null,
        "start_mode": "execute",
        "shutdown_requested": false,
        "status": "starting",
    }))
    .expect("manifest should deserialize with restart defaults");

    assert_eq!(
        manifest.spec.lifecycle.restart_policy,
        SandboxRestartPolicy::Never
    );
    assert_eq!(manifest.spec.lifecycle.stop_timeout, None);
    assert!(
        manifest
            .conmon_launch
            .delete_command
            .program
            .as_os_str()
            .is_empty(),
        "legacy manifests should default the delete command instead of failing to deserialize"
    );
}
