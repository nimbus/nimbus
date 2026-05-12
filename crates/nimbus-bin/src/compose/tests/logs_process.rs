use super::*;

#[test]
fn resolve_service_ctr_log_path_defaults_to_local_project_tenant_and_honors_override() {
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

    let default_path = resolve_service_ctr_log_path(
        &ComposeLogsCommand {
            service: "db".to_owned(),
            file: vec![compose_path.clone()],
            tenant: None,
            follow: false,
        },
        &control_data_dir,
    )
    .expect("default log path should resolve");
    assert!(default_path.ends_with("containers/db-01aaa/ctr.log"));

    let override_path = resolve_service_ctr_log_path(
        &ComposeLogsCommand {
            service: "db".to_owned(),
            file: vec![compose_path],
            tenant: Some(TenantId::new("tenant-other").expect("tenant should parse")),
            follow: false,
        },
        &control_data_dir,
    )
    .expect("override log path should resolve");
    assert!(override_path.ends_with("containers/db-01bbb/ctr.log"));
}

#[test]
fn read_log_chunk_returns_empty_for_missing_files_and_only_appended_bytes() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let log_path = temp_dir.path().join("ctr.log");

    let (missing_chunk, missing_offset) =
        read_log_chunk(&log_path, 0).expect("missing files should read as empty");
    assert!(missing_chunk.is_empty());
    assert_eq!(missing_offset, 0);

    fs::write(&log_path, "line one\nline two\n").expect("log fixture should write");
    let (full_chunk, full_offset) =
        read_log_chunk(&log_path, 0).expect("initial read should succeed");
    assert_eq!(full_chunk, "line one\nline two\n");
    assert_eq!(full_offset, 18);

    fs::write(&log_path, "line one\nline two\nline three\n")
        .expect("appended log fixture should write");
    let (appended_chunk, appended_offset) =
        read_log_chunk(&log_path, full_offset).expect("appended read should succeed");
    assert_eq!(appended_chunk, "line three\n");
    assert_eq!(appended_offset, 29);
}

#[test]
fn read_pid_file_if_exists_returns_none_for_missing_and_parses_trimmed_values() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let pidfile = temp_dir.path().join("pidfile");

    assert_eq!(
        read_pid_file_if_exists(&pidfile).expect("missing pidfile should read as none"),
        None
    );

    fs::write(&pidfile, "1234\n").expect("pidfile should write");
    assert_eq!(
        read_pid_file_if_exists(&pidfile).expect("pidfile should parse"),
        Some(1234)
    );
}

#[test]
fn parse_process_rows_filters_requested_pids_and_preserves_command_text() {
    let stdout = "\
  101   1 /usr/bin/conmon --runtime /usr/libexec/nimbus/crun\n\
  202 101 /usr/libexec/nimbus/crun --root /run/user/1000/crun\n\
  303   1 /usr/sbin/unrelated\n";
    let pid_set = BTreeSet::from([101_u32, 202_u32]);

    let rows = parse_process_rows(stdout, &pid_set);

    assert_eq!(
        rows,
        vec![
            ServiceProcessRow {
                pid: 101,
                ppid: 1,
                command: "/usr/bin/conmon --runtime /usr/libexec/nimbus/crun".to_owned()
            },
            ServiceProcessRow {
                pid: 202,
                ppid: 101,
                command: "/usr/libexec/nimbus/crun --root /run/user/1000/crun".to_owned()
            }
        ]
    );
}

#[test]
fn render_compose_top_reads_pidfiles_from_persisted_state() {
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
    let container_dir = krun_config.state_root.join("containers").join("db-01aaa");
    fs::write(container_dir.join("pidfile"), "4294967294\n").expect("pidfile should write");
    fs::write(container_dir.join("conmon.pid"), "4294967295\n")
        .expect("conmon pidfile should write");

    let rendered = render_compose_top_for_platform(
        &ComposeTopCommand {
            service: "db".to_owned(),
            file: vec![compose_path],
            tenant: None,
            format: ComposeTopOutputFormat::Table,
            no_heading: false,
        },
        &control_data_dir,
        ServiceHostPlatform::Linux,
        None,
    )
    .expect("compose top should render");
    assert!(rendered.contains("Compose top snapshot for db"));
    assert!(rendered.contains("db-01aaa"));
    assert!(rendered.contains("runtime pid: 4294967294"));
    assert!(rendered.contains("conmon pid: 4294967295"));
    assert!(rendered.contains("tracked processes: none"));
}

#[test]
fn render_compose_top_can_omit_process_table_headings() {
    let snapshot = ServiceProcessSnapshot {
        sandbox_id: SandboxId::new("db-01aaa"),
        tenant_id: TenantId::new("tenant-a").expect("tenant should parse"),
        service_name: "db".to_owned(),
        status: SandboxStatus::Ready,
        runtime_pidfile: PathBuf::from("/run/db/pidfile"),
        conmon_pidfile: PathBuf::from("/run/db/conmon.pid"),
        runtime_pid: Some(2002),
        conmon_pid: Some(1001),
        process_rows: vec![ServiceProcessRow {
            pid: 2002,
            ppid: 1001,
            command: "/usr/bin/nimbus".to_owned(),
        }],
    };

    let rendered =
        render_service_process_snapshot_view(&snapshot, ComposeTopOutputFormat::Table, true)
            .expect("table output without headings should render");

    assert!(rendered.contains("Compose top snapshot for db"));
    assert!(!rendered.contains("PID"));
    assert!(!rendered.contains("PPID"));
    assert!(rendered.contains("/usr/bin/nimbus"));
}
