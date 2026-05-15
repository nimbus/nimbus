use crate::{
    MachineCreateRequest, MachineLifecycleFuture, MachineLifecycleManager,
    MachineLifecycleSnapshot, MachineUpdateRequest,
};

use super::*;
use nimbus_core::DocumentId;

#[derive(Clone)]
struct StubMachineLifecycleManager {
    roots: nimbus_machine::MachineRootLayout,
    calls: Arc<Mutex<Vec<String>>>,
}

impl StubMachineLifecycleManager {
    fn new(roots: nimbus_machine::MachineRootLayout) -> Self {
        Self {
            roots,
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls
            .lock()
            .expect("calls lock should not poison")
            .clone()
    }
}

impl MachineLifecycleManager for StubMachineLifecycleManager {
    fn create_machine<'a>(&'a self, request: MachineCreateRequest) -> MachineLifecycleFuture<'a> {
        let roots = self.roots.clone();
        let calls = self.calls.clone();
        Box::pin(async move {
            calls
                .lock()
                .expect("calls lock should not poison")
                .push(format!("create:{}", request.name));
            Ok(snapshot_for_resources(
                &request.name,
                roots,
                nimbus_machine::MachineLifecycle::Stopped,
                request.cpus.unwrap_or(2),
                request.memory_mib.unwrap_or(2048),
                request.disk_gib.unwrap_or(20),
            ))
        })
    }

    fn start_machine<'a>(&'a self, name: &'a str) -> MachineLifecycleFuture<'a> {
        let roots = self.roots.clone();
        let calls = self.calls.clone();
        let name = name.to_owned();
        Box::pin(async move {
            calls
                .lock()
                .expect("calls lock should not poison")
                .push(format!("start:{name}"));
            Ok(snapshot_for(
                &name,
                roots,
                nimbus_machine::MachineLifecycle::Running,
            ))
        })
    }

    fn stop_machine<'a>(&'a self, name: &'a str) -> MachineLifecycleFuture<'a> {
        let roots = self.roots.clone();
        let calls = self.calls.clone();
        let name = name.to_owned();
        Box::pin(async move {
            calls
                .lock()
                .expect("calls lock should not poison")
                .push(format!("stop:{name}"));
            Ok(snapshot_for(
                &name,
                roots,
                nimbus_machine::MachineLifecycle::Stopped,
            ))
        })
    }

    fn restart_machine<'a>(&'a self, name: &'a str) -> MachineLifecycleFuture<'a> {
        let roots = self.roots.clone();
        let calls = self.calls.clone();
        let name = name.to_owned();
        Box::pin(async move {
            calls
                .lock()
                .expect("calls lock should not poison")
                .push(format!("restart:{name}"));
            Ok(snapshot_for(
                &name,
                roots,
                nimbus_machine::MachineLifecycle::Running,
            ))
        })
    }

    fn update_machine<'a>(&'a self, request: MachineUpdateRequest) -> MachineLifecycleFuture<'a> {
        let roots = self.roots.clone();
        let calls = self.calls.clone();
        Box::pin(async move {
            calls
                .lock()
                .expect("calls lock should not poison")
                .push(format!("update:{}", request.name));
            Ok(snapshot_for_resources(
                &request.name,
                roots,
                nimbus_machine::MachineLifecycle::Stopped,
                request.cpus.unwrap_or(2),
                request.memory_mib.unwrap_or(2048),
                request.disk_gib.unwrap_or(20),
            ))
        })
    }

    fn delete_machine<'a>(&'a self, name: &'a str) -> MachineLifecycleFuture<'a> {
        let roots = self.roots.clone();
        let calls = self.calls.clone();
        let name = name.to_owned();
        Box::pin(async move {
            calls
                .lock()
                .expect("calls lock should not poison")
                .push(format!("delete:{name}"));
            Ok(snapshot_for(
                &name,
                roots,
                nimbus_machine::MachineLifecycle::Stopped,
            ))
        })
    }
}

#[tokio::test]
async fn machine_lifecycle_routes_call_manager_and_project_system_state() {
    let temp = tempdir().expect("tempdir should build");
    let service = Arc::new(Service::new(temp.path()).expect("service should create"));
    let roots = nimbus_machine::MachineRootLayout::new(
        temp.path().join("machine-config"),
        temp.path().join("machine-state"),
        temp.path().join("run"),
    );
    let manager = StubMachineLifecycleManager::new(roots);
    let server = ServerFixture::start(
        RouterBuildConfig::core(service.clone())
            .with_machine_lifecycle_manager(Arc::new(manager.clone()))
            .build(),
    )
    .await;
    crate::system_tenant::prepare_system_tenant_async(&service, None)
        .await
        .expect("system tenant should prepare before subscribing");
    let (system_tx, mut system_rx) =
        tokio::sync::mpsc::channel(nimbus_engine::DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY);
    let system_subscription = service
        .subscribe_async(
            crate::system_tenant::system_tenant_id().expect("system id should parse"),
            nimbus_core::Query {
                table: nimbus_core::TableName::new("machines").expect("table should parse"),
                filters: Vec::new(),
                order: None,
                limit: None,
            },
            "system-machines-watch".to_string(),
            system_tx,
        )
        .await
        .expect("system machines table should be subscribable");
    let initial = next_system_machine_documents(&mut system_rx, "initial machines snapshot").await;
    assert!(
        !initial
            .iter()
            .any(|document| document["name"] == json!("demo")),
        "system machines subscription should start without demo machine: {initial:?}"
    );

    let start = server
        .client()
        .post(server.http_url("/api/machines/demo/start"))
        .send()
        .await
        .expect("machine start request should send");
    assert_eq!(start.status(), StatusCode::OK);
    let start_body = start
        .json::<serde_json::Value>()
        .await
        .expect("start response should parse");
    assert_eq!(start_body["name"], json!("demo"));
    assert_eq!(start_body["provider"], json!("krunkit"));
    assert_eq!(start_body["state"], json!("running"));
    assert_eq!(start_body["manager"], json!("ready"));
    assert_eq!(
        start_body["guest"]["provisioning"],
        json!("bootc-machine-config")
    );
    assert_eq!(start_body["runtime"]["sshPort"], json!(10022));

    assert_eq!(manager.calls(), vec!["start:demo"]);
    assert_system_machine_state(&service, "running", "listening", "running").await;
    wait_for_system_machine_state(
        &mut system_rx,
        "running machine subscription update",
        "running",
    )
    .await;
    assert_system_events(&service, &[("machine.lifecycle", "start", "running")]).await;

    let stop = server
        .client()
        .post(server.http_url("/api/machines/demo/stop"))
        .send()
        .await
        .expect("machine stop request should send");
    assert_eq!(stop.status(), StatusCode::OK);
    let stop_body = stop
        .json::<serde_json::Value>()
        .await
        .expect("stop response should parse");
    assert_eq!(stop_body["state"], json!("stopped"));
    assert_eq!(stop_body["manager"], json!("helpers-resolved"));

    assert_eq!(manager.calls(), vec!["start:demo", "stop:demo"]);
    assert_system_machine_state(&service, "stopped", "stopped", "stopped").await;
    wait_for_system_machine_state(
        &mut system_rx,
        "stopped machine subscription update",
        "stopped",
    )
    .await;
    assert_system_events(
        &service,
        &[
            ("machine.lifecycle", "start", "running"),
            ("machine.lifecycle", "stop", "stopped"),
        ],
    )
    .await;
    drop(system_subscription);
}

#[tokio::test]
async fn machine_config_routes_create_update_delete_and_project_system_state() {
    let temp = tempdir().expect("tempdir should build");
    let service = Arc::new(Service::new(temp.path()).expect("service should create"));
    let roots = nimbus_machine::MachineRootLayout::new(
        temp.path().join("machine-config"),
        temp.path().join("machine-state"),
        temp.path().join("run"),
    );
    let manager = StubMachineLifecycleManager::new(roots);
    let server = ServerFixture::start(
        RouterBuildConfig::core(service.clone())
            .with_machine_lifecycle_manager(Arc::new(manager.clone()))
            .build(),
    )
    .await;

    let create = server
        .client()
        .post(server.http_url("/api/machines/team-a/create"))
        .json(&json!({
            "cpus": 4,
            "memoryMiB": 4096,
            "diskGiB": 50,
            "bootcNative": true
        }))
        .send()
        .await
        .expect("machine create request should send");
    assert_eq!(create.status(), StatusCode::OK);
    let create_body = create
        .json::<serde_json::Value>()
        .await
        .expect("create response should parse");
    assert_eq!(create_body["name"], json!("team-a"));
    assert_eq!(create_body["state"], json!("stopped"));
    assert_eq!(create_body["resources"]["cpus"], json!(4));
    assert_eq!(create_body["resources"]["memoryMiB"], json!(4096));
    assert_eq!(create_body["resources"]["diskGiB"], json!(50));
    assert_system_machine_document(&service, "team-a", "stopped", 4, 4096, 50).await;

    let update = server
        .client()
        .patch(server.http_url("/api/machines/team-a"))
        .json(&json!({
            "cpus": 6,
            "memoryMiB": 8192,
            "diskGiB": 80
        }))
        .send()
        .await
        .expect("machine update request should send");
    assert_eq!(update.status(), StatusCode::OK);
    let update_body = update
        .json::<serde_json::Value>()
        .await
        .expect("update response should parse");
    assert_eq!(update_body["resources"]["cpus"], json!(6));
    assert_eq!(update_body["resources"]["memoryMiB"], json!(8192));
    assert_eq!(update_body["resources"]["diskGiB"], json!(80));
    assert_system_machine_document(&service, "team-a", "stopped", 6, 8192, 80).await;

    let delete = server
        .client()
        .delete(server.http_url("/api/machines/team-a"))
        .send()
        .await
        .expect("machine delete request should send");
    assert_eq!(delete.status(), StatusCode::OK);
    let delete_body = delete
        .json::<serde_json::Value>()
        .await
        .expect("delete response should parse");
    assert_eq!(delete_body["name"], json!("team-a"));
    assert_eq!(delete_body["state"], json!("deleted"));
    assert_eq!(delete_body["previousState"], json!("stopped"));
    assert_eq!(
        manager.calls(),
        vec!["create:team-a", "update:team-a", "delete:team-a"]
    );
    assert_system_machine_deleted(&service, "team-a").await;
    assert_system_events_for_machine(
        &service,
        "team-a",
        &[
            ("machine.lifecycle", "create", "stopped"),
            ("machine.lifecycle", "update", "stopped"),
            ("machine.lifecycle", "delete", "deleted"),
        ],
    )
    .await;
}

#[tokio::test]
async fn machine_lifecycle_routes_require_configured_manager() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(RouterBuildConfig::core(fixture.service()).build()).await;

    let response = server
        .client()
        .post(server.http_url("/api/machines/demo/start"))
        .send()
        .await
        .expect("machine start request should send");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

fn snapshot_for(
    name: &str,
    roots: nimbus_machine::MachineRootLayout,
    lifecycle: nimbus_machine::MachineLifecycle,
) -> MachineLifecycleSnapshot {
    snapshot_for_resources(name, roots, lifecycle, 4, 4096, 50)
}

fn snapshot_for_resources(
    name: &str,
    roots: nimbus_machine::MachineRootLayout,
    lifecycle: nimbus_machine::MachineLifecycle,
    cpus: u8,
    memory_mib: u32,
    disk_gib: u32,
) -> MachineLifecycleSnapshot {
    let config = nimbus_machine::MachineConfigRecord {
        version: nimbus_machine::CURRENT_MACHINE_CONFIG_VERSION,
        name: name.to_owned(),
        provider: nimbus_machine::MachineProvider::Krunkit,
        guest: nimbus_machine::MachineGuestConfig {
            image_source: nimbus_machine::MachineImageSource::OciReference {
                reference: "docker://ghcr.io/nimbus/machine-os:v0.1.31".to_owned(),
            },
            provisioning: nimbus_machine::MachineGuestProvisioning::BootcMachineConfig,
            ssh_user: "nimbus".to_owned(),
            ssh_identity_path: None,
            ignition_file_path: None,
            efi_variable_store_path: None,
        },
        resources: nimbus_machine::MachineResources {
            cpus,
            memory_mib,
            disk_gib,
        },
        volumes: Vec::new(),
        roots: roots.clone(),
    };
    let paths = roots.paths(name);
    let state = nimbus_machine::MachineStateRecord {
        version: nimbus_machine::CURRENT_MACHINE_STATE_VERSION,
        lifecycle,
        manager: if matches!(lifecycle, nimbus_machine::MachineLifecycle::Running) {
            nimbus_machine::MachineManagerState::Ready
        } else {
            nimbus_machine::MachineManagerState::HelpersResolved
        },
        runtime: Some(nimbus_machine::MachineRuntimeState {
            helper_binaries: nimbus_machine::MachineHelperBinaryPaths {
                krunkit: paths.runtime_dir.join("krunkit"),
                gvproxy: paths.runtime_dir.join("gvproxy"),
            },
            image_path: paths.materialized_image_path,
            efi_variable_store_path: paths.efi_variable_store_path,
            machine_image_source: "docker://ghcr.io/nimbus/machine-os:v0.1.31".to_owned(),
            ssh_port: 10022,
            rest_uri: format!("unix://{}", paths.api_socket_path.display()),
            ready_vsock_port: 1025,
        }),
        last_error: None,
    };
    MachineLifecycleSnapshot::new(config, state)
}

async fn assert_system_machine_state(
    service: &Arc<Service>,
    machine_state: &str,
    listener_state: &str,
    port_state: &str,
) {
    let tenant_id = crate::system_tenant::system_tenant_id().expect("system id should parse");
    let machines = service
        .list_documents_async(
            tenant_id.clone(),
            TableName::new("machines").expect("table should parse"),
        )
        .await
        .expect("machines should list");
    assert_eq!(machines.len(), 1);
    assert_eq!(machines[0].fields.get("name"), Some(&json!("demo")));
    assert_eq!(machines[0].fields.get("state"), Some(&json!(machine_state)));

    let listeners = service
        .list_documents_async(
            tenant_id.clone(),
            TableName::new("listeners").expect("table should parse"),
        )
        .await
        .expect("listeners should list");
    assert_eq!(listeners.len(), 1);
    assert_eq!(listeners[0].fields.get("adapter"), Some(&json!("machine")));
    assert_eq!(
        listeners[0].fields.get("state"),
        Some(&json!(listener_state))
    );

    let ports = service
        .list_documents_async(
            tenant_id,
            TableName::new("ports").expect("table should parse"),
        )
        .await
        .expect("ports should list");
    assert_eq!(ports.len(), 1);
    assert_eq!(ports[0].fields.get("machineId"), Some(&json!("demo")));
    assert_eq!(ports[0].fields.get("hostPort"), Some(&json!(10022)));
    assert_eq!(ports[0].fields.get("state"), Some(&json!(port_state)));
}

async fn assert_system_machine_document(
    service: &Arc<Service>,
    name: &str,
    state: &str,
    cpus: u8,
    memory_mib: u32,
    disk_gib: u32,
) {
    let machine = service
        .get_document_async(
            crate::system_tenant::system_tenant_id().expect("system id should parse"),
            TableName::new("machines").expect("table should parse"),
            DocumentId::from_key(format!("machine:{name}")).expect("id should parse"),
        )
        .await
        .expect("machine document should exist");
    assert_eq!(machine.fields.get("name"), Some(&json!(name)));
    assert_eq!(machine.fields.get("state"), Some(&json!(state)));
    assert_eq!(machine.fields["resources"]["cpus"], json!(cpus));
    assert_eq!(machine.fields["resources"]["memoryMiB"], json!(memory_mib));
    assert_eq!(machine.fields["resources"]["diskGiB"], json!(disk_gib));
}

async fn assert_system_machine_deleted(service: &Arc<Service>, name: &str) {
    let tenant_id = crate::system_tenant::system_tenant_id().expect("system id should parse");
    for (table, document_id) in [
        ("machines", format!("machine:{name}")),
        ("listeners", format!("listener:machine-api:{name}")),
        ("ports", format!("port:machine:{name}:ssh")),
    ] {
        let missing = service
            .get_document_async(
                tenant_id.clone(),
                TableName::new(table).expect("table should parse"),
                DocumentId::from_key(document_id).expect("id should parse"),
            )
            .await
            .expect_err("machine projection document should be deleted");
        assert!(
            matches!(missing, nimbus_core::Error::DocumentNotFound(_)),
            "expected missing projection document, got {missing:?}"
        );
    }
}

async fn assert_system_events(service: &Arc<Service>, expected: &[(&str, &str, &str)]) {
    assert_system_events_for_machine(service, "demo", expected).await;
}

async fn assert_system_events_for_machine(
    service: &Arc<Service>,
    machine_name: &str,
    expected: &[(&str, &str, &str)],
) {
    let events = service
        .list_documents_async(
            crate::system_tenant::system_tenant_id().expect("system id should parse"),
            TableName::new("events").expect("table should parse"),
        )
        .await
        .expect("events should list");
    assert_eq!(events.len(), expected.len());
    let mut actual = Vec::new();
    for event in &events {
        assert_eq!(event.fields.get("source"), Some(&json!("machine")));
        assert_eq!(event.fields.get("level"), Some(&json!("info")));
        assert_eq!(event.fields["data"]["machineId"], json!(machine_name));
        assert!(
            event
                .fields
                .get("createdAt")
                .and_then(|value| value.as_u64())
                .is_some(),
            "event should have numeric createdAt: {event:?}"
        );
        actual.push((
            event.fields["category"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            event.fields["data"]["action"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            event.fields["data"]["state"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        ));
    }
    let mut expected = expected
        .iter()
        .map(|(category, action, state)| {
            (
                (*category).to_owned(),
                (*action).to_owned(),
                (*state).to_owned(),
            )
        })
        .collect::<Vec<_>>();
    actual.sort();
    expected.sort();
    assert_eq!(actual, expected);
}

async fn next_system_machine_documents(
    updates: &mut tokio::sync::mpsc::Receiver<nimbus_engine::SubscriptionUpdate>,
    description: &str,
) -> Vec<serde_json::Value> {
    match timeout(Duration::from_secs(5), updates.recv()).await {
        Ok(Some(nimbus_engine::SubscriptionUpdate::Result { snapshot, .. })) => {
            snapshot.to_json_documents()
        }
        Ok(Some(nimbus_engine::SubscriptionUpdate::Error { message, .. })) => {
            panic!("{description} failed with subscription error: {message}")
        }
        Ok(None) => panic!("{description} failed because subscription channel closed"),
        Err(_) => panic!("timed out waiting for {description}"),
    }
}

async fn wait_for_system_machine_state(
    updates: &mut tokio::sync::mpsc::Receiver<nimbus_engine::SubscriptionUpdate>,
    description: &str,
    state: &str,
) -> Vec<serde_json::Value> {
    timeout(Duration::from_secs(5), async {
        loop {
            let documents = next_system_machine_documents(updates, description).await;
            if documents.iter().any(|document| {
                document["name"] == json!("demo") && document["state"] == json!(state)
            }) {
                return documents;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {description}"))
}
