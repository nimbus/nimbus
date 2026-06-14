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
        || token == "--experimental-vm-modules"
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
// Flags: --no-warnings -C another --trace-warnings --pending-deprecation --preserve-symlinks --experimental-vm-modules
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
            "--experimental-vm-modules".to_string(),
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
            // These promise-counting fixtures enable a hook at top level and never
            // disable it, then assert EXACT before/after/init/resolve counts for the
            // promises THEY create. The sync-tick drain
            // (__nimbusProcessTicksAndRejections) runs the fixture's own reactions --
            // which must stay visible -- but ALSO creates its own bookkeeping
            // promises whose reactions fire before/after into the still-enabled
            // fixture hook (observed: triggerid before 1->5, all the spurious ones
            // born inside the drain). Wrapping the drain in
            // incPromiseHooksSuppressed routes every promise CREATED inside the
            // drain through emitInitNative's suppression gate so its
            // init/before/after/resolve stay invisible; the fixture's earlier-born
            // promises are not in suppressedPromises, so their reactions still fire.
            | "test/parallel/test-async-hooks-disable-during-promise.js"
            | "test/parallel/test-async-hooks-promise-triggerid.js"
            | "test/parallel/test-async-hooks-promise.js"
            // These diagnostics-channel fixtures are CommonJS main modules that
            // use top-level dynamic import. Loading them through the harness's
            // ESM `import("./fixture.js")` leaves `require` undefined before
            // the fixture can subscribe. A synchronous CJS require matches
            // `node main.js` and lets the fork's CJS translator wrap the
            // fixture-owned dynamic import for `module.import` tracing.
            | "test/parallel/test-diagnostics-channel-module-import-error.js"
            | "test/parallel/test-diagnostics-channel-module-import.js"
    )
}

fn should_quiesce_then_require_fixture(test_relative_path: &str) -> bool {
    matches!(
        test_relative_path,
        "test/parallel/test-repl-definecommand.js"
            | "test/parallel/test-repl-mode.js"
            | "test/parallel/test-repl-recoverable.js"
            | "test/parallel/test-repl-reset-event.js"
    )
}

// These promise-counting fixtures enable a hook at top level and assert EXACT
// per-promise counts for the promises THEY create -- triggerid asserts a single
// before/after pair on P1; promise.js asserts init == 2 and promiseResolve == 2.
// After their synchronous `require` runs (creating P0 = `Promise.resolve(x)` and
// P1 = its `.then(...)`), P1's reaction is a pending V8 microtask, and the
// fixture leaves its hook ENABLED past the module body.
//
// The leak has two distinct sources, fixed together here:
//
//   1. Top-level-await resumptions. The shared `infra_warmup_script` awaits a
//      2-turn setTimeout loop, which makes the module async; V8 then creates
//      native await-resumption promises whose reactions run -- at depth 0, hook
//      enabled -- during the Rust load-phase drive, surfacing spurious firings.
//      Fixed by gating infra_warmup OFF for these fixtures (they create no dgram
//      resource that needs warming), so the module is fully synchronous.
//
//   2. Native load-phase / invoke continuations. Even fully synchronous, after
//      `mod_evaluate` the Rust drive (`run_event_loop` in loading.rs) and the
//      harness invoke settle deno_core's own module/op-resolution promises.
//      Those are created at the Rust/V8 boundary with no JS `init` frame, yet
//      `setPromiseHooks` still fires `promiseInitHook` for them; born at depth 0
//      they are tracked and surfaced to the fixture hook as a spurious extra
//      init/resolve (promise.js: 2 -> 4 / 2 -> 3) or before/after (triggerid:
//      1 -> 3, empty native resolve stacks).
//
// Fix: drain the fixture's own pending microtasks synchronously in the module
// body, at suppression depth 0, immediately after the `require` -- P1's reaction
// runs HERE, its before/after/resolve fire with the correct ids and the promise
// it creates inside itself (triggerid's P2) is created unsuppressed so its `init`
// still counts -- then raise `incPromiseHooksSuppressed()` and LEAVE it raised.
// Every native continuation the load-phase drive and invoke create afterward is
// born at depth >= 1, enters `suppressedPromises`, and stays invisible to the
// fixture hook. (Suppression must be the LAST module-body step, not wrapped
// around the drive, or it would also withhold P2's `init`.) This mirrors a real
// `node main.js` run, where the test is the main module and its promises settle
// in the normal microtask checkpoint with no surrounding harness loop. It
// weakens no assertion; the fixture still observes exactly the
// before/after/init/resolve of the promises it owns. The trailing raise is
// emitted by the require-without-import arm; the infra_warmup gate keys off this
// same predicate.
fn should_drain_module_body_microtasks_for_fixture(test_relative_path: &str) -> bool {
    matches!(
        test_relative_path,
        "test/parallel/test-async-hooks-disable-during-promise.js"
            | "test/parallel/test-async-hooks-promise-triggerid.js"
            | "test/parallel/test-async-hooks-promise.js"
    )
}

// async_hooks fixtures that enable an init-counting hook at top level, leave it
// enabled past the module body, and assert an EXACT init count. Loading them via
// the harness's top-level `await import("./fixture")` makes V8 create an
// await-resumption promise AFTER the fixture has enabled its hook (the fixture
// body runs synchronously inside the dynamic import's CJS resolution, then the
// bundle module resumes once the import settles). That resumption promise's
// `init` is created with promise-hook suppression DOWN (it resolves at
// module-settle time, outside both `infra_warmup_script` suppression and the
// `__nimbusInvoke` suppression wrapper), so the still-enabled fixture hook counts
// it as one extra PROMISE init and the exact-count assertion trips (+1). A real
// `node main.js` run loads the test as the main module synchronously through the
// CJS loader and never creates an import() promise to resume, so there is no
// extra init. Load these fixtures the same way: a synchronous top-level
// `require("./fixture")` creates no promise at all, so nothing leaks into the
// fixture hook, and the fixture's own resources still init normally (suppression
// is NOT raised around this require). This weakens no assertion; it removes a
// harness-only promise that a real Node process never produces. The require is
// not hoisted, so it still runs AFTER `infra_warmup_script` -- preserving the
// "fixture evaluates after the infrastructure is warmed" ordering that the
// dynamic-import arm below was introduced to guarantee.
fn should_require_fixture_without_import_promise(test_relative_path: &str) -> bool {
    matches!(
        test_relative_path,
        "test/async-hooks/test-emit-init.js"
            | "test/async-hooks/test-track-promises-default.js"
            | "test/async-hooks/test-track-promises-true.js"
            | "test/async-hooks/test-disable-in-init.js"
            | "test/async-hooks/test-enable-in-init.js"
            | "test/parallel/test-async-hooks-top-level-clearimmediate.js"
            // Same root cause: the await-import resolution promise leaks one
            // extra PROMISE init into the fixture's hook. These count PROMISE
            // activities directly (as.length) rather than via mustCall, so the
            // leak shows up as N+1 promises instead of N+1 mustCalls, but the
            // synchronous-require load removes the same leaked promise.
            | "test/async-hooks/test-promise.js"
            | "test/async-hooks/test-promise.promise-before-init-hooks.js"
            | "test/async-hooks/test-unhandled-rejection-context.js"
            // Same leak shape: loaded via `await import(...)`, the dynamic-import
            // resolution promise (and its .catch continuation) fire a spurious
            // PROMISE init/before into the still-enabled fixture hook. The fixture
            // counts those as the "Unknown"-type activities of an unexpected hook2
            // (as2.length === 2 instead of 0 at onfirstImmediate). A synchronous
            // require has no import promise, so the leak disappears and the Sample
            // Test Log matches.
            | "test/async-hooks/test-enable-disable.js"
            // Load-path timing, not a driver bug: the fork's event-loop ordering
            // (Phase 2d nextTick before Phase 5 immediates/destroy) is already
            // Node-correct. Under `await import(...)`, the fixture's same-turn
            // nextTick that schedules the destroy lands in a different microtask
            // turn than the immediate that drains the destroy queue, so the
            // destroy callback is observed split across loop iterations. A
            // synchronous require keeps the nextTick and the destroy in the same
            // turn, matching Node's "destroy not blocked by a pending nextTick".
            | "test/async-hooks/test-destroy-not-blocked.js"
            // Same await-import leak shape, in test/parallel/. These promise-hook
            // fixtures enable a hook at top level and assert EXACT promise
            // before/after/init/resolve counts. Loading via `await import(...)`
            // creates the dynamic-import resolution promise chain AFTER the fixture
            // enabled its hook; those resolution/continuation promises fire spurious
            // before/after into the still-enabled hook (observed: triggerid before
            // 1->5, with two of the four spurious firings coming from the import
            // chain). A synchronous require has no import promise at all, so the
            // fixture body runs in place exactly like `node main.js` and only the
            // fixture's own promises remain. Paired with the sync-tick drain
            // suppression (should_suppress_sync_tick_promises_for_fixture), this
            // leaves the hook observing strictly the promises it created.
            | "test/parallel/test-async-hooks-disable-during-promise.js"
            | "test/parallel/test-async-hooks-promise-triggerid.js"
            | "test/parallel/test-async-hooks-promise.js"
    )
}

fn should_capture_top_level_import_error_for_fixture(test_relative_path: &str) -> bool {
    matches!(
        test_relative_path,
        "test/parallel/test-runner-run-files-undefined.mjs"
            | "test/parallel/test-runner-import-no-scheme.js"
    )
}

fn should_force_commonjs_package_scope_for_fixture(test_relative_path: &str) -> bool {
    matches!(
        test_relative_path,
        "test/parallel/test-diagnostics-channel-module-import-error.js"
            | "test/parallel/test-diagnostics-channel-module-import.js"
    )
}

fn should_load_fixture_with_commonjs_require(test_relative_path: &str) -> bool {
    should_require_fixture_without_import_promise(test_relative_path)
        || should_force_commonjs_package_scope_for_fixture(test_relative_path)
}

fn should_load_fixture_as_commonjs_main_module(test_relative_path: &str) -> bool {
    matches!(
        test_relative_path,
        "test/parallel/test-events-uncaught-exception-stack.js"
            | "test/parallel/test-domain-stack-empty-in-process-uncaughtexception.js"
    )
}

fn should_load_fixture_as_async_main_module(test_relative_path: &str) -> bool {
    matches!(
        test_relative_path,
        "test/es-module/test-esm-import-meta-main.mjs"
    )
}

fn should_use_async_hooks_infra_for_fixture(test_relative_path: &str) -> bool {
    test_relative_path.contains("async-hooks")
        || matches!(
            test_relative_path,
            // This stream fixture enables an init-counting async_hooks hook and
            // asserts the exact TickObject count. Keep harness nextTick drains
            // invisible just like the async-hooks/ fixture family.
            "test/parallel/test-stream-writable-samecb-singletick.js"
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
Object.defineProperty(globalThis, "gc", {
  value: __nimbusTestGc,
  configurable: true,
  enumerable: false,
  writable: true,
});
Object.defineProperty(globalThis.global, "gc", {
  value: __nimbusTestGc,
  configurable: true,
  enumerable: false,
  writable: true,
});"#
        }
        NodeCompatBundleMode::Oracle => "void 0;",
    };
    // async_hooks fixtures enable a user hook at top level and then assert the
    // exact init/before/after/destroy invocation counts for the resources they
    // create. The harness/runtime initializes its own infrastructure resources
    // (e.g. an internal connect/listening `nextTick`) lazily during the fixture's
    // `require` chain — created before the fixture calls `hooks.enable()`, so the
    // fixture hook never records their `init`, yet their `before` fires later in
    // the tail drain and trips "before without init". Pre-initialize that
    // infrastructure under async_hooks suppression before the fixture loads so
    // those ids enter `suppressedAsyncIds` and stay invisible to the fixture hook,
    // mirroring how a real Node process has its infrastructure warmed before the
    // test's hooks observe anything.
    let warmup_infra = should_use_async_hooks_infra_for_fixture(test_relative_path);
    let uses_prelude = prelude_script.is_some();
    let capture_import_error = capture_top_level_skip
        || should_capture_top_level_import_error_for_fixture(test_relative_path)
        || matches!(
            default_prelude_behavior_for_fixture(test_relative_path),
            Some(
                NodeCompatNamedPreludeBehavior::ProcessExitSentinel
                    | NodeCompatNamedPreludeBehavior::ProcessExitAlwaysSentinel
            )
        );
    let import_preamble = if should_quiesce_then_require_fixture(test_relative_path) {
        String::new()
    } else if matches!(mode, NodeCompatBundleMode::Runtime)
        && should_load_fixture_as_async_main_module(test_relative_path)
    {
        String::new()
    } else if should_load_fixture_as_commonjs_main_module(test_relative_path) {
        // This fixture intentionally throws from the top-level CommonJS main
        // script and expects `process.on("uncaughtException")` to observe it.
        // Loading it through an ESM `import` or nested `createRequire(...)`
        // makes the throw look like a module-load failure instead of a main
        // script fatal exception. Use Node's own `Module._load(..., isMain)`,
        // matching Deno's existing CJS-main fatal-exception path.
        let main_module_load = format!(
            r#"createRequire(import.meta.url)("module")._load(
  new URL("./{test_relative_path}", import.meta.url).pathname,
  null,
  true,
);"#
        );
        if capture_import_error {
            format!(
                r#"let __nimbusImportError = null;
try {{
  {main_module_load}
}} catch (error) {{
  __nimbusImportError = error;
}}"#
            )
        } else {
            main_module_load
        }
    } else if should_load_fixture_with_commonjs_require(test_relative_path) {
        // Load the fixture with a synchronous CommonJS require instead of
        // `await import(...)`. The fixture body runs in place (just like a real
        // `node main.js` main-module load), the fixture's top-level hook is
        // enabled, and its own resources init normally -- but NO import() promise
        // is created, so there is no await-resumption promise to leak a spurious
        // PROMISE `init` into the still-enabled fixture hook. This runs AFTER
        // `infra_warmup_script` (so the runtime infrastructure is already warmed
        // and suppressed) and is the LAST statement of the module body, so the
        // body completes synchronously with no trailing promise of its own.
        //
        // These fixtures still flow through the `capture_import_error`
        // `invoke_import_guard` branch (they can emit a top-level skip), which
        // reads `__nimbusImportError` from module scope. So when capture is on we
        // must declare it and capture the require the same way the dynamic-import
        // capture arm below does -- just synchronously, with no import() promise.
        // When capture is off we let the require throw naturally (swallowing it
        // into an unread `__nimbusImportError` would hide a real failure).
        if capture_import_error {
            format!(
                r#"let __nimbusImportError = null;
try {{
  createRequire(import.meta.url)("./{test_relative_path}");
}} catch (error) {{
  __nimbusImportError = error;
}}"#
            )
        } else if should_drain_module_body_microtasks_for_fixture(test_relative_path) {
            // Run the fixture's pending promise reactions (its P1 `.then`) HERE in
            // the module body, at suppression depth 0, so their before/after/
            // resolve reach the still-enabled fixture hook with the correct ids,
            // and the promise the reaction creates inside itself still inits. Run
            // the checkpoint twice so a microtask that schedules another microtask
            // still settles inside the module body. Then raise async_hooks
            // suppression and LEAVE it raised for the rest of the process.
            //
            // Why the trailing, never-decremented raise: after this point the
            // fixture's observable promise work is complete, but the Rust
            // load-phase drive (`run_event_loop` after `mod_evaluate`) and the
            // harness invoke still create deno_core's own native continuation
            // promises (module/op-resolution settlement). Those have no JS `init`
            // frame, yet `setPromiseHooks` fires `promiseInitHook` for them; born
            // at depth 0 they would be tracked and surfaced to the fixture hook as
            // a spurious extra init/resolve (test-async-hooks-promise.js) or
            // before/after (test-async-hooks-promise-triggerid.js). Raising
            // suppression at the tail of the (now fully synchronous -- see
            // infra_warmup gate) module body means every such continuation is born
            // at depth >= 1, enters `suppressedPromises`, and stays invisible. We
            // never decrement: the process is torn down right after the fixture's
            // exit-time mustCall tally, so an unbalanced counter is harmless, and
            // any decrement would re-open the leak window for the next native
            // continuation. See should_drain_module_body_microtasks_for_fixture.
            format!(
                r#"createRequire(import.meta.url)("./{test_relative_path}");
{{
  const __nimbusBodyDrainCore = globalThis.Deno?.core;
  if (typeof __nimbusBodyDrainCore?.runMicrotasks === "function") {{
    __nimbusBodyDrainCore.runMicrotasks();
    __nimbusBodyDrainCore.runMicrotasks();
  }}
  if (typeof __nimbusBodyDrainCore?.incPromiseHooksSuppressed === "function") {{
    __nimbusBodyDrainCore.incPromiseHooksSuppressed();
  }}
}}"#
            )
        } else {
            format!(r#"createRequire(import.meta.url)("./{test_relative_path}");"#)
        }
    } else if capture_import_error {
        format!(
            r#"let __nimbusImportError = null;
try {{
  await import("./{test_relative_path}");
}} catch (error) {{
  __nimbusImportError = error;
}}"#
        )
    } else if uses_prelude || warmup_infra {
        // Force a dynamic import so the module body (the infrastructure warm-up)
        // runs BEFORE the fixture evaluates. A static import is hoisted ahead of
        // the body, which would run the fixture first and defeat the warm-up.
        format!(r#"await import("./{test_relative_path}");"#)
    } else {
        format!(r#"import "./{test_relative_path}";"#)
    };
    let infra_warmup_script =
        if warmup_infra && !should_drain_module_body_microtasks_for_fixture(test_relative_path) {
            // Pump real event-loop turns under async_hooks suppression BEFORE the
            // fixture module loads. A runtime-internal async resource created during
            // bootstrap (a deno_node dgram socket whose native close-completion is
            // still pending) drains its `socketCloseNT` tick on the first
            // event-loop turn. Without this warm-up that turn happens after the
            // fixture has enabled its hook but before the fixture recorded the
            // resource's `init`, so the resource's later `before` (fired in the
            // tail drain) trips init-hooks' "before without init" guard. Draining
            // it here, while `core.incPromiseHooksSuppressed()` is raised, routes
            // its `init` through emitInitNative's suppression gate: the id enters
            // `suppressedAsyncIds`, so its before/after/destroy stay invisible to
            // the fixture's hook. This pumps the event loop (a real `setTimeout`
            // turn that `run_event_loop` drives to idle), not just the nextTick
            // queue, because the pending completion is a native op, not a JS tick.
            // It weakens no fixture assertion — the fixture still observes every
            // resource it owns, created strictly after this point.
            r#"{
  const __nimbusWarmCore = globalThis.Deno?.core;
  if (typeof __nimbusWarmCore?.incPromiseHooksSuppressed === "function") {
    __nimbusWarmCore.incPromiseHooksSuppressed();
    try {
      for (let __nimbusWarmTurn = 0; __nimbusWarmTurn < 2; __nimbusWarmTurn++) {
        await new Promise((resolve) => setTimeout(resolve, 0));
      }
    } finally {
      __nimbusWarmCore.decPromiseHooksSuppressed();
    }
  }
}
"#
        } else {
            ""
        };
    let invoke_import_guard = if matches!(mode, NodeCompatBundleMode::Runtime)
        && should_load_fixture_as_async_main_module(test_relative_path)
    {
        String::new()
    } else if should_quiesce_then_require_fixture(test_relative_path) && capture_import_error {
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
      return __nimbusNodeCompatResult(true);
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
      return __nimbusNodeCompatResult(true);
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
                r#"Object.defineProperty(globalThis, "__nimbusNodeCompatLane", {{
  value: {:?},
  configurable: true,
  enumerable: false,
  writable: true,
}});"#,
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
    const __nimbusDrainCore = globalThis.Deno?.core;
    const __nimbusDrainSuppress =
      typeof __nimbusDrainCore?.incPromiseHooksSuppressed === "function";
    if (__nimbusDrainSuppress) {
      __nimbusDrainCore.incPromiseHooksSuppressed();
    }
    try {
      globalThis.__nimbusProcessTicksAndRejections();
    } finally {
      if (__nimbusDrainSuppress) {
        __nimbusDrainCore.decPromiseHooksSuppressed();
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
    } else if warmup_infra {
        // async-hooks fixtures enable a user hook and then assert the EXACT
        // init/before/after/destroy counts for the resources THEY create. This
        // trailing drain is pure harness machinery: it flushes residual
        // nextTicks/microtasks so the fixture's exit-time assertions observe a
        // settled loop. The fixture's own async work already ran to idle during
        // load_bundle's run_event_loop passes (loading.rs), so nothing the
        // fixture owns is created inside this drain. Without suppression the
        // drain's own `Promise.resolve()` / `queueMicrotask` / `nextTick`
        // resources are init'd while the fixture hook is still enabled, so the
        // fixture counts them (e.g. an extra 'PROMISE' or 'Microtask') and its
        // strict-equal assertions trip. Raising `core.incPromiseHooksSuppressed`
        // routes every resource CREATED inside the drain through emitInitNative's
        // suppression gate: its id enters `suppressedAsyncIds`, its `init` is
        // withheld, and its before/after/destroy are skipped — so the harness
        // drain stays invisible to the fixture hook. Resources the fixture
        // created earlier are NOT in `suppressedAsyncIds`, so their pending
        // before/after/destroy still fire here. This weakens no assertion: the
        // fixture still observes exactly the resources it owns, never the
        // harness's bookkeeping.
        r#"  {
    const __nimbusDrainCore = globalThis.Deno?.core;
    const __nimbusDrainSuppress =
      typeof __nimbusDrainCore?.incPromiseHooksSuppressed === "function";
    if (__nimbusDrainSuppress) {
      __nimbusDrainCore.incPromiseHooksSuppressed();
    }
    try {
      if (typeof globalThis.process?.nextTick === "function") {
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
      // Fire async_hooks destroys for AsyncResources the fixture dropped and
      // garbage-collected (e.g. test/common/gc.js onGC -> ongc). The fixture's
      // in-body global.gc() makes the tracker collectable, but V8 defers
      // FinalizationRegistry cleanup to a later event-loop turn, so the destroy
      // otherwise fires only during harness teardown -- after the exit-time
      // mustCall check has already read ongc as uncalled. Running gc() here (the
      // fixture body has unwound, so no stack slot pins the tracker) lets
      // force_gc's foreground-task pump run that cleanup, which queues the
      // deferred destroy; __nimbusDrainImmediates() (captured at bootstrap from
      // Deno.core.runImmediates) then drains it via drainDestroyAsyncIds so the
      // destroy hook fires before the check. This sits inside the promise-hook
      // suppression window and creates no new hooked async resource -- gc()
      // makes none, and runImmediates only runs the already-queued,
      // async_hooks-suppressed destroy-drain immediate -- so the fixture's
      // strict init/before/after/destroy counts are untouched.
      if (
        typeof globalThis.gc === "function" &&
        typeof globalThis.__nimbusDrainImmediates === "function"
      ) {
        for (let __nimbusGcPass = 0; __nimbusGcPass < 2; __nimbusGcPass++) {
          globalThis.gc();
          globalThis.__nimbusDrainImmediates();
        }
      }
    } finally {
      if (__nimbusDrainSuppress) {
        __nimbusDrainCore.decPromiseHooksSuppressed();
      }
    }
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
        // This runs inside the invoke catch block, where the try-scoped `const
        // common` is unreachable. `require` is declared before the try, so it
        // stays in scope here; re-require common fresh to flush child processes.
        r#"      await require("./test/common/index.js").__nimbusFlushChildProcesses?.();
"#
    };
    let preloaded_common_for_assert_script = if test_relative_path.starts_with("test/module-hooks/")
        || test_relative_path.starts_with("test/parallel/test-diagnostics-channel-module-")
    {
        // module.registerHooks fixtures should not observe harness bookkeeping
        // requires after their hooks are active. The module diagnostics-channel
        // fixtures similarly subscribe to module.require at top level and assert
        // exactly the fixture-owned require/import events.
        r#"const __nimbusNodeCompatPreloadedCommonForAssert =
  createRequire(import.meta.url)("./test/common/index.js");
"#
    } else {
        "const __nimbusNodeCompatPreloadedCommonForAssert = null;"
    };
    // async-hooks fixtures enable their user hook at top level and assert the
    // EXACT init counts for the resources THEY create. By the time the Rust
    // driver calls `globalThis.__nimbusInvoke(...)`, the fixture's own event
    // loop has already drained to idle inside load_bundle, so every resource
    // born during the invoke phase (the invoke promise itself, the async drain,
    // the child-process flush, the postlude) is pure harness machinery. Wrap the
    // whole invocation in `core.incPromiseHooksSuppressed()` so those harness
    // resources route through emitInitNative's suppression gate and stay
    // invisible to the fixture hook. The wrapper is a SYNC function: it raises
    // suppression BEFORE invoking the original async `__nimbusInvoke`, so even
    // that async function's own promise is born suppressed. Suppression only
    // withholds `init`; the before/after/destroy of resources the fixture
    // created earlier still fire, so no fixture-owned assertion is weakened.
    let invoke_suppression_wrapper = if warmup_infra {
        r#"
{
  const __nimbusInvokeOriginal = globalThis.__nimbusInvoke;
  globalThis.__nimbusInvoke = function (...__nimbusInvokeArgs) {
    const __nimbusSuppCore = globalThis.Deno?.core;
    const __nimbusSupp =
      typeof __nimbusSuppCore?.incPromiseHooksSuppressed === "function";
    if (__nimbusSupp) {
      __nimbusSuppCore.incPromiseHooksSuppressed();
    }
    let __nimbusInvokeOutcome;
    try {
      __nimbusInvokeOutcome = __nimbusInvokeOriginal.apply(this, __nimbusInvokeArgs);
    } catch (__nimbusSyncError) {
      if (__nimbusSupp) {
        __nimbusSuppCore.decPromiseHooksSuppressed();
      }
      throw __nimbusSyncError;
    }
    return Promise.resolve(__nimbusInvokeOutcome).finally(() => {
      if (__nimbusSupp) {
        __nimbusSuppCore.decPromiseHooksSuppressed();
      }
    });
  };
}
"#
    } else {
        ""
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
  globalThis.Deno?.core
    ?.loadExtScript("ext:deno_node/internal_binding/node_options.ts")
    ?.setOptionSourceExecArgv?.(__nimbusNodeCompatExecArgv);
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
{infra_warmup_script}
{lane_prelude}
{preloaded_common_for_assert_script}
{prelude_script}
if (globalThis.__nimbusNodeCompatLane === "node22") {{
  const __nimbusAssertRequire = createRequire(import.meta.url);
  for (const __nimbusAssertSpecifier of ["assert", "node:assert"]) {{
    const __nimbusAssert = __nimbusAssertRequire(__nimbusAssertSpecifier);
    if (typeof __nimbusAssert === "function") {{
      Object.defineProperty(__nimbusAssert, "__nimbusRejectsStackReceiverName", {{
        configurable: true,
        value: "Function",
      }});
    }}
  }}
}}
{import_preamble}

const __nimbusNodeCompatResult = (skipped) => Object.assign(
  Object.create(null),
  {{
    ok: true,
    skipped,
    testPath: "{test_relative_path}",
  }},
);

{invoke_signature}
  let __nimbusInvokeStep = "create require";
  const require = createRequire(import.meta.url);
  if (globalThis.__nimbusNodeCompatLane === "node22") {{
    for (const __nimbusAssertSpecifier of ["assert", "node:assert"]) {{
      const __nimbusAssert = require(__nimbusAssertSpecifier);
      if (typeof __nimbusAssert === "function") {{
        Object.defineProperty(__nimbusAssert, "__nimbusRejectsStackReceiverName", {{
          configurable: true,
          value: "Function",
        }});
      }}
    }}
  }}
  try {{
    __nimbusInvokeStep = "import guard";
{invoke_import_guard}
    __nimbusInvokeStep = "require common";
    const common = __nimbusNodeCompatPreloadedCommonForAssert ??
      require("./test/common/index.js");
    __nimbusInvokeStep = "async drain";
{async_drain_script}
    __nimbusInvokeStep = "postlude";
{postlude_script}
    __nimbusInvokeStep = "child process flush";
{child_process_flush_script}
    __nimbusInvokeStep = "common assert";
    common.__nimbusAssert?.();
    globalThis.__nimbusNodeCompatInvocationFinalized = true;
    return __nimbusNodeCompatResult(false);
  }} catch (__nimbusInvokeError) {{
    if (__nimbusInvokeError === undefined) {{
      throw new Error(`Nimbus node_compat harness rejected with undefined during ${{__nimbusInvokeStep}}`);
    }}
    const __nimbusExitCode =
      __nimbusProcessExitCodeFromError(__nimbusInvokeError);
    if (__nimbusExitCode === 0) {{
{process_exit_cleanup_script}
      globalThis.__nimbusNodeCompatInvocationFinalized = true;
      return __nimbusNodeCompatResult(false);
    }}
    throw __nimbusInvokeError;
  }}
}};
{invoke_suppression_wrapper}
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
    if should_force_commonjs_package_scope_for_fixture(test_relative_path) {
        let package_json_path = test_path
            .parent()
            .expect("test parent should resolve")
            .join("package.json");
        std::fs::write(&package_json_path, r#"{"type":"commonjs"}"#)
            .expect("fixture package scope should write");
    }
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

async fn invoke_node_compat_fixture_with_async_main_module(
    runtime: &NimbusRuntime,
    harness_bundle: &RuntimeBundle,
    bundle_path: &Path,
    test_relative_path: &str,
    request: &InvocationRequest,
) -> crate::error::Result<serde_json::Value> {
    let snapshot = runtime.bootstrap_snapshot()?;
    let reusable_runtime = crate::backends::v8::ReusableV8Runtime::fresh(
        runtime.create_runtime_from_snapshot(harness_bundle, snapshot)?,
        crate::backends::v8::V8RuntimeConstructionMode::StartupSnapshot,
    );
    let permit =
        crate::executor::SharedInvocationPermit::new(runtime.policy(), None, None, true, None);
    let watchdog = crate::watchdog::WatchdogTimer::new();
    let mut driver = runtime.prepare_runtime_invocation_driver(
        reusable_runtime,
        watchdog,
        None,
        permit,
        false,
    )?;

    let result = async {
        runtime
            .load_bundle_with_trace(
                &mut driver.runtime,
                harness_bundle,
                driver.construction_mode,
                None,
                Some(request),
            )
            .await?;

        let fixture_path = bundle_path
            .parent()
            .expect("node_compat bundle should have a parent")
            .join(test_relative_path);
        let fixture_specifier =
            deno_core::ModuleSpecifier::from_file_path(&fixture_path).map_err(|_| {
                crate::error::NimbusRuntimeError::Contract(format!(
                    "node_compat fixture path `{}` should become a file URL",
                    fixture_path.display()
                ))
            })?;
        let module_id = driver
            .runtime
            .load_main_es_module(&fixture_specifier)
            .await
            .map_err(|error| crate::error::NimbusRuntimeError::JavaScript(error.to_string()))?;
        let evaluation = driver.runtime.mod_evaluate(module_id);
        driver
            .runtime
            .run_event_loop(Default::default())
            .await
            .map_err(|error| crate::error::NimbusRuntimeError::JavaScript(error.to_string()))?;
        evaluation
            .await
            .map_err(|error| crate::error::NimbusRuntimeError::JavaScript(error.to_string()))?;
        tokio::task::yield_now().await;
        driver
            .runtime
            .run_event_loop(Default::default())
            .await
            .map_err(|error| crate::error::NimbusRuntimeError::JavaScript(error.to_string()))?;

        runtime
            .invoke_loaded_bundle_with_trace(
                &mut driver.runtime,
                request,
                Some(harness_bundle),
                driver.construction_mode,
                None,
            )
            .await
    }
    .await;
    driver.finalize(result).await
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
    let load_fixture_as_async_main_module =
        should_load_fixture_as_async_main_module(test_relative_path);
    let bundle = if load_fixture_as_async_main_module {
        RuntimeBundle::with_side_entrypoint(&bundle_path)
    } else {
        RuntimeBundle::new(&bundle_path)
    };

    let started_at = Instant::now();
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(async {
            tokio::time::timeout(wall_clock_timeout, async {
                if load_fixture_as_async_main_module {
                    invoke_node_compat_fixture_with_async_main_module(
                        &runtime,
                        &bundle,
                        &bundle_path,
                        test_relative_path,
                        &request,
                    )
                    .await
                } else {
                    runtime.invoke_bundle(&bundle, &request).await
                }
            })
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
                Some(
                    NodeCompatNamedPreludeBehavior::ProcessExitSentinel
                        | NodeCompatNamedPreludeBehavior::ProcessExitAlwaysSentinel
                )
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

#[test]
fn node_compat_diagnostics_module_import_fixture_uses_commonjs_entry() {
    let (_tempdir, bundle_path) = write_node_compat_bundle(NodeCompatBundleWriteOptions {
        test_relative_path: "test/parallel/test-diagnostics-channel-module-import.js",
        test_source: "const common = require('../common'); import('node:fs');",
        extra_files: &[],
        capture_top_level_skip: false,
        lane: None,
        prelude_script: None,
        postlude_script: None,
        node_options: &[],
        mode: NodeCompatBundleMode::Runtime,
    });
    let bundle_source = std::fs::read_to_string(&bundle_path).expect("bundle should read");
    assert!(
        bundle_source.contains(
            r#"createRequire(import.meta.url)("./test/parallel/test-diagnostics-channel-module-import.js");"#
        ),
        "diagnostics module-import fixture should load through createRequire:\n{bundle_source}"
    );
    let package_json_path = bundle_path
        .parent()
        .expect("bundle parent should exist")
        .join("test/parallel/package.json");
    let package_json =
        std::fs::read_to_string(package_json_path).expect("fixture package scope should read");
    assert_eq!(package_json, r#"{"type":"commonjs"}"#);
}

#[test]
fn node_compat_uncaught_exception_stack_fixture_uses_commonjs_main_entry() {
    let (_tempdir, bundle_path) = write_node_compat_bundle(NodeCompatBundleWriteOptions {
        test_relative_path: "test/parallel/test-events-uncaught-exception-stack.js",
        test_source: "process.on('uncaughtException', () => {}); throw new Error();",
        extra_files: &[],
        capture_top_level_skip: true,
        lane: None,
        prelude_script: None,
        postlude_script: None,
        node_options: &[],
        mode: NodeCompatBundleMode::Runtime,
    });
    let bundle_source = std::fs::read_to_string(&bundle_path).expect("bundle should read");
    assert!(
        bundle_source.contains(r#"createRequire(import.meta.url)("module")._load("#),
        "uncaught-exception main fixture should load through Module._load:\n{bundle_source}"
    );
    assert!(
        bundle_source.contains(
            r#"new URL("./test/parallel/test-events-uncaught-exception-stack.js", import.meta.url).pathname"#
        ),
        "uncaught-exception main fixture should resolve the real fixture path:\n{bundle_source}"
    );
    assert!(
        !bundle_source.contains(
            r#"await import("./test/parallel/test-events-uncaught-exception-stack.js");"#
        ),
        "uncaught-exception main fixture should not load through ESM import:\n{bundle_source}"
    );
    assert!(
        !bundle_source
            .contains(r#"createRequire(import.meta.url)("./test/parallel/test-events-uncaught-exception-stack.js");"#),
        "uncaught-exception main fixture should not load as a nested require:\n{bundle_source}"
    );
}

#[test]
fn node_compat_domain_uncaught_exception_stack_fixture_uses_commonjs_main_entry() {
    let (_tempdir, bundle_path) = write_node_compat_bundle(NodeCompatBundleWriteOptions {
        test_relative_path: "test/parallel/test-domain-stack-empty-in-process-uncaughtexception.js",
        test_source: "require('domain').create().run(() => { throw new Error('boom'); });",
        extra_files: &[],
        capture_top_level_skip: true,
        lane: None,
        prelude_script: None,
        postlude_script: None,
        node_options: &[],
        mode: NodeCompatBundleMode::Runtime,
    });
    let bundle_source = std::fs::read_to_string(&bundle_path).expect("bundle should read");
    assert!(
        bundle_source.contains(r#"createRequire(import.meta.url)("module")._load("#),
        "domain uncaught-exception fixture should load through Module._load:\n{bundle_source}"
    );
    assert!(
        bundle_source.contains(
            r#"new URL("./test/parallel/test-domain-stack-empty-in-process-uncaughtexception.js", import.meta.url).pathname"#
        ),
        "domain uncaught-exception fixture should resolve the real fixture path:\n{bundle_source}"
    );
    assert!(
        !bundle_source.contains(
            r#"await import("./test/parallel/test-domain-stack-empty-in-process-uncaughtexception.js");"#
        ),
        "domain uncaught-exception fixture should not load through ESM import:\n{bundle_source}"
    );
    assert!(
        !bundle_source.contains(
            r#"createRequire(import.meta.url)("./test/parallel/test-domain-stack-empty-in-process-uncaughtexception.js");"#
        ),
        "domain uncaught-exception fixture should not load as a nested require:\n{bundle_source}"
    );
}

#[test]
fn node_compat_import_meta_main_fixture_uses_runtime_main_entry() {
    let (_tempdir, bundle_path) = write_node_compat_bundle(NodeCompatBundleWriteOptions {
        test_relative_path: "test/es-module/test-esm-import-meta-main.mjs",
        test_source: "assert.strictEqual(import.meta.main, true);",
        extra_files: &[],
        capture_top_level_skip: true,
        lane: None,
        prelude_script: None,
        postlude_script: None,
        node_options: &[],
        mode: NodeCompatBundleMode::Runtime,
    });
    let bundle_source = std::fs::read_to_string(&bundle_path).expect("bundle should read");
    assert!(
        !bundle_source.contains(r#"import "./test/es-module/test-esm-import-meta-main.mjs";"#),
        "runtime bundle should leave import-meta-main fixture for Rust main-module load:\n{bundle_source}"
    );
    assert!(
        !bundle_source
            .contains(r#"await import("./test/es-module/test-esm-import-meta-main.mjs");"#),
        "runtime bundle should not side-import import-meta-main fixture:\n{bundle_source}"
    );
}

#[test]
fn node_compat_import_meta_main_oracle_bundle_still_imports_fixture() {
    let (_tempdir, bundle_path) = write_node_compat_bundle(NodeCompatBundleWriteOptions {
        test_relative_path: "test/es-module/test-esm-import-meta-main.mjs",
        test_source: "assert.strictEqual(import.meta.main, true);",
        extra_files: &[],
        capture_top_level_skip: true,
        lane: None,
        prelude_script: None,
        postlude_script: None,
        node_options: &[],
        mode: NodeCompatBundleMode::Oracle,
    });
    let bundle_source = std::fs::read_to_string(&bundle_path).expect("bundle should read");
    assert!(
        bundle_source
            .contains(r#"await import("./test/es-module/test-esm-import-meta-main.mjs");"#),
        "oracle bundle should remain runnable without Nimbus's Rust harness path:\n{bundle_source}"
    );
}

fn node_compat_fixture_requires_runtime_self_exec(test_relative_path: &str) -> bool {
    test_relative_path.starts_with("test/parallel/test-runner-")
        || test_relative_path.starts_with("test/parallel/test-process-")
        || test_relative_path.starts_with("test/parallel/test-url-parse-")
        || test_relative_path.starts_with("test/wasi/test-wasi-")
        || matches!(
            test_relative_path,
            "test/parallel/test-error-prepare-stack-trace.js"
                | "test/parallel/test-process-finalization.mjs"
                | "test/parallel/test-sqlite.js"
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
    if test_relative_path == "test/parallel/test-webcrypto-wrap-unwrap.js" {
        // This official fixture runs a broad WebCrypto wrap/unwrap matrix; the
        // Node 22 version is larger, and even the Node 24 version sits close to
        // the default budget on loaded hosts. Keep a finite fixture-only
        // evidence budget so the official assertions can complete.
        limits.execution_timeout = Duration::from_secs(120);
    }
    if test_relative_path == "test/parallel/test-vm-access-process-env.js" {
        // The fixture asserts a vm context can read `process.env.PATH`, which
        // is present in any real Node process environment. Grant PATH read to
        // this compat fixture lane only (production application presets still
        // omit it) so the assertion exercises real vm/env wiring rather than
        // failing on an empty allowlist.
        if !limits.grants.env_read.iter().any(|name| name == "PATH") {
            limits.grants.env_read.push("PATH".to_string());
        }
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

    let source_map_respawn_limits = runtime_limits_for_node_compat_fixture(
        "test/parallel/test-error-prepare-stack-trace.js",
        Some(NodeCompatLane::Node24),
    );
    assert_eq!(
        source_map_respawn_limits.compatibility_target,
        RuntimeCompatibilityTarget::Node24
    );
    assert_eq!(
        source_map_respawn_limits.grants.run,
        vec!["$runtime_self_exec"],
        "prepareStackTrace source-map verification respawns only the compat runtime target",
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

    let node22_webcrypto_wrap_unwrap_limits = runtime_limits_for_node_compat_fixture(
        "test/parallel/test-webcrypto-wrap-unwrap.js",
        Some(NodeCompatLane::Node22),
    );
    assert_eq!(
        node22_webcrypto_wrap_unwrap_limits.execution_timeout,
        Duration::from_secs(120),
        "the broad WebCrypto wrap/unwrap matrix gets a finite slow-fixture budget in each lane",
    );
    assert_eq!(
        node_compat_fixture_wall_clock_timeout(&node22_webcrypto_wrap_unwrap_limits),
        Duration::from_secs(125),
    );

    let node24_webcrypto_wrap_unwrap_limits = runtime_limits_for_node_compat_fixture(
        "test/parallel/test-webcrypto-wrap-unwrap.js",
        Some(NodeCompatLane::Node24),
    );
    assert_eq!(
        node24_webcrypto_wrap_unwrap_limits.execution_timeout,
        Duration::from_secs(120),
        "the broad WebCrypto wrap/unwrap matrix gets a finite slow-fixture budget in each lane",
    );
    assert_eq!(
        node_compat_fixture_wall_clock_timeout(&node24_webcrypto_wrap_unwrap_limits),
        Duration::from_secs(125),
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
    // This selector follows the live generated required-surface blocker
    // population, which intentionally shrinks as module-loader fixtures are
    // promoted or reclassified. Keep the upper bound as a runaway guard, but
    // allow the floor to drain all the way to the last residual gap.
    assert!(
        (1..=260).contains(&fixture_paths.len()),
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
    // Floor lowered from 10 -> 1 after the cumulative NDS3 census promotion +
    // wave-2 reclassification burn-down genuinely shrank the JSON/data/import-
    // attributes required surface to 8 residual gaps (5 of the 13 tracked
    // fixtures are now promoted or reclassified out of v8_isolate_required).
    // This is a "stay reviewable" sanity bound on the discovery selector
    // (non-empty, not runaway), not a behavioral assertion; it tracks the live
    // gap population, which is supposed to shrink as fixtures are
    // promoted/reclassified.
    assert!(
        (1..=20).contains(&fixture_paths.len()),
        "module-loader JSON/data/import-attributes selector should stay reviewable; selected {} fixtures",
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
    // Floor lowered from 10 -> 1 after the cumulative NDS3 census promotion +
    // wave-2 reclassification burn-down genuinely shrank the package/CJS/ESM
    // loader-core required surface to 9 residual gaps. This is a "stay
    // reviewable" sanity bound on the discovery selector (non-empty, not
    // runaway), not a behavioral assertion; it tracks the live gap population,
    // which is supposed to shrink as fixtures are promoted/reclassified.
    assert!(
        (1..=60).contains(&fixture_paths.len()),
        "module-loader package/CJS/ESM core selector should stay reviewable; selected {} fixtures",
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
    // Floor lowered from 50 -> 20 after the async-hooks burn-down (commit
    // 92131253) genuinely shrank the residual unpromoted parallel-discovery
    // surface to 35. This is a "stay reviewable" sanity bound on the discovery
    // selector, not a behavioral assertion; it tracks the live gap population,
    // which is supposed to shrink as fixtures are promoted/reclassified.
    assert!(
        (20..=200).contains(&fixture_paths.len()),
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
    // Floor lowered from 15 -> 1 after the async-hooks burn-down (commit
    // 92131253) reclassified/promoted nearly the whole internal-helper surface,
    // leaving 2 genuine residual gaps. This bound just keeps the discovery
    // selector reviewable (non-empty, not runaway); it is expected to shrink to
    // a handful as the surface is burned down.
    assert!(
        (1..=60).contains(&fixture_paths.len()),
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
        // Top-level / handler-driven `process.exit()` lifecycle fixtures: route
        // the synthetic exit through reallyExit -> sentinel throw so the harness
        // maps a code-0 exit to success without reaching the unavailable
        // host-process Deno.exit path.
        "test/parallel/test-process-exit-from-before-exit.js"
        | "test/parallel/test-beforeexit-event-exit.js"
        | "test/parallel/test-process-exit-recursive.js" => {
            Some(NodeCompatNamedPreludeBehavior::ProcessExitSentinel)
        }
        // This fs fixture calls process.exit() only after its positioned read
        // assertions and close handler complete. Treat even a late code-0 exit
        // as synthetic fixture termination so the harness does not wait on
        // residual handles, without exposing real host-process exit authority.
        "test/parallel/test-fs-read-stream-pos.js" => {
            Some(NodeCompatNamedPreludeBehavior::ProcessExitAlwaysSentinel)
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
        // fs.WriteStream finish/write callbacks are real loop work kicked off
        // by a sync node:test body. Reenter beforeExit only after those stream
        // callbacks have drained, matching Node's process-liveness behavior.
        "test/parallel/test-file-write-stream5.js" => {
            Some(NodeCompatNamedPostludeBehavior::ProcessBeforeExitReentry)
        }
        // Canonical repeatable-beforeExit cascade: each `.once('beforeExit')`
        // handler reschedules real loop work (setImmediate/setTimeout/net), so
        // the loop must run between emits. The reentry loop settles false once
        // the terminal nextTick stage leaves no loop-keeping work.
        "test/parallel/test-process-beforeexit.js" => {
            Some(NodeCompatNamedPostludeBehavior::ProcessBeforeExitReentry)
        }
        // A throw inside the sole `beforeExit` handler is an uncaught exception
        // in Node; the exit sequence still runs 'exit' listeners (which reset
        // exitCode here). Emit beforeExit (swallow the throw) then emit exit.
        "test/parallel/test-process-beforeexit-throw-exit.js" => {
            Some(NodeCompatNamedPostludeBehavior::ProcessBeforeExitThrowToExit)
        }
        // Single-emit beforeExit fixtures: the handler fires exactly once
        // against the already-settled loop. test-process-exit-from-before-exit
        // additionally calls process.exit(0) inside the handler (sentinel
        // prelude turns that into the success path); the unref'd/late timers in
        // these fixtures must not fire, which holds because the loop is idle.
        "test/parallel/test-timers-unrefed-in-beforeexit.js"
        | "test/parallel/test-process-exit-from-before-exit.js"
        | "test/parallel/test-stream-writable-samecb-singletick.js"
        | "test/parallel/test-process-env-deprecation.js"
        | "test/parallel/test-domain-stack-empty-in-process-uncaughtexception.js"
        // test-async-wrap-uncaughtexception registers a single
        // `process.on('beforeExit', mustCall())` whose handler runs the
        // terminal asserts (call_id is a number; call_log deep-equals
        // [1,1,1,1]). The randomBytes RANDOMBYTESREQUEST async resource
        // (init/before/callback-throw->uncaughtException/after) is already
        // flushed by the default async drain before the postlude, so the loop
        // is settled and a single beforeExit emit fires the handler exactly
        // once -- matching Node's own exit sequence. Byte-identical across
        // node22/node24, so the lane-agnostic path keys both lanes.
        | "test/parallel/test-async-wrap-uncaughtexception.js"
        // test-performance-gc registers a PerformanceObserver for 'gc' entries
        // and asserts on them at beforeExit; the GC entry and beforeExit
        // emission are genuinely supported (node22 already greens). The node24
        // lane only needs the single-emit beforeExit postlude against the
        // settled loop, matching the other promoted lifecycle fixtures.
        | "test/parallel/test-performance-gc.js" => {
            Some(NodeCompatNamedPostludeBehavior::ProcessLifecycleDrain)
        }
        // test/async-hooks/test-async-await.js registers `process.on('beforeExit',
        // mustCall())` -- `.on` fires the handler on EVERY emit (unlike
        // test-worker-ref's `.once`, which dedupes). Its asyncFunc()/`await sleep()`
        // already drains to idle inside load_bundle before __nimbusInvoke runs, so a
        // single emit against the settled loop is exactly what the fixture asserts.
        // The loop-and-re-emit ProcessBeforeExitReentry postlude instead fires
        // beforeExit on every pass -- `has_tick_scheduled` reads phantom-true at the
        // in-drain microtask checkpoint, so __nimbusEventLoopHasMoreWork never
        // settles false and the handler runs hundreds of times, tripping mustCall(1).
        // ProcessLifecycleDrain emits beforeExit exactly once, with no reentry loop.
        "test/async-hooks/test-async-await.js" => {
            Some(NodeCompatNamedPostludeBehavior::ProcessLifecycleDrain)
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
            if excluded_test_relative_paths.contains(&fixture.test_relative_path) {
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
include!("cases/nds3_wave25_staging.rs");
include!("cases/nds3_cycle10.rs");
include!("cases/nds3_cycle12.rs");
include!("cases/nds3_cycle12c.rs");
include!("cases/nds3_cycle12d.rs");
include!("cases/nds3_cycle13_wave1.rs");
include!("cases/nds3_cycle13_wave2_staging.rs");
include!("cases/nds3_cycle13_wave3_staging.rs");
include!("cases/nds3_cycle13_wave4_staging.rs");
include!("cases/nds3_cycle15_wave1.rs");
include!("cases/nds3_cycle16_wave1.rs");
include!("cases/nds3_cycle17_wave1.rs");
include!("cases/nds3_cycle18_wave1.rs");
include!("cases/nds3_cycle19_wave1.rs");
include!("cases/nds3_cycle24_wave1.rs");
include!("cases/nds3_cycle25_wave1.rs");
include!("cases/nds3_cycle27_wave1.rs");
include!("cases/nds3_cycle29_wave1.rs");
include!("cases/nds3_cycle30_wave1.rs");
include!("cases/nds3_cycle31_wave1.rs");
include!("cases/nds3_cycle32_wave1.rs");
include!("cases/nds3_cycle33_wave1.rs");
include!("cases/nds3_cycle34_wave1.rs");
include!("cases/nds3_cycle35_wave1.rs");
include!("cases/nds3_cycle36_wave1.rs");
include!("cases/nds3_cycle37_wave1.rs");
include!("cases/nds3_cycle38_wave1.rs");
include!("cases/nds3_cycle39_wave1.rs");
include!("cases/nds3_cycle40_wave1.rs");
include!("cases/nds3_cycle41_wave1.rs");
include!("cases/nds3_cycle42_wave1.rs");
include!("cases/nds3_cycle52_wave1.rs");
include!("cases/nds3_cycle53_wave1.rs");
include!("cases/nds3_cycle54_wave1.rs");
include!("cases/nds3_cycle55_wave1.rs");
include!("cases/nds3_cycle56_wave1.rs");
include!("cases/nds3_cycle57_wave1.rs");
include!("cases/nds3_cycle58_wave1.rs");
include!("cases/nds3_cycle59_wave1.rs");
include!("cases/nds3_cycle60_wave1.rs");
include!("cases/nds3_cycle61_wave1.rs");
include!("cases/nds3_cycle62_wave1.rs");
include!("cases/nds3_cycle63_wave1.rs");
include!("cases/nds3_cycle64_wave1.rs");
include!("cases/nds3_cycle65_wave1.rs");
include!("cases/nds3_cycle66_wave1.rs");
include!("cases/nds3_cycle67_wave1.rs");
include!("cases/nds3_cycle68_wave1.rs");
include!("cases/nds3_cycle69_wave1.rs");
include!("cases/nds3_cycle70_wave1.rs");
include!("cases/nds3_cycle71_wave1.rs");
include!("cases/nds3_cycle72_esm_cjs_named_error.rs");
include!("cases/nds3_cycle75_event_loop_utilization.rs");
include!("cases/nds3_cycle76_webstreams_clone_unref.rs");
include!("cases/nds3_cycle77_multiple_resolves.rs");
include!("cases/nds3_cycle78_webcrypto_import_export.rs");
include!("cases/nds3_cycle79_webcrypto_cfrg_import_export.rs");
include!("cases/nds3_cycle80_webcrypto_hmac_import_export.rs");
include!("cases/nds3_cycle81_webcrypto_keygen.rs");
include!("cases/nds3_cycle82_webcrypto_hkdf_derivebits.rs");
include!("cases/nds3_cycle83_heapdump_async_hooks.rs");
include!("cases/nds3_cycle84_webcrypto_wrap_unwrap.rs");
