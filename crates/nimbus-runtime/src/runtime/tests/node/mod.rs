use std::borrow::Cow;
use std::ffi::OsString;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::*;
use crate::test_support::acquire_runtime_suite_lock;
use crate::{RuntimeCompatibilityTarget, RuntimeLimits};

mod supplementary_batches;

include!("batches.rs");

include!("behavior.rs");

#[derive(Clone, Copy)]
struct NodeCompatExtraFixtureEntry {
    runtime_path: &'static str,
    fixture_source_path: &'static str,
}

#[derive(Clone, Copy, Debug)]
enum NodeCompatLane {
    Node20,
    Node22,
    Node24,
    Node26,
}

#[derive(Clone, Copy)]
enum NodeCompatBundleMode {
    Runtime,
    Oracle,
}

struct NodeCompatBundleWriteOptions<'a> {
    test_relative_path: &'a str,
    test_source: &'a str,
    extra_files: &'a [(&'a str, &'a [u8])],
    capture_top_level_skip: bool,
    lane: Option<NodeCompatLane>,
    prelude_script: Option<&'a str>,
    postlude_script: Option<&'a str>,
    node_options: &'a [String],
    mode: NodeCompatBundleMode,
}

fn node_compat_lane_name(lane: NodeCompatLane) -> &'static str {
    match lane {
        NodeCompatLane::Node20 => "node20",
        NodeCompatLane::Node22 => "node22",
        NodeCompatLane::Node24 => "node24",
        NodeCompatLane::Node26 => "node26",
    }
}

fn node_compat_lane_from_manifest_name(lane: &str) -> std::result::Result<NodeCompatLane, String> {
    match lane {
        "node20" => Ok(NodeCompatLane::Node20),
        "node22" => Ok(NodeCompatLane::Node22),
        "node24" => Ok(NodeCompatLane::Node24),
        "node26" => Ok(NodeCompatLane::Node26),
        other => Err(format!("unsupported manifest lane `{other}`")),
    }
}

fn inferred_node_compat_lane_from_fixture_source_path(
    fixture_source_path: &str,
) -> Option<NodeCompatLane> {
    if fixture_source_path.starts_with("node20/") {
        Some(NodeCompatLane::Node20)
    } else if fixture_source_path.starts_with("node22/") {
        Some(NodeCompatLane::Node22)
    } else if fixture_source_path.starts_with("node24/") {
        Some(NodeCompatLane::Node24)
    } else if fixture_source_path.starts_with("node26/") {
        Some(NodeCompatLane::Node26)
    } else {
        None
    }
}

#[derive(Clone, Copy)]
struct NodeCompatBatchEntry {
    test_relative_path: &'static str,
    node20_fixture_source_path: Option<&'static str>,
    node22_fixture_source_path: Option<&'static str>,
    node24_fixture_source_path: Option<&'static str>,
    shared_extra_files: &'static [NodeCompatExtraFixtureEntry],
    node20_extra_files: &'static [NodeCompatExtraFixtureEntry],
    node22_extra_files: &'static [NodeCompatExtraFixtureEntry],
    node24_extra_files: &'static [NodeCompatExtraFixtureEntry],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NodeCompatBatchEntrySnapshot {
    pub(super) test_relative_path: &'static str,
    pub(super) node20_fixture_source_path: Option<&'static str>,
    pub(super) node22_fixture_source_path: Option<&'static str>,
    pub(super) node24_fixture_source_path: Option<&'static str>,
}

struct NodeCompatFixtureOutcome {
    skipped: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NodeCompatFixtureDiagnosticFamily {
    EventLoop,
    Vm,
    Worker,
    MessagePort,
    Subprocess,
    General,
}

impl NodeCompatFixtureDiagnosticFamily {
    fn as_str(self) -> &'static str {
        match self {
            Self::EventLoop => "event_loop",
            Self::Vm => "vm",
            Self::Worker => "worker",
            Self::MessagePort => "message_port",
            Self::Subprocess => "subprocess",
            Self::General => "general",
        }
    }

    fn exit_criterion(self) -> &'static str {
        match self {
            Self::MessagePort => {
                "Worker MessagePort fixtures may be promoted only after NLRT8 proves production in-process profiles do not grant worker_threads authority, or after the fixture passes with bounded teardown diagnostics and the watchpoint catalog removes the corresponding ignore."
            }
            Self::Worker => {
                "Worker fixtures may be promoted only after worker lifecycle, cancellation, and policy inheritance are bounded by production profile tests."
            }
            Self::Vm => {
                "VM fixtures may be promoted only after dynamic-code execution remains bounded by runtime timeout diagnostics and production admission policy."
            }
            Self::Subprocess => {
                "Subprocess-shaped fixtures may be promoted only through the runtime self-exec seam or a service/microVM profile, never by granting ambient process execution."
            }
            Self::EventLoop => {
                "Event-loop fixtures may be promoted only when timer, microtask, and nextTick drains settle inside the per-fixture wall-clock budget."
            }
            Self::General => {
                "General fixture failures require a specific owner classification before they can become support claims."
            }
        }
    }
}

#[derive(serde::Serialize)]
struct NodeCompatFixtureExecutionDiagnostic<'a> {
    schema_version: u32,
    report_kind: &'static str,
    generated_at_unix_ms: u64,
    lane: &'a str,
    test_relative_path: &'a str,
    bundle_path: &'a str,
    diagnostic_family: &'static str,
    outcome: &'a str,
    timeout_ms: u64,
    elapsed_ms: u64,
    detail: &'a str,
    exit_criterion: &'static str,
}

#[derive(serde::Serialize)]
struct NodeCompatPathBatchExecutionSummary<'a> {
    schema_version: u32,
    report_kind: &'static str,
    generated_at_unix_ms: u64,
    batch_name: &'a str,
    lane: &'a str,
    selected: usize,
    passed: usize,
    skipped: usize,
    failed: usize,
    selected_paths: &'a [String],
    passed_paths: &'a [String],
    skipped_paths: &'a [String],
    failed_paths: &'a [String],
}

#[derive(Debug)]
pub(super) struct NodeCompatSeededFixtureObservedOutcome {
    pub(super) state: node_compat_manifest_report::NodeCompatObservedFixtureState,
    pub(super) detail: Option<String>,
}

#[derive(Debug)]
pub(super) struct NodeCompatMaterializedSeededFixtureBundle {
    pub(super) family: String,
    pub(super) slice: String,
    pub(super) lane: String,
    pub(super) test_relative_path: String,
    pub(super) fixture_source_path: String,
    pub(super) bundle_path: PathBuf,
    pub(super) tempdir: tempfile::TempDir,
    pub(super) startup_flags: Vec<String>,
}

struct ScopedProcessEnvVar {
    key: &'static str,
    previous_value: Option<String>,
}

impl ScopedProcessEnvVar {
    fn set(key: &'static str, value: &str) -> Self {
        let previous_value = std::env::var(key).ok();
        // SAFETY: node_compat fixture execution is serialized under
        // acquire_runtime_suite_lock() before this helper is used, so the test
        // harness can temporarily model a process-level TERM value for the
        // embedded runtime without concurrent mutation from sibling tests.
        unsafe {
            std::env::set_var(key, value);
        }
        Self {
            key,
            previous_value,
        }
    }
}

impl Drop for ScopedProcessEnvVar {
    fn drop(&mut self) {
        // SAFETY: see ScopedProcessEnvVar::set; restoration happens while the
        // same serialized node_compat execution scope is still active.
        unsafe {
            if let Some(previous_value) = &self.previous_value {
                std::env::set_var(self.key, previous_value);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }
}

fn node_compat_supported_node_options_flag(token: &str) -> bool {
    token == "--pending-deprecation"
        || token == "--no-warnings"
        || token == "--trace-warnings"
        || token == "--experimental-require-module"
        || token == "--require-module"
        || token == "--no-experimental-require-module"
        || token == "--no-require-module"
        || token == "--preserve-symlinks"
        || token == "--preserve-symlinks-main"
        || token == "--no-preserve-symlinks"
        || token == "--no-preserve-symlinks-main"
        || token.starts_with("--unhandled-rejections=")
}

fn push_node_options_flag(flags: &mut Vec<String>, flag: String) {
    if !flags.iter().any(|existing| existing == &flag) {
        flags.push(flag);
    }
}

fn push_node_conditions_flag(flags: &mut Vec<String>, condition: &str) {
    if !condition.is_empty() {
        push_node_options_flag(flags, format!("--conditions={condition}"));
    }
}

pub(super) fn fixture_requested_node_options(test_source: &str) -> Vec<String> {
    let mut flags = Vec::new();
    for line in test_source.lines().take(40) {
        let Some((_, raw_flags)) = line.split_once("Flags:") else {
            continue;
        };
        let mut tokens = raw_flags.split_whitespace();
        while let Some(token) = tokens.next() {
            if token == "--conditions" || token == "-C" {
                if let Some(condition) = tokens.next() {
                    push_node_conditions_flag(&mut flags, condition);
                }
            } else if let Some(condition) = token.strip_prefix("--conditions=") {
                push_node_conditions_flag(&mut flags, condition);
            } else if let Some(condition) = token.strip_prefix("-C") {
                push_node_conditions_flag(&mut flags, condition);
            } else if node_compat_supported_node_options_flag(token) {
                push_node_options_flag(&mut flags, token.to_string());
            }
        }
    }
    flags
}

fn fixture_requested_node_conditions(flags: &[String]) -> Vec<String> {
    let require_module = flags
        .iter()
        .any(|flag| flag == "--experimental-require-module" || flag == "--require-module")
        && !flags.iter().any(|flag| {
            flag == "--no-experimental-require-module" || flag == "--no-require-module"
        });
    let mut conditions = Vec::new();
    if require_module {
        push_node_condition(&mut conditions, "module-sync");
    }
    for condition in flags
        .iter()
        .filter_map(|flag| flag.strip_prefix("--conditions="))
    {
        push_node_condition(&mut conditions, condition);
    }
    conditions
}

fn push_node_condition(conditions: &mut Vec<String>, condition: &str) {
    if !condition.is_empty() && !conditions.iter().any(|existing| existing == condition) {
        conditions.push(condition.to_string());
    }
}

pub(super) fn fixture_requests_pending_deprecation(test_source: &str) -> bool {
    fixture_requested_node_options(test_source)
        .iter()
        .any(|flag| flag == "--pending-deprecation")
}

#[test]
fn fixture_requested_node_options_filters_and_preserves_order() {
    let source = r#"
// Flags: --trace-warnings --inspect --conditions=custom --unhandled-rejections=warn
// Flags: --no-warnings -C another --trace-warnings --pending-deprecation --preserve-symlinks
// Flags: --preserve-symlinks-main --no-preserve-symlinks-main
"#;

    let flags = fixture_requested_node_options(source);
    assert_eq!(
        flags,
        vec![
            "--trace-warnings".to_string(),
            "--conditions=custom".to_string(),
            "--unhandled-rejections=warn".to_string(),
            "--no-warnings".to_string(),
            "--conditions=another".to_string(),
            "--pending-deprecation".to_string(),
            "--preserve-symlinks".to_string(),
            "--preserve-symlinks-main".to_string(),
            "--no-preserve-symlinks-main".to_string(),
        ]
    );
    assert_eq!(
        fixture_requested_node_conditions(&flags),
        vec!["custom".to_string(), "another".to_string()]
    );
    assert!(fixture_requests_pending_deprecation(source));
}

#[test]
fn fixture_requested_node_conditions_include_module_sync_when_require_module_is_enabled() {
    let flags = fixture_requested_node_options(
        "// Flags: --conditions=custom --experimental-require-module -C another",
    );

    assert_eq!(
        fixture_requested_node_conditions(&flags),
        vec![
            "module-sync".to_string(),
            "custom".to_string(),
            "another".to_string(),
        ]
    );

    let disabled = fixture_requested_node_options(
        "// Flags: --experimental-require-module --no-experimental-require-module --conditions=custom",
    );
    assert_eq!(
        fixture_requested_node_conditions(&disabled),
        vec!["custom".to_string()]
    );
}

#[test]
fn node_compat_fixture_node_options_exposes_preserve_symlinks_flags() {
    let outcome = execute_upstream_node_compat_test_with_extra_files(
        "test/parallel/__nimbus-preserve-symlinks-options-probe.js",
        r#"
// Flags: --preserve-symlinks --preserve-symlinks-main
'use strict';
const assert = require('assert');
const { getOptionValue } = require('internal/options');

assert.match(process.env.NODE_OPTIONS, /--preserve-symlinks/);
assert.strictEqual(getOptionValue('--preserve-symlinks'), true);
assert.strictEqual(getOptionValue('--preserve-symlinks-main'), true);
"#,
        &[],
        false,
        Some(NodeCompatLane::Node22),
        None,
        None,
    )
    .expect("preserve-symlinks options probe should execute");

    assert!(!outcome.skipped);
}

fn scoped_node_options_flags(flags: &[String]) -> ScopedProcessEnvVar {
    let next_value = match std::env::var("NODE_OPTIONS").ok() {
        Some(existing) => {
            let mut tokens: Vec<String> = existing
                .split_whitespace()
                .map(|token| token.to_string())
                .collect();
            for flag in flags {
                if !tokens.iter().any(|token| token == flag) {
                    tokens.push(flag.clone());
                }
            }
            tokens.join(" ")
        }
        None => flags.join(" "),
    };
    ScopedProcessEnvVar::set("NODE_OPTIONS", &next_value)
}

fn grant_node_options_read_for_fixture_flags(limits: &mut RuntimeLimits) {
    const NODE_OPTIONS: &str = "NODE_OPTIONS";
    if !limits
        .grants
        .env_read
        .iter()
        .any(|name| name == NODE_OPTIONS)
    {
        limits.grants.env_read.push(NODE_OPTIONS.to_string());
    }
}

impl NodeCompatBatchEntry {
    fn fixture_source_path_for_lane(self, lane: NodeCompatLane) -> Option<Cow<'static, str>> {
        match lane {
            NodeCompatLane::Node20 => self.node20_fixture_source_path.map(Cow::Borrowed),
            NodeCompatLane::Node22 => self.node22_fixture_source_path.map(Cow::Borrowed),
            NodeCompatLane::Node24 => self.node24_fixture_source_path.map(Cow::Borrowed),
            NodeCompatLane::Node26 => {
                let fixture_source_path = format!("node26/{}", self.test_relative_path);
                if node_compat_fixture_root()
                    .join(&fixture_source_path)
                    .is_file()
                {
                    Some(Cow::Owned(fixture_source_path))
                } else {
                    None
                }
            }
        }
    }

    fn extra_files_for_lane(self, lane: NodeCompatLane) -> &'static [NodeCompatExtraFixtureEntry] {
        match lane {
            NodeCompatLane::Node20 if !self.node20_extra_files.is_empty() => {
                self.node20_extra_files
            }
            NodeCompatLane::Node22 if !self.node22_extra_files.is_empty() => {
                self.node22_extra_files
            }
            NodeCompatLane::Node24 if !self.node24_extra_files.is_empty() => {
                self.node24_extra_files
            }
            NodeCompatLane::Node26 => self.shared_extra_files,
            _ => self.shared_extra_files,
        }
    }
}

fn node_compat_fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/runtime/tests/node_compat_fixtures")
}

fn node_compat_repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("nimbus-runtime should live under crates/")
        .to_path_buf()
}

fn duration_millis_u64(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn node_compat_fixture_wall_clock_timeout(limits: &RuntimeLimits) -> Duration {
    limits
        .execution_timeout
        .checked_add(Duration::from_secs(5))
        .expect("node_compat wall-clock timeout slack should not overflow")
}

fn node_compat_fixture_diagnostic_family(
    test_relative_path: &str,
) -> NodeCompatFixtureDiagnosticFamily {
    if test_relative_path.contains("message-port") {
        NodeCompatFixtureDiagnosticFamily::MessagePort
    } else if test_relative_path.contains("worker") {
        NodeCompatFixtureDiagnosticFamily::Worker
    } else if test_relative_path.contains("vm") {
        NodeCompatFixtureDiagnosticFamily::Vm
    } else if node_compat_fixture_requires_runtime_self_exec(test_relative_path)
        || test_relative_path.contains("cluster")
        || test_relative_path.contains("child")
    {
        NodeCompatFixtureDiagnosticFamily::Subprocess
    } else if test_relative_path.contains("async")
        || test_relative_path.contains("next-tick")
        || test_relative_path.contains("timer")
        || test_relative_path.contains("timeout")
        || test_relative_path.contains("promise")
        || test_relative_path.contains("immediate")
    {
        NodeCompatFixtureDiagnosticFamily::EventLoop
    } else {
        NodeCompatFixtureDiagnosticFamily::General
    }
}

fn node_compat_diagnostic_root() -> PathBuf {
    std::env::var_os("NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| node_compat_repo_root().join("target/node-compat/diagnostics"))
}

fn sanitize_node_compat_artifact_stem(value: &str) -> String {
    let mut sanitized = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            sanitized.push(character);
        } else {
            sanitized.push('_');
        }
    }
    sanitized.trim_matches('_').to_string()
}

fn write_node_compat_fixture_diagnostic(
    lane_name: &str,
    test_relative_path: &str,
    bundle_path: &Path,
    timeout: Duration,
    elapsed: Duration,
    outcome: &str,
    detail: &str,
) -> Option<PathBuf> {
    let diagnostic_family = node_compat_fixture_diagnostic_family(test_relative_path);
    let bundle_path_string = bundle_path.to_string_lossy().to_string();
    let generated_at_unix_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(duration_millis_u64)
        .unwrap_or_default();
    let diagnostic = NodeCompatFixtureExecutionDiagnostic {
        schema_version: 1,
        report_kind: "node_compat_fixture_execution_diagnostic",
        generated_at_unix_ms,
        lane: lane_name,
        test_relative_path,
        bundle_path: bundle_path_string.as_str(),
        diagnostic_family: diagnostic_family.as_str(),
        outcome,
        timeout_ms: duration_millis_u64(timeout),
        elapsed_ms: duration_millis_u64(elapsed),
        detail,
        exit_criterion: diagnostic_family.exit_criterion(),
    };
    let path = node_compat_diagnostic_root()
        .join(diagnostic_family.as_str())
        .join(format!(
            "{}__{}.json",
            lane_name,
            sanitize_node_compat_artifact_stem(test_relative_path)
        ));
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return None;
    }
    let bytes = serde_json::to_vec_pretty(&diagnostic).ok()?;
    std::fs::write(&path, bytes).ok()?;
    Some(path)
}

fn write_node_compat_path_batch_summary(
    batch_name: &str,
    lane_name: &str,
    selected_paths: &[String],
    passed_paths: &[String],
    skipped_paths: &[String],
    failed_paths: &[String],
) -> Option<PathBuf> {
    let generated_at_unix_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(duration_millis_u64)
        .unwrap_or_default();
    let summary = NodeCompatPathBatchExecutionSummary {
        schema_version: 1,
        report_kind: "node_compat_path_batch_execution_summary",
        generated_at_unix_ms,
        batch_name,
        lane: lane_name,
        selected: selected_paths.len(),
        passed: passed_paths.len(),
        skipped: skipped_paths.len(),
        failed: failed_paths.len(),
        selected_paths,
        passed_paths,
        skipped_paths,
        failed_paths,
    };
    let path = node_compat_diagnostic_root().join("batch").join(format!(
        "{}__{}__summary.json",
        lane_name,
        sanitize_node_compat_artifact_stem(batch_name)
    ));
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return None;
    }
    let bytes = serde_json::to_vec_pretty(&summary).ok()?;
    std::fs::write(&path, bytes).ok()?;
    Some(path)
}

fn read_node_compat_fixture_bytes(fixture_source_path: &str) -> Vec<u8> {
    let path = node_compat_fixture_root().join(fixture_source_path);
    std::fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "node_compat fixture `{}` should read from `{}`: {error}",
            fixture_source_path,
            path.display()
        )
    })
}

fn read_node_compat_fixture_text(fixture_source_path: &str) -> String {
    let fixture_bytes = read_node_compat_fixture_bytes(fixture_source_path);
    String::from_utf8(fixture_bytes).unwrap_or_else(|error| {
        panic!(
            "node_compat fixture `{}` should contain valid UTF-8 text: {error}",
            fixture_source_path
        )
    })
}

fn read_node_compat_extra_fixture_entries(
    extra_files: &[NodeCompatExtraFixtureEntry],
) -> Vec<(String, Vec<u8>)> {
    extra_files
        .iter()
        .map(|entry| {
            (
                entry.runtime_path.to_string(),
                read_node_compat_fixture_bytes(entry.fixture_source_path),
            )
        })
        .collect()
}

const NODE_COMPAT_SYNTHETIC_COMMON_RUNTIME_PATHS: &[&str] = &[
    "test/common/index.js",
    "test/common/fixtures.js",
    "test/common/tmpdir.js",
];

fn append_lane_extra_fixture_file(
    owned_extra_files: &mut Vec<(String, Vec<u8>)>,
    lane: NodeCompatLane,
    runtime_path: &str,
) {
    let lane_name = node_compat_lane_name(lane);
    let source_path = node_compat_fixture_root()
        .join(lane_name)
        .join(runtime_path);
    let bytes = std::fs::read(&source_path).unwrap_or_else(|error| {
        panic!(
            "node_compat extra fixture `{}` should read: {error}",
            source_path.display()
        )
    });
    owned_extra_files.push((runtime_path.to_string(), bytes));
}

fn append_lane_extra_fixture_directory(
    owned_extra_files: &mut Vec<(String, Vec<u8>)>,
    lane: NodeCompatLane,
    runtime_dir: &str,
) {
    let lane_name = node_compat_lane_name(lane);
    let lane_root = node_compat_fixture_root().join(lane_name);
    let source_dir = lane_root.join(runtime_dir);
    let mut pending = vec![source_dir.clone()];
    let mut files = Vec::new();
    while let Some(dir) = pending.pop() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap_or_else(|error| {
                panic!(
                    "node_compat extra fixture directory `{}` should read: {error}",
                    dir.display()
                )
            })
            .map(|entry| {
                entry
                    .unwrap_or_else(|error| {
                        panic!(
                            "node_compat extra fixture directory `{}` entry should read: {error}",
                            dir.display()
                        )
                    })
                    .path()
            })
            .collect();
        entries.sort();
        for path in entries.into_iter().rev() {
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files.sort();
    for source_path in files {
        let runtime_path = source_path
            .strip_prefix(&lane_root)
            .unwrap_or_else(|error| {
                panic!(
                    "node_compat extra fixture `{}` should live under `{}`: {error}",
                    source_path.display(),
                    lane_root.display()
                )
            })
            .to_string_lossy()
            .into_owned();
        if NODE_COMPAT_SYNTHETIC_COMMON_RUNTIME_PATHS
            .iter()
            .any(|path| path == &runtime_path)
        {
            continue;
        }
        let bytes = std::fs::read(&source_path).unwrap_or_else(|error| {
            panic!(
                "node_compat extra fixture `{}` should read: {error}",
                source_path.display()
            )
        });
        owned_extra_files.push((runtime_path, bytes));
    }
}

// Some async_hooks promise-enable fixtures intentionally count promise hook
// callbacks around already in-flight promises. The default node_compat bundle
// wrapper adds extra Promise/queueMicrotask drains after import, which becomes
// observable noise for those files and obscures the real owner seam.
fn should_skip_default_async_drains_for_fixture(test_relative_path: &str) -> bool {
    matches!(
        test_relative_path,
        "test/parallel/test-async-hooks-enable-recursive.js"
            | "test/parallel/test-async-hooks-disable-during-promise.js"
            | "test/parallel/test-async-hooks-enable-before-promise-resolve.js"
            | "test/parallel/test-async-hooks-enable-during-promise.js"
            | "test/parallel/test-async-hooks-promise-triggerid.js"
            | "test/parallel/test-async-hooks-promise.js"
    )
}

fn should_use_sync_tick_drain_for_fixture(test_relative_path: &str) -> bool {
    matches!(
        test_relative_path,
        "test/parallel/test-async-hooks-enable-recursive.js"
            | "test/parallel/test-async-hooks-disable-during-promise.js"
            | "test/parallel/test-async-hooks-enable-before-promise-resolve.js"
            | "test/parallel/test-async-hooks-enable-during-promise.js"
            | "test/parallel/test-async-hooks-promise-triggerid.js"
            | "test/parallel/test-async-hooks-promise.js"
    )
}

fn should_suppress_sync_tick_promises_for_fixture(test_relative_path: &str) -> bool {
    matches!(
        test_relative_path,
        "test/parallel/test-async-hooks-enable-recursive.js"
    )
}

fn should_quiesce_then_require_fixture(test_relative_path: &str) -> bool {
    matches!(
        test_relative_path,
        "test/parallel/test-async-hooks-disable-during-promise.js"
            | "test/parallel/test-async-hooks-promise-triggerid.js"
            | "test/parallel/test-async-hooks-promise.js"
            | "test/parallel/test-repl-definecommand.js"
            | "test/parallel/test-repl-mode.js"
            | "test/parallel/test-repl-recoverable.js"
            | "test/parallel/test-repl-reset-event.js"
    )
}

fn should_capture_top_level_import_error_for_fixture(test_relative_path: &str) -> bool {
    matches!(
        test_relative_path,
        "test/parallel/test-runner-run-files-undefined.mjs"
            | "test/parallel/test-runner-import-no-scheme.js"
    )
}

fn write_node_compat_bundle(
    options: NodeCompatBundleWriteOptions<'_>,
) -> (tempfile::TempDir, PathBuf) {
    let NodeCompatBundleWriteOptions {
        test_relative_path,
        test_source,
        extra_files,
        capture_top_level_skip,
        lane,
        prelude_script,
        postlude_script,
        node_options,
        mode,
    } = options;
    let tempdir = if std::path::Path::new("/private/tmp").is_dir() {
        tempfile::Builder::new()
            .prefix("nvx-")
            .tempdir_in("/private/tmp")
            .expect("tempdir should build")
    } else {
        tempfile::Builder::new()
            .prefix("nvx-")
            .tempdir()
            .expect("tempdir should build")
    };
    let bundle_dir = tempdir.path().join("app/.nimbus/convex");
    std::fs::create_dir_all(&bundle_dir).expect("bundle dir should build");
    let bundle_path = bundle_dir.join("bundle.mjs");
    let compat_exec_setup = match mode {
        NodeCompatBundleMode::Runtime => {
            let current_exec_path =
                std::env::current_exe().expect("current executable should resolve");
            let current_exec_name = current_exec_path
                .file_name()
                .expect("current executable should have a file name");
            let compat_exec_path = bundle_dir.join("bin").join(current_exec_name);
            std::fs::create_dir_all(
                compat_exec_path
                    .parent()
                    .expect("compat exec parent should resolve"),
            )
            .expect("compat exec dir should build");
            std::fs::copy(&current_exec_path, &compat_exec_path).expect("compat exec should copy");
            format!(
                "const __nimbusCompatExecPath = {:?};",
                compat_exec_path.to_string_lossy()
            )
        }
        NodeCompatBundleMode::Oracle => {
            "const __nimbusCompatExecPath = globalThis.process?.execPath ?? \"\";".to_string()
        }
    };
    let gc_setup_script = match mode {
        NodeCompatBundleMode::Runtime => {
            r#"const __nimbusTestGc = function gc() {
  return globalThis.__nimbusSyncHostValue("op_nimbus_runtime_test_force_gc");
};
globalThis.gc = __nimbusTestGc;
globalThis.global.gc = __nimbusTestGc;"#
        }
        NodeCompatBundleMode::Oracle => "void 0;",
    };
    let uses_prelude = prelude_script.is_some();
    let capture_import_error = capture_top_level_skip
        || should_capture_top_level_import_error_for_fixture(test_relative_path)
        || matches!(
            default_prelude_behavior_for_fixture(test_relative_path),
            Some(NodeCompatNamedPreludeBehavior::ProcessExitSentinel)
        );
    let import_preamble = if should_quiesce_then_require_fixture(test_relative_path) {
        String::new()
    } else if capture_import_error {
        format!(
            r#"let __nimbusImportError = null;
try {{
  await import("./{test_relative_path}");
}} catch (error) {{
  __nimbusImportError = error;
}}"#
        )
    } else if uses_prelude {
        format!(r#"await import("./{test_relative_path}");"#)
    } else {
        format!(r#"import "./{test_relative_path}";"#)
    };
    let invoke_import_guard =
        if should_quiesce_then_require_fixture(test_relative_path) && capture_import_error {
            format!(
                r#"  if (typeof globalThis.__nimbusProcessTicksAndRejections === "function") {{
    globalThis.__nimbusProcessTicksAndRejections();
  }}
  let __nimbusImportError = null;
  try {{
    require("./{test_relative_path}");
  }} catch (error) {{
    __nimbusImportError = error;
  }}
  if (__nimbusImportError) {{
    if ({capture_top_level_skip} &&
        (__nimbusImportError?.__nimbusSkip ||
         __nimbusImportError?.code === "NIMBUS_NODE_COMPAT_SKIP")) {{
      return {{
        ok: true,
        skipped: true,
        testPath: "{test_relative_path}",
      }};
    }}
    throw __nimbusImportError;
  }}
"#
            )
        } else if should_quiesce_then_require_fixture(test_relative_path) {
            format!(
                r#"  if (typeof globalThis.__nimbusProcessTicksAndRejections === "function") {{
    globalThis.__nimbusProcessTicksAndRejections();
  }}
  require("./{test_relative_path}");
"#
            )
        } else if capture_import_error {
            format!(
                r#"  if (__nimbusImportError) {{
    if ({capture_top_level_skip} &&
        (__nimbusImportError?.__nimbusSkip ||
         __nimbusImportError?.code === "NIMBUS_NODE_COMPAT_SKIP")) {{
      return {{
        ok: true,
        skipped: true,
        testPath: "{test_relative_path}",
      }};
    }}
    throw __nimbusImportError;
  }}
"#
            )
        } else {
            String::new()
        };
    let lane_prelude = lane
        .map(|lane| {
            format!(
                "globalThis.__nimbusNodeCompatLane = {:?};",
                node_compat_lane_name(lane)
            )
        })
        .unwrap_or_default();
    let prelude_script = prelude_script.unwrap_or("");
    let postlude_script = postlude_script.unwrap_or("");
    let node_options_exec_argv = format!("{node_options:?}");
    let use_sync_tick_drain = should_use_sync_tick_drain_for_fixture(test_relative_path);
    let async_drain_script = if use_sync_tick_drain
        && should_suppress_sync_tick_promises_for_fixture(test_relative_path)
    {
        r#"  if (typeof globalThis.__nimbusProcessTicksAndRejections === "function") {
    const __nimbusSuppressionSymbol = Symbol.for("nimbus.asyncHooksSuppressionDepth");
    const __nimbusPreviousSuppressionDepth = globalThis[__nimbusSuppressionSymbol] || 0;
    globalThis[__nimbusSuppressionSymbol] = __nimbusPreviousSuppressionDepth + 1;
    try {
      globalThis.__nimbusProcessTicksAndRejections();
    } finally {
      if (__nimbusPreviousSuppressionDepth === 0) {
        delete globalThis[__nimbusSuppressionSymbol];
      } else {
        globalThis[__nimbusSuppressionSymbol] = __nimbusPreviousSuppressionDepth;
      }
    }
  }
"#
    } else if use_sync_tick_drain {
        r#"  if (typeof globalThis.__nimbusProcessTicksAndRejections === "function") {
    globalThis.__nimbusProcessTicksAndRejections();
  }
"#
    } else if should_skip_default_async_drains_for_fixture(test_relative_path) {
        r#"  if (typeof globalThis.__nimbusFlushEmbeddedTests === "function") {
    await globalThis.__nimbusFlushEmbeddedTests();
  }
"#
    } else {
        r#"  if (typeof globalThis.process?.nextTick === "function") {
    await new Promise((resolve) => globalThis.process.nextTick(resolve));
  }
  if (typeof globalThis.__nimbusFlushEmbeddedTests === "function") {
    await globalThis.__nimbusFlushEmbeddedTests();
  }
  await Promise.resolve();
  await new Promise((resolve) => queueMicrotask(resolve));
  if (typeof globalThis.process?.nextTick === "function") {
    await new Promise((resolve) => globalThis.process.nextTick(resolve));
  }
"#
    };
    let invoke_signature = if use_sync_tick_drain {
        "globalThis.__nimbusInvoke = function () {"
    } else {
        "globalThis.__nimbusInvoke = async function () {"
    };
    let child_process_flush_script = if use_sync_tick_drain {
        ""
    } else {
        r#"  await common.__nimbusFlushChildProcesses?.();
"#
    };
    let process_exit_cleanup_script = if use_sync_tick_drain {
        ""
    } else {
        r#"      await common.__nimbusFlushChildProcesses?.();
"#
    };
    std::fs::write(
        &bundle_path,
        format!(
            r#"
import {{ createRequire }} from "node:module";
{compat_exec_setup}
const __nimbusCompatMainScriptPath = new URL(
  "./{test_relative_path}",
  import.meta.url,
).pathname;
globalThis.global ??= globalThis;
const __nimbusProcessExitCodeFromError = (error) => {{
  const marker = "__NIMBUS_NODE_COMPAT_PROCESS_EXIT__:";
  if (error?.code !== "NIMBUS_NODE_COMPAT_PROCESS_EXIT") {{
    return null;
  }}
  const rendered =
    typeof error?.message === "string" ? error.message : String(error);
  const markerIndex = rendered.indexOf(marker);
  if (markerIndex < 0) {{
    return Number(globalThis.process?.exitCode ?? 0);
  }}
  const numericPrefix = rendered
    .slice(markerIndex + marker.length)
    .match(/^-?\d+/)?.[0];
  return numericPrefix === undefined
    ? Number(globalThis.process?.exitCode ?? 0)
    : Number(numericPrefix);
}};
{gc_setup_script}
if (typeof globalThis.process === "object" && globalThis.process !== null) {{
  globalThis.process.execPath = __nimbusCompatExecPath;
  const __nimbusNodeCompatExecArgv = {node_options_exec_argv};
  globalThis.process.execArgv = Array.from(__nimbusNodeCompatExecArgv);
  if (Array.isArray(globalThis.process.argv)) {{
    if (globalThis.process.argv.length === 0) {{
      globalThis.process.argv.push(__nimbusCompatExecPath);
    }} else {{
      globalThis.process.argv[0] = __nimbusCompatExecPath;
    }}
    if (globalThis.process.argv.length >= 2) {{
      globalThis.process.argv[1] = __nimbusCompatMainScriptPath;
    }} else {{
      globalThis.process.argv.push(__nimbusCompatMainScriptPath);
    }}
  }}
}}
{lane_prelude}
{prelude_script}
{import_preamble}

{invoke_signature}
  let __nimbusInvokeStep = "create require";
  const require = createRequire(import.meta.url);
  try {{
    __nimbusInvokeStep = "import guard";
{invoke_import_guard}
    __nimbusInvokeStep = "require common";
    const common = require("./test/common/index.js");
    __nimbusInvokeStep = "async drain";
{async_drain_script}
    __nimbusInvokeStep = "postlude";
{postlude_script}
    __nimbusInvokeStep = "child process flush";
{child_process_flush_script}
    __nimbusInvokeStep = "common assert";
    common.__nimbusAssert?.();
    globalThis.__nimbusNodeCompatInvocationFinalized = true;
    return {{
      ok: true,
      skipped: false,
      testPath: "{test_relative_path}",
    }};
  }} catch (__nimbusInvokeError) {{
    if (__nimbusInvokeError === undefined) {{
      throw new Error(`Nimbus node_compat harness rejected with undefined during ${{__nimbusInvokeStep}}`);
    }}
    const __nimbusExitCode =
      __nimbusProcessExitCodeFromError(__nimbusInvokeError);
    if (__nimbusExitCode === 0) {{
{process_exit_cleanup_script}
      globalThis.__nimbusNodeCompatInvocationFinalized = true;
      return {{
        ok: true,
        skipped: false,
        testPath: "{test_relative_path}",
      }};
    }}
    throw __nimbusInvokeError;
  }}
}};

export {{}};
"#
        ),
    )
    .expect("bundle should write");

    let common_path = bundle_dir.join("test/common/index.js");
    std::fs::create_dir_all(common_path.parent().expect("common parent should resolve"))
        .expect("common dir should build");
    std::fs::write(&common_path, COMMON_INDEX_FIXTURE).expect("common fixture should write");
    let common_fixtures_path = bundle_dir.join("test/common/fixtures.js");
    std::fs::write(&common_fixtures_path, COMMON_FIXTURES_FIXTURE)
        .expect("common fixtures module should write");
    let common_tmpdir_path = bundle_dir.join("test/common/tmpdir.js");
    std::fs::write(&common_tmpdir_path, COMMON_TMPDIR_FIXTURE)
        .expect("common tmpdir module should write");

    let test_path = bundle_dir.join(test_relative_path);
    std::fs::create_dir_all(test_path.parent().expect("test parent should resolve"))
        .expect("test dir should build");
    std::fs::write(&test_path, test_source).expect("upstream test fixture should write");
    for (relative_path, source) in extra_files {
        let fixture_path = bundle_dir.join(relative_path);
        std::fs::create_dir_all(
            fixture_path
                .parent()
                .expect("extra fixture parent should resolve"),
        )
        .expect("extra fixture dir should build");
        std::fs::write(&fixture_path, source).expect("extra fixture should write");
    }

    (tempdir, bundle_path)
}

fn execute_upstream_node_compat_test_with_extra_files(
    test_relative_path: &str,
    test_source: &str,
    extra_files: &[(&str, &[u8])],
    capture_top_level_skip: bool,
    lane: Option<NodeCompatLane>,
    prelude_script: Option<&str>,
    postlude_script: Option<&str>,
) -> std::result::Result<NodeCompatFixtureOutcome, String> {
    let _guard = acquire_runtime_suite_lock();
    let fixture_node_options = fixture_requested_node_options(test_source);
    let fixture_needs_pending_deprecation = fixture_node_options
        .iter()
        .any(|flag| flag == "--pending-deprecation");
    let resolved_prelude_behavior =
        prelude_script.and_then(NodeCompatNamedPreludeBehavior::from_script);
    let _interactive_term_guard = matches!(
        resolved_prelude_behavior,
        Some(NodeCompatNamedPreludeBehavior::InteractiveTerminal)
    )
    .then(|| ScopedProcessEnvVar::set("TERM", "xterm-256color"));
    let _node_options_guard = (!fixture_node_options.is_empty())
        .then(|| scoped_node_options_flags(&fixture_node_options));
    let effective_prelude = if fixture_needs_pending_deprecation {
        format!(
            "{PENDING_DEPRECATION_PRELUDE}\n{}",
            prelude_script.unwrap_or("")
        )
    } else {
        prelude_script.unwrap_or("").to_string()
    };
    let (_tempdir, bundle_path) = write_node_compat_bundle(NodeCompatBundleWriteOptions {
        test_relative_path,
        test_source,
        extra_files,
        capture_top_level_skip,
        lane,
        prelude_script: Some(effective_prelude.as_str()),
        postlude_script,
        node_options: &fixture_node_options,
        mode: NodeCompatBundleMode::Runtime,
    });
    let mut limits = runtime_limits_for_node_compat_fixture(test_relative_path, lane);
    limits.node_conditions = fixture_requested_node_conditions(&fixture_node_options);
    if !fixture_node_options.is_empty() {
        grant_node_options_read_for_fixture_flags(&mut limits);
    }
    let wall_clock_timeout = node_compat_fixture_wall_clock_timeout(&limits);
    let lane_name = lane.map(node_compat_lane_name).unwrap_or("unspecified");
    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(limits)),
    );
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "node_compat:run".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    let started_at = Instant::now();
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(async {
            tokio::time::timeout(
                wall_clock_timeout,
                runtime.invoke_bundle(&RuntimeBundle::new(&bundle_path), &request),
            )
            .await
        });

    let result = match result {
        Ok(result) => result,
        Err(_) => {
            let elapsed = started_at.elapsed();
            let detail = format!(
                "fixture exceeded harness wall-clock timeout of {:?} after {:?}",
                wall_clock_timeout, elapsed
            );
            let artifact = write_node_compat_fixture_diagnostic(
                lane_name,
                test_relative_path,
                &bundle_path,
                wall_clock_timeout,
                elapsed,
                "wall_clock_timeout",
                &detail,
            )
            .map(|path| format!("; diagnostic artifact: {}", path.display()))
            .unwrap_or_default();
            return Err(format!(
                "upstream node_compat fixture `{test_relative_path}` exceeded wall-clock timeout{artifact}"
            ));
        }
    };

    let result = match result {
        Ok(result) => result,
        Err(error) => {
            let error = error.to_string();
            if matches!(
                resolved_prelude_behavior,
                Some(NodeCompatNamedPreludeBehavior::ProcessExitSentinel)
            ) && let Some(exit_code) = node_compat_process_exit_code_from_error(&error)
            {
                if exit_code == 0 {
                    return Ok(NodeCompatFixtureOutcome { skipped: false });
                }
                let artifact = write_node_compat_fixture_diagnostic(
                    lane_name,
                    test_relative_path,
                    &bundle_path,
                    wall_clock_timeout,
                    started_at.elapsed(),
                    "process_exit",
                    &error,
                )
                .map(|path| format!("; diagnostic artifact: {}", path.display()))
                .unwrap_or_default();
                return Err(format!(
                    "upstream node_compat fixture `{test_relative_path}` exited with non-zero code {exit_code}: {error}{artifact}"
                ));
            }
            let artifact = write_node_compat_fixture_diagnostic(
                lane_name,
                test_relative_path,
                &bundle_path,
                wall_clock_timeout,
                started_at.elapsed(),
                "runtime_error",
                &error,
            )
            .map(|path| format!("; diagnostic artifact: {}", path.display()))
            .unwrap_or_default();
            return Err(format!(
                "upstream node_compat fixture `{test_relative_path}` should execute: {error}{artifact}"
            ));
        }
    };

    if result.get("ok") != Some(&serde_json::json!(true)) {
        let detail = format!("fixture returned non-ok payload: {result}");
        let artifact = write_node_compat_fixture_diagnostic(
            lane_name,
            test_relative_path,
            &bundle_path,
            wall_clock_timeout,
            started_at.elapsed(),
            "non_ok_payload",
            &detail,
        )
        .map(|path| format!("; diagnostic artifact: {}", path.display()))
        .unwrap_or_default();
        return Err(format!(
            "upstream node_compat fixture `{test_relative_path}` returned non-ok payload: {result}{artifact}"
        ));
    }

    if result.get("testPath") != Some(&serde_json::json!(test_relative_path)) {
        let detail = format!("fixture returned mismatched testPath payload: {result}");
        let artifact = write_node_compat_fixture_diagnostic(
            lane_name,
            test_relative_path,
            &bundle_path,
            wall_clock_timeout,
            started_at.elapsed(),
            "mismatched_test_path",
            &detail,
        )
        .map(|path| format!("; diagnostic artifact: {}", path.display()))
        .unwrap_or_default();
        return Err(format!(
            "upstream node_compat fixture `{test_relative_path}` returned mismatched testPath payload: {result}{artifact}"
        ));
    }

    Ok(NodeCompatFixtureOutcome {
        skipped: result.get("skipped") == Some(&serde_json::json!(true)),
    })
}

fn node_compat_fixture_requires_runtime_self_exec(test_relative_path: &str) -> bool {
    test_relative_path.starts_with("test/parallel/test-runner-")
        || test_relative_path.starts_with("test/parallel/test-process-")
        || test_relative_path.starts_with("test/parallel/test-url-parse-")
        || test_relative_path.starts_with("test/wasi/test-wasi-")
        || matches!(
            test_relative_path,
            "test/parallel/test-process-finalization.mjs" | "test/parallel/test-sqlite.js"
        )
}

fn runtime_limits_for_node_compat_fixture(
    test_relative_path: &str,
    lane: Option<NodeCompatLane>,
) -> RuntimeLimits {
    let mut limits = match lane.unwrap_or(NodeCompatLane::Node24) {
        NodeCompatLane::Node20 => RuntimeLimits::application_node20_local_development(),
        NodeCompatLane::Node22 => RuntimeLimits::application_node22_local_development(),
        NodeCompatLane::Node24 => RuntimeLimits::application_node24_local_development(),
        NodeCompatLane::Node26 => RuntimeLimits::application_node26_local_development(),
    };
    if node_compat_fixture_requires_runtime_self_exec(test_relative_path) {
        // These compat fixtures respawn the copied harness binary via
        // process.execPath to prove Node CLI/reporter/WASI behavior. Keep the
        // rest of the application-preset contract intact, but allow the
        // synthetic compat exec target so the fixture can drive its own child
        // runtime without reopening general host subprocess access.
        limits.grants.run = vec!["$runtime_self_exec".to_string()];
    }
    if matches!(
        test_relative_path,
        "test/parallel/test-runner-reporters.js" | "test/parallel/test-runner-cli-randomize.js"
    ) {
        // These files are nested-subprocess `node:test` sweeps that stay
        // within the same semantic contract but legitimately run longer than
        // the default 30s application budget inside the embedded compat
        // harness.
        limits.execution_timeout = Duration::from_secs(120);
    }
    limits
}

#[test]
fn node_compat_runtime_limits_only_grant_self_exec_to_known_respawn_fixtures() {
    let runner_limits = runtime_limits_for_node_compat_fixture(
        "test/parallel/test-runner-reporters.js",
        Some(NodeCompatLane::Node22),
    );
    assert_eq!(
        runner_limits.compatibility_target,
        RuntimeCompatibilityTarget::Node22
    );
    assert_eq!(runner_limits.grants.run, vec!["$runtime_self_exec"]);

    let wasi_limits = runtime_limits_for_node_compat_fixture(
        "test/wasi/test-wasi-stdio.js",
        Some(NodeCompatLane::Node24),
    );
    assert_eq!(
        wasi_limits.compatibility_target,
        RuntimeCompatibilityTarget::Node24
    );
    assert_eq!(wasi_limits.grants.run, vec!["$runtime_self_exec"]);

    let ordinary_limits = runtime_limits_for_node_compat_fixture(
        "test/parallel/test-runner-assert.js",
        Some(NodeCompatLane::Node20),
    );
    assert_eq!(
        ordinary_limits.compatibility_target,
        RuntimeCompatibilityTarget::Node20
    );
    assert_eq!(
        ordinary_limits.grants.run,
        vec!["$runtime_self_exec"],
        "test-runner fixtures currently opt into the compat self-exec seam as a family",
    );

    let non_respawn_limits =
        runtime_limits_for_node_compat_fixture("test/parallel/test-repl-mode.js", None);
    assert_eq!(
        non_respawn_limits.compatibility_target,
        RuntimeCompatibilityTarget::Node24
    );
    assert!(non_respawn_limits.grants.run.is_empty());
}

#[test]
fn node_compat_harness_wall_clock_timeout_tracks_fixture_runtime_budget() {
    let ordinary_limits = runtime_limits_for_node_compat_fixture(
        "test/parallel/test-repl-mode.js",
        Some(NodeCompatLane::Node22),
    );
    assert_eq!(
        node_compat_fixture_wall_clock_timeout(&ordinary_limits),
        Duration::from_secs(35),
        "ordinary fixtures get the 30s runtime budget plus harness slack",
    );

    let nested_runner_limits = runtime_limits_for_node_compat_fixture(
        "test/parallel/test-runner-reporters.js",
        Some(NodeCompatLane::Node22),
    );
    assert_eq!(
        nested_runner_limits.execution_timeout,
        Duration::from_secs(120)
    );
    assert_eq!(
        node_compat_fixture_wall_clock_timeout(&nested_runner_limits),
        Duration::from_secs(125),
        "long-running subprocess fixtures still have a finite wall-clock budget",
    );
}

#[test]
fn node_compat_harness_diagnostics_cover_hang_families() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempfile::tempdir().expect("diagnostic tempdir should build");
    let _diagnostic_root_guard = ScopedProcessEnvVar::set(
        "NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT",
        tempdir.path().to_string_lossy().as_ref(),
    );
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};").expect("synthetic bundle should write");

    let cases = [
        (
            "test/parallel/test-next-tick-doesnt-hang.js",
            NodeCompatFixtureDiagnosticFamily::EventLoop,
        ),
        (
            "test/parallel/test-vm-basic.js",
            NodeCompatFixtureDiagnosticFamily::Vm,
        ),
        (
            "test/parallel/test-worker.js",
            NodeCompatFixtureDiagnosticFamily::Worker,
        ),
        (
            "test/parallel/test-worker-message-port.js",
            NodeCompatFixtureDiagnosticFamily::MessagePort,
        ),
        (
            "test/parallel/test-runner-reporters.js",
            NodeCompatFixtureDiagnosticFamily::Subprocess,
        ),
    ];

    for (test_relative_path, expected_family) in cases {
        assert_eq!(
            node_compat_fixture_diagnostic_family(test_relative_path),
            expected_family
        );
        let artifact = write_node_compat_fixture_diagnostic(
            "node22",
            test_relative_path,
            &bundle_path,
            Duration::from_secs(35),
            Duration::from_secs(35),
            "synthetic_timeout",
            "synthetic diagnostic coverage probe",
        )
        .expect("diagnostic artifact should write");
        let payload: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&artifact).expect("diagnostic artifact should read"),
        )
        .expect("diagnostic artifact should parse");
        assert_eq!(
            payload["report_kind"],
            "node_compat_fixture_execution_diagnostic"
        );
        assert_eq!(payload["diagnostic_family"], expected_family.as_str());
        assert_eq!(payload["timeout_ms"], 35_000);
        assert!(
            payload["exit_criterion"]
                .as_str()
                .expect("exit criterion should serialize")
                .len()
                > 20,
            "diagnostic artifact should carry a real exit criterion",
        );
    }
}

#[test]
fn node_compat_harness_message_port_exit_criterion_blocks_unqualified_worker_promotion() {
    let criterion = NodeCompatFixtureDiagnosticFamily::MessagePort.exit_criterion();
    assert!(
        criterion.contains("NLRT8"),
        "MessagePort worker hazards should point at the production profile split"
    );
    assert!(
        criterion.contains("production in-process"),
        "MessagePort worker hazards should not be promoted into production in-process profiles by implication"
    );
}

#[test]
fn node_compat_common_fixture_platform_booleans_track_process_platform() {
    execute_upstream_node_compat_test_with_extra_files(
        "test/parallel/test-nimbus-common-platform-booleans.js",
        r#"'use strict';

const assert = require('assert');
const common = require('../common');

assert.strictEqual(common.isAIX, process.platform === 'aix');
assert.strictEqual(common.isFreeBSD, process.platform === 'freebsd');
assert.strictEqual(common.isIBMi, process.platform === 'os400');
assert.strictEqual(common.isLinux, process.platform === 'linux');
assert.strictEqual(common.isMacOS, process.platform === 'darwin');
assert.strictEqual(common.isOpenBSD, process.platform === 'openbsd');
assert.strictEqual(common.isSunOS, process.platform === 'sunos');
assert.strictEqual(common.isWindows, process.platform === 'win32');
assert.strictEqual(common.isDebug, process.features.debug === true);
assert.strictEqual(common.isASan, process.config?.variables?.asan === 1);
assert.strictEqual(typeof common.isPi, 'function');
assert.strictEqual(typeof common.isInsideDirWithUnusualChars, 'boolean');
"#,
        &[],
        false,
        Some(NodeCompatLane::Node24),
        None,
        None,
    )
    .expect("node_compat common fixture platform booleans should execute");
}

fn execute_manifested_node_compat_test(
    test_relative_path: &str,
    fixture_source_path: &str,
    extra_files: &[NodeCompatExtraFixtureEntry],
    capture_top_level_skip: bool,
    lane: Option<NodeCompatLane>,
    prelude_script: Option<&str>,
    postlude_script: Option<&str>,
) -> std::result::Result<NodeCompatFixtureOutcome, String> {
    let test_source = read_node_compat_fixture_text(fixture_source_path);
    let owned_extra_files = read_node_compat_extra_fixture_entries(extra_files);
    let borrowed_extra_files: Vec<(&str, &[u8])> = owned_extra_files
        .iter()
        .map(|(runtime_path, source)| (runtime_path.as_str(), source.as_slice()))
        .collect();
    let resolved_prelude_behavior = prelude_script
        .and_then(NodeCompatNamedPreludeBehavior::from_script)
        .or_else(|| default_prelude_behavior_for_fixture(test_relative_path));
    let resolved_postlude_behavior = postlude_script
        .and_then(NodeCompatNamedPostludeBehavior::from_script)
        .or_else(|| default_postlude_behavior_for_fixture(test_relative_path));
    execute_upstream_node_compat_test_with_extra_files(
        test_relative_path,
        &test_source,
        &borrowed_extra_files,
        capture_top_level_skip,
        lane.or_else(|| inferred_node_compat_lane_from_fixture_source_path(fixture_source_path)),
        prelude_script.or_else(|| resolved_prelude_behavior.map(|behavior| behavior.script())),
        postlude_script.or_else(|| resolved_postlude_behavior.map(|behavior| behavior.script())),
    )
}

fn execute_manifested_node_compat_test_with_lane_extra_dirs(
    test_relative_path: &str,
    fixture_source_path: &str,
    extra_files: &[NodeCompatExtraFixtureEntry],
    extra_runtime_files: &[&str],
    extra_dirs: &[&str],
    lane: NodeCompatLane,
) -> std::result::Result<NodeCompatFixtureOutcome, String> {
    let test_source = read_node_compat_fixture_text(fixture_source_path);
    let mut owned_extra_files = read_node_compat_extra_fixture_entries(extra_files);
    for extra_runtime_file in extra_runtime_files {
        append_lane_extra_fixture_file(&mut owned_extra_files, lane, extra_runtime_file);
    }
    for extra_dir in extra_dirs {
        append_lane_extra_fixture_directory(&mut owned_extra_files, lane, extra_dir);
    }
    let borrowed_extra_files: Vec<(&str, &[u8])> = owned_extra_files
        .iter()
        .map(|(runtime_path, source)| (runtime_path.as_str(), source.as_slice()))
        .collect();
    let resolved_prelude_behavior = default_prelude_behavior_for_fixture(test_relative_path);
    let resolved_postlude_behavior = default_postlude_behavior_for_fixture(test_relative_path);
    execute_upstream_node_compat_test_with_extra_files(
        test_relative_path,
        &test_source,
        &borrowed_extra_files,
        true,
        Some(lane),
        resolved_prelude_behavior.map(|behavior| behavior.script()),
        resolved_postlude_behavior.map(NodeCompatNamedPostludeBehavior::script),
    )
}

fn node_compat_required_gap_paths_for_selector(
    lane: NodeCompatLane,
    selector: fn(&str) -> bool,
) -> Vec<String> {
    node_compat_posture_paths_for_selector(lane, |entry| {
        entry["support_denominator"] == "v8_isolate_required"
            && entry["test_path"].as_str().is_some_and(selector)
    })
}

fn node_compat_required_gap_paths_for_owner(lane: NodeCompatLane, owner: &str) -> Vec<String> {
    node_compat_posture_paths_for_selector(lane, |entry| {
        entry["support_denominator"] == "v8_isolate_required" && entry["owner"] == owner
    })
}

fn node_compat_posture_paths_for_selector(
    lane: NodeCompatLane,
    selector: impl Fn(&serde_json::Value) -> bool,
) -> Vec<String> {
    let lane_name = node_compat_lane_name(lane);
    let posture_path =
        node_compat_repo_root().join("docs/architecture/runtime/node-default-support-posture.json");
    let posture: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&posture_path).unwrap_or_else(|error| {
            panic!(
                "node_compat posture `{}` should read: {error}",
                posture_path.display()
            )
        }))
        .unwrap_or_else(|error| {
            panic!(
                "node_compat posture `{}` should parse: {error}",
                posture_path.display()
            )
        });
    let mut paths: Vec<String> = posture["lanes"][lane_name]["entries"]
        .as_array()
        .unwrap_or_else(|| panic!("node_compat posture lane `{lane_name}` entries should be array"))
        .iter()
        .filter(|entry| selector(entry))
        .filter_map(|entry| entry["test_path"].as_str())
        .map(str::to_string)
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

fn esm_module_loader_required_gap_path(test_path: &str) -> bool {
    test_path.starts_with("test/es-module/")
        || test_path.starts_with("test/parallel/test-module")
        || test_path.starts_with("test/parallel/test-require")
}

fn esm_inprocess_module_loader_required_gap_path(test_path: &str) -> bool {
    let in_scope = test_path.starts_with("test/es-module/")
        || test_path.starts_with("test/parallel/test-module")
        || test_path == "test/parallel/test-require-process.js";
    if !in_scope {
        return false;
    }

    const LOW_ROI_FRAGMENTS: &[&str] = &[
        "cjs-esm-warn",
        "loader",
        "long-path",
        "module-loading-error",
        "no-addons",
        "nowarn-exports",
        "preload",
        "print-timing",
        "readonly",
        "setsourcemap",
        "source-map",
        "spawn",
        "symlinked-peer",
        "type-flag",
        "typescript",
        "vm-",
        "wasm",
    ];
    if LOW_ROI_FRAGMENTS
        .iter()
        .any(|fragment| test_path.contains(fragment))
    {
        return false;
    }

    const INPROCESS_FRAGMENTS: &[&str] = &[
        "assertion",
        "basic-imports",
        "cjs",
        "conditional",
        "data-url",
        "detect",
        "dynamic-import",
        "error-cache",
        "exports",
        "extension",
        "import-attributes",
        "import-meta",
        "imports",
        "initialization",
        "json",
        "live-binding",
        "main-lookup",
        "module-not-found",
        "named-exports",
        "package",
        "pkg",
        "process",
        "prototype-pollution",
        "require-module",
        "resolve",
        "type-field",
        "unknown-extension",
        "url-extname",
        "virtual-json",
    ];
    INPROCESS_FRAGMENTS
        .iter()
        .any(|fragment| test_path.contains(fragment))
}

fn esm_inprocess_module_loader_required_gap_paths(lane: NodeCompatLane) -> Vec<String> {
    let fixture_paths = node_compat_required_gap_paths_for_selector(
        lane,
        esm_inprocess_module_loader_required_gap_path,
    );
    assert!(
        (50..=100).contains(&fixture_paths.len()),
        "ESM in-process module-loader selector should stay reviewable; selected {} fixtures",
        fixture_paths.len()
    );
    fixture_paths
}

fn module_loader_required_surface_blocker_entry(entry: &serde_json::Value) -> bool {
    if entry["support_denominator"] != "v8_isolate_required" {
        return false;
    }
    let owner = entry["owner"].as_str();
    let Some(test_path) = entry["test_path"].as_str() else {
        return false;
    };

    owner == Some("loader-context/module")
        || owner == Some("loader-context/util")
        || test_path.starts_with("test/es-module/")
        || test_path.starts_with("test/module-hooks/")
        || test_path.contains("test-require-module")
        || test_path.contains("test-import-module")
        || test_path.contains("test-typescript")
        || test_path.contains("test-loaders-")
        || test_path.contains("test-data-url")
}

fn module_loader_required_surface_blocker_paths(lane: NodeCompatLane) -> Vec<String> {
    let fixture_paths =
        node_compat_posture_paths_for_selector(lane, module_loader_required_surface_blocker_entry);
    assert!(
        (50..=260).contains(&fixture_paths.len()),
        "module-loader required-surface blocker selector should stay broad but reviewable; selected {} fixtures",
        fixture_paths.len()
    );
    fixture_paths
}

fn module_loader_json_data_import_attributes_required_surface_entry(
    entry: &serde_json::Value,
) -> bool {
    if !module_loader_required_surface_blocker_entry(entry) {
        return false;
    }
    let Some(test_path) = entry["test_path"].as_str() else {
        return false;
    };

    const JSON_DATA_IMPORT_ATTRIBUTE_PATHS: &[&str] = &[
        "test/es-module/test-esm-assertionless-json-import.js",
        "test/es-module/test-esm-data-urls.js",
        "test/es-module/test-esm-import-assertion-warning.mjs",
        "test/es-module/test-esm-import-attributes-errors.js",
        "test/es-module/test-esm-import-attributes-errors.mjs",
        "test/es-module/test-esm-import-attributes-identity.mjs",
        "test/es-module/test-esm-import-attributes-validation.js",
        "test/es-module/test-esm-invalid-data-urls.js",
        "test/es-module/test-esm-invalid-pjson.js",
        "test/es-module/test-esm-json-cache.mjs",
        "test/es-module/test-esm-json.mjs",
        "test/es-module/test-esm-virtual-json.mjs",
        "test/parallel/test-data-url.js",
    ];
    JSON_DATA_IMPORT_ATTRIBUTE_PATHS.contains(&test_path)
}

fn module_loader_json_data_import_attributes_required_surface_paths(
    lane: NodeCompatLane,
) -> Vec<String> {
    let fixture_paths = node_compat_posture_paths_for_selector(
        lane,
        module_loader_json_data_import_attributes_required_surface_entry,
    );
    assert!(
        (10..=20).contains(&fixture_paths.len()),
        "module-loader JSON/data/import-attributes selector should stay exact; selected {} fixtures",
        fixture_paths.len()
    );
    fixture_paths
}

fn module_loader_package_core_required_surface_entry(entry: &serde_json::Value) -> bool {
    if !module_loader_required_surface_blocker_entry(entry) {
        return false;
    }
    let Some(test_path) = entry["test_path"].as_str() else {
        return false;
    };
    let host_or_adjacent_scope = test_path.starts_with("test/module-hooks/")
        || test_path.contains("wasm")
        || test_path.contains("preload")
        || test_path.contains("print-execution")
        || test_path.contains("warning")
        || test_path.contains("warn.js")
        || test_path.contains("nowarn")
        || test_path.contains("preserve-symlinks-main")
        || test_path.contains("feature-detect")
        || test_path.contains("import-require-tla-twice")
        || test_path.contains("legacyMainResolve")
        || test_path.contains("loader-http-imports")
        || test_path.contains("require-module-cycle")
        || test_path.contains("require-module-errors")
        || test_path.contains("require-esm-from-imported-cjs")
        || test_path.contains("transpiled");
    if host_or_adjacent_scope {
        return false;
    }

    test_path.contains("cjs-esm")
        || test_path.contains("disable-require-module")
        || test_path.contains("dynamic-import-commonjs")
        || test_path.contains("legacyMainResolve")
        || test_path.contains("module-not-found")
        || test_path.contains("pkg")
        || test_path.contains("preserve-symlinks")
        || test_path.contains("require-esm")
        || test_path.contains("require-module")
        || test_path.contains("symlink")
        || test_path.contains("tla")
        || test_path.contains("type-field")
        || test_path.contains("conditional-exports")
        || test_path.contains("exports")
        || test_path.contains("imports")
}

fn module_loader_package_core_required_surface_paths(lane: NodeCompatLane) -> Vec<String> {
    let fixture_paths = node_compat_posture_paths_for_selector(
        lane,
        module_loader_package_core_required_surface_entry,
    );
    assert!(
        (10..=60).contains(&fixture_paths.len()),
        "module-loader package/CJS/ESM core selector should stay focused but broad enough to matter; selected {} fixtures",
        fixture_paths.len()
    );
    fixture_paths
}

fn async_hooks_required_gap_path(test_path: &str) -> bool {
    test_path.starts_with("test/async-hooks/")
        || test_path.starts_with("test/parallel/test-async-hooks")
}

/// async_hooks required-gap fixtures that bind a real TCP/UDP/TLS socket or
/// drive an HTTP parser over a live connection. In a FaaS isolate these park
/// the single-threaded executor on the blocking bind, so the in-process soft
/// wall-clock can never fire and the whole batch hangs (the summary writes only
/// at batch end, so one park suppresses the summary for every fixture). They
/// are the structural-networking tension owned separately from the promise/
/// timer/resource lifecycle closure; excluding them lets the non-blocking batch
/// complete and emit an honest summary.
const ASYNC_HOOKS_SOCKET_BIND_BLOCKING_PATHS: &[&str] = &[
    "test/async-hooks/test-async-exec-resource-http-32060.js",
    "test/async-hooks/test-async-exec-resource-http-agent.js",
    "test/async-hooks/test-async-exec-resource-http.js",
    "test/async-hooks/test-graph.http.js",
    "test/async-hooks/test-graph.tcp.js",
    "test/async-hooks/test-graph.tls-write-12.js",
    "test/async-hooks/test-graph.tls-write.js",
    "test/async-hooks/test-httpparser-reuse.js",
    "test/async-hooks/test-httpparser.request.js",
    "test/async-hooks/test-httpparser.response.js",
    "test/async-hooks/test-tcpwrap.js",
    "test/async-hooks/test-tlswrap.js",
    "test/parallel/test-async-hooks-http-parser-destroy.js",
];

fn async_hooks_nonblocking_required_gap_path(test_path: &str) -> bool {
    async_hooks_required_gap_path(test_path)
        && !ASYNC_HOOKS_SOCKET_BIND_BLOCKING_PATHS.contains(&test_path)
}

fn webcrypto_required_gap_path(test_path: &str) -> bool {
    test_path.starts_with("test/parallel/test-webcrypto")
}

fn event_required_gap_path(test_path: &str) -> bool {
    test_path.starts_with("test/parallel/test-event")
}

fn networking_crypto_required_gap_path(test_path: &str) -> bool {
    test_path.starts_with("test/parallel/test-crypto")
        || test_path.starts_with("test/async-hooks/test-crypto")
}

fn module_hooks_required_gap_path(test_path: &str) -> bool {
    test_path.starts_with("test/module-hooks/")
}

fn parallel_js_platform_required_gap_path(test_path: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "test/parallel/test-abort-controller",
        "test/parallel/test-abortcontroller",
        "test/parallel/test-aborted-util",
        "test/parallel/test-error",
        "test/parallel/test-errors",
        "test/parallel/test-eventtarget",
        "test/parallel/test-global",
        "test/parallel/test-performance",
        "test/parallel/test-performanceobserver",
        "test/parallel/test-promise",
        "test/parallel/test-promises",
        "test/parallel/test-util",
    ];
    PREFIXES.iter().any(|prefix| test_path.starts_with(prefix))
}

const UNPROMOTED_PARALLEL_DISCOVERY_EXCLUDED_PREFIXES: &[&str] = &[
    "test/parallel/test-abort-controller",
    "test/parallel/test-abortcontroller",
    "test/parallel/test-aborted-util",
    "test/parallel/test-async-hooks",
    "test/parallel/test-blob",
    "test/parallel/test-cli",
    "test/parallel/test-compression-decompression-stream",
    "test/parallel/test-crypto",
    "test/parallel/test-debug",
    "test/parallel/test-diagnostic-channel",
    "test/parallel/test-diagnostics-channel",
    "test/parallel/test-dns",
    "test/parallel/test-domain",
    "test/parallel/test-double-tls",
    "test/parallel/test-error",
    "test/parallel/test-errors",
    "test/parallel/test-eslint",
    "test/parallel/test-eventtarget",
    "test/parallel/test-fs",
    "test/parallel/test-gc",
    "test/parallel/test-global",
    "test/parallel/test-heapdump",
    "test/parallel/test-http",
    "test/parallel/test-https",
    "test/parallel/test-module",
    "test/parallel/test-node-output",
    "test/parallel/test-performance",
    "test/parallel/test-performanceobserver",
    "test/parallel/test-permission",
    "test/parallel/test-preload",
    "test/parallel/test-promise",
    "test/parallel/test-promises",
    "test/parallel/test-quic",
    "test/parallel/test-set-process-debug",
    "test/parallel/test-snapshot",
    "test/parallel/test-strace",
    "test/parallel/test-stream",
    "test/parallel/test-tick-processor",
    "test/parallel/test-timers",
    "test/parallel/test-trace",
    "test/parallel/test-url",
    "test/parallel/test-urlpattern",
    "test/parallel/test-util",
    "test/parallel/test-v8",
    "test/parallel/test-webcrypto",
    "test/parallel/test-webstream",
    "test/parallel/test-webstreams",
    "test/parallel/test-whatwg",
    "test/parallel/test-windows",
];

fn unpromoted_parallel_discovery_fixture_paths(lane: NodeCompatLane) -> Vec<String> {
    let mut fixture_paths =
        node_compat_required_gap_paths_for_owner(lane, "node-compat/unpromoted-surface");
    fixture_paths.retain(|path| {
        path.starts_with("test/parallel/")
            && !UNPROMOTED_PARALLEL_DISCOVERY_EXCLUDED_PREFIXES
                .iter()
                .any(|prefix| path.starts_with(prefix))
    });
    fixture_paths.sort();
    fixture_paths.dedup();
    assert!(
        (50..=200).contains(&fixture_paths.len()),
        "unpromoted parallel discovery selector should stay reviewable; selected {} fixtures",
        fixture_paths.len()
    );
    fixture_paths
}

fn unpromoted_internal_helper_required_gap_path(test_path: &str) -> bool {
    test_path == "test/parallel/test-abortcontroller-internal.js"
        || test_path == "test/parallel/test-require-process.js"
        || test_path.starts_with("test/parallel/test-internal-")
}

fn unpromoted_internal_helper_required_gap_paths(lane: NodeCompatLane) -> Vec<String> {
    let fixture_paths = node_compat_posture_paths_for_selector(lane, |entry| {
        entry["support_denominator"] == "v8_isolate_required"
            && entry["owner"] == "node-compat/unpromoted-surface"
            && entry["test_path"]
                .as_str()
                .is_some_and(unpromoted_internal_helper_required_gap_path)
    });
    assert!(
        (15..=60).contains(&fixture_paths.len()),
        "unpromoted internal-helper selector should stay reviewable; selected {} fixtures",
        fixture_paths.len()
    );
    fixture_paths
}

fn resolve_seeded_fixture_context(
    lane_name: &str,
    test_relative_path: &str,
) -> std::result::Result<
    (
        NodeCompatLane,
        String,
        String,
        &'static NodeCompatBatchEntry,
        String,
    ),
    String,
> {
    let lane = node_compat_lane_from_manifest_name(lane_name)?;
    let resolved = node_compat_manifest_catalog::load_family_catalogs_from_disk();
    let mut matches = resolved
        .family_catalogs
        .iter()
        .flat_map(|family_catalog| {
            family_catalog
                .fixture_seeds
                .iter()
                .filter(move |fixture| fixture.id == test_relative_path)
                .map(move |fixture| (family_catalog, fixture))
        })
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return Err(format!(
            "seeded manifest fixture `{test_relative_path}` is not present in the carried family catalogs"
        ));
    }
    if matches.len() > 1 {
        let families = matches
            .iter()
            .map(|(family_catalog, _)| family_catalog.family.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "seeded manifest fixture `{test_relative_path}` is ambiguous across families: {families}"
        ));
    }
    let (family_catalog, fixture_seed) = matches.pop().expect("match should exist");
    let manifest_fixture_source_path = match fixture_seed.lane_sources.get(lane_name) {
        Some(source_path) => source_path.to_string(),
        None if matches!(lane, NodeCompatLane::Node26) => {
            // Node26 current-line oracle samples are tracked against the
            // vendored official corpus without promoting the whole lane into
            // the green subset manifest.
            let source_path = format!("{lane_name}/{test_relative_path}");
            let fixture_path = node_compat_fixture_root().join(&source_path);
            if !fixture_path.is_file() {
                return Err(format!(
                    "seeded manifest fixture `{test_relative_path}` has no `{lane_name}` source and default source `{source_path}` is missing"
                ));
            }
            source_path
        }
        None => {
            return Err(format!(
                "seeded manifest fixture `{test_relative_path}` has no `{lane_name}` source"
            ));
        }
    };
    let batch_entry = family_batch_entries(&family_catalog.family)?
        .iter()
        .find(|entry| entry.test_relative_path == test_relative_path)
        .ok_or_else(|| {
            format!(
                "seeded manifest fixture `{test_relative_path}` is missing from family batch `{}`",
                family_catalog.family
            )
        })?;
    let batch_fixture_source_path = match batch_entry.fixture_source_path_for_lane(lane) {
        Some(batch_fixture_source_path) => {
            if batch_fixture_source_path.as_ref() != manifest_fixture_source_path {
                return Err(format!(
                    "seeded manifest fixture `{test_relative_path}` mismatched `{lane_name}` source: manifest=`{manifest_fixture_source_path}` batch=`{batch_fixture_source_path}`"
                ));
            }
            batch_fixture_source_path.to_string()
        }
        None if matches!(lane, NodeCompatLane::Node26) => {
            let fixture_path = node_compat_fixture_root().join(&manifest_fixture_source_path);
            if !fixture_path.is_file() {
                return Err(format!(
                    "seeded manifest fixture `{test_relative_path}` references missing `{lane_name}` source `{manifest_fixture_source_path}`"
                ));
            }
            manifest_fixture_source_path
        }
        None => {
            return Err(format!(
                "seeded family batch `{}` fixture `{test_relative_path}` has no `{lane_name}` source",
                family_catalog.family
            ));
        }
    };
    Ok((
        lane,
        family_catalog.family.clone(),
        fixture_seed.slice.clone(),
        batch_entry,
        batch_fixture_source_path,
    ))
}

pub(super) fn observe_seeded_fixture_runtime_outcome(
    lane_name: &str,
    test_relative_path: &str,
) -> std::result::Result<NodeCompatSeededFixtureObservedOutcome, String> {
    let (lane, _family, _slice, batch_entry, fixture_source_path) =
        resolve_seeded_fixture_context(lane_name, test_relative_path)?;
    let snapshot = NodeCompatHostProcessSnapshot::capture();
    let execution = panic::catch_unwind(AssertUnwindSafe(|| {
        execute_manifested_node_compat_test(
            batch_entry.test_relative_path,
            &fixture_source_path,
            batch_entry.extra_files_for_lane(lane),
            matches!(lane, NodeCompatLane::Node24 | NodeCompatLane::Node26),
            Some(lane),
            None,
            None,
        )
    }));
    snapshot.restore();
    let outcome = match execution {
        Ok(Ok(outcome)) if outcome.skipped => NodeCompatSeededFixtureObservedOutcome {
            state: node_compat_manifest_report::NodeCompatObservedFixtureState::Skip,
            detail: None,
        },
        Ok(Ok(_outcome)) => NodeCompatSeededFixtureObservedOutcome {
            state: node_compat_manifest_report::NodeCompatObservedFixtureState::Pass,
            detail: None,
        },
        Ok(Err(error)) => NodeCompatSeededFixtureObservedOutcome {
            state: node_compat_manifest_report::NodeCompatObservedFixtureState::Fail,
            detail: Some(error),
        },
        Err(payload) => NodeCompatSeededFixtureObservedOutcome {
            state: node_compat_manifest_report::NodeCompatObservedFixtureState::Fail,
            detail: Some(format!("panic: {}", panic_payload_to_string(payload))),
        },
    };
    Ok(outcome)
}

pub(super) fn materialize_seeded_fixture_bundle_for_lane(
    lane_name: &str,
    test_relative_path: &str,
) -> std::result::Result<NodeCompatMaterializedSeededFixtureBundle, String> {
    let (lane, family, slice, batch_entry, fixture_source_path) =
        resolve_seeded_fixture_context(lane_name, test_relative_path)?;
    let test_source = read_node_compat_fixture_text(&fixture_source_path);
    let owned_extra_files: Vec<(String, Vec<u8>)> = batch_entry
        .extra_files_for_lane(lane)
        .iter()
        .map(|entry| {
            (
                entry.runtime_path.to_string(),
                read_node_compat_fixture_bytes(entry.fixture_source_path),
            )
        })
        .collect();
    let borrowed_extra_files: Vec<(&str, &[u8])> = owned_extra_files
        .iter()
        .map(|(runtime_path, source)| (runtime_path.as_str(), source.as_slice()))
        .collect();
    let resolved_prelude_behavior = default_prelude_behavior_for_fixture(test_relative_path);
    let resolved_postlude_behavior = default_postlude_behavior_for_fixture(test_relative_path);
    let mut startup_flags = fixture_requested_node_options(&test_source);
    let fixture_needs_pending_deprecation = startup_flags
        .iter()
        .any(|flag| flag == "--pending-deprecation");
    if matches!(
        resolved_prelude_behavior,
        Some(NodeCompatNamedPreludeBehavior::ExposeGc)
    ) {
        startup_flags.push("--expose-gc".to_string());
    }
    let effective_prelude = if fixture_needs_pending_deprecation {
        format!(
            "{PENDING_DEPRECATION_PRELUDE}\n{}",
            resolved_prelude_behavior
                .map(NodeCompatNamedPreludeBehavior::script)
                .unwrap_or("")
        )
    } else {
        resolved_prelude_behavior
            .map(NodeCompatNamedPreludeBehavior::script)
            .unwrap_or("")
            .to_string()
    };
    let (tempdir, bundle_path) = write_node_compat_bundle(NodeCompatBundleWriteOptions {
        test_relative_path,
        test_source: &test_source,
        extra_files: &borrowed_extra_files,
        capture_top_level_skip: matches!(lane, NodeCompatLane::Node24 | NodeCompatLane::Node26),
        lane: Some(lane),
        prelude_script: Some(effective_prelude.as_str()),
        postlude_script: resolved_postlude_behavior.map(NodeCompatNamedPostludeBehavior::script),
        node_options: &startup_flags,
        mode: NodeCompatBundleMode::Oracle,
    });
    Ok(NodeCompatMaterializedSeededFixtureBundle {
        family,
        slice,
        lane: lane_name.to_string(),
        test_relative_path: test_relative_path.to_string(),
        fixture_source_path: fixture_source_path.to_string(),
        bundle_path,
        tempdir,
        startup_flags,
    })
}

fn run_manifested_fixture_with_postlude(
    test_relative_path: &str,
    fixture_source_path: &str,
    extra_files: &[NodeCompatExtraFixtureEntry],
    postlude_script: &str,
) {
    execute_manifested_node_compat_test(
        test_relative_path,
        fixture_source_path,
        extra_files,
        false,
        None,
        None,
        Some(postlude_script),
    )
    .unwrap_or_else(|error| panic!("{error}"));
}

pub(super) fn default_prelude_behavior_for_fixture(
    test_relative_path: &str,
) -> Option<NodeCompatNamedPreludeBehavior> {
    match test_relative_path {
        "test/parallel/test-http2-compat-write-early-hints-invalid-argument-type.js"
        | "test/parallel/test-http2-compat-write-early-hints-invalid-argument-value.js" => {
            Some(NodeCompatNamedPreludeBehavior::ProcessExitSentinel)
        }
        "test/parallel/test-cluster-worker-events.js"
        | "test/parallel/test-cluster-worker-exit.js" => {
            Some(NodeCompatNamedPreludeBehavior::ProcessExitSentinel)
        }
        "test/parallel/test-inspector-open.js" | "test/parallel/test-inspector-enabled.js" => {
            Some(NodeCompatNamedPreludeBehavior::ProcessExitSentinel)
        }
        "test/parallel/test-readline-interface.js"
        | "test/parallel/test-readline-promises-interface.js" => {
            Some(NodeCompatNamedPreludeBehavior::InteractiveTerminal)
        }
        "test/parallel/test-dns-default-order-ipv4.js" => {
            Some(NodeCompatNamedPreludeBehavior::DnsResultOrderIpv4First)
        }
        "test/parallel/test-dns-default-order-ipv6.js" => {
            Some(NodeCompatNamedPreludeBehavior::DnsResultOrderIpv6First)
        }
        "test/parallel/test-dns-default-order-verbatim.js" => {
            Some(NodeCompatNamedPreludeBehavior::DnsResultOrderVerbatim)
        }
        "test/parallel/test-zlib-invalid-input-memory.js"
        | "test/parallel/test-zlib-unused-weak.js" => {
            Some(NodeCompatNamedPreludeBehavior::ExposeGc)
        }
        "test/parallel/test-process-load-env-file.js" => {
            Some(NodeCompatNamedPreludeBehavior::CheckoutRootCwd)
        }
        _ => None,
    }
}

fn node_compat_process_exit_code_from_error(error: &str) -> Option<i32> {
    let marker = format!("{NODE_COMPAT_PROCESS_EXIT_SENTINEL_MARKER}:");
    let (_, remainder) = error.split_once(&marker)?;
    let numeric_prefix: String = remainder
        .chars()
        .take_while(|character| character.is_ascii_digit() || *character == '-')
        .collect();
    if numeric_prefix.is_empty() {
        return None;
    }
    numeric_prefix.parse::<i32>().ok()
}

pub(super) fn default_postlude_behavior_for_fixture(
    test_relative_path: &str,
) -> Option<NodeCompatNamedPostludeBehavior> {
    match test_relative_path {
        "test/parallel/test-fs-open-no-close.js" | "test/parallel/test-fs-writefile-with-fd.js" => {
            Some(NodeCompatNamedPostludeBehavior::ProcessLifecycleDrain)
        }
        "test/parallel/test-trace-events-api.js"
        | "test/parallel/test-cluster-worker-init.js"
        | "test/parallel/test-cluster-worker-isdead.js"
        | "test/parallel/test-cluster-worker-isconnected.js"
        | "test/parallel/test-cluster-worker-disconnect.js"
        | "test/parallel/test-cluster-worker-forced-exit.js"
        | "test/parallel/test-cluster-worker-kill.js" => {
            Some(NodeCompatNamedPostludeBehavior::ForkChildSettle)
        }
        "test/parallel/test-worker-ref.js" => {
            Some(NodeCompatNamedPostludeBehavior::ProcessBeforeExitReentry)
        }
        _ => None,
    }
}

fn panic_payload_to_string(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

struct NodeCompatHostProcessSnapshot {
    cwd: Option<PathBuf>,
    env: Vec<(OsString, OsString)>,
}

impl NodeCompatHostProcessSnapshot {
    fn capture() -> Self {
        Self {
            cwd: std::env::current_dir().ok(),
            env: std::env::vars_os().collect(),
        }
    }

    fn restore(&self) {
        let current_keys = std::env::vars_os()
            .map(|(key, _)| key)
            .collect::<Vec<OsString>>();
        for key in current_keys {
            if self.env.iter().any(|(saved_key, _)| saved_key == &key) {
                continue;
            }
            unsafe {
                std::env::remove_var(&key);
            }
        }
        for (key, value) in &self.env {
            unsafe {
                std::env::set_var(key, value);
            }
        }
        if let Some(cwd) = &self.cwd {
            let _ = std::env::set_current_dir(cwd);
        }
    }
}

fn run_manifested_subset_for_lane(
    batch_name: &str,
    lane: NodeCompatLane,
    fixtures: &[NodeCompatBatchEntry],
) {
    run_manifested_subset_for_lane_excluding(batch_name, lane, fixtures, &[]);
}

fn run_manifested_subset_for_lane_excluding(
    batch_name: &str,
    lane: NodeCompatLane,
    fixtures: &[NodeCompatBatchEntry],
    excluded_test_relative_paths: &[&str],
) {
    let lane_name = node_compat_lane_name(lane);
    let mut passed = 0usize;
    let mut skipped = Vec::new();
    let mut excluded = Vec::new();
    let mut failures = Vec::new();

    for fixture in fixtures {
        if let Some(fixture_source_path) = fixture.fixture_source_path_for_lane(lane) {
            if excluded_test_relative_paths
                .iter()
                .any(|path| *path == fixture.test_relative_path)
            {
                excluded.push(fixture.test_relative_path);
                continue;
            }
            eprintln!(
                "node_compat {batch_name} {lane_name} -> {}",
                fixture.test_relative_path
            );
            let snapshot = NodeCompatHostProcessSnapshot::capture();
            let execution = panic::catch_unwind(AssertUnwindSafe(|| {
                execute_manifested_node_compat_test(
                    fixture.test_relative_path,
                    fixture_source_path.as_ref(),
                    fixture.extra_files_for_lane(lane),
                    matches!(lane, NodeCompatLane::Node24 | NodeCompatLane::Node26),
                    Some(lane),
                    None,
                    None,
                )
            }));
            snapshot.restore();
            match execution {
                Ok(Ok(outcome)) => {
                    if outcome.skipped {
                        skipped.push(fixture.test_relative_path);
                    } else {
                        passed += 1;
                    }
                }
                Ok(Err(error)) => failures.push(format!("{}: {error}", fixture.test_relative_path)),
                Err(payload) => failures.push(format!(
                    "{}: panic: {}",
                    fixture.test_relative_path,
                    panic_payload_to_string(payload)
                )),
            }
        }
    }

    eprintln!(
        "node_compat {batch_name} {lane_name} summary -> passed: {passed}, skipped: {}, excluded: {}, failed: {}",
        skipped.len(),
        excluded.len(),
        failures.len()
    );
    if !skipped.is_empty() {
        eprintln!(
            "node_compat {batch_name} {lane_name} skipped fixtures:\n{}",
            skipped.join("\n")
        );
    }
    if !excluded.is_empty() {
        eprintln!(
            "node_compat {batch_name} {lane_name} excluded fixtures:\n{}",
            excluded.join("\n")
        );
    }
    if !failures.is_empty() {
        panic!(
            "node_compat {batch_name} {lane_name} had {} failing fixtures:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}

fn run_node_compat_watchpoint_for_lane(
    test_relative_path: &str,
    fixture_source_path: &str,
    extra_files: &[NodeCompatExtraFixtureEntry],
    lane: NodeCompatLane,
) {
    let snapshot = NodeCompatHostProcessSnapshot::capture();
    let result = execute_manifested_node_compat_test(
        test_relative_path,
        fixture_source_path,
        extra_files,
        false,
        Some(lane),
        None,
        None,
    );
    snapshot.restore();
    result.unwrap_or_else(|error| panic!("{error}"));
}

fn run_node_compat_watchpoint(
    test_relative_path: &str,
    fixture_source_path: &str,
    extra_files: &[NodeCompatExtraFixtureEntry],
) {
    let snapshot = NodeCompatHostProcessSnapshot::capture();
    let result = execute_manifested_node_compat_test(
        test_relative_path,
        fixture_source_path,
        extra_files,
        false,
        None,
        None,
        None,
    );
    snapshot.restore();
    result.unwrap_or_else(|error| panic!("{error}"));
}

fn run_node_compat_watchpoint_batch(
    batch_name: &str,
    lane_name: &str,
    fixture_paths: &[&str],
    extra_files: &[NodeCompatExtraFixtureEntry],
) {
    let lane = match lane_name {
        "node20" => NodeCompatLane::Node20,
        "node22" => NodeCompatLane::Node22,
        "node24" => NodeCompatLane::Node24,
        "node26" => NodeCompatLane::Node26,
        other => panic!("unsupported node_compat watchpoint lane `{other}`"),
    };
    let mut failures = Vec::new();

    for test_relative_path in fixture_paths {
        eprintln!("node_compat {batch_name} {lane_name} -> {test_relative_path}");
        let fixture_source_path = format!("{lane_name}/{test_relative_path}");
        let snapshot = NodeCompatHostProcessSnapshot::capture();
        let execution = panic::catch_unwind(AssertUnwindSafe(|| {
            run_node_compat_watchpoint_for_lane(
                test_relative_path,
                &fixture_source_path,
                extra_files,
                lane,
            );
        }));
        snapshot.restore();
        if let Err(payload) = execution {
            failures.push(format!(
                "{test_relative_path}: {}",
                panic_payload_to_string(payload)
            ));
        }
    }

    if !failures.is_empty() {
        panic!(
            "node_compat {batch_name} {lane_name} had {} failing fixtures:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}

fn run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
    batch_name: &str,
    lane: NodeCompatLane,
    fixture_paths: &[String],
    extra_runtime_files: &[&str],
    extra_dirs: &[&str],
) {
    let lane_name = node_compat_lane_name(lane);
    let mut failures = Vec::new();
    let mut passed_paths = Vec::new();
    let mut skipped_paths = Vec::new();
    let mut failed_paths = Vec::new();

    eprintln!(
        "node_compat {batch_name} {lane_name} selected fixtures: {}",
        fixture_paths.len()
    );
    for test_relative_path in fixture_paths {
        eprintln!("node_compat {batch_name} {lane_name} -> {test_relative_path}");
        let fixture_source_path = format!("{lane_name}/{test_relative_path}");
        let snapshot = NodeCompatHostProcessSnapshot::capture();
        let execution = panic::catch_unwind(AssertUnwindSafe(|| {
            execute_manifested_node_compat_test_with_lane_extra_dirs(
                test_relative_path,
                &fixture_source_path,
                &[],
                extra_runtime_files,
                extra_dirs,
                lane,
            )
        }));
        snapshot.restore();
        match execution {
            Ok(Ok(outcome)) => {
                if outcome.skipped {
                    skipped_paths.push(test_relative_path.clone());
                } else {
                    passed_paths.push(test_relative_path.clone());
                }
            }
            Ok(Err(error)) => {
                failed_paths.push(test_relative_path.clone());
                failures.push(format!("{test_relative_path}: {error}"));
            }
            Err(payload) => {
                failed_paths.push(test_relative_path.clone());
                failures.push(format!(
                    "{test_relative_path}: {}",
                    panic_payload_to_string(payload)
                ));
            }
        }
    }

    eprintln!(
        "node_compat {batch_name} {lane_name} summary: selected={}, passed={}, skipped={}, failed={}",
        fixture_paths.len(),
        passed_paths.len(),
        skipped_paths.len(),
        failures.len()
    );
    if !skipped_paths.is_empty() {
        eprintln!(
            "node_compat {batch_name} {lane_name} skipped fixtures:\n{}",
            skipped_paths.join("\n")
        );
    }
    if let Some(summary_path) = write_node_compat_path_batch_summary(
        batch_name,
        lane_name,
        fixture_paths,
        &passed_paths,
        &skipped_paths,
        &failed_paths,
    ) {
        eprintln!(
            "node_compat {batch_name} {lane_name} summary artifact: {}",
            summary_path.display()
        );
    }

    if !failures.is_empty() {
        panic!(
            "node_compat {batch_name} {lane_name} had {} failing fixtures:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}

fn run_node_compat_watchpoint_entry_batch(
    batch_name: &str,
    lane: NodeCompatLane,
    fixtures: &[NodeCompatBatchEntry],
) {
    let lane_name = node_compat_lane_name(lane);
    let mut failures = Vec::new();

    for fixture in fixtures {
        if let Some(fixture_source_path) = fixture.fixture_source_path_for_lane(lane) {
            eprintln!(
                "node_compat {batch_name} {lane_name} -> {}",
                fixture.test_relative_path
            );
            let snapshot = NodeCompatHostProcessSnapshot::capture();
            let execution = panic::catch_unwind(AssertUnwindSafe(|| {
                run_node_compat_watchpoint_for_lane(
                    fixture.test_relative_path,
                    fixture_source_path.as_ref(),
                    fixture.extra_files_for_lane(lane),
                    lane,
                );
            }));
            snapshot.restore();
            if let Err(payload) = execution {
                failures.push(format!(
                    "{}: {}",
                    fixture.test_relative_path,
                    panic_payload_to_string(payload)
                ));
            }
        }
    }

    if !failures.is_empty() {
        panic!(
            "node_compat {batch_name} {lane_name} had {} failing fixtures:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}

pub(super) fn collect_seeded_slice_observed_result_records(
    family: &str,
    slice: &str,
) -> std::result::Result<
    Vec<node_compat_manifest_report::NodeCompatObservedLaneFixtureResultRecord>,
    String,
> {
    let resolved = node_compat_manifest_catalog::load_family_catalogs_from_disk();
    let plan = resolved.resolve_lane_execution_plan(family, slice)?;
    let batch_entries = family_batch_entries(family)?;
    let mut records = Vec::new();

    for lane_plan in plan.lanes {
        let lane = node_compat_lane_from_manifest_name(lane_plan.lane)?;
        let lane_name = node_compat_lane_name(lane);
        let mut passed = 0usize;
        let mut skipped = 0usize;
        let mut failed = 0usize;

        for resolved_fixture in lane_plan.fixtures {
            let batch_entry = batch_entries
                .iter()
                .find(|entry| entry.test_relative_path == resolved_fixture.fixture.id)
                .ok_or_else(|| {
                    format!(
                        "seeded manifest fixture `{}` is missing from family batch `{family}`",
                        resolved_fixture.fixture.id
                    )
                })?;
            let fixture_source_path = batch_entry
                .fixture_source_path_for_lane(lane)
                .ok_or_else(|| {
                    format!(
                        "seeded manifest fixture `{}` has no `{lane_name}` source in family batch `{family}`",
                        resolved_fixture.fixture.id
                    )
                })?;
            if fixture_source_path.as_ref() != resolved_fixture.fixture_source_path {
                return Err(format!(
                    "seeded manifest fixture `{}` mismatched `{lane_name}` source: manifest=`{}` batch=`{}`",
                    resolved_fixture.fixture.id,
                    resolved_fixture.fixture_source_path,
                    fixture_source_path
                ));
            }

            eprintln!(
                "node_compat report live {family}:{slice} {lane_name} -> {}",
                batch_entry.test_relative_path
            );
            let snapshot = NodeCompatHostProcessSnapshot::capture();
            let execution = panic::catch_unwind(AssertUnwindSafe(|| {
                execute_manifested_node_compat_test(
                    batch_entry.test_relative_path,
                    fixture_source_path.as_ref(),
                    batch_entry.extra_files_for_lane(lane),
                    matches!(lane, NodeCompatLane::Node24 | NodeCompatLane::Node26),
                    Some(lane),
                    None,
                    None,
                )
            }));
            snapshot.restore();
            let state = match execution {
                Ok(Ok(outcome)) if outcome.skipped => {
                    skipped += 1;
                    node_compat_manifest_report::NodeCompatObservedFixtureState::Skip
                }
                Ok(Ok(_outcome)) => {
                    passed += 1;
                    node_compat_manifest_report::NodeCompatObservedFixtureState::Pass
                }
                Ok(Err(error)) => {
                    failed += 1;
                    eprintln!(
                        "node_compat report live {family}:{slice} {lane_name} fixture {} failed: {error}",
                        batch_entry.test_relative_path
                    );
                    node_compat_manifest_report::NodeCompatObservedFixtureState::Fail
                }
                Err(payload) => {
                    failed += 1;
                    eprintln!(
                        "node_compat report live {family}:{slice} {lane_name} fixture {} panicked: {}",
                        batch_entry.test_relative_path,
                        panic_payload_to_string(payload)
                    );
                    node_compat_manifest_report::NodeCompatObservedFixtureState::Fail
                }
            };
            records.push(
                node_compat_manifest_report::NodeCompatObservedLaneFixtureResultRecord {
                    lane: lane_name.to_string(),
                    fixture_id: resolved_fixture.fixture.id.clone(),
                    state,
                },
            );
        }

        eprintln!(
            "node_compat report live {family}:{slice} {lane_name} summary -> passed: {passed}, skipped: {skipped}, failed: {failed}",
        );
    }

    Ok(records)
}

// Keep the large Node compatibility fixture catalogs and explicit watchpoint
// tests in include-owned slices so the runner/control plane in this file stays
// reviewable while preserving the historical libtest paths used by manifests.
include!("cases/networking_fixtures.rs");
include!("cases/loader_context_foundation.rs");
include!("cases/loader_context_zlib_crypto.rs");
include!("cases/loader_context_catalog.rs");
include!("cases/watchpoints_core.rs");
include!("cases/watchpoints_loader_and_tools.rs");
include!("cases/watchpoints_extended.rs");
