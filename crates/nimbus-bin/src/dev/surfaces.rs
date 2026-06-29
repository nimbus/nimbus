use std::path::Path;

/// Wire-protocol surfaces detected from the app's declared dependencies.
///
/// App adapters ([`super::adapter::DevAdapter`]) stay singular — one
/// wiring/codegen/watch owner per app. Wire surfaces are a set and combine
/// freely with any app adapter: a Convex app that also installs `mongodb`
/// keeps the Convex dev loop AND gets the MongoDB listener surface.
///
/// Detection reads only runtime `dependencies` in `package.json` — no
/// `devDependencies`/`optionalDependencies`/`peerDependencies`, no
/// `node_modules` traversal, no network. Unlike app-adapter identity
/// detection (which may read every dependency section), enabling a wire
/// surface starts a listener, generates credentials, and writes
/// `.env.local`, so only a dependency the app ships against may trigger
/// that infrastructure.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct WireSurfaces {
    /// `mongodb` or `mongoose` dependency declared.
    pub(super) mongodb: bool,
    /// `@aws-sdk/client-dynamodb` or `@aws-sdk/lib-dynamodb` declared.
    pub(super) dynamodb: bool,
    /// `@aws-sdk/client-s3` or `@aws-sdk/lib-storage` declared.
    pub(super) s3: bool,
    /// Bare `aws-sdk` (v2) declared without a v3 DynamoDB client. Too broad
    /// to auto-enable — using AWS does not imply using DynamoDB or S3 — so this
    /// signal only feeds a banner hint, never enablement (decision D3).
    pub(super) aws_sdk_v2_hint: bool,
}

pub(super) fn detect_wire_surfaces(app_dir: &Path) -> WireSurfaces {
    let mongodb =
        has_runtime_dependency(app_dir, "mongodb") || has_runtime_dependency(app_dir, "mongoose");
    let dynamodb = has_runtime_dependency(app_dir, "@aws-sdk/client-dynamodb")
        || has_runtime_dependency(app_dir, "@aws-sdk/lib-dynamodb");
    let s3 = has_runtime_dependency(app_dir, "@aws-sdk/client-s3")
        || has_runtime_dependency(app_dir, "@aws-sdk/lib-storage");
    let aws_sdk_v2_hint = !dynamodb && !s3 && has_runtime_dependency(app_dir, "aws-sdk");
    WireSurfaces {
        mongodb,
        dynamodb,
        s3,
        aws_sdk_v2_hint,
    }
}

/// True when `package.json` declares `package_name` under runtime
/// `dependencies`. Missing or malformed `package.json` is `false`: wire
/// surfaces fail closed on unreadable manifests.
fn has_runtime_dependency(app_dir: &Path, package_name: &str) -> bool {
    let Ok(content) = std::fs::read_to_string(app_dir.join("package.json")) else {
        return false;
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) else {
        return false;
    };
    parsed["dependencies"].get(package_name).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_package_json(dir: &Path, contents: &str) {
        std::fs::write(dir.join("package.json"), contents).expect("write package.json");
    }

    #[test]
    fn wire_surfaces_default_empty_without_package_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(detect_wire_surfaces(dir.path()), WireSurfaces::default());
    }

    #[test]
    fn mongodb_dependency_enables_mongodb_surface() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_package_json(dir.path(), r#"{"dependencies": {"mongodb": "^6.0.0"}}"#);
        let surfaces = detect_wire_surfaces(dir.path());
        assert!(surfaces.mongodb);
        assert!(!surfaces.dynamodb);
        assert!(!surfaces.s3);
        assert!(!surfaces.aws_sdk_v2_hint);
    }

    #[test]
    fn mongoose_dependency_enables_mongodb_surface() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_package_json(dir.path(), r#"{"dependencies": {"mongoose": "^8.0.0"}}"#);
        let surfaces = detect_wire_surfaces(dir.path());
        assert!(surfaces.mongodb);
        assert!(!surfaces.dynamodb);
        assert!(!surfaces.s3);
        assert!(!surfaces.aws_sdk_v2_hint);
    }

    #[test]
    fn dynamodb_sdk_dependency_enables_dynamodb_surface() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_package_json(
            dir.path(),
            r#"{"dependencies": {"@aws-sdk/client-dynamodb": "^3.0.0"}}"#,
        );
        let surfaces = detect_wire_surfaces(dir.path());
        assert!(surfaces.dynamodb);
        assert!(!surfaces.s3);
        assert!(!surfaces.mongodb);
        assert!(!surfaces.aws_sdk_v2_hint);
    }

    #[test]
    fn s3_sdk_dependency_enables_s3_surface() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_package_json(
            dir.path(),
            r#"{"dependencies": {"@aws-sdk/client-s3": "^3.0.0"}}"#,
        );
        let surfaces = detect_wire_surfaces(dir.path());
        assert!(surfaces.s3);
        assert!(!surfaces.dynamodb);
        assert!(!surfaces.mongodb);
        assert!(!surfaces.aws_sdk_v2_hint);
    }

    #[test]
    fn bare_aws_sdk_v2_is_hint_only() {
        // Decision D3: the v2 monolith covers every AWS service, so its
        // presence does not imply DynamoDB usage. It raises the banner hint
        // and must never enable a listener surface.
        let dir = tempfile::tempdir().expect("tempdir");
        write_package_json(dir.path(), r#"{"dependencies": {"aws-sdk": "^2.1500.0"}}"#);
        let surfaces = detect_wire_surfaces(dir.path());
        assert!(surfaces.aws_sdk_v2_hint);
        assert!(
            !surfaces.dynamodb,
            "the v2 hint must never enable the DynamoDB surface"
        );
        assert!(!surfaces.s3, "the v2 hint must never enable S3");
        assert!(!surfaces.mongodb);
    }

    #[test]
    fn dev_dependencies_do_not_enable_wire_surfaces() {
        // Wire surfaces trigger infrastructure (listener + credentials +
        // .env.local), so only runtime `dependencies` count: a driver that
        // appears in any other section is a test/tooling concern.
        let dir = tempfile::tempdir().expect("tempdir");
        write_package_json(
            dir.path(),
            r#"{
                "devDependencies": {"mongodb": "^6.0.0", "@aws-sdk/client-dynamodb": "^3.0.0", "@aws-sdk/client-s3": "^3.0.0"},
                "optionalDependencies": {"mongoose": "^8.0.0"},
                "peerDependencies": {"@aws-sdk/lib-dynamodb": "^3.0.0", "@aws-sdk/lib-storage": "^3.0.0", "aws-sdk": "^2.1500.0"}
            }"#,
        );
        assert_eq!(detect_wire_surfaces(dir.path()), WireSurfaces::default());
    }

    #[test]
    fn malformed_package_json_yields_no_wire_surfaces() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_package_json(dir.path(), r#"{"dependencies": {"mongodb": "#);
        assert_eq!(detect_wire_surfaces(dir.path()), WireSurfaces::default());
    }

    #[test]
    fn wire_surfaces_resolve_without_app_adapter() {
        // Wire surfaces are independent of app-adapter detection: a plain
        // client app with only a driver dependency still resolves a surface.
        let dir = tempfile::tempdir().expect("tempdir");
        write_package_json(
            dir.path(),
            r#"{"dependencies": {"mongodb": "^6.0.0", "@aws-sdk/lib-dynamodb": "^3.0.0", "@aws-sdk/lib-storage": "^3.0.0"}}"#,
        );
        let surfaces = detect_wire_surfaces(dir.path());
        assert!(surfaces.mongodb);
        assert!(surfaces.dynamodb);
        assert!(surfaces.s3);
        assert!(!surfaces.aws_sdk_v2_hint);
    }

    #[test]
    fn wire_surfaces_v3_dynamodb_client_supersedes_bare_aws_sdk_hint() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_package_json(
            dir.path(),
            r#"{"dependencies": {"aws-sdk": "^2.1500.0", "@aws-sdk/client-dynamodb": "^3.0.0"}}"#,
        );
        let surfaces = detect_wire_surfaces(dir.path());
        assert!(surfaces.dynamodb);
        assert!(!surfaces.s3);
        assert!(
            !surfaces.aws_sdk_v2_hint,
            "hint is redundant once the v3 client enables the surface"
        );
    }

    #[test]
    fn wire_surfaces_v3_s3_client_supersedes_bare_aws_sdk_hint() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_package_json(
            dir.path(),
            r#"{"dependencies": {"aws-sdk": "^2.1500.0", "@aws-sdk/client-s3": "^3.0.0"}}"#,
        );
        let surfaces = detect_wire_surfaces(dir.path());
        assert!(surfaces.s3);
        assert!(
            !surfaces.aws_sdk_v2_hint,
            "hint is redundant once the v3 S3 client enables the surface"
        );
    }

    #[test]
    fn wire_surfaces_ignore_lookalike_packages() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_package_json(
            dir.path(),
            r#"{"dependencies": {"mongodb-memory-server": "^9.0.0", "s3": "^1.0.0"}}"#,
        );
        assert_eq!(detect_wire_surfaces(dir.path()), WireSurfaces::default());
    }
}
