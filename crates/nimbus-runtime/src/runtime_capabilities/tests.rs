use super::paths::canonicalize_preserving_missing_suffix;
use super::permissions::RuntimePermissionDescriptorParser;
use super::*;
use crate::RuntimeGrants;
use crate::RuntimeLimits;
use crate::runtime::RuntimeBundle;
use deno_permissions::{OpenAccessKind, PermissionDescriptorParser};
use std::borrow::Cow;
use std::path::Path;
use std::path::PathBuf;

fn privileged_permission_profile_fixture() -> (
    tempfile::TempDir,
    RuntimePathPolicy,
    RuntimeEnvPolicy,
    RuntimeLimits,
    PathBuf,
) {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let bundle_root = tempdir.path().join("app/.nimbus/convex");
    std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
    let bundle_path = bundle_root.join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
    let bundle = RuntimeBundle::new(&bundle_path);

    let mut limits = RuntimeLimits::privileged_operator();
    limits.grants.read = vec!["$generated_root".to_string()];
    limits.grants.write = vec!["$generated_root".to_string()];
    limits.grants.net_connect = vec!["127.0.0.1".to_string()];
    limits.grants.run = vec!["$runtime_host_exec".to_string()];
    limits.grants.ffi = vec![
        bundle_path
            .canonicalize()
            .expect("bundle path should canonicalize")
            .display()
            .to_string(),
    ];

    let policy = RuntimePathPolicy::for_bundle(&bundle, &limits).expect("policy should build");
    let env = RuntimeEnvPolicy::for_grants(&limits.grants);
    (tempdir, policy, env, limits, bundle_path)
}

#[test]
fn ambient_denied_container_denies_net_fs_run_ffi() {
    let (_tempdir, policy, env, limits, bundle_path) = privileged_permission_profile_fixture();
    let mut permissions = build_ambient_denied_permissions_container(&policy, &env, &limits)
        .expect("permissions should build");

    let net = permissions
        .check_net(&("127.0.0.1", Some(8080)), "test")
        .expect_err("ambient-denied container should deny net access");
    assert!(
        net.to_string().contains("Requires net access"),
        "unexpected net denial: {net}"
    );

    let read = permissions
        .check_open(
            Cow::Borrowed(Path::new("./bundle.mjs")),
            OpenAccessKind::Read,
            Some("test"),
        )
        .expect_err("ambient-denied container should deny fs read access");
    assert!(
        read.to_string().contains("Requires read access"),
        "unexpected read denial: {read}"
    );

    let write = permissions
        .check_open(
            Cow::Borrowed(Path::new("./created.txt")),
            OpenAccessKind::Write,
            Some("test"),
        )
        .expect_err("ambient-denied container should deny fs write access");
    assert!(
        write.to_string().contains("Requires write access"),
        "unexpected write denial: {write}"
    );

    let parser = RuntimePermissionDescriptorParser::new(policy.cwd().to_path_buf());
    let run_path = policy.run_targets()[0].to_string_lossy().into_owned();
    let run_query = parser
        .parse_run_query(run_path.as_str())
        .expect("runtime host exec query should parse");
    let run = permissions
        .check_run(&run_query, "test")
        .expect_err("ambient-denied container should deny run access");
    assert!(
        run.to_string().contains("Requires run access"),
        "unexpected run denial: {run}"
    );

    let ffi_path = bundle_path
        .canonicalize()
        .expect("bundle path should canonicalize");
    let ffi = permissions
        .check_ffi(Cow::Borrowed(ffi_path.as_path()))
        .expect_err("ambient-denied container should deny ffi access");
    assert!(
        ffi.to_string().contains("Requires ffi access"),
        "unexpected ffi denial: {ffi}"
    );
}

#[test]
fn worker_container_preserves_configured_authority_for_all_kinds() {
    let (_tempdir, policy, env, limits, bundle_path) = privileged_permission_profile_fixture();
    let mut permissions =
        build_permissions_container(&policy, &env, &limits).expect("permissions should build");

    permissions
        .check_net(&("127.0.0.1", Some(8080)), "test")
        .expect("worker container should preserve configured net authority");
    permissions
        .check_open(
            Cow::Borrowed(Path::new("./bundle.mjs")),
            OpenAccessKind::Read,
            Some("test"),
        )
        .expect("worker container should preserve configured fs read authority");
    permissions
        .check_open(
            Cow::Borrowed(Path::new("./created.txt")),
            OpenAccessKind::Write,
            Some("test"),
        )
        .expect("worker container should preserve configured fs write authority");

    let parser = RuntimePermissionDescriptorParser::new(policy.cwd().to_path_buf());
    let run_path = policy.run_targets()[0].to_string_lossy().into_owned();
    let run_query = parser
        .parse_run_query(run_path.as_str())
        .expect("runtime host exec query should parse");
    permissions
        .check_run(&run_query, "test")
        .expect("worker container should preserve configured run authority");

    let ffi_path = bundle_path
        .canonicalize()
        .expect("bundle path should canonicalize");
    permissions
        .check_ffi(Cow::Borrowed(ffi_path.as_path()))
        .expect("worker container should preserve configured ffi authority");
}

#[test]
fn application_preset_roots_stay_within_generated_bundle_root() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let bundle_root = tempdir.path().join("app/.nimbus/convex");
    std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
    let bundle_path = bundle_root.join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
    let bundle = RuntimeBundle::new(&bundle_path);

    let limits = RuntimeLimits::application_node22();
    let policy = RuntimePathPolicy::for_bundle(&bundle, &limits).expect("policy should build");
    let env = RuntimeEnvPolicy::for_grants(&limits.grants);
    let permissions =
        build_permissions_container(&policy, &env, &limits).expect("permissions should build");

    let expected_cwd = bundle_root
        .canonicalize()
        .expect("bundle root should canonicalize");
    assert_eq!(policy.cwd(), expected_cwd.as_path());
    let expected_package_json = expected_cwd.join("package.json");
    let checked = permissions
        .check_open(
            Cow::Borrowed(Path::new("./package.json")),
            OpenAccessKind::Read,
            Some("test"),
        )
        .expect("read path should resolve");
    assert_eq!(checked.into_owned_path(), expected_package_json);
    let denied = permissions
        .check_open(
            Cow::Borrowed(Path::new("../package.json")),
            OpenAccessKind::Read,
            Some("test"),
        )
        .expect_err("parent traversal should be denied");
    assert!(
        denied.to_string().contains("Requires read access"),
        "unexpected error: {denied}"
    );
}

#[test]
fn path_roots_are_driven_by_grants_not_preset_name() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let app_root = tempdir.path().join("app");
    let bundle_root = app_root.join(".nimbus/convex");
    std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
    let bundle_path = bundle_root.join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
    let bundle = RuntimeBundle::new(&bundle_path);

    let mut limits = RuntimeLimits::tooling_node22();
    limits.grants = RuntimeGrants::application_node();
    let policy = RuntimePathPolicy::for_bundle(&bundle, &limits).expect("policy should build");

    let expected_cwd = bundle_root
        .canonicalize()
        .expect("bundle root should canonicalize");
    assert_eq!(
        policy.cwd(),
        expected_cwd.as_path(),
        "a tooling preset must not widen cwd without matching read grants"
    );
    let denied = policy
        .ensure_read_path_lexical(&app_root.join("package.json"))
        .expect_err("app-root read should require an app-root read grant");
    assert!(
        denied
            .to_string()
            .contains("runtime read capability denied"),
        "unexpected denial: {denied}"
    );
}

#[test]
fn tooling_preset_uses_app_root_as_cwd_and_allows_tmp_writes() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let app_root = tempdir.path().join("app");
    let bundle_root = app_root.join(".nimbus/convex");
    std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
    let bundle_path = bundle_root.join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
    let bundle = RuntimeBundle::new(&bundle_path);

    let limits = RuntimeLimits::tooling_node22();
    let policy = RuntimePathPolicy::for_bundle(&bundle, &limits).expect("policy should build");
    let env = RuntimeEnvPolicy::for_grants(&limits.grants);
    let permissions =
        build_permissions_container(&policy, &env, &limits).expect("permissions should build");

    let expected_cwd = app_root
        .canonicalize()
        .expect("app root should canonicalize");
    assert_eq!(policy.cwd(), expected_cwd.as_path());
    let expected_tmp_write = expected_cwd.join(".nimbus/tmp/cache.txt");
    let checked = permissions
        .check_open(
            Cow::Borrowed(Path::new(".nimbus/tmp/cache.txt")),
            OpenAccessKind::Write,
            Some("test"),
        )
        .expect("tmp write should resolve");
    assert_eq!(checked.into_owned_path(), expected_tmp_write);
    let denied = permissions
        .check_open(
            Cow::Borrowed(Path::new("../outside.txt")),
            OpenAccessKind::Write,
            Some("test"),
        )
        .expect_err("escape write should be denied");
    assert!(
        denied.to_string().contains("Requires write access"),
        "unexpected error: {denied}"
    );
}

#[test]
fn permissions_container_resolves_paths_from_runtime_scoped_cwd() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let app_root = tempdir.path().join("app");
    let bundle_root = app_root.join(".nimbus/convex");
    std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
    let bundle_path = bundle_root.join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
    let bundle = RuntimeBundle::new(&bundle_path);

    let limits = RuntimeLimits::tooling_node22();
    let paths = RuntimePathPolicy::for_bundle(&bundle, &limits).expect("path policy should build");
    let env = RuntimeEnvPolicy::for_grants(&limits.grants);
    let permissions =
        build_permissions_container(&paths, &env, &limits).expect("permissions should build");

    let checked = permissions
        .check_open(
            Cow::Borrowed(Path::new(".nimbus/tmp/cache.txt")),
            OpenAccessKind::Write,
            Some("test"),
        )
        .expect("tmp path should be allowed");
    let expected = app_root
        .join(".nimbus/tmp/cache.txt")
        .canonicalize()
        .unwrap_or_else(|_| {
            canonicalize_preserving_missing_suffix(&app_root.join(".nimbus/tmp/cache.txt"))
                .expect("expected path should canonicalize")
        });
    assert_eq!(checked.into_owned_path(), expected);

    let denied = permissions
        .check_open(
            Cow::Borrowed(Path::new("../outside.txt")),
            OpenAccessKind::Write,
            Some("test"),
        )
        .expect_err("parent traversal should be denied");
    assert!(
        denied.to_string().contains("Requires write access"),
        "unexpected error: {denied}"
    );
}

#[test]
fn ensure_write_path_allows_in_root_parent_traversal_after_missing_segment() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let app_root = tempdir.path().join("app");
    let bundle_root = app_root.join(".nimbus/convex");
    std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
    let bundle_path = bundle_root.join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
    let bundle = RuntimeBundle::new(&bundle_path);

    let paths = RuntimePathPolicy::for_bundle(&bundle, &RuntimeLimits::application_node22())
        .expect("path policy should build");

    let checked = paths
        .ensure_write_path(Path::new("test10/../test11/test12"))
        .expect("in-root mkdir path should normalize");
    let expected = bundle_root
        .canonicalize()
        .expect("bundle root should canonicalize")
        .join("test11/test12");
    assert_eq!(checked, expected);
}

#[cfg(unix)]
#[test]
fn ensure_write_link_path_authorizes_entry_without_following_symlink_target() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let app_root = tempdir.path().join("app");
    let bundle_root = app_root.join(".nimbus/convex");
    let outside_root = tempdir.path().join("outside");
    std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
    std::fs::create_dir_all(&outside_root).expect("outside root should build");
    let bundle_path = bundle_root.join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
    std::os::unix::fs::symlink(&outside_root, bundle_root.join("outside-link"))
        .expect("symlink should write");
    let bundle = RuntimeBundle::new(&bundle_path);

    let paths = RuntimePathPolicy::for_bundle(&bundle, &RuntimeLimits::application_node22())
        .expect("path policy should build");

    let checked = paths
        .ensure_write_link_path(Path::new("outside-link"))
        .expect("link entry should be removable inside the writable root");
    let expected = bundle_root
        .canonicalize()
        .expect("bundle root should canonicalize")
        .join("outside-link");
    assert_eq!(checked, expected);
}

#[test]
fn ensure_symlink_target_path_allows_in_root_relative_parent_traversal() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let app_root = tempdir.path().join("app");
    let bundle_root = app_root.join(".nimbus/convex");
    std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
    let bundle_path = bundle_root.join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
    let bundle = RuntimeBundle::new(&bundle_path);

    let paths = RuntimePathPolicy::for_bundle(&bundle, &RuntimeLimits::application_node22())
        .expect("path policy should build");

    let link_path = paths
        .ensure_write_path(Path::new("fixtures/a/symlink/a/b/c"))
        .expect("symlink destination should be allowed");
    let checked = paths
        .ensure_symlink_target_path(Path::new("../.."), &link_path)
        .expect("relative symlink target should normalize against the link parent");
    assert_eq!(checked, PathBuf::from("../.."));
}

#[test]
fn ensure_read_metadata_path_denies_ancestor_of_approved_root() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let app_root = tempdir.path().join("app");
    let bundle_root = app_root.join(".nimbus/convex");
    std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
    let bundle_path = bundle_root.join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
    let bundle = RuntimeBundle::new(&bundle_path);

    let paths = RuntimePathPolicy::for_bundle(&bundle, &RuntimeLimits::application_node22())
        .expect("path policy should build");

    let error = paths
        .ensure_read_metadata_path(Path::new("/"))
        .expect_err("ancestor metadata should be denied outside approved roots");
    assert!(
        error
            .to_string()
            .contains("runtime read capability denied for /"),
        "unexpected metadata denial: {error}"
    );
}

#[test]
fn application_preset_has_no_run_targets_and_denies_subprocess_queries() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let app_root = tempdir.path().join("app");
    let bundle_root = app_root.join(".nimbus/convex");
    std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
    let bundle_path = bundle_root.join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
    let bundle = RuntimeBundle::new(&bundle_path);

    let limits = RuntimeLimits::application_node22();
    let policy = RuntimePathPolicy::for_bundle(&bundle, &limits).expect("policy should build");
    assert!(
        policy.run_targets().is_empty(),
        "application preset should not expose runnable targets"
    );

    let env = RuntimeEnvPolicy::for_grants(&limits.grants);
    let permissions =
        build_permissions_container(&policy, &env, &limits).expect("permissions should build");
    let parser = RuntimePermissionDescriptorParser::new(policy.cwd().to_path_buf());
    let current_exec = std::env::current_exe().expect("current exec should resolve");
    let current_exec_query = current_exec.to_string_lossy().into_owned();
    let run_query = parser
        .parse_run_query(current_exec_query.as_str())
        .expect("current exec query should parse");
    let error = permissions
        .check_run(&run_query, "test")
        .expect_err("application preset should deny subprocess execution");
    assert!(
        error.to_string().contains("Requires run access"),
        "unexpected run denial: {error}"
    );
}

#[test]
fn application_self_exec_run_grant_only_allows_compat_exec_target() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let app_root = tempdir.path().join("app");
    let bundle_root = app_root.join(".nimbus/convex");
    std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
    let bundle_path = bundle_root.join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
    let bundle = RuntimeBundle::new(&bundle_path);

    let mut limits = RuntimeLimits::application_node22();
    limits.grants.run = vec!["$runtime_self_exec".to_string()];
    let policy = RuntimePathPolicy::for_bundle(&bundle, &limits).expect("policy should build");
    assert_eq!(
        policy.run_targets().len(),
        1,
        "self-exec grant should expose exactly one compat exec target"
    );

    let env = RuntimeEnvPolicy::for_grants(&limits.grants);
    let permissions =
        build_permissions_container(&policy, &env, &limits).expect("permissions should build");
    let parser = RuntimePermissionDescriptorParser::new(policy.cwd().to_path_buf());

    let allowed_path = policy.run_targets()[0].to_string_lossy().into_owned();
    let allowed = parser
        .parse_run_query(allowed_path.as_str())
        .expect("self exec query should parse");
    permissions
        .check_run(&allowed, "test")
        .expect("self-exec target should be runnable");

    let current_exec = std::env::current_exe().expect("current exec should resolve");
    let current_exec_query = current_exec.to_string_lossy().into_owned();
    let denied = parser
        .parse_run_query(current_exec_query.as_str())
        .expect("host exec query should parse");
    let error = permissions
        .check_run(&denied, "test")
        .expect_err("self-exec grant should still deny host exec");
    assert!(
        error.to_string().contains("Requires run access"),
        "unexpected run denial: {error}"
    );
}

#[test]
fn tooling_preset_discovers_staged_run_targets_and_denies_escape_runs() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let app_root = tempdir.path().join("app");
    let bundle_root = app_root.join(".nimbus/convex");
    let binary_root = app_root.join("node_modules/esbuild/bin");
    std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
    std::fs::create_dir_all(&binary_root).expect("binary root should build");
    let binary_path = binary_root.join(binary_name());
    write_test_executable(&binary_path);

    let bundle_path = bundle_root.join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
    let bundle = RuntimeBundle::new(&bundle_path);

    let limits = RuntimeLimits::tooling_node22();
    let policy = RuntimePathPolicy::for_bundle(&bundle, &limits).expect("policy should build");
    assert!(
        policy.run_targets().contains(
            &binary_path
                .canonicalize()
                .expect("binary path should canonicalize")
        ),
        "tooling run targets should include staged package binaries"
    );

    let env = RuntimeEnvPolicy::for_grants(&limits.grants);
    let permissions =
        build_permissions_container(&policy, &env, &limits).expect("permissions should build");
    let parser = RuntimePermissionDescriptorParser::new(policy.cwd().to_path_buf());

    let allowed_path = binary_path.to_string_lossy().into_owned();
    let allowed = parser
        .parse_run_query(allowed_path.as_str())
        .expect("binary query should parse");
    permissions
        .check_run(&allowed, "test")
        .expect("staged package binary should be runnable");

    let outside_binary = tempdir.path().join("outside").join(binary_name());
    std::fs::create_dir_all(
        outside_binary
            .parent()
            .expect("outside parent should exist"),
    )
    .expect("outside parent should build");
    write_test_executable(&outside_binary);
    let denied_path = outside_binary.to_string_lossy().into_owned();
    let denied = parser
        .parse_run_query(denied_path.as_str())
        .expect("outside query should parse");
    let error = permissions
        .check_run(&denied, "test")
        .expect_err("outside binary should be denied");
    assert!(
        error.to_string().contains("Requires run access"),
        "unexpected run denial: {error}"
    );
}

#[test]
fn run_targets_are_driven_by_grants_not_preset_name() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let app_root = tempdir.path().join("app");
    let bundle_root = app_root.join(".nimbus/convex");
    let binary_root = app_root.join("node_modules/.bin");
    std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
    std::fs::create_dir_all(&binary_root).expect("binary root should build");
    let binary_path = binary_root.join(binary_name());
    write_test_executable(&binary_path);

    let bundle_path = bundle_root.join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
    let bundle = RuntimeBundle::new(&bundle_path);

    let mut invalid_application_limits = RuntimeLimits::application_node22();
    invalid_application_limits.grants.run = vec!["$discovered_tooling".to_string()];
    assert!(
        std::panic::catch_unwind(|| invalid_application_limits.normalized()).is_err(),
        "$discovered_tooling should still require the Tooling preset guardrail"
    );

    let mut limits = RuntimeLimits::tooling_node22();
    limits.grants.run = vec![binary_path.to_string_lossy().into_owned()];
    let policy = RuntimePathPolicy::for_bundle(&bundle, &limits).expect("policy should build");
    assert_eq!(
        policy.run_targets(),
        &[binary_path
            .canonicalize()
            .expect("binary path should canonicalize")]
    );
}

#[test]
fn application_node22_local_development_permissions_allow_local_network_hosts() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let bundle_root = tempdir.path().join("app/.nimbus/convex");
    std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
    let bundle_path = bundle_root.join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
    let bundle = RuntimeBundle::new(&bundle_path);

    let limits = RuntimeLimits::application_node22_local_development();
    let policy = RuntimePathPolicy::for_bundle(&bundle, &limits).expect("policy should build");
    let env = RuntimeEnvPolicy::for_grants(&limits.grants);
    let mut permissions =
        build_permissions_container(&policy, &env, &limits).expect("permissions should build");

    permissions
        .check_net(&("localhost", Some(8080)), "test")
        .expect("loopback hostname should be allowed");
    permissions
        .check_net(&("127.0.0.1", Some(8080)), "test")
        .expect("loopback ipv4 should be allowed");
    permissions
        .check_net(&("127.0.0.1", Some(0)), "test")
        .expect("loopback ipv4 ephemeral listen port should be allowed");
    permissions
        .check_net(&("0.0.0.0", Some(0)), "test")
        .expect("wildcard listen host should be allowed");
    permissions
        .check_sys("hostname", "test")
        .expect("hostname sys capability should be allowed");
}

#[test]
fn node_network_permissions_are_driven_by_grants() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let bundle_root = tempdir.path().join("app/.nimbus/convex");
    std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
    let bundle_path = bundle_root.join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
    let bundle = RuntimeBundle::new(&bundle_path);

    let limits = RuntimeLimits::application_node22();
    let policy = RuntimePathPolicy::for_bundle(&bundle, &limits).expect("policy should build");
    let env = RuntimeEnvPolicy::for_grants(&limits.grants);
    let mut permissions =
        build_permissions_container(&policy, &env, &limits).expect("permissions should build");

    let error = permissions
        .check_net(&("127.0.0.1", Some(8080)), "test")
        .expect_err("Node target should still require explicit net grants");
    assert!(
        error.to_string().contains("Requires net access"),
        "unexpected net denial: {error}"
    );
}

#[test]
fn web_standard_permissions_deny_local_network_hosts() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let bundle_root = tempdir.path().join("app/.nimbus/convex");
    std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
    let bundle_path = bundle_root.join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
    let bundle = RuntimeBundle::new(&bundle_path);

    let limits = RuntimeLimits::application_web_standard();
    let policy = RuntimePathPolicy::for_bundle(&bundle, &limits).expect("policy should build");
    let env = RuntimeEnvPolicy::for_grants(&limits.grants);
    let mut permissions =
        build_permissions_container(&policy, &env, &limits).expect("permissions should build");

    let error = permissions
        .check_net(&("127.0.0.1", Some(8080)), "test")
        .expect_err("web-standard runtime should still deny raw net access");
    assert!(
        error.to_string().contains("Requires net access"),
        "unexpected net denial: {error}"
    );
}

/// Outbound `fetch()` and `new WebSocket()` funnel through the same
/// `allow_net` gate (`check_net_url` -> the merged `net_connect`/`net_listen`
/// allowlist). No runtime profile grants a non-loopback host, so an isolate
/// can never reach a public `https://`/`wss://` endpoint, not even the
/// local-development profile, whose loopback grant must not be mistaken for
/// public egress. This pins that invariant across every profile that ships a
/// net grant.
#[test]
fn node_isolates_deny_public_host_egress_in_every_profile() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let bundle_root = tempdir.path().join("app/.nimbus/convex");
    std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
    let bundle_path = bundle_root.join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
    let bundle = RuntimeBundle::new(&bundle_path);

    // (profile label, limits): production denies all net; local-development
    // grants loopback only. A public host must be denied under both.
    let profiles = [
        ("production", RuntimeLimits::application_node22()),
        (
            "local-development",
            RuntimeLimits::application_node22_local_development(),
        ),
    ];

    for (label, limits) in profiles {
        let policy = RuntimePathPolicy::for_bundle(&bundle, &limits).expect("policy should build");
        let env = RuntimeEnvPolicy::for_grants(&limits.grants);
        let mut permissions =
            build_permissions_container(&policy, &env, &limits).expect("permissions should build");

        // `wss://echo.example.com:443` resolves to this check_net descriptor.
        // expect_err panics if a public host was (wrongly) permitted.
        let denial = permissions
            .check_net(&("echo.example.com", Some(443)), "new WebSocket()")
            .expect_err("public-host WebSocket/fetch egress must be denied in every profile");
        assert!(
            denial.to_string().contains("Requires net access"),
            "{label}: unexpected public-host net denial: {denial}"
        );
    }
}

#[cfg(unix)]
fn binary_name() -> &'static str {
    "esbuild"
}

#[cfg(windows)]
fn binary_name() -> &'static str {
    "esbuild.cmd"
}

fn write_test_executable(path: &PathBuf) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        std::fs::write(path, "#!/bin/sh\nexit 0\n").expect("test executable should write");
        let mut permissions = std::fs::metadata(path)
            .expect("test executable metadata should load")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions)
            .expect("test executable permissions should update");
    }
    #[cfg(windows)]
    {
        std::fs::write(path, "@echo off\r\nexit /b 0\r\n").expect("test executable should write");
    }
}
