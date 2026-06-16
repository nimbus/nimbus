use super::*;

#[tokio::test]
async fn dev_serves_firestore_routes_without_firebase_markers() {
    // DX contract: dev mounts the Firestore-compatible route family
    // unconditionally — zero Firebase markers in the app (no
    // firebase.json, no firebase dependency). The routes ride the main
    // HTTP listener and are inert without callers.
    let temp = tempdir().expect("tempdir should build");
    create_source_root(temp.path(), "convex");

    let plan = resolve_dev_plan(parse_dev(["nimbus", "dev"]), temp.path())
        .expect("dev plan should resolve");
    assert!(
        plan.start_command.firestore,
        "dev plan must request the Firestore route family unconditionally"
    );

    // Resolve through the same enablement path `nimbus start` uses; the
    // wire listeners are always available too (D6/D7), store-backed on
    // the plan's ephemeral ports.
    let enablement = crate::start::adapters::resolve_adapter_enablement_with_env(
        &plan.start_command,
        plan.start_command
            .control_data_dir
            .as_deref()
            .expect("dev plan sets the control data dir"),
        |_| None,
        |_| true,
    )
    .expect("dev enablement should resolve");
    assert!(enablement.firebase.is_some());
    assert!(enablement.mongodb.is_some());
    assert!(enablement.dynamodb.is_some());

    let engine = std::sync::Arc::new(
        nimbus::Engine::new(temp.path().join("engine")).expect("engine should build"),
    );
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("listener should bind");
    let addr = listener.local_addr().expect("addr should resolve");
    let task = tokio::spawn(nimbus_server::serve(
        listener,
        enablement.apply_to(nimbus_server::ServeOptions::new(engine.clone())),
    ));
    crate::test_support::wait_for_live_server_health(
        "dev-shaped server should answer /health",
        addr,
        &task,
    )
    .await;

    let response = reqwest::Client::new()
        .get(format!(
            "http://{addr}/v1/projects/demo/databases/(default)/documents/notes"
        ))
        .send()
        .await
        .expect("firestore request should send");
    assert_ne!(
        response.status(),
        reqwest::StatusCode::NOT_FOUND,
        "dev must answer the Firestore route family without Firebase markers"
    );

    task.abort();
    let _ = task.await;
    engine.quiesce().await;
}

#[tokio::test]
async fn pure_convex_dev_serves_wire_listeners_on_ephemeral_ports() {
    // D6: a pure-Convex app (no driver deps) still gets both wire
    // listeners — on ephemeral ports nothing in the app references, so
    // a real mongod or DynamoDB Local beside it sees zero interference.
    let temp = tempdir().expect("tempdir should build");
    create_source_root(temp.path(), "convex");

    let plan = resolve_dev_plan(parse_dev(["nimbus", "dev"]), temp.path())
        .expect("dev plan should resolve");
    assert!(!plan.wire_surfaces.mongodb && !plan.wire_surfaces.dynamodb);
    let mongodb_port = plan
        .start_command
        .mongodb_port
        .expect("dev plan pins the mongodb port");
    let dynamodb_port = plan
        .start_command
        .dynamodb_port
        .expect("dev plan pins the dynamodb port");

    let enablement = crate::start::adapters::resolve_adapter_enablement_with_env(
        &plan.start_command,
        plan.start_command
            .control_data_dir
            .as_deref()
            .expect("dev plan sets the control data dir"),
        |_| None,
        |_| true,
    )
    .expect("dev enablement should resolve");
    assert!(enablement.mongodb.is_some());
    assert!(enablement.dynamodb.is_some());

    let engine = std::sync::Arc::new(
        nimbus::Engine::new(temp.path().join("engine")).expect("engine should build"),
    );
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("listener should bind");
    let addr = listener.local_addr().expect("addr should resolve");
    let task = tokio::spawn(nimbus_server::serve(
        listener,
        enablement.apply_to(nimbus_server::ServeOptions::new(engine.clone())),
    ));
    crate::test_support::wait_for_live_server_health(
        "dev-shaped server should answer /health",
        addr,
        &task,
    )
    .await;

    assert!(
        tokio::net::TcpStream::connect(("127.0.0.1", mongodb_port))
            .await
            .is_ok(),
        "the mongodb listener should accept connections on the plan's ephemeral port"
    );
    assert!(
        tokio::net::TcpStream::connect(("127.0.0.1", dynamodb_port))
            .await
            .is_ok(),
        "the dynamodb listener should accept connections on the plan's ephemeral port"
    );

    task.abort();
    let _ = task.await;
    engine.quiesce().await;
}

fn workspace_wire_selftest_dependencies_available(repo_root: &Path) -> bool {
    let root_node_modules = repo_root.join("node_modules");
    let has_dependency = |package_dir: &str, scoped_segments: &[&str]| {
        let candidates = [
            root_node_modules.clone(),
            repo_root.join(package_dir).join("node_modules"),
        ];
        candidates.iter().any(|node_modules| {
            let mut path = node_modules.clone();
            for segment in scoped_segments {
                path.push(segment);
            }
            path.is_dir()
        })
    };

    repo_root
        .join("packages/mongodb/src/selftest.mjs")
        .is_file()
        && repo_root
            .join("packages/dynamodb/src/selftest.mjs")
            .is_file()
        && has_dependency("packages/mongodb", &["mongodb"])
        && has_dependency("packages/dynamodb", &["@aws-sdk", "client-dynamodb"])
}

#[tokio::test]
async fn detected_wire_app_round_trips_mongodb_and_dynamodb_drivers() {
    // DXW3 live gate: an app declaring both drivers, served by a
    // dev-shaped server, completes real driver round-trips using
    // exactly the credentials dev advertises in `.env.local` — and the
    // selftests' wrong-credential probes prove the listeners reject
    // anything else.
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root should exist");
    if !workspace_wire_selftest_dependencies_available(repo_root) {
        eprintln!(
            "skipping wire round-trip selftests because JS workspace dependencies are unavailable"
        );
        return;
    }

    let temp = tempdir().expect("tempdir should build");
    fs::create_dir_all(temp.path().join(".git")).expect(".git boundary should create");
    create_source_root(temp.path(), "convex");
    fs::write(
        temp.path().join("package.json"),
        r#"{"dependencies": {"mongodb": "^6.0.0", "@aws-sdk/client-dynamodb": "^3.600.0"}}"#,
    )
    .expect("package.json should write");

    let plan = resolve_dev_plan(parse_dev(["nimbus", "dev"]), temp.path())
        .expect("dev plan should resolve");
    assert!(plan.wire_surfaces.mongodb && plan.wire_surfaces.dynamodb);

    let enablement = crate::start::adapters::resolve_adapter_enablement_with_env(
        &plan.start_command,
        plan.start_command
            .control_data_dir
            .as_deref()
            .expect("dev plan sets the control data dir"),
        |_| None,
        |_| true,
    )
    .expect("dev enablement should resolve");
    let engine = std::sync::Arc::new(
        nimbus::Engine::new(temp.path().join("engine")).expect("engine should build"),
    );
    // Mirror boot's ensure_auto_tenant: the DynamoDB binding targets it.
    let auto_tenant = plan
        .start_command
        .auto_tenant
        .clone()
        .expect("dev plan sets an auto tenant");
    engine
        .create_tenant(nimbus::TenantId::new(&auto_tenant).expect("tenant id is valid"))
        .expect("auto tenant should create");
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("listener should bind");
    let addr = listener.local_addr().expect("addr should resolve");
    let task = tokio::spawn(nimbus_server::serve(
        listener,
        enablement.apply_to(nimbus_server::ServeOptions::new(engine.clone())),
    ));
    crate::test_support::wait_for_live_server_health(
        "dev-shaped server should answer /health",
        addr,
        &task,
    )
    .await;

    // MongoDB: real driver CRUD + aggregation with the store
    // credentials, plus the selftest's wrong-password rejection probe.
    let mongodb_port = plan
        .start_command
        .mongodb_port
        .expect("dev plan pins the mongodb port");
    let output = tokio::process::Command::new("node")
        .current_dir(repo_root)
        .arg("./packages/mongodb/src/selftest.mjs")
        .arg("--smoke-only")
        .arg("--smoke-port")
        .arg(mongodb_port.to_string())
        .arg("--smoke-username")
        .arg(&plan.wire.credentials.mongodb_username)
        .arg("--smoke-password")
        .arg(&plan.wire.credentials.mongodb_password)
        .output()
        .await
        .expect("mongodb selftest should run");
    assert!(
        output.status.success(),
        "mongodb round-trip selftest should pass\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // DynamoDB: real SigV4 driver round-trip with the store access
    // key, plus the selftest's wrong-secret rejection probe.
    let dynamodb_port = plan
        .start_command
        .dynamodb_port
        .expect("dev plan pins the dynamodb port");
    let output = tokio::process::Command::new("node")
        .current_dir(repo_root)
        .arg("./packages/dynamodb/src/selftest.mjs")
        .arg("--smoke-only")
        .arg("--smoke-port")
        .arg(dynamodb_port.to_string())
        .arg("--smoke-access-key-id")
        .arg(&plan.wire.credentials.dynamodb_access_key_id)
        .arg("--smoke-secret-access-key")
        .arg(&plan.wire.credentials.dynamodb_secret_access_key)
        .output()
        .await
        .expect("dynamodb selftest should run");
    assert!(
        output.status.success(),
        "dynamodb round-trip selftest should pass\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    task.abort();
    let _ = task.await;
    engine.quiesce().await;
}

#[tokio::test]
async fn mid_session_mongodb_adoption_round_trips_with_subscriptions_intact() {
    // DXL1 live gate: adoption is presentation-only (D6). The MongoDB
    // listener has served on the plan's port since boot; the manifest
    // watch notices the new dependency and refreshes `.env.local` from
    // the boot-time wire plan; the official driver then round-trips via
    // that unchanged listener; and a main-listener subscription opened
    // before the adoption keeps receiving pushes — nothing restarts.
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root should exist");
    if !workspace_wire_selftest_dependencies_available(repo_root) {
        eprintln!(
            "skipping mid-session adoption selftest because JS workspace dependencies are unavailable"
        );
        return;
    }

    let temp = tempdir().expect("tempdir should build");
    fs::create_dir_all(temp.path().join(".git")).expect(".git boundary should create");
    create_source_root(temp.path(), "convex");
    fs::write(temp.path().join("package.json"), r#"{"dependencies": {}}"#)
        .expect("package.json should write");

    let plan = resolve_dev_plan(parse_dev(["nimbus", "dev"]), temp.path())
        .expect("dev plan should resolve");
    assert!(
        !plan.wire_surfaces.mongodb,
        "the app must not declare the driver at boot"
    );
    let mongodb_port = plan
        .start_command
        .mongodb_port
        .expect("dev plan pins the mongodb port");

    let enablement = crate::start::adapters::resolve_adapter_enablement_with_env(
        &plan.start_command,
        plan.start_command
            .control_data_dir
            .as_deref()
            .expect("dev plan sets the control data dir"),
        |_| None,
        |_| true,
    )
    .expect("dev enablement should resolve");
    assert!(
        enablement.mongodb.is_some(),
        "the listener serves from boot regardless of detection (D6)"
    );
    let engine = std::sync::Arc::new(
        nimbus::Engine::new(temp.path().join("engine")).expect("engine should build"),
    );
    let auto_tenant = plan
        .start_command
        .auto_tenant
        .clone()
        .expect("dev plan sets an auto tenant");
    engine
        .create_tenant(nimbus::TenantId::new(&auto_tenant).expect("tenant id is valid"))
        .expect("auto tenant should create");
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("listener should bind");
    let addr = listener.local_addr().expect("addr should resolve");
    let task = tokio::spawn(nimbus_server::serve(
        listener,
        enablement.apply_to(nimbus_server::ServeOptions::new(engine.clone())),
    ));
    crate::test_support::wait_for_live_server_health(
        "dev-shaped server should answer /health",
        addr,
        &task,
    )
    .await;

    // Open a main-listener subscription before the adoption; it must
    // stay live across the rescan.
    let mut socket =
        nimbus_testing::WebSocketFixture::connect(&format!("ws://{addr}/ws"), &auto_tenant).await;
    socket.subscribe_all("dxl1", "tasks").await;
    let initial = socket.next_json().await;
    assert_eq!(initial["type"], serde_json::json!("subscription_result"));
    assert_eq!(initial["request_id"], serde_json::json!("dxl1"));
    assert_eq!(initial["data"], serde_json::json!([]));

    // Drive the real manifest watch loop. Prime it once so the baseline
    // snapshot predates the dependency write — the future is lazy, so
    // without this poll the baseline would already include the change.
    let (watch_roots_tx, _watch_roots_rx) = tokio::sync::watch::channel(plan.initial_watch_roots());
    let manifest_loop = redetect::run_manifest_watch_loop(redetect::ManifestWatch {
        app_dir: &plan.app_dir,
        wire: &plan.wire,
        initial_surfaces: plan.wire_surfaces,
        initial_adapter: plan.adapter.clone(),
        boot_auto_tenant: auto_tenant.clone(),
        watch_roots: &watch_roots_tx,
    });
    tokio::pin!(manifest_loop);
    let primed =
        tokio::time::timeout(std::time::Duration::from_millis(50), &mut manifest_loop).await;
    assert!(primed.is_err(), "the manifest watch loop must keep running");

    fs::write(
        temp.path().join("package.json"),
        r#"{"dependencies": {"mongodb": "^6.0.0"}}"#,
    )
    .expect("package.json should gain the driver");

    let env_path = temp.path().join(".env.local");
    let wait_for_refresh = async {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if let Ok(content) = fs::read_to_string(&env_path)
                && content.contains("NIMBUS_MONGODB_URL=")
            {
                break content;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                ".env.local should gain NIMBUS_MONGODB_URL within 10s of the manifest change"
            );
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    };
    let env_content = tokio::select! {
        _ = &mut manifest_loop => panic!("the manifest watch loop must not exit"),
        content = wait_for_refresh => content,
    };
    // The advertised URL carries the boot-time port and credentials —
    // the rescan presents the listener that has been serving all along;
    // it never re-resolves.
    let expected_url = format!(
        "NIMBUS_MONGODB_URL=mongodb://{}:{}@127.0.0.1:{}/",
        plan.wire.credentials.mongodb_username,
        plan.wire.credentials.mongodb_password,
        mongodb_port,
    );
    assert!(
        env_content.contains(&expected_url),
        ".env.local must advertise the boot-time listener: {env_content}"
    );

    // The driver round-trips through the already-serving listener.
    let output = tokio::process::Command::new("node")
        .current_dir(repo_root)
        .arg("./packages/mongodb/src/selftest.mjs")
        .arg("--smoke-only")
        .arg("--smoke-port")
        .arg(mongodb_port.to_string())
        .arg("--smoke-username")
        .arg(&plan.wire.credentials.mongodb_username)
        .arg("--smoke-password")
        .arg(&plan.wire.credentials.mongodb_password)
        .output()
        .await
        .expect("mongodb selftest should run");
    assert!(
        output.status.success(),
        "mid-session mongodb round-trip should pass\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // The pre-adoption subscription is still live: an insert through
    // the main listener pushes to the socket opened before the rescan.
    let insert = reqwest::Client::new()
        .post(format!("http://{addr}/api/tenants/{auto_tenant}/documents"))
        .json(&serde_json::json!({
            "table": "tasks",
            "fields": { "title": "still-live" },
        }))
        .send()
        .await
        .expect("document insert should send");
    assert!(
        insert.status().is_success(),
        "document insert should succeed: {}",
        insert.status()
    );
    let pushed = socket
        .next_json_with_timeout(std::time::Duration::from_secs(5))
        .await
        .expect("the pre-adoption subscription should still receive pushes");
    assert_eq!(pushed["type"], serde_json::json!("subscription_result"));
    assert_eq!(pushed["data"][0]["title"], serde_json::json!("still-live"));

    task.abort();
    let _ = task.await;
    engine.quiesce().await;
}

fn workspace_firebase_selftest_dependencies_available(repo_root: &Path) -> bool {
    let root_node_modules = repo_root.join("node_modules");
    let package_node_modules = repo_root.join("packages/firebase/node_modules");
    let has_dependency = |node_modules: &Path, scoped_segments: &[&str]| {
        let mut path = node_modules.to_path_buf();
        for segment in scoped_segments {
            path.push(segment);
        }
        path.is_dir()
    };

    repo_root
        .join("packages/firebase/src/selftest.mjs")
        .is_file()
        && (has_dependency(&root_node_modules, &["esbuild"])
            || has_dependency(&package_node_modules, &["esbuild"]))
        && (has_dependency(&root_node_modules, &["@connectrpc", "connect"])
            || has_dependency(&package_node_modules, &["@connectrpc", "connect"]))
        && (has_dependency(&root_node_modules, &["@connectrpc", "connect-web"])
            || has_dependency(&package_node_modules, &["@connectrpc", "connect-web"]))
        && (has_dependency(&root_node_modules, &["@bufbuild", "protobuf"])
            || has_dependency(&package_node_modules, &["@bufbuild", "protobuf"]))
}

#[tokio::test]
async fn covered_app_round_trips_firestore_via_emulator_connection() {
    // DXF4 live gate: a covered Firestore client app under a dev-shaped
    // server completes addDoc/getDocs/onSnapshot through
    // connectFirestoreEmulator, addressing the project id dev discovered
    // — proving the auto-created tenant IS the tenant the app's requests
    // resolve to.
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root should exist");
    if !workspace_firebase_selftest_dependencies_available(repo_root) {
        eprintln!(
            "skipping firestore round-trip selftest because JS workspace dependencies are unavailable"
        );
        return;
    }

    let temp = tempdir().expect("tempdir should build");
    fs::create_dir_all(temp.path().join(".git")).expect(".git boundary should create");
    fs::write(
        temp.path().join("package.json"),
        r#"{"dependencies": {"firebase": "^11.0.0"}}"#,
    )
    .expect("package.json should write");
    fs::write(
        temp.path().join(".firebaserc"),
        r#"{"projects": {"default": "dxf4-round-trip"}}"#,
    )
    .expect(".firebaserc should write");

    let plan = resolve_dev_plan(parse_dev(["nimbus", "dev"]), temp.path())
        .expect("dev plan should resolve");
    assert_eq!(plan.adapter, Some(DevAdapter::FirestoreClient));
    assert_eq!(
        plan.start_command.auto_tenant,
        Some("dxf4-round-trip".to_string()),
        "the auto-created tenant must be the discovered project id"
    );

    let enablement = crate::start::adapters::resolve_adapter_enablement_with_env(
        &plan.start_command,
        plan.start_command
            .control_data_dir
            .as_deref()
            .expect("dev plan sets the control data dir"),
        |_| None,
        |_| true,
    )
    .expect("dev enablement should resolve");
    let engine = std::sync::Arc::new(
        nimbus::Engine::new(temp.path().join("engine")).expect("engine should build"),
    );
    // Mirror boot's ensure_auto_tenant for the planned auto_tenant.
    engine
        .create_tenant(nimbus::TenantId::new("dxf4-round-trip").expect("tenant id is valid"))
        .expect("mapped tenant should create");
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("listener should bind");
    let addr = listener.local_addr().expect("addr should resolve");
    let task = tokio::spawn(nimbus_server::serve(
        listener,
        enablement.apply_to(nimbus_server::ServeOptions::new(engine.clone())),
    ));
    crate::test_support::wait_for_live_server_health(
        "dev-shaped server should answer /health",
        addr,
        &task,
    )
    .await;

    let output = tokio::process::Command::new("node")
        .current_dir(repo_root)
        .arg("./packages/firebase/src/selftest.mjs")
        .arg("--round-trip-base-url")
        .arg(format!("http://{addr}/"))
        .arg("--round-trip-project-id")
        .arg("dxf4-round-trip")
        .output()
        .await
        .expect("firestore round-trip selftest should run");
    assert!(
        output.status.success(),
        "firestore round-trip selftest should pass\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    task.abort();
    let _ = task.await;
    engine.quiesce().await;
}
