use std::path::Path;

use super::adapter::has_package_dependency;

/// Wire-protocol surfaces detected from the app's declared dependencies.
///
/// App adapters ([`super::adapter::DevAdapter`]) stay singular — one
/// wiring/codegen/watch owner per app. Wire surfaces are a set and combine
/// freely with any app adapter: a Convex app that also installs `mongodb`
/// keeps the Convex dev loop AND gets the MongoDB listener surface.
///
/// Detection reads only declared dependencies in `package.json` — no
/// `node_modules` traversal, no network.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct WireSurfaces {
    /// `mongodb` or `mongoose` dependency declared.
    pub(super) mongodb: bool,
    /// `@aws-sdk/client-dynamodb` or `@aws-sdk/lib-dynamodb` declared.
    pub(super) dynamodb: bool,
    /// Bare `aws-sdk` (v2) declared without a v3 DynamoDB client. Too broad
    /// to auto-enable — using AWS does not imply using DynamoDB — so this
    /// signal only feeds a banner hint, never enablement (decision D3).
    pub(super) aws_sdk_v2_hint: bool,
}

pub(super) fn detect_wire_surfaces(app_dir: &Path) -> WireSurfaces {
    let mongodb =
        has_package_dependency(app_dir, "mongodb") || has_package_dependency(app_dir, "mongoose");
    let dynamodb = has_package_dependency(app_dir, "@aws-sdk/client-dynamodb")
        || has_package_dependency(app_dir, "@aws-sdk/lib-dynamodb");
    let aws_sdk_v2_hint = !dynamodb && has_package_dependency(app_dir, "aws-sdk");
    WireSurfaces {
        mongodb,
        dynamodb,
        aws_sdk_v2_hint,
    }
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
    fn wire_surfaces_resolve_without_app_adapter() {
        // Wire surfaces are independent of app-adapter detection: a plain
        // client app with only a driver dependency still resolves a surface.
        let dir = tempfile::tempdir().expect("tempdir");
        write_package_json(
            dir.path(),
            r#"{"dependencies": {"mongodb": "^6.0.0", "@aws-sdk/lib-dynamodb": "^3.0.0"}}"#,
        );
        let surfaces = detect_wire_surfaces(dir.path());
        assert!(surfaces.mongodb);
        assert!(surfaces.dynamodb);
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
        assert!(
            !surfaces.aws_sdk_v2_hint,
            "hint is redundant once the v3 client enables the surface"
        );
    }

    #[test]
    fn wire_surfaces_ignore_lookalike_packages() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_package_json(
            dir.path(),
            r#"{"dependencies": {"mongodb-memory-server": "^9.0.0", "@aws-sdk/client-s3": "^3.0.0"}}"#,
        );
        assert_eq!(detect_wire_surfaces(dir.path()), WireSurfaces::default());
    }
}
