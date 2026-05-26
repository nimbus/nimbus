use std::sync::Arc;

use nimbus_core::{DocumentId, Error, TableName, TenantId};
use nimbus_engine::Service;
use serde_json::{Value, json};

use super::*;

#[test]
fn system_table_schemas_are_valid_and_cover_control_plane_contract() {
    let schemas = system_table_schemas().expect("system table schemas should build");
    let tables = schemas
        .iter()
        .map(|schema| schema.table.as_str())
        .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(
        tables,
        std::collections::BTreeSet::from([
            "adapter_capabilities",
            "bundles",
            "cron_jobs",
            "events",
            "functions",
            "listeners",
            "machines",
            "ports",
            "routes",
            "runs",
            "scheduled_jobs",
            "services",
            "subscriptions",
            "system_status",
            "tables",
        ])
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
    let service = Arc::new(Service::new(temp.path()).expect("service should create"));

    ensure_system_tenant_async(&service)
        .await
        .expect("system tenant should initialize");
    ensure_system_tenant_async(&service)
        .await
        .expect("system tenant initialization should be idempotent");

    let tenants = service
        .list_tenants_async()
        .await
        .expect("tenants should list");
    assert_eq!(
        tenants,
        vec![system_tenant_id().expect("system id should parse")]
    );

    let schema = service
        .get_schema_async(system_tenant_id().expect("system id should parse"))
        .await
        .expect("system tenant schema should load");
    assert_eq!(schema.tables.len(), system_table_schemas().unwrap().len());
    assert!(
        schema
            .tables
            .contains_key(&TableName::new("machines").unwrap())
    );
    assert!(
        schema
            .tables
            .contains_key(&TableName::new("adapter_capabilities").unwrap())
    );
    assert!(
        schema
            .tables
            .contains_key(&TableName::new("system_status").unwrap())
    );
}

#[tokio::test]
async fn prepare_system_tenant_seeds_network_and_adapter_posture_documents() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let service = Arc::new(Service::new(temp.path()).expect("service should create"));
    let listen_addr = "127.0.0.1:34567".parse().expect("listen addr should parse");

    prepare_system_tenant_async(&service, Some(listen_addr))
        .await
        .expect("system tenant should prepare");
    prepare_system_tenant_async(&service, Some(listen_addr))
        .await
        .expect("system tenant preparation should be idempotent");

    let tenant_id = system_tenant_id().expect("system id should parse");
    let routes = service
        .list_documents_async(
            tenant_id.clone(),
            TableName::new("routes").expect("table should parse"),
        )
        .await
        .expect("routes should list");
    assert_eq!(routes.len(), route_inventory().len());
    assert!(routes.iter().any(|document| {
        document.fields.get("path") == Some(&json!("/api/tenants"))
            && document.fields.get("method") == Some(&json!("GET"))
    }));

    let capabilities = service
        .list_documents_async(
            tenant_id.clone(),
            TableName::new("adapter_capabilities").expect("table should parse"),
        )
        .await
        .expect("capabilities should list");
    assert_eq!(capabilities.len(), adapter_capability_inventory().len());
    assert!(capabilities.iter().any(|document| {
        document.fields.get("adapter") == Some(&json!("machine"))
            && document.fields.get("feature") == Some(&json!("bootc-macos-machine"))
    }));

    let listeners = service
        .list_documents_async(
            tenant_id.clone(),
            TableName::new("listeners").expect("table should parse"),
        )
        .await
        .expect("listeners should list");
    assert_eq!(listeners.len(), 1);
    assert_eq!(
        listeners[0].fields.get("address"),
        Some(&json!(listen_addr.to_string()))
    );
    assert_eq!(listeners[0].fields.get("state"), Some(&json!("listening")));

    let status = service
        .get_document_async(
            tenant_id,
            TableName::new("system_status").expect("table should parse"),
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
async fn record_machine_state_projects_machine_listener_and_port_documents() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let service = Arc::new(Service::new(temp.path()).expect("service should create"));
    let roots = nimbus_machine::MachineRootLayout::new(
        temp.path().join("config"),
        temp.path().join("state"),
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

    record_machine_state_async(&service, &config, &state)
        .await
        .expect("stopped machine state should project");

    let tenant_id = system_tenant_id().expect("system id should parse");
    let machine = service
        .get_document_async(
            tenant_id.clone(),
            TableName::new("machines").expect("table should parse"),
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
            krunkit: temp.path().join("krunkit"),
            gvproxy: temp.path().join("gvproxy"),
        },
        image_path: temp.path().join("default.raw"),
        efi_variable_store_path: temp.path().join("efi"),
        machine_image_source: "docker://ghcr.io/nimbus/machine-os:v0.1.31".to_string(),
        ssh_port: 2222,
        rest_uri: "unix:///tmp/nimbus/default-krunkit.sock".to_string(),
        ready_vsock_port: 1025,
    });

    record_machine_state_async(&service, &config, &state)
        .await
        .expect("running machine state should project");

    let listener = service
        .get_document_async(
            tenant_id.clone(),
            TableName::new("listeners").expect("table should parse"),
            DocumentId::from_key(machine_listener_document_id("default")).expect("id should parse"),
        )
        .await
        .expect("machine listener document should exist");
    assert_eq!(listener.fields.get("adapter"), Some(&json!("machine")));
    assert_eq!(listener.fields.get("protocol"), Some(&json!("unix")));
    assert_eq!(listener.fields.get("state"), Some(&json!("listening")));

    let ssh_port = service
        .get_document_async(
            tenant_id,
            TableName::new("ports").expect("table should parse"),
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
async fn record_subscription_state_projects_live_subscription_document() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let service = Arc::new(Service::new(temp.path()).expect("service should create"));
    let tenant_id = TenantId::new("demo").expect("tenant should parse");

    record_subscription_state_async(
        &service,
        &tenant_id,
        "convex",
        42,
        "named:{\"name\":\"messages:list\"}",
    )
    .await
    .expect("subscription state should project");

    let document = service
        .get_document_async(
            system_tenant_id().expect("system id should parse"),
            TableName::new("subscriptions").expect("table should parse"),
            DocumentId::from_key(subscription_document_id("convex", &tenant_id, 42))
                .expect("id should parse"),
        )
        .await
        .expect("subscription document should exist");
    assert_eq!(document.fields.get("tenantId"), Some(&json!("demo")));
    assert_eq!(document.fields.get("adapter"), Some(&json!("convex")));
    assert_eq!(document.fields.get("clientCount"), Some(&json!(1)));

    delete_subscription_state_async(&service, &tenant_id, "convex", 42)
        .await
        .expect("subscription state should delete");
    let deleted = service
        .get_document_async(
            system_tenant_id().expect("system id should parse"),
            TableName::new("subscriptions").expect("table should parse"),
            DocumentId::from_key(subscription_document_id("convex", &tenant_id, 42))
                .expect("id should parse"),
        )
        .await;
    assert!(matches!(deleted, Err(Error::DocumentNotFound(_))));
}

#[test]
fn user_tenant_id_rejects_reserved_prefix() {
    let error = user_tenant_id("_demo").expect_err("reserved user tenant should fail");

    assert!(matches!(error, Error::InvalidInput(message) if message.contains("reserved")));
}
