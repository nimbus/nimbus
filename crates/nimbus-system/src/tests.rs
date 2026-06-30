use std::sync::Arc;

use nimbus_core::{DocumentId, Error, Mutation, PrincipalContext, TableName, TenantId};
use nimbus_engine::Engine;
use nimbus_runtime::{RuntimeLimits, RuntimePolicy};
use serde_json::{Value, json};

use nimbus_node::{
    HostLifecycleBackendKind, HostLifecycleStatusReason, NodeStatusAuthorizer, StatusEvidenceWrite,
    StatusEvidenceWriter, SystemdTransientCapabilities, SystemdUnitName, TenantNodeObservationIds,
    TenantWorkloadDiagnostics, TenantWorkloadLifecycleEvidence, TenantWorkloadPhase,
    TenantWorkloadStatusPatch,
};
use nimbus_tenant::{
    RuntimeIsolationTier, TenantIsolationContext, TenantIsolationMode, TenantIsolationPolicyInput,
    TenantServiceGrantPolicyDecision, TenantStoragePolicyDecision, WorkloadAttributes,
    WorkloadLocation,
};
use nimbus_workloads::LocalEnforcementBinding;

use super::*;

fn table_name(table: SystemTable) -> TableName {
    table.table_name().expect("system table name should parse")
}

fn assert_system_index_fields(
    schemas: &[nimbus_core::TableSchema],
    table: SystemTable,
    index_name: &str,
    fields: &[&str],
) {
    let table_name = table_name(table);
    let schema = schemas
        .iter()
        .find(|schema| schema.table == table_name)
        .expect("system table schema should exist");
    let index = schema
        .indexes
        .iter()
        .find(|index| index.name == index_name)
        .expect("system table index should exist");
    let actual_fields = index.fields.iter().map(String::as_str).collect::<Vec<_>>();

    assert_eq!(actual_fields, fields);
}

#[test]
fn system_table_schemas_are_valid_and_cover_control_plane_contract() {
    let schemas = system_table_schemas().expect("system table schemas should build");
    let tables = schemas
        .iter()
        .map(|schema| schema.table.as_str())
        .collect::<std::collections::BTreeSet<_>>();

    let expected_tables = std::collections::BTreeSet::from([
        "adapter_capabilities",
        "bundles",
        "cron_jobs",
        "events",
        "functions",
        "listeners",
        "machines",
        "modules",
        "ports",
        "routes",
        "runs",
        "scheduled_jobs",
        "services",
        "source_packages",
        "subscriptions",
        "system_status",
        "tables",
        "workload_status",
    ]);
    let typed_tables = SystemTable::ALL
        .into_iter()
        .map(SystemTable::name)
        .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(typed_tables, expected_tables);
    assert_eq!(schemas.len(), SystemTable::ALL.len());
    assert_eq!(tables, expected_tables);
    assert_system_index_fields(
        &schemas,
        SystemTable::ScheduledJobs,
        "by_tenantId_and_status",
        &["tenantId", "status"],
    );
    assert_system_index_fields(
        &schemas,
        SystemTable::CronJobs,
        "by_tenantId_and_status",
        &["tenantId", "status"],
    );
    for schema in schemas {
        schema
            .validate_indexes()
            .expect("system table indexes should be valid");
        schema
            .validate_access_policy()
            .expect("system table access policy should be valid");
    }
}

#[tokio::test]
async fn ensure_system_tenant_creates_reserved_tenant_and_schemas_idempotently() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));

    ensure_system_tenant_async(&engine)
        .await
        .expect("system tenant should initialize");
    ensure_system_tenant_async(&engine)
        .await
        .expect("system tenant initialization should be idempotent");

    let tenants = engine
        .list_tenants_async()
        .await
        .expect("tenants should list");
    assert_eq!(
        tenants,
        vec![system_tenant_id().expect("system id should parse")]
    );

    let schema = engine
        .get_schema_async(system_tenant_id().expect("system id should parse"))
        .await
        .expect("system tenant schema should load");
    assert_eq!(schema.tables.len(), system_table_schemas().unwrap().len());
    assert!(
        schema
            .tables
            .contains_key(&table_name(SystemTable::Machines))
    );
    assert!(
        schema
            .tables
            .contains_key(&table_name(SystemTable::AdapterCapabilities))
    );
    assert!(
        schema
            .tables
            .contains_key(&table_name(SystemTable::SystemStatus))
    );
}

#[tokio::test]
async fn prepare_system_tenant_seeds_network_and_adapter_posture_documents() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
    let listen_addr = "127.0.0.1:34567".parse().expect("listen addr should parse");

    prepare_system_tenant_async(&engine, Some(listen_addr))
        .await
        .expect("system tenant should prepare");
    prepare_system_tenant_async(&engine, Some(listen_addr))
        .await
        .expect("system tenant preparation should be idempotent");

    let tenant_id = system_tenant_id().expect("system id should parse");
    let routes = engine
        .list_documents_async(tenant_id.clone(), table_name(SystemTable::Routes))
        .await
        .expect("routes should list");
    assert_eq!(routes.len(), route_inventory().len());
    assert!(routes.iter().any(|document| {
        document.fields.get("path") == Some(&json!("/api/tenants"))
            && document.fields.get("method") == Some(&json!("GET"))
    }));

    let capabilities = engine
        .list_documents_async(
            tenant_id.clone(),
            table_name(SystemTable::AdapterCapabilities),
        )
        .await
        .expect("capabilities should list");
    assert_eq!(capabilities.len(), adapter_capability_inventory().len());
    assert!(capabilities.iter().any(|document| {
        document.fields.get("adapter") == Some(&json!("machine"))
            && document.fields.get("feature") == Some(&json!("bootc-macos-machine"))
    }));

    let listeners = engine
        .list_documents_async(tenant_id.clone(), table_name(SystemTable::Listeners))
        .await
        .expect("listeners should list");
    assert_eq!(listeners.len(), 1);
    assert_eq!(
        listeners[0].fields.get("address"),
        Some(&json!(listen_addr.to_string()))
    );
    assert_eq!(listeners[0].fields.get("state"), Some(&json!("listening")));

    let status = engine
        .get_document_async(
            tenant_id,
            table_name(SystemTable::SystemStatus),
            DocumentId::from_key("system:server").expect("id should parse"),
        )
        .await
        .expect("system status should exist");
    assert_eq!(status.fields.get("name"), Some(&json!("server")));
    assert_eq!(status.fields.get("health"), Some(&json!("ok")));
    assert_eq!(
        status.fields["details"]["listenAddress"],
        json!(listen_addr.to_string())
    );
    assert!(
        status.fields.get("startedAt").is_some_and(Value::is_number),
        "system status should record server start time: {status:?}"
    );
}

#[tokio::test]
async fn source_package_build_store_record_and_read_round_trip() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
    let store = DiskSourcePackageStore::new(temp.path().join("source-packages"));

    // Build a source package the way the deploy client does, store it
    // content-addressed, project its rows, then read a module back end-to-end.
    let modules = std::collections::BTreeMap::from([
        (
            "messages".to_owned(),
            crate::ModuleInput {
                source: "export const list = query({});\n".to_owned(),
                source_map: None,
                type_info: None,
            },
        ),
        (
            "admin/users".to_owned(),
            crate::ModuleInput {
                source: "export const create = mutation({});\n".to_owned(),
                source_map: None,
                type_info: None,
            },
        ),
    ]);
    let bytes = build_source_package(&modules);
    let stored = store.put(&bytes).expect("store put");
    let parsed = parse_source_package(&bytes).expect("parse");
    let module_inputs = parsed
        .modules
        .iter()
        .map(|module| SystemModuleRecordInput {
            path: &module.path,
            sha256: &module.sha256,
        })
        .collect::<Vec<_>>();
    record_source_package_state_async(
        &engine,
        &SystemSourcePackageRecordInput {
            digest: &stored.digest,
            storage_key: &stored.storage_key,
            size_bytes: stored.size_bytes,
            unpacked_bytes: parsed.unpacked_bytes,
            modules: module_inputs,
        },
    )
    .await
    .expect("record source package");

    let resolved = read_module_source_async(&engine, &store, "messages")
        .await
        .expect("read module source")
        .expect("messages module present");
    assert_eq!(resolved.path, "messages");
    assert_eq!(resolved.digest, stored.digest);
    assert!(resolved.source.contains("query"));

    let nested = read_module_source_async(&engine, &store, "admin/users")
        .await
        .expect("read nested module")
        .expect("admin/users module present");
    assert!(nested.source.contains("mutation"));

    // Unknown module resolves to None (the endpoint returns 404).
    assert!(
        read_module_source_async(&engine, &store, "does/not/exist")
            .await
            .expect("read unknown module")
            .is_none()
    );
}

#[tokio::test]
async fn record_source_package_state_projects_package_and_modules_with_gc() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));

    let input = SystemSourcePackageRecordInput {
        digest: "aaaa1111",
        storage_key: "source-packages/aa/aaaa1111",
        size_bytes: 120,
        unpacked_bytes: 300,
        modules: vec![
            SystemModuleRecordInput {
                path: "messages",
                sha256: "messages-v1",
            },
            SystemModuleRecordInput {
                path: "admin/users",
                sha256: "admin-users-v1",
            },
        ],
    };
    record_source_package_state_async(&engine, &input)
        .await
        .expect("source package state should project");

    let tenant_id = system_tenant_id().expect("system id should parse");
    let packages = engine
        .list_documents_async(tenant_id.clone(), table_name(SystemTable::SourcePackages))
        .await
        .expect("source packages should list");
    assert_eq!(packages.len(), 1);
    assert_eq!(packages[0].fields.get("digest"), Some(&json!("aaaa1111")));
    assert_eq!(packages[0].fields.get("status"), Some(&json!("active")));
    assert_eq!(
        packages[0].fields.get("sizeBytes").and_then(Value::as_u64),
        Some(120)
    );

    let modules = engine
        .list_documents_async(tenant_id.clone(), table_name(SystemTable::Modules))
        .await
        .expect("modules should list");
    assert_eq!(modules.len(), 2);
    assert!(modules.iter().any(|document| {
        document.fields.get("path") == Some(&json!("admin/users"))
            && document.fields.get("sourcePackageId") == Some(&json!("aaaa1111"))
            && document.fields.get("sha256") == Some(&json!("admin-users-v1"))
    }));

    // Re-recording the same digest is idempotent.
    record_source_package_state_async(&engine, &input)
        .await
        .expect("re-record should be idempotent");
    let packages = engine
        .list_documents_async(tenant_id.clone(), table_name(SystemTable::SourcePackages))
        .await
        .expect("source packages should list");
    assert_eq!(packages.len(), 1);

    // A new digest with fewer modules GCs the prior package and its stale modules.
    let next = SystemSourcePackageRecordInput {
        digest: "bbbb2222",
        storage_key: "source-packages/bb/bbbb2222",
        size_bytes: 90,
        unpacked_bytes: 200,
        modules: vec![SystemModuleRecordInput {
            path: "messages",
            sha256: "messages-v2",
        }],
    };
    record_source_package_state_async(&engine, &next)
        .await
        .expect("next source package state should project");

    let packages = engine
        .list_documents_async(tenant_id.clone(), table_name(SystemTable::SourcePackages))
        .await
        .expect("source packages should list");
    assert_eq!(packages.len(), 1, "prior source package should be GC'd");
    assert_eq!(packages[0].fields.get("digest"), Some(&json!("bbbb2222")));

    let modules = engine
        .list_documents_async(tenant_id.clone(), table_name(SystemTable::Modules))
        .await
        .expect("modules should list");
    assert_eq!(modules.len(), 1, "stale modules should be GC'd");
    assert_eq!(modules[0].fields.get("path"), Some(&json!("messages")));
    assert_eq!(
        modules[0].fields.get("sourcePackageId"),
        Some(&json!("bbbb2222"))
    );
    assert_eq!(modules[0].fields.get("sha256"), Some(&json!("messages-v2")));
}

#[tokio::test]
async fn record_deployment_state_projects_neutral_bundle_and_functions() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));

    let input = SystemDeploymentRecordInput {
        source_ref: "deploy:test",
        functions: vec![
            SystemDeploymentFunctionRecordInput {
                name: "messages:send",
                kind: "mutation",
                fingerprint: "messages-send-v1",
            },
            SystemDeploymentFunctionRecordInput {
                name: "messages:list",
                kind: "query",
                fingerprint: "messages-list-v1",
            },
        ],
        http_routes: vec![SystemDeploymentHttpRouteRecordInput {
            key: "POST /echo",
            fingerprint: "echo-route-v1",
        }],
        schema_fingerprint: Some("schema-v1"),
        index_fingerprint: Some("indexes-v1"),
        runtime_bundle_fingerprint: Some("runtime-bundle-sha"),
    };
    record_deployment_state_async(&engine, &input)
        .await
        .expect("neutral deployment state should project");

    let tenant_id = system_tenant_id().expect("system id should parse");
    let bundles = engine
        .list_documents_async(tenant_id.clone(), table_name(SystemTable::Bundles))
        .await
        .expect("bundles should list");
    assert_eq!(bundles.len(), 1);
    assert_eq!(
        bundles[0].fields.get("sha256"),
        Some(&json!("runtime-bundle-sha"))
    );
    assert_eq!(
        bundles[0].fields.get("sourceRef"),
        Some(&json!("deploy:test"))
    );

    let functions = engine
        .list_documents_async(tenant_id.clone(), table_name(SystemTable::Functions))
        .await
        .expect("functions should list");
    assert_eq!(functions.len(), 2);
    assert!(functions.iter().any(|document| {
        document.fields.get("bundleId") == Some(&json!("runtime-bundle-sha"))
            && document.fields.get("path") == Some(&json!("messages:send"))
            && document.fields.get("kind") == Some(&json!("mutation"))
    }));

    let fallback_input = SystemDeploymentRecordInput {
        source_ref: "deploy:fallback",
        functions: vec![SystemDeploymentFunctionRecordInput {
            name: "http:echo",
            kind: "httpAction",
            fingerprint: "http-echo-v2",
        }],
        http_routes: vec![SystemDeploymentHttpRouteRecordInput {
            key: "GET /echo",
            fingerprint: "echo-route-v2",
        }],
        schema_fingerprint: Some("schema-v2"),
        index_fingerprint: Some("indexes-v2"),
        runtime_bundle_fingerprint: None,
    };
    record_deployment_state_async(&engine, &fallback_input)
        .await
        .expect("fallback deployment hash should project");

    let bundles = engine
        .list_documents_async(tenant_id.clone(), table_name(SystemTable::Bundles))
        .await
        .expect("bundles should list after replacement");
    assert_eq!(bundles.len(), 1);
    let fallback_sha = bundles[0]
        .fields
        .get("sha256")
        .and_then(Value::as_str)
        .expect("fallback bundle sha should be recorded");
    assert_eq!(fallback_sha.len(), 64);
    assert_eq!(
        bundles[0].fields.get("sourceRef"),
        Some(&json!("deploy:fallback"))
    );

    let functions = engine
        .list_documents_async(tenant_id, table_name(SystemTable::Functions))
        .await
        .expect("functions should list after replacement");
    assert_eq!(functions.len(), 1);
    assert_eq!(functions[0].fields.get("path"), Some(&json!("http:echo")));
    assert_eq!(
        functions[0].fields.get("bundleId"),
        Some(&json!(fallback_sha))
    );
}

#[tokio::test]
async fn sync_scheduler_state_deletes_only_matching_tenant_pending_projection() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
    let tenant_id = TenantId::new("demo").expect("tenant should parse");
    let other_tenant_id = TenantId::new("other").expect("tenant should parse");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .create_tenant_async(other_tenant_id.clone())
        .await
        .expect("other tenant should create");

    let stale_demo = nimbus_core::ScheduledJob {
        id: DocumentId::from_key("demo-stale-job").expect("job id should parse"),
        run_at: nimbus_core::Timestamp(10_000),
        mutation: Mutation::Insert {
            table: TableName::new("tasks").expect("table should parse"),
            id: None,
            fields: serde_json::Map::from_iter([("title".to_string(), json!("demo"))]),
        },
        created_at: nimbus_core::Timestamp(1_000),
    };
    let stale_other = nimbus_core::ScheduledJob {
        id: DocumentId::from_key("other-stale-job").expect("job id should parse"),
        run_at: nimbus_core::Timestamp(20_000),
        mutation: Mutation::Insert {
            table: TableName::new("tasks").expect("table should parse"),
            id: None,
            fields: serde_json::Map::from_iter([("title".to_string(), json!("other"))]),
        },
        created_at: nimbus_core::Timestamp(2_000),
    };
    crate::records::record_scheduled_job_state_async(&engine, &tenant_id, &stale_demo)
        .await
        .expect("stale demo scheduled projection should seed");
    crate::records::record_scheduled_job_state_async(&engine, &other_tenant_id, &stale_other)
        .await
        .expect("stale other scheduled projection should seed");

    sync_scheduler_state_for_tenant_async(&engine, &tenant_id)
        .await
        .expect("scheduler sync should delete stale demo projection");

    let scheduled_jobs = engine
        .list_documents_async(
            system_tenant_id().expect("system tenant should parse"),
            table_name(SystemTable::ScheduledJobs),
        )
        .await
        .expect("scheduled jobs should list");
    assert!(!scheduled_jobs.iter().any(|document| {
        document.fields.get("tenantId") == Some(&json!("demo"))
            && document.fields.get("status") == Some(&json!("pending"))
    }));
    assert!(scheduled_jobs.iter().any(|document| {
        document.fields.get("tenantId") == Some(&json!("other"))
            && document.fields.get("status") == Some(&json!("pending"))
    }));
}

#[tokio::test]
async fn record_machine_state_projects_machine_listener_and_port_documents() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
    let roots = nimbus_machine::MachineRootLayout::new(
        temp.path().join("config"),
        temp.path().join("state"),
        temp.path().join("data"),
        temp.path().join("cache"),
        temp.path().join("run"),
    );
    let config = nimbus_machine::MachineConfigRecord {
        version: nimbus_machine::CURRENT_MACHINE_CONFIG_VERSION,
        name: "default".to_string(),
        provider: nimbus_machine::MachineProvider::Krunkit,
        guest: nimbus_machine::MachineGuestConfig {
            image_source: nimbus_machine::MachineImageSource::OciReference {
                reference: "docker://ghcr.io/nimbus/machine-os:v0.1.31".to_string(),
            },
            provisioning: nimbus_machine::MachineGuestProvisioning::BootcMachineConfig,
            ssh_user: "nimbus".to_string(),
            ssh_identity_path: None,
            ignition_file_path: None,
            efi_variable_store_path: None,
        },
        resources: nimbus_machine::MachineResources {
            cpus: 4,
            memory_mib: 4096,
            disk_gib: 50,
        },
        volumes: vec![],
        roots,
    };
    let mut state = nimbus_machine::MachineStateRecord::initialized();

    record_machine_state_async(&engine, &config, &state)
        .await
        .expect("stopped machine state should project");

    let tenant_id = system_tenant_id().expect("system id should parse");
    let machine = engine
        .get_document_async(
            tenant_id.clone(),
            table_name(SystemTable::Machines),
            DocumentId::from_key(machine_document_id("default")).expect("id should parse"),
        )
        .await
        .expect("machine document should exist");
    assert_eq!(machine.fields.get("state"), Some(&json!("stopped")));
    assert_eq!(machine.fields["resources"]["memoryMiB"], json!(4096));
    assert_eq!(
        machine.fields["meta"]["image"],
        json!("docker://ghcr.io/nimbus/machine-os:v0.1.31")
    );

    state.lifecycle = nimbus_machine::MachineLifecycle::Running;
    state.manager = nimbus_machine::MachineManagerState::Ready;
    state.runtime = Some(nimbus_machine::MachineRuntimeState {
        helper_binaries: nimbus_machine::MachineHelperBinaryPaths {
            vmm: temp.path().join("krunkit"),
            gvproxy: temp.path().join("gvproxy"),
        },
        image_path: temp.path().join("default.raw"),
        efi_variable_store_path: temp.path().join("efi"),
        machine_image_source: "docker://ghcr.io/nimbus/machine-os:v0.1.31".to_string(),
        ssh_port: 2222,
        rest_uri: "unix:///tmp/nimbus/default-krunkit.sock".to_string(),
        ready_vsock_port: 1025,
    });

    record_machine_state_async(&engine, &config, &state)
        .await
        .expect("running machine state should project");

    let listener = engine
        .get_document_async(
            tenant_id.clone(),
            table_name(SystemTable::Listeners),
            DocumentId::from_key(machine_listener_document_id("default")).expect("id should parse"),
        )
        .await
        .expect("machine listener document should exist");
    assert_eq!(listener.fields.get("adapter"), Some(&json!("machine")));
    assert_eq!(listener.fields.get("protocol"), Some(&json!("unix")));
    assert_eq!(listener.fields.get("state"), Some(&json!("listening")));

    let ssh_port = engine
        .get_document_async(
            tenant_id,
            table_name(SystemTable::Ports),
            DocumentId::from_key(machine_port_document_id("default", "ssh"))
                .expect("id should parse"),
        )
        .await
        .expect("machine ssh port document should exist");
    assert_eq!(ssh_port.fields.get("machineId"), Some(&json!("default")));
    assert_eq!(ssh_port.fields.get("hostPort"), Some(&json!(2222)));
    assert_eq!(ssh_port.fields.get("guestPort"), Some(&json!(22)));
    assert_eq!(ssh_port.fields.get("state"), Some(&json!("running")));
}

#[tokio::test]
async fn record_service_handle_removes_only_stale_ports_for_that_service() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
    let tenant_id = TenantId::new("demo").expect("tenant should parse");

    let first_search = nimbus_sandbox::SandboxHandle::new(
        tenant_id.clone(),
        nimbus_sandbox::SandboxId::new("sandbox-search"),
        "search",
        nimbus_sandbox::SandboxBackendKind::Container,
        nimbus_sandbox::SandboxStatus::Ready,
        vec![
            nimbus_sandbox::PublishedEndpoint::new(
                "http",
                nimbus_sandbox::PublishedEndpointProtocol::Http,
                "127.0.0.1:18080".parse().expect("endpoint should parse"),
            )
            .with_guest_port(8080),
            nimbus_sandbox::PublishedEndpoint::new(
                "metrics",
                nimbus_sandbox::PublishedEndpointProtocol::Tcp,
                "127.0.0.1:19090".parse().expect("endpoint should parse"),
            )
            .with_guest_port(9090),
        ],
    );
    record_service_handle_async(&engine, &tenant_id, &first_search)
        .await
        .expect("initial service handle should project");

    let billing = nimbus_sandbox::SandboxHandle::new(
        tenant_id.clone(),
        nimbus_sandbox::SandboxId::new("sandbox-billing"),
        "billing",
        nimbus_sandbox::SandboxBackendKind::Container,
        nimbus_sandbox::SandboxStatus::Ready,
        vec![
            nimbus_sandbox::PublishedEndpoint::new(
                "api",
                nimbus_sandbox::PublishedEndpointProtocol::Http,
                "127.0.0.1:18081".parse().expect("endpoint should parse"),
            )
            .with_guest_port(8081),
        ],
    );
    record_service_handle_async(&engine, &tenant_id, &billing)
        .await
        .expect("other service handle should project");

    let updated_search = nimbus_sandbox::SandboxHandle::new(
        tenant_id.clone(),
        nimbus_sandbox::SandboxId::new("sandbox-search"),
        "search",
        nimbus_sandbox::SandboxBackendKind::Container,
        nimbus_sandbox::SandboxStatus::Ready,
        vec![
            nimbus_sandbox::PublishedEndpoint::new(
                "http",
                nimbus_sandbox::PublishedEndpointProtocol::Http,
                "127.0.0.1:28080".parse().expect("endpoint should parse"),
            )
            .with_guest_port(8080),
        ],
    );
    record_service_handle_async(&engine, &tenant_id, &updated_search)
        .await
        .expect("updated service handle should replace stale ports");

    let ports = engine
        .list_documents_async(
            system_tenant_id().expect("system tenant should parse"),
            table_name(SystemTable::Ports),
        )
        .await
        .expect("ports should list");
    assert_eq!(
        ports.len(),
        2,
        "expected one search port and one billing port"
    );
    assert!(ports.iter().any(|document| {
        document.fields.get("serviceName") == Some(&json!("search"))
            && document.fields.get("endpointName") == Some(&json!("http"))
            && document.fields.get("hostPort") == Some(&json!(28080))
    }));
    assert!(!ports.iter().any(|document| {
        document.fields.get("serviceName") == Some(&json!("search"))
            && document.fields.get("endpointName") == Some(&json!("metrics"))
    }));
    assert!(ports.iter().any(|document| {
        document.fields.get("serviceName") == Some(&json!("billing"))
            && document.fields.get("endpointName") == Some(&json!("api"))
    }));
}

#[tokio::test]
async fn record_subscription_state_projects_live_subscription_document() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
    let tenant_id = TenantId::new("demo").expect("tenant should parse");

    record_subscription_state_async(
        &engine,
        &tenant_id,
        "convex",
        42,
        "named:{\"name\":\"messages:list\"}",
    )
    .await
    .expect("subscription state should project");

    let document = engine
        .get_document_async(
            system_tenant_id().expect("system id should parse"),
            table_name(SystemTable::Subscriptions),
            DocumentId::from_key(subscription_document_id("convex", &tenant_id, 42))
                .expect("id should parse"),
        )
        .await
        .expect("subscription document should exist");
    assert_eq!(document.fields.get("tenantId"), Some(&json!("demo")));
    assert_eq!(document.fields.get("adapter"), Some(&json!("convex")));
    assert_eq!(document.fields.get("clientCount"), Some(&json!(1)));

    delete_subscription_state_async(&engine, &tenant_id, "convex", 42)
        .await
        .expect("subscription state should delete");
    let deleted = engine
        .get_document_async(
            system_tenant_id().expect("system id should parse"),
            table_name(SystemTable::Subscriptions),
            DocumentId::from_key(subscription_document_id("convex", &tenant_id, 42))
                .expect("id should parse"),
        )
        .await;
    assert!(matches!(deleted, Err(Error::DocumentNotFound(_))));
}

#[tokio::test]
async fn subscription_projection_skips_system_tenant_state_delivery_and_error() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
    prepare_system_tenant_async(&engine, None)
        .await
        .expect("system tenant should prepare");
    let tenant_id = system_tenant_id().expect("system tenant should parse");

    record_subscription_state_async(
        &engine,
        &tenant_id,
        "convex",
        42,
        "named:{\"name\":\"subscriptions:list\"}",
    )
    .await
    .expect("system tenant subscription state should be skipped");
    record_subscription_delivery_async(
        &engine,
        &tenant_id,
        "convex",
        42,
        "named:{\"name\":\"subscriptions:list\"}",
    )
    .await
    .expect("system tenant subscription delivery should be skipped");
    record_subscription_error_async(
        &engine,
        &tenant_id,
        "convex",
        42,
        "named:{\"name\":\"subscriptions:list\"}",
        "subscription failed",
    )
    .await
    .expect("system tenant subscription error should be skipped");

    let subscriptions = engine
        .list_documents_async(tenant_id, table_name(SystemTable::Subscriptions))
        .await
        .expect("subscriptions should list");
    assert!(
        subscriptions.is_empty(),
        "system tenant subscription churn must not project into _nimbus.subscriptions: {subscriptions:?}"
    );
}

#[tokio::test]
async fn workload_status_projection_requires_system_or_operator_authority() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
    let tenant_id = TenantId::new("tenant-a").expect("tenant should parse");
    let context = TenantIsolationContext::application(
        tenant_id.clone(),
        PrincipalContext {
            authenticated: true,
            claims: serde_json::Map::from_iter([("tenant_id".to_string(), json!("tenant-a"))]),
            verified_claims: serde_json::Map::new(),
        },
        "system_tenant.workload_status.test",
    )
    .with_deployment_generation(3)
    .with_workload_location(WorkloadLocation::new().with_node_id("node-a"));
    let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
    let decision = context
        .admit_decision(
            TenantIsolationPolicyInput::new(
                WorkloadAttributes::runtime_function(
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
            .with_storage(TenantStoragePolicyDecision::namespace("tenant-a")),
        )
        .expect("decision should admit");
    let binding =
        LocalEnforcementBinding::from_decision(&decision).expect("binding should materialize");
    let spec = binding.spec();
    let lifecycle = TenantWorkloadLifecycleEvidence::for_observed_unit(
        HostLifecycleBackendKind::SystemdTransientUnit,
        &SystemdUnitName::new("nimbus-tw-system-record.service").expect("unit should parse"),
        HostLifecycleStatusReason::Running,
    )
    .with_job_path("/org/freedesktop/systemd1/job/77")
    .expect("job path should parse")
    .with_process_id(777)
    .with_cgroup_path("/system.slice/nimbus-tw-system-record.service")
    .expect("cgroup path should parse");
    let status = NodeStatusAuthorizer
        .authorize(
            spec,
            TenantWorkloadStatusPatch::observed_status(spec)
                .with_phase(TenantWorkloadPhase::Running)
                .with_lifecycle_evidence(lifecycle)
                .with_node_observation_ids(
                    TenantNodeObservationIds::new()
                        .with_node_lease_id("lease-node-a")
                        .expect("lease id should parse")
                        .with_heartbeat_id("heartbeat-node-a")
                        .expect("heartbeat id should parse"),
                )
                .with_diagnostics(TenantWorkloadDiagnostics::new().with_backend_capabilities([
                    SystemdTransientCapabilities::available().to_backend_capabilities(),
                ]))
                .with_evidence_correlation_ids([
                    "nimbus-tw-system-record.service",
                    "/org/freedesktop/systemd1/job/77",
                ]),
        )
        .expect("node status should authorize");
    let projection = binding.system_evidence_projection();

    let application_writer = SystemTenantStatusEvidenceWriter::new(engine.clone(), context);
    let application_error = application_writer
        .write_status(
            StatusEvidenceWrite::new(&projection, &status)
                .expect("projection/status should match before persistence"),
        )
        .await
        .expect_err("application context must not write _nimbus workload status");
    assert!(
        application_error
            .to_string()
            .contains("requires system/operator authority"),
        "error should explain system/operator requirement: {application_error}"
    );

    let operator = TenantIsolationContext::operator(
        system_tenant_id().expect("system tenant should parse"),
        "system_tenant.workload_status.test",
    );
    let operator_writer = SystemTenantStatusEvidenceWriter::new(engine.clone(), operator);
    operator_writer
        .write_status(
            StatusEvidenceWrite::new(&projection, &status)
                .expect("projection/status should match before persistence"),
        )
        .await
        .expect("operator authority should project workload status");

    let document = engine
        .get_document_async(
            system_tenant_id().expect("system tenant should parse"),
            table_name(SystemTable::WorkloadStatus),
            DocumentId::from_key(workload_status_document_id(
                projection.tenant_id(),
                projection.workload_uid().as_str(),
            ))
            .expect("document id should parse"),
        )
        .await
        .expect("workload status document should exist");
    assert_eq!(document.fields.get("tenantId"), Some(&json!("tenant-a")));
    assert_eq!(
        document.fields.get("decisionId"),
        Some(&json!(spec.decision_id().as_str()))
    );
    assert_eq!(document.fields.get("phase"), Some(&json!("running")));
    assert_eq!(
        document.fields["evidence"]["lifecycle"]["unit_name"],
        json!("nimbus-tw-system-record.service")
    );
    assert_eq!(
        document.fields["evidence"]["lifecycle"]["job_path"],
        json!("/org/freedesktop/systemd1/job/77")
    );
    assert_eq!(
        document.fields["evidence"]["nodeObservation"]["node_lease_id"],
        json!("lease-node-a")
    );
    assert_eq!(
        document.fields["diagnostics"]["backend_capabilities"][0]["available"],
        json!(true)
    );
}

#[test]
fn user_tenant_id_rejects_reserved_prefix() {
    let error = user_tenant_id("_demo").expect_err("reserved user tenant should fail");

    assert!(matches!(error, Error::InvalidInput(message) if message.contains("reserved")));
}
