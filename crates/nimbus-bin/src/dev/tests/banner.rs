use super::*;

#[test]
fn dev_banner_lines_report_explicit_compose_file() {
    let temp = tempdir().expect("tempdir should build");
    create_source_root(temp.path(), "convex");
    fs::write(
        temp.path().join("compose.custom.yaml"),
        "services:\n  db:\n    image: busybox:latest\n",
    )
    .expect("compose fixture should write");

    let plan = resolve_dev_plan(
        parse_dev(["nimbus", "dev", "--compose-file", "./compose.custom.yaml"]),
        temp.path(),
    )
    .expect("dev plan should resolve");

    let lines = dev_banner_lines(&plan);

    assert!(
        lines
            .iter()
            .any(|line| line == "Compose:    ./compose.custom.yaml")
    );
}

#[test]
fn dev_banner_lines_report_auto_discovered_override_selection() {
    let temp = tempdir().expect("tempdir should build");
    create_source_root(temp.path(), "convex");
    fs::write(
        temp.path().join("compose.yaml"),
        "services:\n  db:\n    image: busybox:latest\n",
    )
    .expect("compose fixture should write");
    fs::write(
        temp.path().join("compose.override.yaml"),
        "services:\n  worker:\n    image: redis:7\n",
    )
    .expect("compose override fixture should write");

    let plan = resolve_dev_plan(parse_dev(["nimbus", "dev"]), temp.path())
        .expect("dev plan should resolve");

    let lines = dev_banner_lines(&plan);
    let expected = format!(
        "Compose:    auto-discovered {} (+ compose.override.yaml)",
        temp.path().join("compose.yaml").display()
    );

    assert!(lines.iter().any(|line| line == &expected), "{lines:?}");
}

#[test]
fn dev_banner_lines_report_compose_file_environment_selection() {
    let selection = crate::compose::discovery::ResolvedComposeSelection {
        origin: crate::compose::discovery::ComposeSelectionOrigin::ExplicitEnvironment,
        project_root: PathBuf::from("/workspace"),
        files: vec![
            PathBuf::from("/workspace/compose.yaml"),
            PathBuf::from("/workspace/compose.dev.yaml"),
        ],
        display_files: vec![
            PathBuf::from("./compose.yaml"),
            PathBuf::from("./compose.dev.yaml"),
        ],
    };
    let plan = DevPlan {
        app_dir: PathBuf::from("/workspace"),
        data_dir: PathBuf::from("/workspace/.nimbus/dev"),
        deployment_slug: "workspace-abcd1234".to_owned(),
        compose_selection: Some(selection),
        local_url: "http://localhost:3210/".to_owned(),
        adapter: None,
        firestore_tenant: None,
        wire_surfaces: surfaces::WireSurfaces::default(),
        wire: wire::WirePlan::fixture(),
        once: false,
        tail_logs: DevTailLogsMode::PauseOnSync,
        start_command: StartCommand::default(),
        auto_open_decision: AutoOpenDecision::open(),
    };

    let lines = dev_banner_lines(&plan);

    assert!(lines.iter().any(|line| {
        line == "Compose:    COMPOSE_FILE=./compose.yaml (+ 1 extra Compose files)"
    }));
}

#[test]
fn dev_banner_lists_detected_wire_endpoints() {
    let mut plan = DevPlan {
        app_dir: PathBuf::from("/workspace"),
        data_dir: PathBuf::from("/workspace/.nimbus/dev"),
        deployment_slug: "workspace-abcd1234".to_owned(),
        compose_selection: None,
        local_url: "http://localhost:3210/".to_owned(),
        adapter: None,
        firestore_tenant: None,
        wire_surfaces: surfaces::WireSurfaces {
            mongodb: true,
            dynamodb: true,
            aws_sdk_v2_hint: false,
        },
        wire: wire::WirePlan::fixture(),
        once: false,
        tail_logs: DevTailLogsMode::PauseOnSync,
        start_command: StartCommand::default(),
        auto_open_decision: AutoOpenDecision::open(),
    };

    let lines = dev_banner_lines(&plan);
    assert!(
        lines.iter().any(|line| line
            == "MongoDB:    mongodb://127.0.0.1:27017/ (NIMBUS_MONGODB_URL in .env.local)"),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("new MongoClient(process.env.NIMBUS_MONGODB_URL)")),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|line| line
            == "DynamoDB:   http://127.0.0.1:8000 (NIMBUS_DYNAMODB_ENDPOINT in .env.local)"),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("new DynamoDBClient(")
            && line.contains("process.env.NIMBUS_DYNAMODB_SECRET_ACCESS_KEY")),
        "{lines:?}"
    );
    // The banner references env keys, never credential values.
    assert!(
        lines.iter().all(|line| {
            !line.contains(&plan.wire.credentials.mongodb_password)
                && !line.contains(&plan.wire.credentials.dynamodb_secret_access_key)
        }),
        "the banner must never print secrets: {lines:?}"
    );

    // D3: an aws-sdk v2 import alone earns a hint, not endpoint
    // promotion.
    plan.wire_surfaces = surfaces::WireSurfaces {
        mongodb: false,
        dynamodb: false,
        aws_sdk_v2_hint: true,
    };
    let lines = dev_banner_lines(&plan);
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("Hint:") && line.contains("@aws-sdk/client-dynamodb")),
        "{lines:?}"
    );
    assert!(
        lines.iter().all(|line| !line.starts_with("DynamoDB:")),
        "an undetected surface must not be promoted: {lines:?}"
    );
}

#[test]
fn dev_banner_includes_deployment_line() {
    let temp = tempdir().expect("tempdir should build");
    create_source_root(temp.path(), "convex");
    let plan = resolve_dev_plan(parse_dev(["nimbus", "dev"]), temp.path())
        .expect("dev plan should resolve");
    let lines = dev_banner_lines(&plan);
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("Deployment: local:")),
        "banner must include Deployment line, got: {lines:?}"
    );
}
