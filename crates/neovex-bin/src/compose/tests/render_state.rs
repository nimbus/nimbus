use super::*;

#[test]
fn render_service_list_defaults_to_local_project_tenant_and_can_expand_to_all_tenants() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let compose_path = write_compose_fixture(temp_dir.path());
    let control_data_dir = temp_dir.path().join("control");
    let context = load_compose_project_context(&compose_path, &control_data_dir)
        .expect("compose project context should load");
    let krun_config = context.control_plane.krun_backend_config();

    write_manifest(
        &krun_config.state_root,
        "db-01aaa",
        context.control_plane.local_tenant_id.as_str(),
        "db",
        SandboxStatus::Ready,
    );
    write_manifest(
        &krun_config.state_root,
        "db-01bbb",
        "tenant-other",
        "db",
        SandboxStatus::Ready,
    );

    let rendered_local = render_service_list_for_platform(
        &ComposePsCommand {
            file: compose_path.clone(),
            format: ComposePsOutputFormat::Table,
            no_heading: false,
            all_tenants: false,
        },
        &control_data_dir,
        ServiceHostPlatform::Linux,
        None,
    )
    .expect("local list should render");
    assert!(rendered_local.contains("SERVICE"), "{rendered_local}");
    assert!(rendered_local.contains("SANDBOX"), "{rendered_local}");
    assert!(rendered_local.contains("db-01aaa"), "{rendered_local}");
    assert!(rendered_local.contains(context.control_plane.local_tenant_id.as_str()));
    assert!(!rendered_local.contains("tenant-other"));

    let rendered_all = render_service_list_for_platform(
        &ComposePsCommand {
            file: compose_path,
            format: ComposePsOutputFormat::Table,
            no_heading: false,
            all_tenants: true,
        },
        &control_data_dir,
        ServiceHostPlatform::Linux,
        None,
    )
    .expect("all-tenant list should render");
    assert!(rendered_all.contains(context.control_plane.local_tenant_id.as_str()));
    assert!(rendered_all.contains("tenant-other"));
}

#[test]
fn render_service_list_can_omit_headings() {
    let summaries = vec![ServiceSandboxSummaryView {
        sandbox_id: SandboxId::new("db-01aaa"),
        tenant_id: TenantId::new("tenant-a").expect("tenant should parse"),
        service_name: "db".to_owned(),
        status: SandboxStatus::Ready,
        published_endpoints: Vec::new(),
        restart_count: 1,
        last_exit_code: None,
        shutdown_requested: false,
    }];

    let rendered = render_service_list_view(&summaries, ComposePsOutputFormat::Table, true)
        .expect("table output without headings should render");

    assert!(!rendered.contains("SERVICE"));
    assert!(!rendered.contains("TENANT"));
    assert!(rendered.contains("db"));
    assert!(rendered.contains("tenant-a"));
}

#[test]
fn render_service_inspect_defaults_to_local_project_tenant_and_honors_tenant_override() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let compose_path = write_compose_fixture(temp_dir.path());
    let control_data_dir = temp_dir.path().join("control");
    let context = load_compose_project_context(&compose_path, &control_data_dir)
        .expect("compose project context should load");
    let krun_config = context.control_plane.krun_backend_config();

    write_manifest(
        &krun_config.state_root,
        "db-01aaa",
        context.control_plane.local_tenant_id.as_str(),
        "db",
        SandboxStatus::Ready,
    );
    write_manifest(
        &krun_config.state_root,
        "db-01bbb",
        "tenant-other",
        "db",
        SandboxStatus::Stopped,
    );

    let rendered_default = render_service_inspect_for_platform(
        &ComposeInspectCommand {
            service: "db".to_owned(),
            file: compose_path.clone(),
            tenant: None,
            format: ComposeInspectOutputFormat::Json,
        },
        &control_data_dir,
        ServiceHostPlatform::Linux,
        None,
    )
    .expect("default inspect should render");
    assert!(rendered_default.contains(context.control_plane.local_tenant_id.as_str()));
    assert!(rendered_default.contains("\"service_name\": \"db\""));
    assert!(rendered_default.contains("db-01aaa"));
    assert!(rendered_default.contains("ctr.log"));

    let rendered_override = render_service_inspect_for_platform(
        &ComposeInspectCommand {
            service: "db".to_owned(),
            file: compose_path,
            tenant: Some(TenantId::new("tenant-other").expect("tenant should parse")),
            format: ComposeInspectOutputFormat::Yaml,
        },
        &control_data_dir,
        ServiceHostPlatform::Linux,
        None,
    )
    .expect("tenant override inspect should render");
    assert!(rendered_override.contains("tenant-other"));
    assert!(rendered_override.contains("db-01bbb"));
    assert!(rendered_override.contains("service_name: db"));
}
