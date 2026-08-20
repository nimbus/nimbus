use super::*;

#[test]
fn convex_app_with_mongodb_dep_resolves_adapter_and_surface() {
    // App adapters are singular; wire surfaces are a set that combines
    // with any app adapter. A Convex app that also declares `mongodb`
    // must resolve BOTH, independently.
    let temp = tempdir().expect("tempdir should build");
    create_source_root(temp.path(), "convex");
    fs::write(
        temp.path().join("package.json"),
        r#"{"dependencies": {"mongodb": "^6.0.0"}}"#,
    )
    .expect("package.json should write");

    let plan = resolve_dev_plan(parse_dev(["nimbus", "dev"]), temp.path())
        .expect("dev plan should resolve");

    assert!(
        matches!(plan.adapter, Some(DevAdapter::Convex { .. })),
        "Convex app adapter must resolve despite the driver dependency"
    );
    assert!(
        plan.wire_surfaces.mongodb,
        "mongodb dependency must resolve the MongoDB wire surface"
    );
    assert!(!plan.wire_surfaces.dynamodb);
    assert!(!plan.wire_surfaces.s3);
    assert!(!plan.wire_surfaces.aws_sdk_v2_hint);
}

#[test]
fn dev_plan_without_driver_deps_resolves_empty_wire_surfaces() {
    let temp = tempdir().expect("tempdir should build");
    create_source_root(temp.path(), "convex");

    let plan = resolve_dev_plan(parse_dev(["nimbus", "dev"]), temp.path())
        .expect("dev plan should resolve");

    assert_eq!(plan.wire_surfaces, surfaces::WireSurfaces::default());
}

#[test]
fn dev_plan_prefers_native_source_root_for_watch_when_both_exist() {
    let temp = tempdir().expect("tempdir should build");
    create_source_root(temp.path(), "convex");
    create_source_root(temp.path(), "nimbus");

    let plan = resolve_dev_plan(parse_dev(["nimbus", "dev"]), temp.path())
        .expect("dev plan should resolve");

    assert_eq!(
        plan.adapter,
        Some(DevAdapter::Convex {
            source_root: plan.app_dir.join("nimbus"),
            package_target: crate::authoring_root::NIMBUS_TARGET,
        })
    );
    assert_eq!(
        plan.adapter.as_ref().and_then(DevAdapter::provision_target),
        Some(crate::authoring_root::NIMBUS_TARGET),
        "a native source root provisions the Nimbus SDK, not the Convex \
         compatibility package"
    );
}

#[test]
fn source_snapshot_detects_source_file_changes() {
    let temp = tempdir().expect("tempdir should build");
    let root = temp.path().join("convex");
    fs::create_dir_all(&root).expect("source root should build");
    fs::write(root.join("messages.ts"), "export const list = 1;\n")
        .expect("source file should write");

    let before = collect_source_snapshot(temp.path(), std::slice::from_ref(&root))
        .expect("snapshot should collect");
    fs::write(root.join("messages.ts"), "export const list = 12345;\n")
        .expect("source file should update");
    let after = collect_source_snapshot(temp.path(), std::slice::from_ref(&root))
        .expect("snapshot should recollect");

    assert_ne!(before, after);
}

#[test]
fn source_snapshot_ignores_generated_files() {
    let temp = tempdir().expect("tempdir should build");
    let root = temp.path().join("convex");
    fs::create_dir_all(root.join("_generated")).expect("generated root should build");
    fs::write(root.join("messages.ts"), "export const list = 1;\n")
        .expect("source file should write");
    fs::write(root.join("_generated").join("api.ts"), "first\n")
        .expect("generated file should write");

    let before = collect_source_snapshot(temp.path(), std::slice::from_ref(&root))
        .expect("snapshot should collect");
    fs::write(
        root.join("_generated").join("api.ts"),
        "second and longer\n",
    )
    .expect("generated file should update");
    let after = collect_source_snapshot(temp.path(), std::slice::from_ref(&root))
        .expect("snapshot should recollect");

    assert_eq!(before, after);
}

#[test]
fn dev_plan_empty_dir_has_no_source_root() {
    let temp = tempdir().expect("tempdir should build");
    let app_dir_str = temp.path().to_str().unwrap();

    let plan = resolve_dev_plan(
        parse_dev(["nimbus", "dev", "--app-dir", app_dir_str]),
        temp.path(),
    )
    .expect("dev plan should resolve");
    assert!(
        plan.adapter.is_none(),
        "empty dir should have no source root"
    );
}

#[test]
fn dev_plan_with_source_root_resolves() {
    let temp = tempdir().expect("tempdir should build");
    create_source_root(temp.path(), "convex");
    let app_dir_str = temp.path().to_str().unwrap();

    let plan = resolve_dev_plan(
        parse_dev(["nimbus", "dev", "--app-dir", app_dir_str]),
        temp.path(),
    )
    .expect("dev plan should resolve");
    assert!(
        plan.adapter.is_some(),
        "existing source root should be detected"
    );
}

#[test]
fn dev_skip_codegen_allows_no_source_root() {
    let temp = tempdir().expect("tempdir should build");
    let app_dir_str = temp.path().to_str().unwrap();
    let command = parse_dev(["nimbus", "dev", "--skip-codegen", "--app-dir", app_dir_str]);
    assert!(command.skip_codegen);

    let plan = resolve_dev_plan(command, temp.path()).expect("dev plan should resolve");
    assert!(plan.adapter.is_none());
}

#[test]
fn app_dir_nonexistent_errors_in_resolve() {
    let temp = tempdir().expect("tempdir should build");
    let new_dir = temp.path().join("new-project");
    let dir_str = new_dir.to_str().unwrap();

    let command = parse_dev(["nimbus", "dev", "--app-dir", dir_str]);
    assert!(!new_dir.exists());

    let plan = resolve_dev_plan(command, temp.path());
    assert!(
        plan.is_err(),
        "nonexistent --app-dir should error in resolve_dev_plan without pre-creation"
    );
}

#[test]
fn app_dir_empty_has_no_source_root() {
    let temp = tempdir().expect("tempdir should build");
    let empty_dir = temp.path().join("empty");
    fs::create_dir_all(&empty_dir).unwrap();
    let dir_str = empty_dir.to_str().unwrap();

    let plan = resolve_dev_plan(
        parse_dev(["nimbus", "dev", "--app-dir", dir_str]),
        temp.path(),
    )
    .expect("dev plan should resolve for empty --app-dir");

    assert!(plan.adapter.is_none());
}

#[test]
fn app_dir_nonempty_without_source_root_detected() {
    let temp = tempdir().expect("tempdir should build");
    let nonempty = temp.path().join("existing");
    fs::create_dir_all(&nonempty).unwrap();
    fs::write(nonempty.join("index.js"), "console.log('hi')").unwrap();
    let dir_str = nonempty.to_str().unwrap();

    let plan = resolve_dev_plan(
        parse_dev(["nimbus", "dev", "--app-dir", dir_str]),
        temp.path(),
    )
    .expect("dev plan should resolve");

    assert!(plan.adapter.is_none());
}

#[test]
fn app_dir_with_source_root_skips_edge_case_check() {
    let temp = tempdir().expect("tempdir should build");
    let project = temp.path().join("project");
    fs::create_dir_all(project.join("convex")).unwrap();
    fs::write(project.join("index.js"), "console.log('hi')").unwrap();
    let dir_str = project.to_str().unwrap();

    let plan = resolve_dev_plan(
        parse_dev(["nimbus", "dev", "--app-dir", dir_str]),
        temp.path(),
    )
    .expect("dev plan should resolve");

    assert!(
        plan.adapter.is_some(),
        "should detect source root in non-empty dir"
    );
}

#[test]
fn detect_cloud_functions_firebase_json() {
    let temp = tempdir().expect("tempdir should build");
    fs::create_dir_all(temp.path().join("functions")).unwrap();
    fs::write(
        temp.path().join("firebase.json"),
        r#"{"functions": {"source": "functions"}}"#,
    )
    .unwrap();

    let adapter = detect_dev_adapter(temp.path()).expect("adapter detection should succeed");
    assert_eq!(
        adapter,
        Some(DevAdapter::CloudFunctions {
            source_roots: vec![temp.path().join("functions").canonicalize().unwrap()],
        })
    );
}

#[test]
fn detect_cloud_functions_firebase_json_custom_source() {
    let temp = tempdir().expect("tempdir should build");
    fs::create_dir_all(temp.path().join("backend")).unwrap();
    fs::write(
        temp.path().join("firebase.json"),
        r#"{"functions": {"source": "backend"}}"#,
    )
    .unwrap();

    let adapter = detect_dev_adapter(temp.path()).expect("adapter detection should succeed");
    assert_eq!(
        adapter,
        Some(DevAdapter::CloudFunctions {
            source_roots: vec![temp.path().join("backend").canonicalize().unwrap()],
        })
    );
}

#[test]
fn detect_cloud_functions_firebase_json_array() {
    let temp = tempdir().expect("tempdir should build");
    fs::create_dir_all(temp.path().join("api")).unwrap();
    fs::write(
        temp.path().join("firebase.json"),
        r#"{"functions": [{"source": "api", "codebase": "api"}]}"#,
    )
    .unwrap();

    let adapter = detect_dev_adapter(temp.path()).expect("adapter detection should succeed");
    assert_eq!(
        adapter,
        Some(DevAdapter::CloudFunctions {
            source_roots: vec![temp.path().join("api").canonicalize().unwrap()],
        })
    );
}

#[test]
fn detect_cloud_functions_firebase_json_multi_codebase_preserves_all_roots() {
    let temp = tempdir().expect("tempdir should build");
    fs::create_dir_all(temp.path().join("packages/app-functions")).unwrap();
    fs::create_dir_all(temp.path().join("packages/admin-functions")).unwrap();
    fs::write(
        temp.path().join("firebase.json"),
        r#"{"functions": [{"source": "packages/app-functions", "codebase": "app"}, {"source": "packages/admin-functions", "codebase": "admin"}]}"#,
    )
    .unwrap();

    let adapter = detect_dev_adapter(temp.path()).expect("adapter detection should succeed");
    assert_eq!(
        adapter,
        Some(DevAdapter::CloudFunctions {
            source_roots: vec![
                temp.path()
                    .join("packages/app-functions")
                    .canonicalize()
                    .unwrap(),
                temp.path()
                    .join("packages/admin-functions")
                    .canonicalize()
                    .unwrap(),
            ],
        })
    );
}

#[test]
fn detect_cloud_functions_reports_missing_source_dir() {
    let temp = tempdir().expect("tempdir should build");
    fs::write(
        temp.path().join("firebase.json"),
        r#"{"functions": {"source": "functions"}}"#,
    )
    .unwrap();

    let error = detect_dev_adapter(temp.path()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("does not exist or is not readable"),
        "unexpected missing-source error: {error}"
    );
}

#[test]
fn detect_cloud_functions_framework_package() {
    let temp = tempdir().expect("tempdir should build");
    fs::write(
        temp.path().join("package.json"),
        r#"{"dependencies": {"@google-cloud/functions-framework": "^3.0.0"}}"#,
    )
    .unwrap();

    let adapter = detect_dev_adapter(temp.path()).expect("adapter detection should succeed");
    assert_eq!(
        adapter,
        Some(DevAdapter::CloudFunctions {
            source_roots: vec![temp.path().to_path_buf()],
        })
    );
}

#[test]
fn convex_adapter_takes_priority_over_cloud_functions() {
    let temp = tempdir().expect("tempdir should build");
    fs::create_dir_all(temp.path().join("convex")).unwrap();
    fs::write(temp.path().join("firebase.json"), "{}").unwrap();

    let adapter = detect_dev_adapter(temp.path()).expect("adapter detection should succeed");
    assert!(
        matches!(adapter, Some(DevAdapter::Convex { .. })),
        "convex should take priority over cloud-functions"
    );
}

#[test]
fn cloud_functions_adapter_npm_install_dirs() {
    let adapter = DevAdapter::CloudFunctions {
        source_roots: vec![
            PathBuf::from("/project/functions"),
            PathBuf::from("/project/admin-functions"),
        ],
    };
    assert_eq!(
        adapter.npm_install_dirs(Path::new("/project")),
        vec![
            PathBuf::from("/project/functions"),
            PathBuf::from("/project/admin-functions"),
        ]
    );
}

#[test]
fn convex_adapter_npm_install_dirs() {
    let adapter = DevAdapter::Convex {
        source_root: PathBuf::from("/project/convex"),
        package_target: crate::authoring_root::CONVEX_TARGET,
    };
    assert_eq!(
        adapter.npm_install_dirs(Path::new("/project")),
        vec![PathBuf::from("/project")]
    );
}

#[test]
fn dev_plan_detects_cloud_functions_adapter() {
    let temp = tempdir().expect("tempdir should build");
    fs::create_dir_all(temp.path().join("functions")).unwrap();
    fs::write(
        temp.path().join("firebase.json"),
        r#"{"functions": {"source": "functions"}}"#,
    )
    .unwrap();
    let app_dir_str = temp.path().to_str().unwrap();

    let plan = resolve_dev_plan(
        parse_dev(["nimbus", "dev", "--app-dir", app_dir_str]),
        temp.path(),
    )
    .expect("dev plan should resolve");

    let canonical = temp.path().canonicalize().unwrap();
    assert_eq!(
        plan.adapter,
        Some(DevAdapter::CloudFunctions {
            source_roots: vec![canonical.join("functions")],
        })
    );
}

#[test]
fn convex_json_functions_override_relocates_source_root() {
    let temp = tempdir().expect("tempdir should build");
    fs::create_dir_all(temp.path().join("src/backend")).unwrap();
    fs::write(
        temp.path().join("convex.json"),
        r#"{"functions": "src/backend"}"#,
    )
    .unwrap();

    let adapter = detect_dev_adapter(temp.path()).expect("adapter detection should succeed");
    assert_eq!(
        adapter,
        Some(DevAdapter::Convex {
            source_root: temp.path().join("src/backend"),
            package_target: crate::authoring_root::CONVEX_TARGET,
        })
    );
}

#[test]
fn convex_json_functions_override_takes_priority_over_default_convex_dir() {
    let temp = tempdir().expect("tempdir should build");
    create_source_root(temp.path(), "convex");
    fs::create_dir_all(temp.path().join("src/backend")).unwrap();
    fs::write(
        temp.path().join("convex.json"),
        r#"{"functions": "src/backend"}"#,
    )
    .unwrap();

    let adapter = detect_dev_adapter(temp.path()).expect("adapter detection should succeed");
    assert_eq!(
        adapter,
        Some(DevAdapter::Convex {
            source_root: temp.path().join("src/backend"),
            package_target: crate::authoring_root::CONVEX_TARGET,
        }),
        "an explicit functions override must win over the default convex/ directory"
    );
}

#[test]
fn convex_json_functions_override_reports_missing_directory() {
    let temp = tempdir().expect("tempdir should build");
    fs::write(
        temp.path().join("convex.json"),
        r#"{"functions": "src/backend"}"#,
    )
    .unwrap();

    let error = detect_dev_adapter(temp.path()).unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains("\"functions\": \"src/backend\"") && message.contains("not a directory"),
        "missing functions-override directory must name the setting and path: {message}"
    );
}

#[test]
fn convex_json_without_functions_field_falls_back_to_default_heuristic() {
    let temp = tempdir().expect("tempdir should build");
    create_source_root(temp.path(), "convex");
    fs::write(temp.path().join("convex.json"), r#"{"node": {}}"#).unwrap();

    let adapter = detect_dev_adapter(temp.path()).expect("adapter detection should succeed");
    assert_eq!(
        adapter,
        Some(DevAdapter::Convex {
            source_root: temp.path().join("convex"),
            package_target: crate::authoring_root::CONVEX_TARGET,
        })
    );
}

#[test]
fn malformed_convex_json_falls_back_to_default_heuristic_instead_of_erroring() {
    let temp = tempdir().expect("tempdir should build");
    create_source_root(temp.path(), "convex");
    fs::write(temp.path().join("convex.json"), "{not json").unwrap();

    let adapter = detect_dev_adapter(temp.path()).expect(
        "detection-time convex.json read is best-effort; strict validation happens in codegen",
    );
    assert_eq!(
        adapter,
        Some(DevAdapter::Convex {
            source_root: temp.path().join("convex"),
            package_target: crate::authoring_root::CONVEX_TARGET,
        })
    );
}
