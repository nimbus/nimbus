use super::*;

#[test]
fn start_missing_functions_manifest_reports_actionable_error() {
    let temp = tempdir_in_repo_target();
    let app_dir = temp.path().to_path_buf();
    let command = StartCommand {
        app_dir: Some(app_dir.clone()),
        skip_codegen: true,
        ..StartCommand::default()
    };

    let resolved_app_dir =
        super::boot::resolve_start_app_dir(&command).expect("app dir should resolve");
    let error = super::boot::load_convex_registry(
        &command,
        resolved_app_dir.as_ref(),
        &RuntimeLimits::default(),
    )
    .expect_err("missing functions manifest should fail registry loading");
    let rendered = error.to_string();
    let functions_path = app_dir
        .join(".nimbus")
        .join("convex")
        .join("functions.json");
    assert!(
        rendered.contains(&format!(
            "No generated function manifest found at {}.",
            functions_path.display()
        )),
        "error should point at the missing manifest: {rendered}"
    );
    assert!(
        rendered.contains(&format!("nimbus codegen --app {}", app_dir.display())),
        "error should include the exact codegen command: {rendered}"
    );
    assert!(
        rendered.contains("--skip-codegen"),
        "error should explain the skip-codegen escape hatch: {rendered}"
    );
}

#[test]
fn load_convex_registry_accepts_manifest_only_app_dir_without_bundle() {
    let temp = tempdir_in_repo_target();
    let convex_dir = temp.path().join(".nimbus").join("convex");
    fs::create_dir_all(&convex_dir).expect("convex manifest directory should build");
    fs::write(
        convex_dir.join("functions.json"),
        serde_json::to_vec_pretty(&json!({
            "functions": [{
                "name": "messages:list",
                "kind": "query",
                "plan": {
                    "type": "limit",
                    "source": { "type": "scan", "table": "messages" },
                    "limit": 20
                }
            }]
        }))
        .expect("manifest json should serialize"),
    )
    .expect("manifest should write");

    let command = StartCommand {
        app_dir: Some(temp.path().to_path_buf()),
        skip_codegen: true,
        ..StartCommand::default()
    };
    let resolved_app_dir =
        super::boot::resolve_start_app_dir(&command).expect("app dir should resolve");
    let registry = super::boot::load_convex_registry(
        &command,
        resolved_app_dir.as_ref(),
        &RuntimeLimits::default(),
    )
    .expect("manifest-only app dir should load");
    assert!(
        registry.is_some(),
        "manifest-only app dir should still load a registry without bundle.mjs"
    );
}

#[test]
fn load_cloud_functions_registry_accepts_generated_app_dir() {
    let temp = tempdir_in_repo_target();
    write_generated_cloud_functions_artifacts(temp.path());

    let command = StartCommand {
        app_dir: Some(temp.path().to_path_buf()),
        skip_codegen: true,
        ..StartCommand::default()
    };
    let resolved_app_dir =
        super::boot::resolve_start_app_dir(&command).expect("app dir should resolve");
    let registry = super::boot::load_cloud_functions_registry(
        &command,
        resolved_app_dir.as_ref(),
        &RuntimeLimits::default(),
    )
    .expect("generated cloud functions app dir should load");
    assert!(
        registry.is_some(),
        "generated cloud functions app dir should load a registry"
    );
}

#[test]
fn resolve_start_app_dir_auto_detects_firebase_project_root_from_nested_child() {
    let temp = tempdir_in_repo_target();
    write_firebase_cloud_functions_fixture(temp.path());
    let nested_child = temp.path().join("functions").join("src");

    let resolved = with_current_dir(&nested_child, || {
        super::boot::resolve_start_app_dir(&StartCommand::default())
    })
    .expect("start app dir should resolve")
    .expect("start app dir should auto-detect");

    assert_eq!(
        resolved,
        super::boot::ResolvedStartAppDir::AutoDetected(
            temp.path()
                .canonicalize()
                .expect("tempdir should canonicalize")
        )
    );
}

#[test]
fn load_cloud_functions_registry_auto_detects_generated_app_dir_from_nested_child() {
    let temp = tempdir_in_repo_target();
    write_firebase_cloud_functions_fixture(temp.path());
    write_generated_cloud_functions_artifacts(temp.path());
    let nested_child = temp.path().join("functions").join("src");

    let registry = with_current_dir(&nested_child, || {
        let command = StartCommand {
            skip_codegen: true,
            ..StartCommand::default()
        };
        let resolved = super::boot::resolve_start_app_dir(&command)
            .expect("start app dir should resolve")
            .expect("start app dir should auto-detect");
        super::boot::load_cloud_functions_registry(
            &command,
            Some(&resolved),
            &RuntimeLimits::default(),
        )
        .expect("generated cloud functions app dir should load")
    });

    assert!(registry.is_some(), "auto-detected Firebase app should load");
}

#[test]
fn load_cloud_functions_registry_honors_explicit_override_for_nested_framework_package() {
    let temp = tempdir_in_repo_target();
    write_firebase_cloud_functions_fixture(temp.path());
    write_generated_cloud_functions_artifacts(temp.path());

    let nested_framework = temp.path().join("packages").join("functions");
    fs::create_dir_all(&nested_framework).expect("nested framework dir should create");
    write_framework_cloud_functions_fixture(&nested_framework);
    write_generated_cloud_functions_artifacts(&nested_framework);

    let registry = with_current_dir(temp.path(), || {
        let command = StartCommand {
            app_dir: Some(nested_framework.clone()),
            skip_codegen: true,
            ..StartCommand::default()
        };
        let resolved = super::boot::resolve_start_app_dir(&command)
            .expect("explicit app dir should resolve")
            .expect("explicit app dir should persist");
        super::boot::load_cloud_functions_registry(
            &command,
            Some(&resolved),
            &RuntimeLimits::default(),
        )
        .expect("explicit framework app dir should load")
        .expect("explicit framework app dir should produce a registry")
    });

    assert_eq!(
        registry.artifact_dir(),
        nested_framework
            .join(".nimbus")
            .join("firebase")
            .canonicalize()
            .expect("framework artifact dir should canonicalize")
    );
}

#[tokio::test]
async fn start_codegen_preflight_generates_runtime_artifacts() {
    if !workspace_codegen_dependencies_available() {
        eprintln!(
            "skipping codegen preflight integration test; workspace JS dependencies are unavailable"
        );
        return;
    }

    let temp = tempdir_in_repo_target();
    write_codegen_source_fixture(temp.path());

    let command = StartCommand {
        app_dir: Some(temp.path().to_path_buf()),
        ..StartCommand::default()
    };

    let resolved_app_dir =
        super::boot::resolve_start_app_dir(&command).expect("app dir should resolve");
    super::boot::run_codegen_preflight(&command, resolved_app_dir.as_ref())
        .await
        .expect("codegen preflight should succeed");

    let convex_dir = temp.path().join(".nimbus").join("convex");
    assert!(
        convex_dir.join("functions.json").is_file(),
        "functions manifest should be generated"
    );
    assert!(
        convex_dir.join("bundle.mjs").is_file(),
        "runtime bundle should be generated"
    );
    assert!(
        temp.path()
            .join("convex")
            .join("_generated")
            .join("api.ts")
            .is_file(),
        "_generated api file should be generated"
    );
}

#[tokio::test]
async fn start_codegen_preflight_generates_cloud_functions_artifacts() {
    if !workspace_codegen_dependencies_available() {
        eprintln!(
            "skipping cloud functions codegen preflight integration test; workspace JS dependencies are unavailable"
        );
        return;
    }

    let temp = tempdir_in_repo_target();
    write_firebase_cloud_functions_fixture(temp.path());

    let command = StartCommand {
        app_dir: Some(temp.path().to_path_buf()),
        ..StartCommand::default()
    };

    let resolved_app_dir =
        super::boot::resolve_start_app_dir(&command).expect("app dir should resolve");
    super::boot::run_codegen_preflight(&command, resolved_app_dir.as_ref())
        .await
        .expect("cloud functions codegen preflight should succeed");

    let firebase_dir = temp.path().join(".nimbus").join("firebase");
    assert!(
        firebase_dir.join("artifact.json").is_file(),
        "cloud functions artifact manifest should be generated"
    );
    assert!(
        firebase_dir.join("targets.json").is_file(),
        "cloud functions targets manifest should be generated"
    );
    assert!(
        firebase_dir.join("bundle.mjs").is_file(),
        "cloud functions runtime bundle should be generated"
    );
}

#[tokio::test]
async fn start_codegen_preflight_generates_framework_cloud_functions_artifacts() {
    if !workspace_codegen_dependencies_available() {
        eprintln!(
            "skipping framework cloud functions codegen preflight integration test; workspace JS dependencies are unavailable"
        );
        return;
    }

    let temp = tempdir_in_repo_target();
    write_framework_cloud_functions_fixture(temp.path());

    let command = StartCommand {
        app_dir: Some(temp.path().to_path_buf()),
        ..StartCommand::default()
    };

    let resolved_app_dir =
        super::boot::resolve_start_app_dir(&command).expect("app dir should resolve");
    super::boot::run_codegen_preflight(&command, resolved_app_dir.as_ref())
        .await
        .expect("framework cloud functions codegen preflight should succeed");

    let firebase_dir = temp.path().join(".nimbus").join("firebase");
    assert!(
        firebase_dir.join("artifact.json").is_file(),
        "cloud functions artifact manifest should be generated"
    );
    assert!(
        firebase_dir.join("targets.json").is_file(),
        "framework targets manifest should be preserved and normalized"
    );
    assert!(
        firebase_dir.join("bundle.mjs").is_file(),
        "cloud functions runtime bundle should be generated"
    );
}

#[tokio::test]
async fn start_codegen_preflight_honors_skip_codegen() {
    let temp = tempdir_in_repo_target();
    write_codegen_source_fixture(temp.path());

    let command = StartCommand {
        app_dir: Some(temp.path().to_path_buf()),
        skip_codegen: true,
        ..StartCommand::default()
    };

    let resolved_app_dir =
        super::boot::resolve_start_app_dir(&command).expect("app dir should resolve");
    super::boot::run_codegen_preflight(&command, resolved_app_dir.as_ref())
        .await
        .expect("skip-codegen should bypass preflight");

    let convex_dir = temp.path().join(".nimbus").join("convex");
    assert!(
        !convex_dir.join("functions.json").exists(),
        "skip-codegen should leave manifests untouched"
    );
    assert!(
        !temp
            .path()
            .join("convex")
            .join("_generated")
            .join("api.ts")
            .exists(),
        "skip-codegen should leave generated source untouched"
    );
}
