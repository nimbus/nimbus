use super::*;

#[test]
fn resolve_service_down_targets_deduplicates_manifest_history_per_service_identity() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let compose_path = write_compose_fixture(temp_dir.path());
    let control_data_dir = temp_dir.path().join("control");
    let context = load_compose_project_context(&compose_path, &control_data_dir)
        .expect("compose project context should load");
    let krun_config = context.control_plane.krun_backend_config();
    let tenant = context.control_plane.local_tenant_id.clone();

    write_manifest(
        &krun_config.state_root,
        "db-01aaa",
        tenant.as_str(),
        "db",
        SandboxStatus::Stopped,
    );
    write_manifest(
        &krun_config.state_root,
        "db-01bbb",
        tenant.as_str(),
        "db",
        SandboxStatus::Ready,
    );
    write_manifest(
        &krun_config.state_root,
        "cache-01aaa",
        tenant.as_str(),
        "cache",
        SandboxStatus::Stopped,
    );

    let state_view = KrunSandboxStateView::from_config(&krun_config);
    let targets = resolve_service_down_targets(
        &state_view,
        &tenant,
        None,
        &context.control_plane.project_name,
    )
    .expect("targets should resolve");

    assert_eq!(targets.len(), 2);
    assert_eq!(
        targets
            .iter()
            .map(|target| {
                (
                    target.service_name.as_str(),
                    target.sandbox_id.as_str(),
                    target.status,
                )
            })
            .collect::<Vec<_>>(),
        vec![
            ("cache", "cache-01aaa", SandboxStatus::Stopped),
            ("db", "db-01bbb", SandboxStatus::Ready),
        ]
    );
}

#[tokio::test]
async fn start_service_launch_starts_image_launches_and_validates_identity() {
    let tenant = TenantId::new("svc-demo").expect("tenant should parse");
    let backend = StubBackend::default();
    let service_name = "db";
    let launch = SandboxServiceLaunch::image(SandboxImageLaunchSpec::new(
        sample_spec(&tenant, service_name),
        "busybox:latest",
    ));

    let handle = start_service_launch(&backend, &tenant, service_name, launch)
        .await
        .expect("launch should start");

    assert_eq!(handle.name, "db");
    assert_eq!(
        backend
            .started_services
            .lock()
            .expect("started services lock should hold")
            .as_slice(),
        &["db".to_owned()]
    );
}

#[tokio::test]
async fn stop_service_target_stops_active_handles_and_reports_already_stopped_terminal_ones() {
    let tenant = TenantId::new("svc-demo").expect("tenant should parse");
    let active_id = SandboxId::new("db-01aaa");
    let stopped_id = SandboxId::new("db-01bbb");
    let backend = StubBackend::with_handles([
        stub_handle(&active_id, "db", SandboxStatus::Ready),
        stub_handle(&stopped_id, "db", SandboxStatus::Stopped),
    ]);

    let stopped = stop_service_target(
        &backend,
        &tenant,
        ServiceLifecycleTarget {
            sandbox_id: active_id.clone(),
            service_name: "db".to_owned(),
            status: SandboxStatus::Ready,
        },
    )
    .await
    .expect("active handle should stop");
    assert_eq!(stopped.action, ServiceLifecycleAction::Stopped);
    assert_eq!(stopped.status, SandboxStatus::Stopped);

    let already_stopped = stop_service_target(
        &backend,
        &tenant,
        ServiceLifecycleTarget {
            sandbox_id: stopped_id.clone(),
            service_name: "db".to_owned(),
            status: SandboxStatus::Stopped,
        },
    )
    .await
    .expect("stopped handle should no-op");
    assert_eq!(
        already_stopped.action,
        ServiceLifecycleAction::AlreadyStopped
    );

    let stopped_ids = backend
        .stopped_ids
        .lock()
        .expect("stopped ids lock should hold");
    assert_eq!(stopped_ids.as_slice(), &[active_id.as_str().to_owned()]);
}
