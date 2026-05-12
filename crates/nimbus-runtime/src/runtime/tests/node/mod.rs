use std::ffi::OsString;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::RuntimeLimits;
use crate::test_support::acquire_runtime_suite_lock;

mod supplementary_batches;

use self::supplementary_batches::{
    LOADER_CONTEXT_SUPPLEMENTARY_BATCH, LOADER_CONTEXT_SUPPLEMENTARY_GLOBAL_INJECTION_BATCH,
    LOADER_CONTEXT_SUPPLEMENTARY_MODULE_BRIDGE_BATCH, PROCESS_AND_TIMING_SUPPLEMENTARY_BATCH,
    RUNTIME_SUPPLEMENTARY_BATCH, RUNTIME_SUPPLEMENTARY_SIGNAL_LIFECYCLE_BATCH,
};

const COMMON_INDEX_FIXTURE: &str = include_str!("../node_compat_fixtures/test/common/index.js");
const COMMON_FIXTURES_FIXTURE: &str =
    include_str!("../node_compat_fixtures/test/common/fixtures.js");
const COMMON_TMPDIR_FIXTURE: &str = include_str!("../node_compat_fixtures/test/common/tmpdir.js");
const PROCESS_LIFECYCLE_DRAIN_POSTLUDE: &str = r#"
  if (typeof globalThis.process?.emit === "function") {
    globalThis.process.emit("beforeExit", globalThis.process.exitCode ?? 0);
    await Promise.resolve();
    await new Promise((resolve) => queueMicrotask(resolve));
    if (typeof globalThis.process?.nextTick === "function") {
      await new Promise((resolve) => globalThis.process.nextTick(resolve));
    }
    globalThis.process.emit("exit", globalThis.process.exitCode ?? 0);
  }
"#;
const PROCESS_BEFORE_EXIT_REENTRY_POSTLUDE: &str = r#"
  if (typeof globalThis.process?.emit === "function") {
    const __nimbusInitialExitCode = globalThis.process.exitCode ?? 0;
    const __nimbusDeadline = Date.now() + 1000;
    for (let __nimbusPass = 0; ; __nimbusPass += 1) {
      globalThis.process.emit(
        "beforeExit",
        globalThis.process.exitCode ?? __nimbusInitialExitCode,
      );
      if (typeof globalThis.__nimbusProcessTicksAndRejections === "function") {
        globalThis.__nimbusProcessTicksAndRejections();
      }
      await Promise.resolve();
      await new Promise((resolve) => queueMicrotask(resolve));
      if (typeof globalThis.process?.nextTick === "function") {
        await new Promise((resolve) => globalThis.process.nextTick(resolve));
      }
      const __nimbusHasMoreWork =
        typeof globalThis.__nimbusEventLoopHasMoreWork === "function" &&
        globalThis.__nimbusEventLoopHasMoreWork() === true;
      if (!__nimbusHasMoreWork) {
        break;
      }
      if (Date.now() >= __nimbusDeadline) {
        break;
      }
      await new Promise((resolve) => setTimeout(resolve, 0));
    }
    globalThis.process.emit(
      "exit",
      globalThis.process.exitCode ?? __nimbusInitialExitCode,
    );
  }
"#;
const FORK_CHILD_SETTLE_POSTLUDE: &str = r#"
  if (typeof common.__nimbusFlushForkWorkers === "function") {
    await common.__nimbusFlushForkWorkers();
  }
"#;
const NODE_COMPAT_PROCESS_EXIT_SENTINEL_MARKER: &str = "__NIMBUS_NODE_COMPAT_PROCESS_EXIT__";
const PROCESS_EXIT_SENTINEL_PRELUDE: &str = r#"
import { createRequire as __nimbusCreateRequireForProcessExit } from "node:module";

const __nimbusRequireForProcessExit =
  __nimbusCreateRequireForProcessExit(import.meta.url);
const __nimbusProcessExitSentinel = "__NIMBUS_NODE_COMPAT_PROCESS_EXIT__";

if (globalThis.process && typeof globalThis.process.reallyExit === "function") {
  globalThis.process.reallyExit = (code) => {
    const resolvedCode = code ?? globalThis.process.exitCode ?? 0;
    globalThis.process.exitCode = resolvedCode;
    __nimbusRequireForProcessExit("./test/common/index.js").__nimbusAssert?.();
    const exitError = new Error(`${__nimbusProcessExitSentinel}:${resolvedCode}`);
    exitError.code = "NIMBUS_NODE_COMPAT_PROCESS_EXIT";
    throw exitError;
  };
}
"#;

const INTERACTIVE_TERMINAL_PRELUDE: &str = r#"
globalThis.__nimbusNodeCompatTerm = "xterm-256color";
if (globalThis.process?.env) {
  const originalEnv = globalThis.process.env;
  Object.defineProperty(globalThis.process, "env", {
    value: new Proxy(originalEnv, {
      get(target, property, receiver) {
        if (property === "TERM") {
          return "xterm-256color";
        }
        return Reflect.get(target, property, receiver);
      },
      has(target, property) {
        if (property === "TERM") {
          return true;
        }
        return Reflect.has(target, property);
      },
    }),
    configurable: true,
    enumerable: true,
    writable: false,
  });
}
"#;

const DNS_RESULT_ORDER_IPV4FIRST_PRELUDE: &str = r#"
import dns from "node:dns";
dns.setDefaultResultOrder("ipv4first");
"#;

const DNS_RESULT_ORDER_IPV6FIRST_PRELUDE: &str = r#"
import dns from "node:dns";
dns.setDefaultResultOrder("ipv6first");
"#;

const DNS_RESULT_ORDER_VERBATIM_PRELUDE: &str = r#"
import dns from "node:dns";
dns.setDefaultResultOrder("verbatim");
"#;

const EXPOSE_GC_PRELUDE: &str = "void 0;";
const CHECKOUT_ROOT_CWD_PRELUDE: &str = r#"
{
  const __nimbusPreludeRequire = createRequire(import.meta.url);
  const __nimbusPreludeFs = __nimbusPreludeRequire("node:fs");
  const __nimbusPreludePath = __nimbusPreludeRequire("node:path");
  const __nimbusPreludeUrl = __nimbusPreludeRequire("node:url");
  const __nimbusCompatBundleRoot = __nimbusPreludePath.dirname(
    __nimbusPreludeUrl.fileURLToPath(import.meta.url),
  );
  __nimbusPreludeFs.mkdirSync(
    __nimbusPreludePath.join(__nimbusCompatBundleRoot, "lib"),
    { recursive: true },
  );
  if (typeof globalThis.process?.chdir === "function") {
    globalThis.process.chdir(__nimbusCompatBundleRoot);
  }
}
"#;
const PENDING_DEPRECATION_PRELUDE: &str = r#"
{
  const __nimbusNodeProcess = createRequire(import.meta.url)("node:process");
  const existingNodeOptions =
    typeof __nimbusNodeProcess.env.NODE_OPTIONS === "string"
      ? __nimbusNodeProcess.env.NODE_OPTIONS
      : "";
  if (!existingNodeOptions.split(/\s+/u).includes("--pending-deprecation")) {
    __nimbusNodeProcess.env.NODE_OPTIONS = existingNodeOptions.trim().length === 0
      ? "--pending-deprecation"
      : `${existingNodeOptions} --pending-deprecation`;
  }
  globalThis.process ??= __nimbusNodeProcess;
}
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NodeCompatNamedPreludeBehavior {
    ProcessExitSentinel,
    InteractiveTerminal,
    DnsResultOrderIpv4First,
    DnsResultOrderIpv6First,
    DnsResultOrderVerbatim,
    ExposeGc,
    CheckoutRootCwd,
    PendingDeprecation,
}

impl NodeCompatNamedPreludeBehavior {
    pub(super) const ALL: [Self; 8] = [
        Self::ProcessExitSentinel,
        Self::InteractiveTerminal,
        Self::DnsResultOrderIpv4First,
        Self::DnsResultOrderIpv6First,
        Self::DnsResultOrderVerbatim,
        Self::ExposeGc,
        Self::CheckoutRootCwd,
        Self::PendingDeprecation,
    ];

    pub(super) fn id(self) -> &'static str {
        match self {
            Self::ProcessExitSentinel => "process_exit_sentinel",
            Self::InteractiveTerminal => "interactive_terminal",
            Self::DnsResultOrderIpv4First => "dns_result_order_ipv4first",
            Self::DnsResultOrderIpv6First => "dns_result_order_ipv6first",
            Self::DnsResultOrderVerbatim => "dns_result_order_verbatim",
            Self::ExposeGc => "expose_gc",
            Self::CheckoutRootCwd => "checkout_root_cwd",
            Self::PendingDeprecation => "pending_deprecation",
        }
    }

    pub(super) fn phase(self) -> &'static str {
        "prelude"
    }

    pub(super) fn selection_mode(self) -> &'static str {
        match self {
            Self::PendingDeprecation => "implicit_fixture_flag",
            _ => "default_fixture_mapping",
        }
    }

    fn script(self) -> &'static str {
        match self {
            Self::ProcessExitSentinel => PROCESS_EXIT_SENTINEL_PRELUDE,
            Self::InteractiveTerminal => INTERACTIVE_TERMINAL_PRELUDE,
            Self::DnsResultOrderIpv4First => DNS_RESULT_ORDER_IPV4FIRST_PRELUDE,
            Self::DnsResultOrderIpv6First => DNS_RESULT_ORDER_IPV6FIRST_PRELUDE,
            Self::DnsResultOrderVerbatim => DNS_RESULT_ORDER_VERBATIM_PRELUDE,
            Self::ExposeGc => EXPOSE_GC_PRELUDE,
            Self::CheckoutRootCwd => CHECKOUT_ROOT_CWD_PRELUDE,
            Self::PendingDeprecation => PENDING_DEPRECATION_PRELUDE,
        }
    }

    fn from_script(script: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|behavior| behavior.script() == script)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NodeCompatNamedPostludeBehavior {
    ProcessLifecycleDrain,
    ProcessBeforeExitReentry,
    ForkChildSettle,
}

impl NodeCompatNamedPostludeBehavior {
    pub(super) const ALL: [Self; 3] = [
        Self::ProcessLifecycleDrain,
        Self::ProcessBeforeExitReentry,
        Self::ForkChildSettle,
    ];

    pub(super) fn id(self) -> &'static str {
        match self {
            Self::ProcessLifecycleDrain => "process_lifecycle_drain",
            Self::ProcessBeforeExitReentry => "process_before_exit_reentry",
            Self::ForkChildSettle => "fork_child_settle",
        }
    }

    pub(super) fn phase(self) -> &'static str {
        "postlude"
    }

    pub(super) fn selection_mode(self) -> &'static str {
        "default_fixture_mapping"
    }

    fn script(self) -> &'static str {
        match self {
            Self::ProcessLifecycleDrain => PROCESS_LIFECYCLE_DRAIN_POSTLUDE,
            Self::ProcessBeforeExitReentry => PROCESS_BEFORE_EXIT_REENTRY_POSTLUDE,
            Self::ForkChildSettle => FORK_CHILD_SETTLE_POSTLUDE,
        }
    }

    fn from_script(script: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|behavior| behavior.script() == script)
    }
}

#[derive(Clone, Copy)]
struct NodeCompatExtraFixtureEntry {
    runtime_path: &'static str,
    fixture_source_path: &'static str,
}

#[derive(Clone, Copy)]
enum NodeCompatLane {
    Node20,
    Node22,
    Node24,
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
    mode: NodeCompatBundleMode,
}

fn node_compat_lane_name(lane: NodeCompatLane) -> &'static str {
    match lane {
        NodeCompatLane::Node20 => "node20",
        NodeCompatLane::Node22 => "node22",
        NodeCompatLane::Node24 => "node24",
    }
}

fn node_compat_lane_from_manifest_name(lane: &str) -> std::result::Result<NodeCompatLane, String> {
    match lane {
        "node20" => Ok(NodeCompatLane::Node20),
        "node22" => Ok(NodeCompatLane::Node22),
        "node24" => Ok(NodeCompatLane::Node24),
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

pub(super) fn fixture_requests_pending_deprecation(test_source: &str) -> bool {
    test_source
        .lines()
        .take(40)
        .any(|line| line.contains("Flags:") && line.contains("--pending-deprecation"))
}

fn scoped_node_options_flag(flag: &str) -> ScopedProcessEnvVar {
    let next_value = match std::env::var("NODE_OPTIONS").ok() {
        Some(existing) if existing.split_whitespace().any(|token| token == flag) => existing,
        Some(existing) if existing.trim().is_empty() => flag.to_string(),
        Some(existing) => format!("{existing} {flag}"),
        None => flag.to_string(),
    };
    ScopedProcessEnvVar::set("NODE_OPTIONS", &next_value)
}

impl NodeCompatBatchEntry {
    fn fixture_source_path_for_lane(self, lane: NodeCompatLane) -> Option<&'static str> {
        match lane {
            NodeCompatLane::Node20 => self.node20_fixture_source_path,
            NodeCompatLane::Node22 => self.node22_fixture_source_path,
            NodeCompatLane::Node24 => self.node24_fixture_source_path,
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
            _ => self.shared_extra_files,
        }
    }
}

fn snapshot_batch_entries(batch: &[NodeCompatBatchEntry]) -> Vec<NodeCompatBatchEntrySnapshot> {
    batch
        .iter()
        .map(|entry| NodeCompatBatchEntrySnapshot {
            test_relative_path: entry.test_relative_path,
            node20_fixture_source_path: entry.node20_fixture_source_path,
            node22_fixture_source_path: entry.node22_fixture_source_path,
            node24_fixture_source_path: entry.node24_fixture_source_path,
        })
        .collect()
}

pub(super) fn core_semantics_batch_snapshot() -> Vec<NodeCompatBatchEntrySnapshot> {
    snapshot_batch_entries(CORE_SEMANTICS_BATCH)
}

pub(super) fn process_and_timing_batch_snapshot() -> Vec<NodeCompatBatchEntrySnapshot> {
    snapshot_batch_entries(PROCESS_AND_TIMING_BATCH)
}

pub(super) fn streams_and_local_io_batch_snapshot() -> Vec<NodeCompatBatchEntrySnapshot> {
    snapshot_batch_entries(STREAMS_AND_LOCAL_IO_BATCH)
}

pub(super) fn networking_batch_snapshot() -> Vec<NodeCompatBatchEntrySnapshot> {
    snapshot_batch_entries(NETWORKING_BATCH)
}

pub(super) fn loader_context_batch_snapshot() -> Vec<NodeCompatBatchEntrySnapshot> {
    snapshot_batch_entries(LOADER_CONTEXT_BATCH)
}

pub(super) fn loader_context_supplementary_batch_snapshot() -> Vec<NodeCompatBatchEntrySnapshot> {
    snapshot_batch_entries(LOADER_CONTEXT_SUPPLEMENTARY_BATCH)
}

pub(super) fn loader_context_supplementary_module_bridge_batch_snapshot()
-> Vec<NodeCompatBatchEntrySnapshot> {
    snapshot_batch_entries(LOADER_CONTEXT_SUPPLEMENTARY_MODULE_BRIDGE_BATCH)
}

pub(super) fn loader_context_supplementary_global_injection_batch_snapshot()
-> Vec<NodeCompatBatchEntrySnapshot> {
    snapshot_batch_entries(LOADER_CONTEXT_SUPPLEMENTARY_GLOBAL_INJECTION_BATCH)
}

pub(super) fn process_and_timing_supplementary_batch_snapshot() -> Vec<NodeCompatBatchEntrySnapshot>
{
    snapshot_batch_entries(PROCESS_AND_TIMING_SUPPLEMENTARY_BATCH)
}

pub(super) fn runtime_supplementary_batch_snapshot() -> Vec<NodeCompatBatchEntrySnapshot> {
    snapshot_batch_entries(RUNTIME_SUPPLEMENTARY_BATCH)
}

pub(super) fn runtime_supplementary_signal_lifecycle_batch_snapshot()
-> Vec<NodeCompatBatchEntrySnapshot> {
    snapshot_batch_entries(RUNTIME_SUPPLEMENTARY_SIGNAL_LIFECYCLE_BATCH)
}

fn family_batch_entries(
    family: &str,
) -> std::result::Result<&'static [NodeCompatBatchEntry], String> {
    match family {
        "core-semantics" => Ok(CORE_SEMANTICS_BATCH),
        "process-and-timing" => Ok(PROCESS_AND_TIMING_BATCH),
        "process-and-timing-supplementary" => Ok(PROCESS_AND_TIMING_SUPPLEMENTARY_BATCH),
        "streams-and-local-io" => Ok(STREAMS_AND_LOCAL_IO_BATCH),
        "networking" => Ok(NETWORKING_BATCH),
        "loader-context" => Ok(LOADER_CONTEXT_BATCH),
        "loader-context-supplementary" => Ok(LOADER_CONTEXT_SUPPLEMENTARY_BATCH),
        "loader-context-supplementary-module-bridge" => {
            Ok(LOADER_CONTEXT_SUPPLEMENTARY_MODULE_BRIDGE_BATCH)
        }
        "loader-context-supplementary-global-injection" => {
            Ok(LOADER_CONTEXT_SUPPLEMENTARY_GLOBAL_INJECTION_BATCH)
        }
        "runtime-supplementary" => Ok(RUNTIME_SUPPLEMENTARY_BATCH),
        "runtime-supplementary-signal-lifecycle" => {
            Ok(RUNTIME_SUPPLEMENTARY_SIGNAL_LIFECYCLE_BATCH)
        }
        other => Err(format!("unsupported seeded family catalog `{other}`")),
    }
}

macro_rules! shared_batch_case {
    ($test_relative_path:literal, $fixture_source_path:literal) => {
        NodeCompatBatchEntry {
            test_relative_path: $test_relative_path,
            node20_fixture_source_path: Some($fixture_source_path),
            node22_fixture_source_path: Some($fixture_source_path),
            node24_fixture_source_path: Some(concat!("node24/", $test_relative_path)),
            shared_extra_files: &[],
            node20_extra_files: &[],
            node22_extra_files: &[],
            node24_extra_files: &[],
        }
    };
}

macro_rules! shared_batch_case_with_extra {
    ($test_relative_path:literal, $fixture_source_path:literal, $extra_files:expr) => {
        NodeCompatBatchEntry {
            test_relative_path: $test_relative_path,
            node20_fixture_source_path: Some($fixture_source_path),
            node22_fixture_source_path: Some($fixture_source_path),
            node24_fixture_source_path: Some(concat!("node24/", $test_relative_path)),
            shared_extra_files: $extra_files,
            node20_extra_files: &[],
            node22_extra_files: &[],
            node24_extra_files: &[],
        }
    };
}

macro_rules! split_batch_case {
    ($test_relative_path:literal, $node20_fixture_source_path:literal, $node22_fixture_source_path:literal) => {
        NodeCompatBatchEntry {
            test_relative_path: $test_relative_path,
            node20_fixture_source_path: Some($node20_fixture_source_path),
            node22_fixture_source_path: Some($node22_fixture_source_path),
            node24_fixture_source_path: Some(concat!("node24/", $test_relative_path)),
            shared_extra_files: &[],
            node20_extra_files: &[],
            node22_extra_files: &[],
            node24_extra_files: &[],
        }
    };
}

macro_rules! shared_lane_fixture_batch_case {
    ($test_relative_path:literal, $fixture_source_path:literal) => {
        NodeCompatBatchEntry {
            test_relative_path: $test_relative_path,
            node20_fixture_source_path: Some($fixture_source_path),
            node22_fixture_source_path: Some($fixture_source_path),
            node24_fixture_source_path: Some($fixture_source_path),
            shared_extra_files: &[],
            node20_extra_files: &[],
            node22_extra_files: &[],
            node24_extra_files: &[],
        }
    };
}

macro_rules! node20_only_batch_case {
    ($test_relative_path:literal, $fixture_source_path:literal) => {
        NodeCompatBatchEntry {
            test_relative_path: $test_relative_path,
            node20_fixture_source_path: Some($fixture_source_path),
            node22_fixture_source_path: None,
            node24_fixture_source_path: Some(concat!("node24/", $test_relative_path)),
            shared_extra_files: &[],
            node20_extra_files: &[],
            node22_extra_files: &[],
            node24_extra_files: &[],
        }
    };
}

macro_rules! node22_only_batch_case {
    ($test_relative_path:literal, $fixture_source_path:literal) => {
        NodeCompatBatchEntry {
            test_relative_path: $test_relative_path,
            node20_fixture_source_path: None,
            node22_fixture_source_path: Some($fixture_source_path),
            node24_fixture_source_path: Some(concat!("node24/", $test_relative_path)),
            shared_extra_files: &[],
            node20_extra_files: &[],
            node22_extra_files: &[],
            node24_extra_files: &[],
        }
    };
}

macro_rules! node22_default_only_batch_case {
    ($test_relative_path:literal, $fixture_source_path:literal) => {
        NodeCompatBatchEntry {
            test_relative_path: $test_relative_path,
            node20_fixture_source_path: None,
            node22_fixture_source_path: Some($fixture_source_path),
            node24_fixture_source_path: None,
            shared_extra_files: &[],
            node20_extra_files: &[],
            node22_extra_files: &[],
            node24_extra_files: &[],
        }
    };
}

macro_rules! shared_official_batch_case {
    ($test_relative_path:literal) => {
        NodeCompatBatchEntry {
            test_relative_path: $test_relative_path,
            node20_fixture_source_path: Some(concat!("node20/", $test_relative_path)),
            node22_fixture_source_path: Some(concat!("node22/", $test_relative_path)),
            node24_fixture_source_path: Some(concat!("node24/", $test_relative_path)),
            shared_extra_files: &[],
            node20_extra_files: &[],
            node22_extra_files: &[],
            node24_extra_files: &[],
        }
    };
}

macro_rules! shared_official_batch_case_with_extra {
    ($test_relative_path:literal, $extra_files:expr) => {
        NodeCompatBatchEntry {
            test_relative_path: $test_relative_path,
            node20_fixture_source_path: Some(concat!("node20/", $test_relative_path)),
            node22_fixture_source_path: Some(concat!("node22/", $test_relative_path)),
            node24_fixture_source_path: Some(concat!("node24/", $test_relative_path)),
            shared_extra_files: $extra_files,
            node20_extra_files: &[],
            node22_extra_files: &[],
            node24_extra_files: &[],
        }
    };
}

macro_rules! shared_node20_node22_batch_case_with_extra {
    ($test_relative_path:literal, $extra_files:expr) => {
        NodeCompatBatchEntry {
            test_relative_path: $test_relative_path,
            node20_fixture_source_path: Some(concat!("node22/", $test_relative_path)),
            node22_fixture_source_path: Some(concat!("node22/", $test_relative_path)),
            node24_fixture_source_path: Some(concat!("node22/", $test_relative_path)),
            shared_extra_files: $extra_files,
            node20_extra_files: &[],
            node22_extra_files: &[],
            node24_extra_files: &[],
        }
    };
}

macro_rules! shared_node20_node22_with_node24_override_case_with_extra {
    ($test_relative_path:literal, $node24_fixture_source_path:literal, $extra_files:expr) => {
        NodeCompatBatchEntry {
            test_relative_path: $test_relative_path,
            node20_fixture_source_path: Some(concat!("node22/", $test_relative_path)),
            node22_fixture_source_path: Some(concat!("node22/", $test_relative_path)),
            node24_fixture_source_path: Some($node24_fixture_source_path),
            shared_extra_files: $extra_files,
            node20_extra_files: &[],
            node22_extra_files: &[],
            node24_extra_files: &[],
        }
    };
}

const NODE20_ASSERT_FIRST_LINE_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/assert-first-line.js",
        fixture_source_path: "node20/test/fixtures/assert-first-line.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/assert-long-line.js",
        fixture_source_path: "node20/test/fixtures/assert-long-line.js",
    },
];

const NODE20_CONSOLE_GROUP_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/hijackstdio.js",
        fixture_source_path: "node20/test/common/hijackstdio.js",
    }];

const COMMON_HIJACKSTDIO_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    NODE20_CONSOLE_GROUP_EXTRA_FILES;

const NODE20_COMMON_INDEX_MJS_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/index.mjs",
        fixture_source_path: "node20/test/common/index.mjs",
    }];

const NODE22_COMMON_INDEX_MJS_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/index.mjs",
        fixture_source_path: "node22/test/common/index.mjs",
    }];

const NODE24_COMMON_INDEX_MJS_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/index.mjs",
        fixture_source_path: "node24/test/common/index.mjs",
    }];

const COMMON_INDEX_MJS_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/index.mjs",
        fixture_source_path: "test/common/index.mjs",
    }];

const COMMON_TICK_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[NodeCompatExtraFixtureEntry {
    runtime_path: "test/common/tick.js",
    fixture_source_path: "test/common/tick.js",
}];

const COMMON_COUNTDOWN_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/countdown.js",
        fixture_source_path: "test/common/countdown.js",
    }];

const COMMON_HTTP2_COMPAT_SERVERREQUEST_PIPE_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/person-large.jpg",
        fixture_source_path: "test/fixtures/person-large.jpg",
    }];

const COMMON_PERSON_JPG_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/person.jpg",
        fixture_source_path: "test/fixtures/person.jpg",
    }];

const COMMON_CRYPTO_HASH_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/crypto.js",
        fixture_source_path: "test/common/crypto.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/sample.png",
        fixture_source_path: "test/fixtures/sample.png",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/utf8_test_text.txt",
        fixture_source_path: "test/fixtures/utf8_test_text.txt",
    },
];

const COMMON_CRYPTO_AUTHENTICATED_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/crypto.js",
        fixture_source_path: "test/common/crypto.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/aead-vectors.js",
        fixture_source_path: "test/fixtures/aead-vectors.js",
    },
];

const COMMON_TEST_RUNNER_EVENT_METADATA_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/index.js",
        fixture_source_path: "test/fixtures/test-runner/index.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/test-id-fixture.js",
        fixture_source_path: "test/fixtures/test-runner/test-id-fixture.js",
    },
];

const COMMON_TEST_RUNNER_PLAN_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/index.mjs",
        fixture_source_path: "test/common/index.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/fixtures.mjs",
        fixture_source_path: "test/common/fixtures.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/plan/less.mjs",
        fixture_source_path: "test/fixtures/test-runner/plan/less.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/plan/match.mjs",
        fixture_source_path: "test/fixtures/test-runner/plan/match.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/plan/more.mjs",
        fixture_source_path: "test/fixtures/test-runner/plan/more.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/plan/nested-subtests.mjs",
        fixture_source_path: "test/fixtures/test-runner/plan/nested-subtests.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/plan/plan-via-options.mjs",
        fixture_source_path: "test/fixtures/test-runner/plan/plan-via-options.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/plan/streaming.mjs",
        fixture_source_path: "test/fixtures/test-runner/plan/streaming.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/plan/subtest.mjs",
        fixture_source_path: "test/fixtures/test-runner/plan/subtest.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/plan/timeout-basic.mjs",
        fixture_source_path: "test/fixtures/test-runner/plan/timeout-basic.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/plan/timeout-expired.mjs",
        fixture_source_path: "test/fixtures/test-runner/plan/timeout-expired.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/plan/timeout-wait-false.mjs",
        fixture_source_path: "test/fixtures/test-runner/plan/timeout-wait-false.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/plan/timeout-wait-true.mjs",
        fixture_source_path: "test/fixtures/test-runner/plan/timeout-wait-true.mjs",
    },
];

const COMMON_TEST_RUNNER_RUN_EDGE_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/child_process.js",
        fixture_source_path: "test/common/child_process.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/fixtures.js",
        fixture_source_path: "test/common/fixtures.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/index.js",
        fixture_source_path: "test/common/index.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/index.mjs",
        fixture_source_path: "test/common/index.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/tmpdir.js",
        fixture_source_path: "test/common/tmpdir.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/syntax-error-test.mjs",
        fixture_source_path: "test/fixtures/test-runner/syntax-error-test.mjs",
    },
];

const COMMON_TEST_RUNNER_REPORTERS_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/fixtures.js",
        fixture_source_path: "test/common/fixtures.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/index.js",
        fixture_source_path: "test/common/index.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/test-error-reporter.js",
        fixture_source_path: "test/common/test-error-reporter.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/tmpdir.js",
        fixture_source_path: "test/common/tmpdir.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/empty.js",
        fixture_source_path: "test/fixtures/empty.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/reporters.js",
        fixture_source_path: "test/fixtures/test-runner/reporters.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/custom_reporters/custom.js",
        fixture_source_path: "test/fixtures/test-runner/custom_reporters/custom.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/custom_reporters/custom.cjs",
        fixture_source_path: "test/fixtures/test-runner/custom_reporters/custom.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/custom_reporters/custom.mjs",
        fixture_source_path: "test/fixtures/test-runner/custom_reporters/custom.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/custom_reporters/throwing.js",
        fixture_source_path: "test/fixtures/test-runner/custom_reporters/throwing.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/custom_reporters/throwing-async.js",
        fixture_source_path: "test/fixtures/test-runner/custom_reporters/throwing-async.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/default-behavior/index.test.js",
        fixture_source_path: "test/fixtures/test-runner/default-behavior/index.test.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/error-reporter-fail-fast/a.mjs",
        fixture_source_path: "test/fixtures/test-runner/error-reporter-fail-fast/a.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/error-reporter-fail-fast/b.mjs",
        fixture_source_path: "test/fixtures/test-runner/error-reporter-fail-fast/b.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/node_modules/reporter-cjs/index.js",
        fixture_source_path: "test/fixtures/test-runner/node_modules/reporter-cjs/index.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/node_modules/reporter-cjs/package.json",
        fixture_source_path: "test/fixtures/test-runner/node_modules/reporter-cjs/package.json",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/node_modules/reporter-esm/index.mjs",
        fixture_source_path: "test/fixtures/test-runner/node_modules/reporter-esm/index.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/node_modules/reporter-esm/package.json",
        fixture_source_path: "test/fixtures/test-runner/node_modules/reporter-esm/package.json",
    },
];

const COMMON_TEST_RUNNER_CLI_OPTIONS_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/fixtures.js",
        fixture_source_path: "test/common/fixtures.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/index.js",
        fixture_source_path: "test/common/index.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/default-behavior/index.test.js",
        fixture_source_path: "test/fixtures/test-runner/default-behavior/index.test.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/default-behavior/random.test.mjs",
        fixture_source_path: "test/fixtures/test-runner/default-behavior/random.test.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/default-behavior/subdir/subdir_test.js",
        fixture_source_path: "test/fixtures/test-runner/default-behavior/subdir/subdir_test.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/default-behavior/test/random.cjs",
        fixture_source_path: "test/fixtures/test-runner/default-behavior/test/random.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/default-behavior/test/skip_by_name.cjs",
        fixture_source_path: "test/fixtures/test-runner/default-behavior/test/skip_by_name.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/default-behavior/test/suite_and_test.cjs",
        fixture_source_path: "test/fixtures/test-runner/default-behavior/test/suite_and_test.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/default-behavior/node_modules/test-nm.js",
        fixture_source_path: "test/fixtures/test-runner/default-behavior/node_modules/test-nm.js",
    },
];

const COMMON_TEST_RUNNER_CLI_RANDOMIZE_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/fixtures.js",
        fixture_source_path: "test/common/fixtures.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/index.js",
        fixture_source_path: "test/common/index.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/randomize/internal-order.cjs",
        fixture_source_path: "test/fixtures/test-runner/randomize/internal-order.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/rerun-state.json",
        fixture_source_path: "test/fixtures/test-runner/rerun-state.json",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/shards/a.cjs",
        fixture_source_path: "test/fixtures/test-runner/shards/a.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/shards/b.cjs",
        fixture_source_path: "test/fixtures/test-runner/shards/b.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/shards/c.cjs",
        fixture_source_path: "test/fixtures/test-runner/shards/c.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/shards/d.cjs",
        fixture_source_path: "test/fixtures/test-runner/shards/d.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/shards/e.cjs",
        fixture_source_path: "test/fixtures/test-runner/shards/e.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/shards/f.cjs",
        fixture_source_path: "test/fixtures/test-runner/shards/f.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/shards/g.cjs",
        fixture_source_path: "test/fixtures/test-runner/shards/g.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/shards/h.cjs",
        fixture_source_path: "test/fixtures/test-runner/shards/h.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/shards/i.cjs",
        fixture_source_path: "test/fixtures/test-runner/shards/i.cjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/shards/j.cjs",
        fixture_source_path: "test/fixtures/test-runner/shards/j.cjs",
    },
];

const COMMON_TEST_RUNNER_CLI_RERUN_FAILURES_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/fixtures.js",
        fixture_source_path: "test/common/fixtures.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/index.js",
        fixture_source_path: "test/common/index.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/rerun.js",
        fixture_source_path: "test/fixtures/test-runner/rerun.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-runner/rerun-state.json",
        fixture_source_path: "test/fixtures/test-runner/rerun-state.json",
    },
];

const COMMON_ZLIB_GZIP_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/person.jpg",
        fixture_source_path: "test/fixtures/person.jpg",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/person.jpg.gz",
        fixture_source_path: "test/fixtures/person.jpg.gz",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/pseudo-multimember-gzip.z",
        fixture_source_path: "test/fixtures/pseudo-multimember-gzip.z",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/pseudo-multimember-gzip.gz",
        fixture_source_path: "test/fixtures/pseudo-multimember-gzip.gz",
    },
];

const COMMON_ZLIB_BROTLI_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/gc.js",
        fixture_source_path: "test/common/gc.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/person.jpg",
        fixture_source_path: "test/fixtures/person.jpg",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/person.jpg.br",
        fixture_source_path: "test/fixtures/person.jpg.br",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/pss-vectors.json",
        fixture_source_path: "test/fixtures/pss-vectors.json",
    },
];

const COMMON_GC_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[NodeCompatExtraFixtureEntry {
    runtime_path: "test/common/gc.js",
    fixture_source_path: "test/common/gc.js",
}];

const COMMON_REPL_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/arraystream.js",
        fixture_source_path: "test/common/arraystream.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/repl.js",
        fixture_source_path: "test/common/repl.js",
    },
];

const NODE22_COMMON_UDP_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/udp.js",
        fixture_source_path: "node22/test/common/udp.js",
    }];

const COMMON_TLS_KEY_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent1-key.pem",
        fixture_source_path: "test/fixtures/keys/agent1-key.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent1-cert.pem",
        fixture_source_path: "test/fixtures/keys/agent1-cert.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/ca1-cert.pem",
        fixture_source_path: "test/fixtures/keys/ca1-cert.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/dh2048.pem",
        fixture_source_path: "test/fixtures/keys/dh2048.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent2-key.pem",
        fixture_source_path: "test/fixtures/keys/agent2-key.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent2-cert.pem",
        fixture_source_path: "test/fixtures/keys/agent2-cert.pem",
    },
];

const COMMON_TLS_KEY_COUNTDOWN_GC_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent1-key.pem",
        fixture_source_path: "test/fixtures/keys/agent1-key.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent1-cert.pem",
        fixture_source_path: "test/fixtures/keys/agent1-cert.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/ca1-cert.pem",
        fixture_source_path: "test/fixtures/keys/ca1-cert.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/dh2048.pem",
        fixture_source_path: "test/fixtures/keys/dh2048.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent2-key.pem",
        fixture_source_path: "test/fixtures/keys/agent2-key.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent2-cert.pem",
        fixture_source_path: "test/fixtures/keys/agent2-cert.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/countdown.js",
        fixture_source_path: "test/common/countdown.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/gc.js",
        fixture_source_path: "test/common/gc.js",
    },
];

const COMMON_TLS_EXTENDED_CERT_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent1-key.pem",
        fixture_source_path: "test/fixtures/keys/agent1-key.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent1-cert.pem",
        fixture_source_path: "test/fixtures/keys/agent1-cert.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent2-key.pem",
        fixture_source_path: "test/fixtures/keys/agent2-key.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent2-cert.pem",
        fixture_source_path: "test/fixtures/keys/agent2-cert.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent3-key.pem",
        fixture_source_path: "test/fixtures/keys/agent3-key.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent3-cert.pem",
        fixture_source_path: "test/fixtures/keys/agent3-cert.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/ca1-cert.pem",
        fixture_source_path: "test/fixtures/keys/ca1-cert.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/ca2-cert.pem",
        fixture_source_path: "test/fixtures/keys/ca2-cert.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/rsa_private.pem",
        fixture_source_path: "test/fixtures/keys/rsa_private.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/rsa_cert.crt",
        fixture_source_path: "test/fixtures/keys/rsa_cert.crt",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/ec-key.pem",
        fixture_source_path: "test/fixtures/keys/ec-key.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/ec-cert.pem",
        fixture_source_path: "test/fixtures/keys/ec-cert.pem",
    },
];

const COMMON_TLS_SESSION_CERT_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent1-key.pem",
        fixture_source_path: "test/fixtures/keys/agent1-key.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent1-cert.pem",
        fixture_source_path: "test/fixtures/keys/agent1-cert.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent2-key.pem",
        fixture_source_path: "test/fixtures/keys/agent2-key.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/agent2-cert.pem",
        fixture_source_path: "test/fixtures/keys/agent2-cert.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/ca1-cert.pem",
        fixture_source_path: "test/fixtures/keys/ca1-cert.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/rsa_private.pem",
        fixture_source_path: "test/fixtures/keys/rsa_private.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/rsa_cert.crt",
        fixture_source_path: "test/fixtures/keys/rsa_cert.crt",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/rsa_cert.pfx",
        fixture_source_path: "test/fixtures/keys/rsa_cert.pfx",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/selfsigned-no-keycertsign/key.pem",
        fixture_source_path: "test/fixtures/keys/selfsigned-no-keycertsign/key.pem",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/keys/selfsigned-no-keycertsign/cert.pem",
        fixture_source_path: "test/fixtures/keys/selfsigned-no-keycertsign/cert.pem",
    },
];

const PATH_RESOLVE_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[NodeCompatExtraFixtureEntry {
    runtime_path: "test/fixtures/path-resolve.js",
    fixture_source_path: "node20/test/fixtures/path-resolve.js",
}];

const URL_PARSE_DEPRECATION_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/node_modules/url-deprecations.js",
        fixture_source_path: "test/fixtures/node_modules/url-deprecations.js",
    }];

const NODE20_UTIL_PARSE_ENV_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/dotenv/valid.env",
        fixture_source_path: "node20/test/fixtures/dotenv/valid.env",
    }];

const NODE22_UTIL_PARSE_ENV_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/dotenv/valid.env",
        fixture_source_path: "node22/test/fixtures/dotenv/valid.env",
    }];

const NODE24_UTIL_PARSE_ENV_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/dotenv/valid.env",
        fixture_source_path: "node24/test/fixtures/dotenv/valid.env",
    }];

const NODE20_PROCESS_LOAD_ENV_FILE_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/dotenv/valid.env",
        fixture_source_path: "node20/test/fixtures/dotenv/valid.env",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/dotenv/.env",
        fixture_source_path: "test/fixtures/dotenv/.env",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/dotenv/basic-valid.env",
        fixture_source_path: "test/fixtures/dotenv/basic-valid.env",
    },
];

const NODE22_PROCESS_LOAD_ENV_FILE_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/dotenv/valid.env",
        fixture_source_path: "node22/test/fixtures/dotenv/valid.env",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/dotenv/.env",
        fixture_source_path: "test/fixtures/dotenv/.env",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/dotenv/basic-valid.env",
        fixture_source_path: "test/fixtures/dotenv/basic-valid.env",
    },
];

const NODE24_PROCESS_LOAD_ENV_FILE_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/dotenv/valid.env",
        fixture_source_path: "node24/test/fixtures/dotenv/valid.env",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/dotenv/.env",
        fixture_source_path: "test/fixtures/dotenv/.env",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/dotenv/basic-valid.env",
        fixture_source_path: "test/fixtures/dotenv/basic-valid.env",
    },
];

const MIME_WHATWG_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/mime-whatwg.js",
        fixture_source_path: "test/fixtures/mime-whatwg.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/mime-whatwg-generated.js",
        fixture_source_path: "test/fixtures/mime-whatwg-generated.js",
    },
];

const STREAM_FLATMAP_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[NodeCompatExtraFixtureEntry {
    runtime_path: "test/fixtures/x.txt",
    fixture_source_path: "test/fixtures/x.txt",
}];

const SHARED_FIXTURES_DIR_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/a.js",
        fixture_source_path: "test/fixtures/a.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/baz.js",
        fixture_source_path: "test/fixtures/baz.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/empty.js",
        fixture_source_path: "test/fixtures/empty.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/x.txt",
        fixture_source_path: "test/fixtures/x.txt",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/empty.txt",
        fixture_source_path: "test/fixtures/empty.txt",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/elipses.txt",
        fixture_source_path: "test/fixtures/elipses.txt",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/utf8_test_text.txt",
        fixture_source_path: "test/fixtures/utf8_test_text.txt",
    },
];

const CYCLE_FIXTURES_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/cycles/root.js",
        fixture_source_path: "test/fixtures/cycles/root.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/cycles/folder/foo.js",
        fixture_source_path: "test/fixtures/cycles/folder/foo.js",
    },
];

const MODULE_COMMONJS_FIXTURES_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/experimental.json",
        fixture_source_path: "test/fixtures/experimental.json",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/copy/utf/新建文件夹/index.js",
        fixture_source_path: "test/fixtures/copy/utf/新建文件夹/index.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/copy/utf/新建文件夹/experimental.json",
        fixture_source_path: "test/fixtures/copy/utf/新建文件夹/experimental.json",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/GH-7131/a.js",
        fixture_source_path: "test/fixtures/GH-7131/a.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/GH-7131/b.js",
        fixture_source_path: "test/fixtures/GH-7131/b.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/packages/missing-main/index.js",
        fixture_source_path: "test/fixtures/packages/missing-main/index.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/packages/missing-main/package.json",
        fixture_source_path: "test/fixtures/packages/missing-main/package.json",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/shared-lib-util.js",
        fixture_source_path: "test/common/shared-lib-util.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/module-loading-error.node",
        fixture_source_path: "test/fixtures/module-loading-error.node",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/not-main-module.js",
        fixture_source_path: "test/fixtures/not-main-module.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/cjs-module-wrap.js",
        fixture_source_path: "test/fixtures/cjs-module-wrap.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/cjs-module-wrapper.js",
        fixture_source_path: "test/fixtures/cjs-module-wrapper.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/module-wrapper-spawn-wrap-check.js",
        fixture_source_path: "test/fixtures/module-wrapper-spawn-wrap-check.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/module-wrapper-spawn-require-check.js",
        fixture_source_path: "test/fixtures/module-wrapper-spawn-require-check.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/module-wrapper-spawn-wrap-call-check.js",
        fixture_source_path: "test/fixtures/module-wrapper-spawn-wrap-call-check.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/module-wrapper-spawn-node-shape-check.js",
        fixture_source_path: "test/fixtures/module-wrapper-spawn-node-shape-check.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/module-wrapper-spawn-newline-wrap-check.js",
        fixture_source_path: "test/fixtures/module-wrapper-spawn-newline-wrap-check.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-module-loading-globalpaths/home-pkg-in-both/.node_libraries/foo.js",
        fixture_source_path: "test/fixtures/test-module-loading-globalpaths/home-pkg-in-both/.node_libraries/foo.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-module-loading-globalpaths/home-pkg-in-both/.node_modules/foo.js",
        fixture_source_path: "test/fixtures/test-module-loading-globalpaths/home-pkg-in-both/.node_modules/foo.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-module-loading-globalpaths/home-pkg-in-node_libraries/.node_libraries/foo.js",
        fixture_source_path: "test/fixtures/test-module-loading-globalpaths/home-pkg-in-node_libraries/.node_libraries/foo.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-module-loading-globalpaths/home-pkg-in-node_modules/.node_modules/foo.js",
        fixture_source_path: "test/fixtures/test-module-loading-globalpaths/home-pkg-in-node_modules/.node_modules/foo.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-module-loading-globalpaths/local-pkg/node_modules/foo.js",
        fixture_source_path: "test/fixtures/test-module-loading-globalpaths/local-pkg/node_modules/foo.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-module-loading-globalpaths/local-pkg/test.js",
        fixture_source_path: "test/fixtures/test-module-loading-globalpaths/local-pkg/test.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/test-module-loading-globalpaths/node_path/foo.js",
        fixture_source_path: "test/fixtures/test-module-loading-globalpaths/node_path/foo.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-modules/test-esm-ok.mjs",
        fixture_source_path: "test/fixtures/es-modules/test-esm-ok.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-modules/noext",
        fixture_source_path: "test/fixtures/es-modules/noext",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-module-specifiers/index.mjs",
        fixture_source_path: "test/fixtures/es-module-specifiers/index.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-module-specifiers/package.json",
        fixture_source_path: "test/fixtures/es-module-specifiers/package.json",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-module-specifiers/node_modules/explicit-main/entry.mjs",
        fixture_source_path: "test/fixtures/es-module-specifiers/node_modules/explicit-main/entry.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-module-specifiers/node_modules/explicit-main/package.json",
        fixture_source_path: "test/fixtures/es-module-specifiers/node_modules/explicit-main/package.json",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main/entry.js",
        fixture_source_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main/entry.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main/entry.mjs",
        fixture_source_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main/entry.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main/package.json",
        fixture_source_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main/package.json",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main-type-module/entry.js",
        fixture_source_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main-type-module/entry.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main-type-module/entry.mjs",
        fixture_source_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main-type-module/entry.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main-type-module/package.json",
        fixture_source_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main-type-module/package.json",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main-type-commonjs/entry.mjs",
        fixture_source_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main-type-commonjs/entry.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main-type-commonjs/package.json",
        fixture_source_path: "test/fixtures/es-module-specifiers/node_modules/implicit-main-type-commonjs/package.json",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-module-specifiers/node_modules/no-main-field/package.json",
        fixture_source_path: "test/fixtures/es-module-specifiers/node_modules/no-main-field/package.json",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/es-module-specifiers/node_modules/no-main-field/index.js",
        fixture_source_path: "test/fixtures/es-module-specifiers/node_modules/no-main-field/index.js",
    },
];

const INSPECTOR_FRONT_EDGE_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/loop.js",
        fixture_source_path: "test/fixtures/loop.js",
    }];

const PROCESS_FINALIZATION_WATCHPOINT_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/index.mjs",
        fixture_source_path: "node20/test/common/index.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/child_process.js",
        fixture_source_path: "test/common/child_process.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/process/before-exit.mjs",
        fixture_source_path: "test/fixtures/process/before-exit.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/process/close.mjs",
        fixture_source_path: "test/fixtures/process/close.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/process/different-registry-per-thread.mjs",
        fixture_source_path: "test/fixtures/process/different-registry-per-thread.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/process/gc-not-close.mjs",
        fixture_source_path: "test/fixtures/process/gc-not-close.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/process/unregister.mjs",
        fixture_source_path: "test/fixtures/process/unregister.mjs",
    },
];

// Deno's node_compat lane scales by treating the vendored Node corpus as data:
// scan files, then let config decide what runs. Mirror that shape here for the
// focused Nimbus subset so future NLC3 expansion adds manifest rows instead of
// more hand-written Rust test wrappers. Keep both Node20 and Node22 fixture
// roots in one manifest so the default and supported lanes do not drift.
const CORE_SEMANTICS_BATCH: &[NodeCompatBatchEntry] = &[
    shared_batch_case!(
        "test/parallel/test-assert-async.js",
        "node20/test/parallel/test-assert-async.js"
    ),
    split_batch_case!(
        "test/parallel/test-assert-calltracker-getCalls.js",
        "node20/test/parallel/test-assert-calltracker-getCalls.js",
        "node22/test/parallel/test-assert-calltracker-getCalls.js"
    ),
    shared_batch_case!(
        "test/parallel/test-assert-calltracker-report.js",
        "node20/test/parallel/test-assert-calltracker-report.js"
    ),
    shared_batch_case!(
        "test/parallel/test-assert-calltracker-verify.js",
        "node20/test/parallel/test-assert-calltracker-verify.js"
    ),
    node22_only_batch_case!(
        "test/parallel/test-assert-checktag.js",
        "node22/test/parallel/test-assert-checktag.js"
    ),
    node22_only_batch_case!(
        "test/parallel/test-assert-class-destructuring.js",
        "test/parallel/test-assert-class-destructuring.js"
    ),
    node22_only_batch_case!(
        "test/parallel/test-assert-deep-with-error.js",
        "test/parallel/test-assert-deep-with-error.js"
    ),
    shared_batch_case!(
        "test/parallel/test-assert-fail-deprecation.js",
        "node20/test/parallel/test-assert-fail-deprecation.js"
    ),
    shared_batch_case!(
        "test/parallel/test-assert-fail.js",
        "node20/test/parallel/test-assert-fail.js"
    ),
    shared_batch_case_with_extra!(
        "test/parallel/test-assert-first-line.js",
        "node20/test/parallel/test-assert-first-line.js",
        NODE20_ASSERT_FIRST_LINE_EXTRA_FILES
    ),
    shared_batch_case!(
        "test/parallel/test-assert-if-error.js",
        "node20/test/parallel/test-assert-if-error.js"
    ),
    split_batch_case!(
        "test/parallel/test-assert-typedarray-deepequal.js",
        "node20/test/parallel/test-assert-typedarray-deepequal.js",
        "node22/test/parallel/test-assert-typedarray-deepequal.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-alloc.js",
        "node20/test/parallel/test-buffer-alloc.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-arraybuffer.js",
        "node20/test/parallel/test-buffer-arraybuffer.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-ascii.js",
        "node20/test/parallel/test-buffer-ascii.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-badhex.js",
        "node20/test/parallel/test-buffer-badhex.js"
    ),
    split_batch_case!(
        "test/parallel/test-buffer-bigint64.js",
        "node20/test/parallel/test-buffer-bigint64.js",
        "node22/test/parallel/test-buffer-bigint64.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-bytelength.js",
        "node20/test/parallel/test-buffer-bytelength.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-compare.js",
        "node20/test/parallel/test-buffer-compare.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-compare-offset.js",
        "node20/test/parallel/test-buffer-compare-offset.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-copy.js",
        "node20/test/parallel/test-buffer-copy.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-concat.js",
        "node20/test/parallel/test-buffer-concat.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-constants.js",
        "node20/test/parallel/test-buffer-constants.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-constructor-deprecation-error.js",
        "node20/test/parallel/test-buffer-constructor-deprecation-error.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-equals.js",
        "node20/test/parallel/test-buffer-equals.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-failed-alloc-typed-arrays.js",
        "node20/test/parallel/test-buffer-failed-alloc-typed-arrays.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-fill.js",
        "node20/test/parallel/test-buffer-fill.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-fakes.js",
        "node20/test/parallel/test-buffer-fakes.js"
    ),
    split_batch_case!(
        "test/parallel/test-buffer-from.js",
        "node20/test/parallel/test-buffer-from.js",
        "test/parallel/test-buffer-from.js"
    ),
    split_batch_case!(
        "test/parallel/test-buffer-indexof.js",
        "node20/test/parallel/test-buffer-indexof.js",
        "test/parallel/test-buffer-indexof.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-inspect.js",
        "node20/test/parallel/test-buffer-inspect.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-includes.js",
        "node20/test/parallel/test-buffer-includes.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-inheritance.js",
        "node20/test/parallel/test-buffer-inheritance.js"
    ),
    split_batch_case!(
        "test/parallel/test-buffer-isencoding.js",
        "node20/test/parallel/test-buffer-isencoding.js",
        "test/parallel/test-buffer-isencoding.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-iterator.js",
        "node20/test/parallel/test-buffer-iterator.js"
    ),
    split_batch_case!(
        "test/parallel/test-buffer-new.js",
        "node20/test/parallel/test-buffer-new.js",
        "node22/test/parallel/test-buffer-new.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-no-negative-allocation.js",
        "node20/test/parallel/test-buffer-no-negative-allocation.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-nopendingdep-map.js",
        "node20/test/parallel/test-buffer-nopendingdep-map.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-of-no-deprecation.js",
        "node20/test/parallel/test-buffer-of-no-deprecation.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-over-max-length.js",
        "node20/test/parallel/test-buffer-over-max-length.js"
    ),
    split_batch_case!(
        "test/parallel/test-buffer-parent-property.js",
        "node20/test/parallel/test-buffer-parent-property.js",
        "node22/test/parallel/test-buffer-parent-property.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-pending-deprecation.js",
        "node20/test/parallel/test-buffer-pending-deprecation.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-prototype-inspect.js",
        "node20/test/parallel/test-buffer-prototype-inspect.js"
    ),
    split_batch_case!(
        "test/parallel/test-buffer-safe-unsafe.js",
        "node20/test/parallel/test-buffer-safe-unsafe.js",
        "node22/test/parallel/test-buffer-safe-unsafe.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-read.js",
        "node20/test/parallel/test-buffer-read.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-readdouble.js",
        "node20/test/parallel/test-buffer-readdouble.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-readfloat.js",
        "node20/test/parallel/test-buffer-readfloat.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-readint.js",
        "node20/test/parallel/test-buffer-readint.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-readuint.js",
        "node20/test/parallel/test-buffer-readuint.js"
    ),
    node22_only_batch_case!(
        "test/parallel/test-buffer-resizable.js",
        "node22/test/parallel/test-buffer-resizable.js"
    ),
    split_batch_case!(
        "test/parallel/test-buffer-sharedarraybuffer.js",
        "node20/test/parallel/test-buffer-sharedarraybuffer.js",
        "node22/test/parallel/test-buffer-sharedarraybuffer.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-set-inspect-max-bytes.js",
        "node20/test/parallel/test-buffer-set-inspect-max-bytes.js"
    ),
    node22_only_batch_case!(
        "test/parallel/test-buffer-slow.js",
        "node22/test/parallel/test-buffer-slow.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-slice.js",
        "node20/test/parallel/test-buffer-slice.js"
    ),
    split_batch_case!(
        "test/parallel/test-buffer-swap.js",
        "node20/test/parallel/test-buffer-swap.js",
        "node22/test/parallel/test-buffer-swap.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-tojson.js",
        "node20/test/parallel/test-buffer-tojson.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-tostring.js",
        "node20/test/parallel/test-buffer-tostring.js"
    ),
    split_batch_case!(
        "test/parallel/test-buffer-tostring-range.js",
        "node20/test/parallel/test-buffer-tostring-range.js",
        "node22/test/parallel/test-buffer-tostring-range.js"
    ),
    split_batch_case!(
        "test/parallel/test-buffer-tostring-rangeerror.js",
        "node20/test/parallel/test-buffer-tostring-rangeerror.js",
        "node22/test/parallel/test-buffer-tostring-rangeerror.js"
    ),
    split_batch_case!(
        "test/parallel/test-buffer-write.js",
        "node20/test/parallel/test-buffer-write.js",
        "test/parallel/test-buffer-write.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-writedouble.js",
        "node20/test/parallel/test-buffer-writedouble.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-writefloat.js",
        "node20/test/parallel/test-buffer-writefloat.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-writeint.js",
        "node20/test/parallel/test-buffer-writeint.js"
    ),
    shared_batch_case!(
        "test/parallel/test-buffer-writeuint.js",
        "node20/test/parallel/test-buffer-writeuint.js"
    ),
    split_batch_case!(
        "test/parallel/test-console-assign-undefined.js",
        "node20/test/parallel/test-console-assign-undefined.js",
        "test/parallel/test-console-assign-undefined.js"
    ),
    shared_batch_case!(
        "test/parallel/test-console-async-write-error.js",
        "node20/test/parallel/test-console-async-write-error.js"
    ),
    node22_only_batch_case!(
        "test/parallel/test-console-clear.js",
        "test/parallel/test-console-clear.js"
    ),
    shared_batch_case!(
        "test/parallel/test-console-count.js",
        "node20/test/parallel/test-console-count.js"
    ),
    shared_batch_case!(
        "test/parallel/test-console-log-stdio-broken-dest.js",
        "node20/test/parallel/test-console-log-stdio-broken-dest.js"
    ),
    shared_batch_case!(
        "test/parallel/test-console-table.js",
        "node20/test/parallel/test-console-table.js"
    ),
    shared_batch_case!(
        "test/parallel/test-console-tty-colors.js",
        "node20/test/parallel/test-console-tty-colors.js"
    ),
    shared_batch_case!(
        "test/parallel/test-console-log-throw-primitive.js",
        "node20/test/parallel/test-console-log-throw-primitive.js"
    ),
    split_batch_case!(
        "test/parallel/test-console-formatTime.js",
        "node20/test/parallel/test-console-formatTime.js",
        "node22/test/parallel/test-console-formatTime.js"
    ),
    shared_batch_case_with_extra!(
        "test/parallel/test-console-group.js",
        "node20/test/parallel/test-console-group.js",
        NODE20_CONSOLE_GROUP_EXTRA_FILES
    ),
    split_batch_case!(
        "test/parallel/test-console-instance.js",
        "node20/test/parallel/test-console-instance.js",
        "test/parallel/test-console-instance.js"
    ),
    split_batch_case!(
        "test/parallel/test-console-methods.js",
        "node20/test/parallel/test-console-methods.js",
        "test/parallel/test-console-methods.js"
    ),
    shared_batch_case!(
        "test/parallel/test-console-not-call-toString.js",
        "node20/test/parallel/test-console-not-call-toString.js"
    ),
    shared_batch_case!(
        "test/parallel/test-console-no-swallow-stack-overflow.js",
        "node20/test/parallel/test-console-no-swallow-stack-overflow.js"
    ),
    split_batch_case!(
        "test/parallel/test-console-self-assign.js",
        "node20/test/parallel/test-console-self-assign.js",
        "test/parallel/test-console-self-assign.js"
    ),
    shared_batch_case!(
        "test/parallel/test-console-stdio-setters.js",
        "node20/test/parallel/test-console-stdio-setters.js"
    ),
    shared_batch_case!(
        "test/parallel/test-console-sync-write-error.js",
        "node20/test/parallel/test-console-sync-write-error.js"
    ),
    split_batch_case!(
        "test/parallel/test-events-getmaxlisteners.js",
        "node20/test/parallel/test-events-getmaxlisteners.js",
        "test/parallel/test-events-getmaxlisteners.js"
    ),
    split_batch_case!(
        "test/parallel/test-events-list.js",
        "node20/test/parallel/test-events-list.js",
        "test/parallel/test-events-list.js"
    ),
    shared_batch_case!(
        "test/parallel/test-events-listener-count-with-listener.js",
        "node20/test/parallel/test-events-listener-count-with-listener.js"
    ),
    shared_batch_case_with_extra!(
        "test/parallel/test-events-add-abort-listener.mjs",
        "node20/test/parallel/test-events-add-abort-listener.mjs",
        NODE20_COMMON_INDEX_MJS_EXTRA_FILES
    ),
    node22_only_batch_case!(
        "test/parallel/test-events-once.js",
        "node22/test/parallel/test-events-once.js"
    ),
    shared_batch_case!(
        "test/parallel/test-events-static-geteventlisteners.js",
        "node20/test/parallel/test-events-static-geteventlisteners.js"
    ),
    split_batch_case!(
        "test/parallel/test-path-basename.js",
        "node20/test/parallel/test-path-basename.js",
        "test/parallel/test-path-basename.js"
    ),
    split_batch_case!(
        "test/parallel/test-path-dirname.js",
        "node20/test/parallel/test-path-dirname.js",
        "test/parallel/test-path-dirname.js"
    ),
    shared_batch_case!(
        "test/parallel/test-path-extname.js",
        "node20/test/parallel/test-path-extname.js"
    ),
    shared_batch_case!(
        "test/parallel/test-path-glob.js",
        "node20/test/parallel/test-path-glob.js"
    ),
    split_batch_case!(
        "test/parallel/test-path-isabsolute.js",
        "node20/test/parallel/test-path-isabsolute.js",
        "test/parallel/test-path-isabsolute.js"
    ),
    split_batch_case!(
        "test/parallel/test-path-join.js",
        "node20/test/parallel/test-path-join.js",
        "test/parallel/test-path-join.js"
    ),
    node20_only_batch_case!(
        "test/parallel/test-path-normalize.js",
        "node20/test/parallel/test-path-normalize.js"
    ),
    node20_only_batch_case!(
        "test/parallel/test-path-makelong.js",
        "node20/test/parallel/test-path-makelong.js"
    ),
    shared_batch_case!(
        "test/parallel/test-path-parse-format.js",
        "node20/test/parallel/test-path-parse-format.js"
    ),
    shared_batch_case!(
        "test/parallel/test-path-posix-exists.js",
        "node20/test/parallel/test-path-posix-exists.js"
    ),
    shared_batch_case!(
        "test/parallel/test-path-posix-relative-on-windows.js",
        "node20/test/parallel/test-path-posix-relative-on-windows.js"
    ),
    shared_batch_case!(
        "test/parallel/test-path-relative.js",
        "node20/test/parallel/test-path-relative.js"
    ),
    shared_batch_case!(
        "test/parallel/test-path-win32-exists.js",
        "node20/test/parallel/test-path-win32-exists.js"
    ),
    split_batch_case!(
        "test/parallel/test-path-zero-length-strings.js",
        "node20/test/parallel/test-path-zero-length-strings.js",
        "test/parallel/test-path-zero-length-strings.js"
    ),
    split_batch_case!(
        "test/parallel/test-path.js",
        "node20/test/parallel/test-path.js",
        "test/parallel/test-path.js"
    ),
    split_batch_case!(
        "test/parallel/test-punycode.js",
        "node20/test/parallel/test-punycode.js",
        "test/parallel/test-punycode.js"
    ),
    split_batch_case!(
        "test/parallel/test-querystring-escape.js",
        "node20/test/parallel/test-querystring-escape.js",
        "test/parallel/test-querystring-escape.js"
    ),
    split_batch_case!(
        "test/parallel/test-querystring-maxKeys-non-finite.js",
        "node20/test/parallel/test-querystring-maxKeys-non-finite.js",
        "test/parallel/test-querystring-maxKeys-non-finite.js"
    ),
    shared_batch_case!(
        "test/parallel/test-querystring-multichar-separator.js",
        "node20/test/parallel/test-querystring-multichar-separator.js"
    ),
    shared_batch_case!(
        "test/parallel/test-querystring.js",
        "node20/test/parallel/test-querystring.js"
    ),
    split_batch_case!(
        "test/parallel/test-string-decoder-end.js",
        "node20/test/parallel/test-string-decoder-end.js",
        "test/parallel/test-string-decoder-end.js"
    ),
    shared_batch_case!(
        "test/parallel/test-string-decoder-fuzz.js",
        "node20/test/parallel/test-string-decoder-fuzz.js"
    ),
    shared_batch_case!(
        "test/parallel/test-string-decoder.js",
        "node20/test/parallel/test-string-decoder.js"
    ),
    shared_batch_case!(
        "test/parallel/test-url-domain-ascii-unicode.js",
        "node20/test/parallel/test-url-domain-ascii-unicode.js"
    ),
    shared_batch_case!(
        "test/parallel/test-url-fileurltopath.js",
        "node20/test/parallel/test-url-fileurltopath.js"
    ),
    split_batch_case!(
        "test/parallel/test-url-format.js",
        "node20/test/parallel/test-url-format.js",
        "test/parallel/test-url-format.js"
    ),
    split_batch_case!(
        "test/parallel/test-url-format-invalid-input.js",
        "node20/test/parallel/test-url-format-invalid-input.js",
        "test/parallel/test-url-format-invalid-input.js"
    ),
    split_batch_case!(
        "test/parallel/test-url-format-whatwg.js",
        "node20/test/parallel/test-url-format-whatwg.js",
        "test/parallel/test-url-format-whatwg.js"
    ),
    node22_only_batch_case!(
        "test/parallel/test-url-invalid-file-url-path-input.js",
        "test/parallel/test-url-invalid-file-url-path-input.js"
    ),
    split_batch_case!(
        "test/parallel/test-url-parse-format.js",
        "node20/test/parallel/test-url-parse-format.js",
        "node22/test/parallel/test-url-parse-format.js"
    ),
    split_batch_case!(
        "test/parallel/test-url-parse-invalid-input.js",
        "node20/test/parallel/test-url-parse-invalid-input.js",
        "node22/test/parallel/test-url-parse-invalid-input.js"
    ),
    split_batch_case!(
        "test/parallel/test-url-parse-query.js",
        "node20/test/parallel/test-url-parse-query.js",
        "test/parallel/test-url-parse-query.js"
    ),
    shared_batch_case!(
        "test/parallel/test-url-pathtofileurl.js",
        "node20/test/parallel/test-url-pathtofileurl.js"
    ),
    split_batch_case!(
        "test/parallel/test-url-relative.js",
        "node20/test/parallel/test-url-relative.js",
        "test/parallel/test-url-relative.js"
    ),
    split_batch_case!(
        "test/parallel/test-url-revokeobjecturl.js",
        "node20/test/parallel/test-url-revokeobjecturl.js",
        "test/parallel/test-url-revokeobjecturl.js"
    ),
    split_batch_case!(
        "test/parallel/test-url-urltooptions.js",
        "node20/test/parallel/test-url-urltooptions.js",
        "test/parallel/test-url-urltooptions.js"
    ),
];

const PROCESS_AND_TIMING_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-process-default.js"),
    shared_official_batch_case!("test/parallel/test-process-prototype.js"),
    shared_official_batch_case!("test/parallel/test-process-release.js"),
    node22_only_batch_case!(
        "test/parallel/test-process-features.js",
        "node22/test/parallel/test-process-features.js"
    ),
    shared_official_batch_case!("test/parallel/test-process-uptime.js"),
    shared_official_batch_case!("test/parallel/test-process-emitwarning.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-process-warning.js",
        COMMON_HIJACKSTDIO_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-process-no-deprecation.js"),
    shared_official_batch_case!("test/parallel/test-process-env-symbols.js"),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-process-load-env-file.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-process-load-env-file.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-process-load-env-file.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-process-load-env-file.js"),
        shared_extra_files: &[],
        node20_extra_files: NODE20_PROCESS_LOAD_ENV_FILE_EXTRA_FILES,
        node22_extra_files: NODE22_PROCESS_LOAD_ENV_FILE_EXTRA_FILES,
        node24_extra_files: NODE24_PROCESS_LOAD_ENV_FILE_EXTRA_FILES,
    },
    shared_official_batch_case!("test/parallel/test-process-next-tick.js"),
    shared_official_batch_case!("test/parallel/test-next-tick-doesnt-hang.js"),
    shared_official_batch_case!("test/parallel/test-next-tick-errors.js"),
    shared_official_batch_case!("test/parallel/test-next-tick-fixed-queue-regression.js"),
    shared_official_batch_case!("test/parallel/test-next-tick-intentional-starvation.js"),
    shared_official_batch_case!("test/parallel/test-next-tick-ordering.js"),
    shared_official_batch_case!("test/parallel/test-next-tick-ordering2.js"),
    shared_official_batch_case!("test/parallel/test-next-tick.js"),
    shared_official_batch_case!("test/parallel/test-timers.js"),
    shared_official_batch_case!("test/parallel/test-timers-api-refs.js"),
    shared_official_batch_case!("test/parallel/test-timers-args.js"),
    shared_official_batch_case!("test/parallel/test-timers-clear-null-does-not-throw-error.js"),
    shared_official_batch_case!("test/parallel/test-timers-clear-object-does-not-throw-error.js"),
    shared_official_batch_case!("test/parallel/test-timers-clear-timeout-interval-equivalent.js"),
    shared_official_batch_case!("test/parallel/test-timers-clearImmediate.js"),
    shared_official_batch_case!("test/parallel/test-timers-immediate.js"),
    shared_official_batch_case!("test/parallel/test-timers-non-integer-delay.js"),
    shared_official_batch_case!("test/parallel/test-timers-this.js"),
    shared_official_batch_case!("test/parallel/test-timers-throw-when-cb-not-function.js"),
    shared_official_batch_case!("test/parallel/test-timers-zero-timeout.js"),
    shared_official_batch_case!("test/parallel/test-util-deprecate.js"),
    shared_official_batch_case!("test/parallel/test-util-deprecate-invalid-code.js"),
    shared_official_batch_case!("test/parallel/test-util-format.js"),
    shared_official_batch_case!("test/parallel/test-util-inherits.js"),
    shared_official_batch_case!("test/parallel/test-mime-api.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-mime-whatwg.js",
        MIME_WHATWG_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-perf-hooks-usertiming.js"),
    shared_official_batch_case!("test/parallel/test-perf-hooks-histogram.js"),
    node22_only_batch_case!(
        "test/parallel/test-perf-hooks-resourcetiming.js",
        "node22/test/parallel/test-perf-hooks-resourcetiming.js"
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-util-parse-env.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-util-parse-env.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-util-parse-env.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-util-parse-env.js"),
        shared_extra_files: &[],
        node20_extra_files: NODE20_UTIL_PARSE_ENV_EXTRA_FILES,
        node22_extra_files: NODE22_UTIL_PARSE_ENV_EXTRA_FILES,
        node24_extra_files: NODE24_UTIL_PARSE_ENV_EXTRA_FILES,
    },
    shared_official_batch_case!("test/parallel/test-util-text-decoder.js"),
    shared_official_batch_case!("test/parallel/test-util-types-exists.js"),
    shared_official_batch_case!("test/parallel/test-diagnostics-channel-has-subscribers.js"),
    shared_official_batch_case!("test/parallel/test-diagnostics-channel-object-channel-pub-sub.js"),
    shared_official_batch_case!("test/parallel/test-diagnostics-channel-pub-sub.js"),
    shared_official_batch_case!("test/parallel/test-diagnostics-channel-safe-subscriber-errors.js"),
    shared_official_batch_case!("test/parallel/test-diagnostics-channel-symbol-named.js"),
    shared_official_batch_case!("test/parallel/test-diagnostics-channel-sync-unsubscribe.js"),
];

const STREAMS_AND_LOCAL_IO_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-os-eol.js"),
    shared_official_batch_case!("test/parallel/test-tty-stdin-end.js"),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-tty-stdin-pipe.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-tty-stdin-pipe.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-tty-stdin-pipe.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-tty-stdin-pipe.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case!("test/parallel/test-tty-backwards-api.js"),
    shared_official_batch_case!("test/parallel/test-os-checked-function.js"),
    shared_official_batch_case!("test/parallel/test-ttywrap-invalid-fd.js"),
    shared_official_batch_case!("test/parallel/test-ttywrap-stack.js"),
    shared_official_batch_case!("test/parallel/test-readline-csi.js"),
    shared_official_batch_case!("test/parallel/test-readline-carriage-return-between-chunks.js"),
    shared_official_batch_case!("test/parallel/test-readline-input-onerror.js"),
    shared_official_batch_case!("test/parallel/test-readline-async-iterators.js"),
    shared_official_batch_case!("test/parallel/test-readline-async-iterators-backpressure.js"),
    shared_official_batch_case!("test/parallel/test-readline-async-iterators-destroy.js"),
    shared_official_batch_case!("test/parallel/test-readline-interface.js"),
    shared_official_batch_case!("test/parallel/test-readline-promises-interface.js"),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-readline-promises-csi.mjs",
        node20_fixture_source_path: Some("node20/test/parallel/test-readline-promises-csi.mjs"),
        node22_fixture_source_path: Some("node22/test/parallel/test-readline-promises-csi.mjs"),
        node24_fixture_source_path: Some("node24/test/parallel/test-readline-promises-csi.mjs"),
        shared_extra_files: &[],
        node20_extra_files: NODE20_COMMON_INDEX_MJS_EXTRA_FILES,
        node22_extra_files: NODE22_COMMON_INDEX_MJS_EXTRA_FILES,
        node24_extra_files: NODE24_COMMON_INDEX_MJS_EXTRA_FILES,
    },
    shared_official_batch_case!("test/parallel/test-stream-construct.js"),
    shared_official_batch_case!("test/parallel/test-stream-auto-destroy.js"),
    shared_official_batch_case!("test/parallel/test-stream-duplex-destroy.js"),
    shared_official_batch_case!("test/parallel/test-stream-duplex-end.js"),
    shared_official_batch_case!("test/parallel/test-stream-end-of-streams.js"),
    shared_official_batch_case!("test/parallel/test-stream-event-names.js"),
    shared_official_batch_case!("test/parallel/test-stream-end-paused.js"),
    shared_official_batch_case!("test/parallel/test-stream-error-once.js"),
    shared_official_batch_case!("test/parallel/test-stream-events-prepend.js"),
    shared_official_batch_case!("test/parallel/test-stream-inheritance.js"),
    shared_official_batch_case!("test/parallel/test-stream-ispaused.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-aborted.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-constructor-set-methods.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-destroy.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-end-destroyed.js"),
    shared_official_batch_case!("test/parallel/test-stream-duplex-props.js"),
    shared_official_batch_case!("test/parallel/test-stream-duplex-readable-writable.js"),
    shared_official_batch_case!("test/parallel/test-stream-passthrough-drain.js"),
    shared_official_batch_case!("test/parallel/test-stream-pipe-after-end.js"),
    shared_official_batch_case!("test/parallel/test-stream-pipe-await-drain.js"),
    shared_official_batch_case!("test/parallel/test-stream-pipe-cleanup.js"),
    shared_official_batch_case!("test/parallel/test-stream-pipe-event.js"),
    shared_official_batch_case!("test/parallel/test-stream-pipe-flow.js"),
    shared_official_batch_case!("test/parallel/test-stream-pipe-multiple-pipes.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-ended.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-data.js"),
    shared_official_batch_case!("test/parallel/test-stream-decoder-objectmode.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-event.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-emittedReadable.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-default-encoding.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-invalid-chunk.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-next-no-null.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-no-unneeded-readable.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-pause-and-resume.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-readable.js"),
    shared_official_batch_case!("test/parallel/test-stream-reduce.js"),
    shared_official_batch_case!("test/parallel/test-stream-add-abort-signal.js"),
    shared_official_batch_case!(
        "test/parallel/test-stream-base-prototype-accessors-enumerability.js"
    ),
    shared_official_batch_case!("test/parallel/test-stream-catch-rejections.js"),
    shared_official_batch_case!(
        "test/parallel/test-stream-readable-setEncoding-existing-buffers.js"
    ),
    shared_official_batch_case!("test/parallel/test-stream-readable-setEncoding-null.js"),
    shared_official_batch_case!("test/parallel/test-stream-map.js"),
    shared_official_batch_case!("test/parallel/test-stream-filter.js"),
    shared_official_batch_case!("test/parallel/test-stream-forEach.js"),
    shared_official_batch_case!("test/parallel/test-stream-toArray.js"),
    shared_official_batch_case!("test/parallel/test-stream-drop-take.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-stream-flatMap.js",
        STREAM_FLATMAP_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-stream-consumers.js"),
    shared_official_batch_case!("test/parallel/test-stream-promises.js"),
    shared_official_batch_case!("test/parallel/test-stream-pipeline-async-iterator.js"),
    shared_official_batch_case!("test/parallel/test-stream-pipeline-duplex.js"),
    shared_official_batch_case!("test/parallel/test-stream-pipeline-listeners.js"),
    shared_official_batch_case!("test/parallel/test-stream-pipeline-uncaught.js"),
    shared_official_batch_case!("test/parallel/test-stream-pipeline-with-empty-string.js"),
    shared_official_batch_case!("test/parallel/test-stream-compose.js"),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-stream-compose-operator.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-stream-compose-operator.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-stream-compose-operator.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case!("test/parallel/test-stream-destroy-event-order.js"),
    shared_official_batch_case!("test/parallel/test-stream-duplex.js"),
    shared_official_batch_case!("test/parallel/test-stream-duplex-from.js"),
    shared_official_batch_case!("test/parallel/test-stream-duplexpair.js"),
    shared_official_batch_case!("test/parallel/test-stream-set-default-hwm.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-add-chunk-during-data.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-didRead.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-dispose.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-from-web-termination.js"),
    node22_only_batch_case!(
        "test/parallel/test-stream-readable-infinite-read.js",
        "node22/test/parallel/test-stream-readable-infinite-read.js"
    ),
    shared_official_batch_case!("test/parallel/test-stream-readable-hwm-0.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-hwm-0-async.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-hwm-0-no-flow-data.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-reading-readingMore.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-flow-recursion.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-needReadable.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-readable-then-resume.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-resumeScheduled.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-resume-hwm.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-strategy-option.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-to-web-termination.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-unshift.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-with-unimplemented-_read.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-emit-readable-short-stream.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-error-end.js"),
    shared_official_batch_case!("test/parallel/test-stream-push-strings.js"),
    shared_official_batch_case!("test/parallel/test-stream-aliases-legacy.js"),
    shared_official_batch_case!(
        "test/parallel/test-stream-await-drain-writers-in-synchronously-recursion-write.js"
    ),
    shared_official_batch_case!("test/parallel/test-stream-backpressure.js"),
    shared_official_batch_case!("test/parallel/test-stream-big-packet.js"),
    shared_official_batch_case!("test/parallel/test-stream-big-push.js"),
    shared_official_batch_case!("test/parallel/test-stream-err-multiple-callback-construction.js"),
    shared_official_batch_case!("test/parallel/test-stream-pipe-deadlock.js"),
    shared_official_batch_case!("test/parallel/test-stream-pipe-same-destination-twice.js"),
    shared_official_batch_case!("test/parallel/test-stream-push-order.js"),
    shared_official_batch_case!("test/parallel/test-stream-readable-object-multi-push-async.js"),
    shared_official_batch_case!("test/parallel/test-stream-transform-callback-twice.js"),
    shared_official_batch_case!("test/parallel/test-stream-transform-constructor-set-methods.js"),
    shared_official_batch_case!("test/parallel/test-stream-transform-final.js"),
    shared_official_batch_case!("test/parallel/test-stream-transform-final-sync.js"),
    shared_official_batch_case!("test/parallel/test-stream-transform-flush-data.js"),
    shared_official_batch_case!("test/parallel/test-stream-transform-hwm0.js"),
    shared_official_batch_case!("test/parallel/test-stream-transform-objectmode-falsey-value.js"),
    node22_only_batch_case!(
        "test/parallel/test-stream-transform-split-highwatermark.js",
        "node22/test/parallel/test-stream-transform-split-highwatermark.js"
    ),
    node22_only_batch_case!(
        "test/parallel/test-stream-transform-split-objectmode.js",
        "node22/test/parallel/test-stream-transform-split-objectmode.js"
    ),
    shared_official_batch_case!("test/parallel/test-stream-typedarray.js"),
    shared_official_batch_case!("test/parallel/test-stream-unshift-empty-chunk.js"),
    shared_official_batch_case!("test/parallel/test-stream-unshift-read-race.js"),
    shared_official_batch_case!("test/parallel/test-stream-uint8array.js"),
    shared_official_batch_case!("test/parallel/test-stream-writable-aborted.js"),
    shared_official_batch_case!("test/parallel/test-stream-writable-change-default-encoding.js"),
    shared_official_batch_case!("test/parallel/test-stream-writable-clear-buffer.js"),
    shared_official_batch_case!("test/parallel/test-stream-writable-constructor-set-methods.js"),
    shared_official_batch_case!("test/parallel/test-stream-writable-decoded-encoding.js"),
    shared_official_batch_case!("test/parallel/test-stream-writable-destroy.js"),
    shared_official_batch_case!("test/parallel/test-stream-writable-end-cb-error.js"),
    shared_official_batch_case!("test/parallel/test-stream-writable-end-cb-uncaught.js"),
    shared_official_batch_case!("test/parallel/test-stream-writable-end-multiple.js"),
    shared_official_batch_case!("test/parallel/test-stream-writable-final-async.js"),
    shared_official_batch_case!("test/parallel/test-stream-writable-final-destroy.js"),
    shared_official_batch_case!("test/parallel/test-stream-writable-final-throw.js"),
    shared_official_batch_case!("test/parallel/test-stream-writable-finish-destroyed.js"),
    shared_official_batch_case!("test/parallel/test-stream-writable-finished.js"),
    shared_official_batch_case!("test/parallel/test-stream-writable-finished-state.js"),
    shared_official_batch_case!("test/parallel/test-stream-writable-invalid-chunk.js"),
    shared_official_batch_case!("test/parallel/test-stream-writable-needdrain-state.js"),
    shared_official_batch_case!("test/parallel/test-stream-writable-null.js"),
    shared_official_batch_case!("test/parallel/test-stream-writable-properties.js"),
    shared_official_batch_case!("test/parallel/test-stream-writable-write-cb-error.js"),
    shared_official_batch_case!("test/parallel/test-stream-writable-write-cb-twice.js"),
    shared_official_batch_case!("test/parallel/test-stream-writable-write-error.js"),
    shared_official_batch_case!("test/parallel/test-stream-writable-write-writev-finish.js"),
    shared_official_batch_case!("test/parallel/test-stream-writable-writable.js"),
    shared_official_batch_case!("test/parallel/test-stream-writable-ended-state.js"),
    shared_official_batch_case!("test/parallel/test-stream-writableState-ending.js"),
    shared_official_batch_case!(
        "test/parallel/test-stream-writableState-uncorked-bufferedRequestCount.js"
    ),
    shared_official_batch_case!("test/parallel/test-stream-write-drain.js"),
    shared_official_batch_case!("test/parallel/test-stream-write-destroy.js"),
    shared_official_batch_case!("test/parallel/test-stream-write-final.js"),
    shared_official_batch_case!("test/parallel/test-stream-writev.js"),
    node22_only_batch_case!(
        "test/parallel/test-stream-duplex-readable-end.js",
        "node22/test/parallel/test-stream-duplex-readable-end.js"
    ),
    shared_official_batch_case!("test/parallel/test-stream-duplex-writable-finished.js"),
    shared_official_batch_case!("test/parallel/test-stream-objectmode-undefined.js"),
    shared_official_batch_case!("test/parallel/test-stream-unpipe-event.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-fs-buffer.js",
        SHARED_FIXTURES_DIR_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-fs-close.js"),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-fs-constants.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-fs-constants.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-fs-constants.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case!("test/parallel/test-fs-append-file-sync.js"),
    shared_official_batch_case!("test/parallel/test-fs-append-file.js"),
    shared_official_batch_case!("test/parallel/test-fs-access.js"),
    shared_official_batch_case!("test/parallel/test-fs-assert-encoding-error.js"),
    shared_official_batch_case!("test/parallel/test-fs-buffertype-writesync.js"),
    shared_official_batch_case!("test/parallel/test-fs-exists.js"),
    shared_official_batch_case!("test/parallel/test-fs-existssync-false.js"),
    shared_official_batch_case!("test/parallel/test-fs-null-bytes.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-fs-whatwg-url.js",
        SHARED_FIXTURES_DIR_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-fs-read-empty-buffer.js",
        SHARED_FIXTURES_DIR_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-fs-read-zero-length.js",
        SHARED_FIXTURES_DIR_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-fs-read.js",
        SHARED_FIXTURES_DIR_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-fs-read-file-assert-encoding.js",
        SHARED_FIXTURES_DIR_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-fs-read-file-sync.js",
        SHARED_FIXTURES_DIR_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-fs-read-optional-params.js",
        SHARED_FIXTURES_DIR_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-fs-readfile-empty.js",
        SHARED_FIXTURES_DIR_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-fs-readfile-fd.js",
        SHARED_FIXTURES_DIR_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-fs-readfile-flags.js"),
    shared_official_batch_case!("test/parallel/test-fs-readfile.js"),
    shared_official_batch_case!("test/parallel/test-fs-readfile-unlink.js"),
    shared_official_batch_case!("test/parallel/test-fs-readfile-zero-byte-liar.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-fs-readSync-optional-params.js",
        SHARED_FIXTURES_DIR_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-fs-read-type.js",
        SHARED_FIXTURES_DIR_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-fs-readv-promisify.js",
        SHARED_FIXTURES_DIR_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-fs-readv-promises.js"),
    shared_official_batch_case!("test/parallel/test-fs-readv-sync.js"),
    shared_official_batch_case!("test/parallel/test-fs-readv.js"),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-fs-open-flags.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-fs-open-flags.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-fs-open-flags.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-fs-open-flags.js"),
        shared_extra_files: SHARED_FIXTURES_DIR_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case!("test/parallel/test-fs-open-no-close.js"),
    shared_official_batch_case!("test/parallel/test-fs-open-numeric-flags.js"),
    shared_official_batch_case!("test/parallel/test-fs-open-mode-mask.js"),
    shared_official_batch_case!("test/parallel/test-fs-mkdir.js"),
    shared_official_batch_case!("test/parallel/test-fs-mkdir-mode-mask.js"),
    shared_official_batch_case!("test/parallel/test-fs-mkdir-rmdir.js"),
    shared_official_batch_case!("test/parallel/test-fs-chmod.js"),
    shared_official_batch_case!("test/parallel/test-fs-chmod-mask.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-fs-copyfile.js",
        SHARED_FIXTURES_DIR_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-fs-copyfile-respect-permissions.js"),
    shared_official_batch_case!("test/parallel/test-fs-mkdtemp.js"),
    shared_official_batch_case!("test/parallel/test-fs-mkdtemp-prefix-check.js"),
    shared_official_batch_case!("test/parallel/test-fs-link.js"),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-fs-symlink.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-fs-symlink.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-fs-symlink.js"),
        node24_fixture_source_path: None,
        shared_extra_files: CYCLE_FIXTURES_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case_with_extra!(
        "test/parallel/test-fs-realpath-buffer-encoding.js",
        CYCLE_FIXTURES_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-fs-realpath-native.js",
        CYCLE_FIXTURES_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-fs-readdir.js"),
    shared_official_batch_case!("test/parallel/test-fs-readdir-types.js"),
    shared_official_batch_case!("test/parallel/test-fs-readlink-type-check.js"),
    shared_official_batch_case!("test/parallel/test-fs-rename-type-check.js"),
    shared_official_batch_case!("test/parallel/test-fs-unlink-type-check.js"),
    shared_official_batch_case!("test/parallel/test-fs-fchmod.js"),
    shared_official_batch_case!("test/parallel/test-fs-fchown.js"),
    shared_official_batch_case!("test/parallel/test-fs-chown-type-check.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-fs-fsync.js",
        SHARED_FIXTURES_DIR_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-fs-statfs.js"),
    shared_official_batch_case!("test/parallel/test-fs-timestamp-parsing-error.js"),
    shared_official_batch_case!("test/parallel/test-fs-utimes.js"),
    shared_official_batch_case!("test/parallel/test-fs-non-number-arguments-throw.js"),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-fs-opendir.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-fs-opendir.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-fs-opendir.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case!("test/parallel/test-fs-rmdir-recursive.js"),
    shared_official_batch_case!("test/parallel/test-fs-rmdir-type-check.js"),
    shared_official_batch_case!("test/parallel/test-fs-rmdir-recursive-throws-not-found.js"),
    shared_official_batch_case!("test/parallel/test-fs-rmdir-recursive-throws-on-file.js"),
    shared_official_batch_case!("test/parallel/test-fs-rmdir-recursive-warns-not-found.js"),
    shared_official_batch_case!("test/parallel/test-fs-rmdir-recursive-warns-on-file.js"),
    shared_official_batch_case!("test/parallel/test-fs-rmdir-recursive-sync-warns-not-found.js"),
    shared_official_batch_case!("test/parallel/test-fs-rmdir-recursive-sync-warns-on-file.js"),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-fs-truncate.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-fs-truncate.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-fs-truncate.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-fs-truncate.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-fs-truncate-sync.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-fs-truncate-sync.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-fs-truncate-sync.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-fs-truncate-sync.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-fs-truncate-fd.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-fs-truncate-fd.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-fs-truncate-fd.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case!("test/parallel/test-fs-truncate-clear-file-zero.js"),
    node22_only_batch_case!(
        "test/parallel/test-fs-stat.js",
        "node22/test/parallel/test-fs-stat.js"
    ),
    shared_official_batch_case!("test/parallel/test-fs-write-buffer.js"),
    shared_official_batch_case!("test/parallel/test-fs-close-errors.js"),
    shared_official_batch_case!("test/parallel/test-fs-write-file-buffer.js"),
    shared_official_batch_case!("test/parallel/test-fs-writefile-with-fd.js"),
    shared_official_batch_case!("test/parallel/test-fs-write-file-flush.js"),
    shared_official_batch_case!("test/parallel/test-fs-write-file-typedarrays.js"),
    shared_official_batch_case!("test/parallel/test-fs-write-file.js"),
    shared_official_batch_case!("test/parallel/test-fs-write-no-fd.js"),
    shared_official_batch_case!("test/parallel/test-fs-write-optional-params.js"),
    shared_official_batch_case!("test/parallel/test-fs-write-negativeoffset.js"),
    shared_official_batch_case!("test/parallel/test-fs-util-validateoffsetlength.js"),
    shared_official_batch_case!("test/parallel/test-fs-write-sync.js"),
    shared_official_batch_case!("test/parallel/test-fs-write-sync-optional-params.js"),
    shared_official_batch_case!("test/parallel/test-fs-writev-promises.js"),
    shared_official_batch_case!("test/parallel/test-fs-writev.js"),
    shared_official_batch_case!("test/parallel/test-fs-writev-sync.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-fs-read-offset-null.js",
        SHARED_FIXTURES_DIR_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-fs-read-promises-optional-params.js",
        SHARED_FIXTURES_DIR_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-fs-promises-write-optional-params.js"),
    shared_official_batch_case!("test/parallel/test-fs-promisified.js"),
    shared_official_batch_case!("test/parallel/test-fs-promises-file-handle-write.js"),
    shared_official_batch_case!("test/parallel/test-fs-promises-file-handle-append-file.js"),
    shared_official_batch_case!("test/parallel/test-fs-promises-file-handle-stat.js"),
    shared_official_batch_case!("test/parallel/test-fs-promises-file-handle-chmod.js"),
    shared_official_batch_case!("test/parallel/test-fs-promises-file-handle-truncate.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-fs-promises-file-handle-sync.js",
        SHARED_FIXTURES_DIR_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-fs-promises-file-handle-writeFile.js"),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-fs-promises-file-handle-dispose.js",
        node20_fixture_source_path: Some(
            "node20/test/parallel/test-fs-promises-file-handle-dispose.js",
        ),
        node22_fixture_source_path: Some(
            "node22/test/parallel/test-fs-promises-file-handle-dispose.js",
        ),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case!("test/parallel/test-fs-promises-file-handle-stream.js"),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-fs-promises-file-handle-readLines.mjs",
        node20_fixture_source_path: Some(
            "node20/test/parallel/test-fs-promises-file-handle-readLines.mjs",
        ),
        node22_fixture_source_path: Some(
            "node22/test/parallel/test-fs-promises-file-handle-readLines.mjs",
        ),
        node24_fixture_source_path: Some(
            "node24/test/parallel/test-fs-promises-file-handle-readLines.mjs",
        ),
        shared_extra_files: &[],
        node20_extra_files: NODE20_COMMON_INDEX_MJS_EXTRA_FILES,
        node22_extra_files: NODE22_COMMON_INDEX_MJS_EXTRA_FILES,
        node24_extra_files: NODE24_COMMON_INDEX_MJS_EXTRA_FILES,
    },
    shared_official_batch_case!("test/parallel/test-fs-read-stream-file-handle.js"),
    shared_official_batch_case!("test/parallel/test-fs-write-stream-file-handle.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-fs-promises-file-handle-readFile.js",
        COMMON_TICK_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-fs-promises-file-handle-read.js",
        SHARED_FIXTURES_DIR_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-fs-promises-file-handle-op-errors.js"),
    shared_official_batch_case!("test/parallel/test-fs-promises-file-handle-aggregate-errors.js"),
    shared_official_batch_case!("test/parallel/test-fs-promises-file-handle-close.js"),
    shared_official_batch_case!("test/parallel/test-fs-promises-file-handle-close-errors.js"),
    shared_official_batch_case!("test/parallel/test-fs-promises-exists.js"),
    shared_official_batch_case!("test/parallel/test-fs-promises-readfile.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-fs-promises-readfile-empty.js",
        SHARED_FIXTURES_DIR_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-fs-promises-readfile-with-fd.js"),
    shared_official_batch_case!("test/parallel/test-fs-promises-writefile-typedarray.js"),
    shared_official_batch_case!("test/parallel/test-fs-promises-writefile-with-fd.js"),
    shared_official_batch_case!("test/parallel/test-fs-promises-writefile.js"),
    shared_official_batch_case!("test/parallel/test-fs-append-file-flush.js"),
    shared_official_batch_case!("test/parallel/test-fs-read-stream-fd.js"),
    shared_official_batch_case!("test/parallel/test-fs-read-stream-autoClose.js"),
    shared_official_batch_case!("test/parallel/test-fs-read-stream-double-close.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-fs-empty-readStream.js",
        SHARED_FIXTURES_DIR_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-fs-read-stream-encoding.js",
        SHARED_FIXTURES_DIR_EXTRA_FILES
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-fs-write-stream.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-fs-write-stream.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-fs-write-stream.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case!("test/parallel/test-fs-write-stream-end.js"),
    shared_official_batch_case!("test/parallel/test-fs-write-stream-double-close.js"),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-fs-write-stream-autoclose-option.js",
        node20_fixture_source_path: Some(
            "node20/test/parallel/test-fs-write-stream-autoclose-option.js",
        ),
        node22_fixture_source_path: Some(
            "node22/test/parallel/test-fs-write-stream-autoclose-option.js",
        ),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case_with_extra!(
        "test/parallel/test-fs-write-stream-encoding.js",
        SHARED_FIXTURES_DIR_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-fs-ready-event-stream.js"),
    shared_official_batch_case!("test/parallel/test-fs-sync-fd-leak.js"),
    shared_official_batch_case!("test/parallel/test-fs-operations-with-surrogate-pairs.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-fs-symlink-buffer-path.js",
        CYCLE_FIXTURES_EXTRA_FILES
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-fs-glob.mjs",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("node22/test/parallel/test-fs-glob.mjs"),
        node24_fixture_source_path: Some("node24/test/parallel/test-fs-glob.mjs"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: NODE22_COMMON_INDEX_MJS_EXTRA_FILES,
        node24_extra_files: NODE24_COMMON_INDEX_MJS_EXTRA_FILES,
    },
    shared_official_batch_case!("test/parallel/test-fs-options-immutable.js"),
    shared_official_batch_case!("test/parallel/test-fs-watch.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-fs-watch-abort-signal.js",
        SHARED_FIXTURES_DIR_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-fs-watch-ref-unref.js"),
    shared_official_batch_case!("test/parallel/test-fs-watch-stop-async.js"),
    shared_official_batch_case!("test/parallel/test-fs-watch-stop-sync.js"),
    shared_official_batch_case!("test/parallel/test-fs-watch-enoent.js"),
    shared_official_batch_case!("test/parallel/test-fs-watch-encoding.js"),
    shared_official_batch_case!("test/parallel/test-fs-watchfile.js"),
    shared_official_batch_case!("test/parallel/test-fs-watchfile-bigint.js"),
    shared_official_batch_case!("test/parallel/test-fs-watchfile-ref-unref.js"),
    shared_official_batch_case!("test/parallel/test-fs-watch-file-enoent-after-deletion.js"),
    shared_official_batch_case!("test/parallel/test-fs-watch-close-when-destroyed.js"),
    shared_official_batch_case!("test/parallel/test-fs-watch-recursive-promise.js"),
    shared_official_batch_case!("test/parallel/test-fs-watch-recursive-add-file.js"),
    shared_official_batch_case!("test/parallel/test-fs-watch-recursive-update-file.js"),
    shared_official_batch_case!("test/parallel/test-fs-watch-recursive-delete.js"),
    shared_official_batch_case!("test/parallel/test-fs-watch-recursive-add-folder.js"),
    shared_official_batch_case!("test/parallel/test-fs-watch-recursive-add-file-with-url.js"),
    shared_official_batch_case!("test/parallel/test-fs-watch-recursive-validate.js"),
    shared_official_batch_case!("test/parallel/test-fs-watch-recursive-sync-write.js"),
    shared_official_batch_case!("test/parallel/test-fs-watch-recursive-add-file-to-new-folder.js"),
    shared_official_batch_case!(
        "test/parallel/test-fs-watch-recursive-add-file-to-existing-subfolder.js"
    ),
    shared_official_batch_case!("test/parallel/test-fs-watch-recursive-watch-file.js"),
    shared_official_batch_case!("test/parallel/test-fs-watch-recursive-symlink.js"),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-fs-promises-watch.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-fs-promises-watch.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-fs-promises-watch.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NETWORKING_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-dns-get-server.js"),
    shared_official_batch_case!("test/parallel/test-dns-set-default-order.js"),
    shared_official_batch_case!("test/parallel/test-dns-default-order-ipv4.js"),
    shared_official_batch_case!("test/parallel/test-dns-default-order-ipv6.js"),
    shared_official_batch_case!("test/parallel/test-dns-default-order-verbatim.js"),
    shared_official_batch_case!("test/parallel/test-stream-finished.js"),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-stream-pipeline.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-stream-pipeline.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-stream-pipeline.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case!("test/parallel/test-net-connect-options-invalid.js"),
    shared_official_batch_case!("test/parallel/test-net-isip.js"),
    shared_official_batch_case!("test/parallel/test-net-isipv4.js"),
    shared_official_batch_case!("test/parallel/test-net-isipv6.js"),
    shared_official_batch_case!("test/parallel/test-net-connect-no-arg.js"),
    shared_official_batch_case!("test/parallel/test-net-listening.js"),
    shared_official_batch_case!("test/parallel/test-net-listen-close-server.js"),
    shared_official_batch_case!("test/parallel/test-net-server-close.js"),
    shared_official_batch_case!("test/parallel/test-net-server-call-listen-multiple-times.js"),
    shared_official_batch_case!("test/parallel/test-net-server-listen-options.js"),
    shared_official_batch_case!("test/parallel/test-net-server-listen-options-signal.js"),
    shared_official_batch_case!("test/parallel/test-net-server-unref-persistent.js"),
    shared_official_batch_case!("test/parallel/test-net-after-close.js"),
    shared_official_batch_case!("test/parallel/test-net-settimeout.js"),
    shared_official_batch_case!("test/parallel/test-net-can-reset-timeout.js"),
    shared_official_batch_case!("test/parallel/test-net-socket-close-after-end.js"),
    shared_official_batch_case!("test/parallel/test-net-socket-connecting.js"),
    shared_official_batch_case!("test/parallel/test-net-local-address-port.js"),
    shared_official_batch_case!("test/parallel/test-http-client-defaults.js"),
    shared_official_batch_case!("test/parallel/test-http-client-get-url.js"),
    shared_official_batch_case!("test/parallel/test-http-client-request-options.js"),
    shared_official_batch_case!("test/parallel/test-http-client-upload.js"),
    shared_official_batch_case!("test/parallel/test-http-client-upload-buf.js"),
    shared_official_batch_case!("test/parallel/test-http-automatic-headers.js"),
    shared_official_batch_case!("test/parallel/test-http-client-close-event.js"),
    shared_official_batch_case!("test/parallel/test-http-agent-getname.js"),
    shared_official_batch_case!("test/parallel/test-http-agent-close.js"),
    shared_official_batch_case!("test/parallel/test-http-agent-timeout-option.js"),
    shared_official_batch_case!("test/parallel/test-http-agent-keepalive.js"),
    shared_official_batch_case!("test/parallel/test-http-agent-keepalive-delay.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-http-agent-maxsockets.js",
        COMMON_COUNTDOWN_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-http-agent-maxsockets-respected.js",
        COMMON_COUNTDOWN_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-http-agent-maxtotalsockets.js",
        COMMON_COUNTDOWN_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-http-agent-scheduling.js"),
    shared_official_batch_case!("test/parallel/test-http-agent-timeout.js"),
    shared_official_batch_case!("test/parallel/test-http-agent-false.js"),
    shared_official_batch_case!("test/parallel/test-http-agent-no-protocol.js"),
    shared_official_batch_case!("test/parallel/test-http-agent-null.js"),
    shared_official_batch_case!("test/parallel/test-http-agent-remove.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-http-agent-destroyed-socket.js",
        COMMON_COUNTDOWN_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-http-agent-error-on-idle.js"),
    shared_official_batch_case!("test/parallel/test-http-agent-uninitialized.js"),
    shared_official_batch_case!("test/parallel/test-http-agent-uninitialized-with-handle.js"),
    shared_official_batch_case!("test/parallel/test-http-agent-abort-controller.js"),
    shared_official_batch_case!("test/parallel/test-http-client-timeout-option.js"),
    shared_official_batch_case!("test/parallel/test-http-client-set-timeout.js"),
    shared_official_batch_case!("test/parallel/test-http-client-response-timeout.js"),
    shared_official_batch_case!("test/parallel/test-http-set-timeout.js"),
    shared_official_batch_case!("test/parallel/test-http-contentLength0.js"),
    shared_official_batch_case!("test/parallel/test-http-head-request.js"),
    shared_official_batch_case!("test/parallel/test-http-server-options-incoming-message.js"),
    shared_official_batch_case!("test/parallel/test-http-server-options-server-response.js"),
    shared_official_batch_case!("test/parallel/test-http-response-add-header-after-sent.js"),
    shared_official_batch_case!("test/parallel/test-http-response-remove-header-after-sent.js"),
    shared_official_batch_case!("test/parallel/test-http-response-no-headers.js"),
    shared_official_batch_case!("test/parallel/test-http-response-readable.js"),
    shared_official_batch_case!("test/parallel/test-http-response-setheaders.js"),
    shared_official_batch_case!("test/parallel/test-http-response-close.js"),
    shared_official_batch_case!("test/parallel/test-http-response-cork.js"),
    shared_official_batch_case!("test/parallel/test-http-response-multi-content-length.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-http-status-code.js",
        COMMON_COUNTDOWN_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-http-response-multiheaders.js",
        COMMON_COUNTDOWN_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-http-head-response-has-no-body.js"),
    shared_official_batch_case!("test/parallel/test-http-head-response-has-no-body-end.js"),
    shared_official_batch_case!(
        "test/parallel/test-http-head-response-has-no-body-end-implicit-headers.js"
    ),
    shared_official_batch_case!("test/parallel/test-http-head-throw-on-response-body-write.js"),
    shared_official_batch_case!("test/parallel/test-http-status-message.js"),
    shared_official_batch_case!("test/parallel/test-http-write-head-2.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-http-status-reason-invalid-chars.js",
        COMMON_COUNTDOWN_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-http-response-statuscode.js",
        COMMON_COUNTDOWN_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-http-response-status-message.js",
        COMMON_COUNTDOWN_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-http-write-head.js"),
    shared_official_batch_case!("test/parallel/test-http-response-writehead-returns-this.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-http-response-splitting.js",
        COMMON_COUNTDOWN_EXTRA_FILES
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-http-write-head-after-set-header.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some(
            "node22/test/parallel/test-http-write-head-after-set-header.js",
        ),
        node24_fixture_source_path: Some(
            "node24/test/parallel/test-http-write-head-after-set-header.js",
        ),
        shared_extra_files: COMMON_COUNTDOWN_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case!("test/parallel/test-net-end-without-connect.js"),
    shared_official_batch_case!("test/parallel/test-http2-util-asserts.js"),
    shared_official_batch_case!("test/parallel/test-http2-util-assert-valid-pseudoheader.js"),
    shared_official_batch_case!("test/parallel/test-http2-util-nghttp2error.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-agent-constructor.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-agent-getname.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-agent.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-agent-abort-controller.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-server-options-incoming-message.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-server-options-server-response.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-client-get-url.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-http2-getpackedsettings.js"),
    shared_official_batch_case!("test/parallel/test-http2-util-headers-list.js"),
    shared_official_batch_case!("test/parallel/test-http2-util-update-options-buffer.js"),
    shared_official_batch_case!("test/parallel/test-http2-misc-util.js"),
    shared_official_batch_case!("test/parallel/test-http2-status-code.js"),
    shared_official_batch_case!("test/parallel/test-http2-status-code-invalid.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-http2-multi-content-length.js",
        COMMON_COUNTDOWN_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-http2-response-splitting.js"),
    shared_official_batch_case!("test/parallel/test-http2-options-server-request.js"),
    shared_official_batch_case!("test/parallel/test-http2-options-server-response.js"),
    shared_official_batch_case!("test/parallel/test-http2-zero-length-header.js"),
    shared_official_batch_case!("test/parallel/test-http2-multiheaders.js"),
    shared_official_batch_case!("test/parallel/test-http2-multiheaders-raw.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-end.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-write.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-writehead.js"),
    shared_official_batch_case!(
        "test/parallel/test-http2-compat-serverresponse-writehead-array.js"
    ),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-statuscode.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-statusmessage.js"),
    shared_official_batch_case!(
        "test/parallel/test-http2-compat-serverresponse-statusmessage-property.js"
    ),
    shared_official_batch_case!(
        "test/parallel/test-http2-compat-serverresponse-statusmessage-property-set.js"
    ),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-headers.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverrequest.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverrequest-end.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverrequest-headers.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverrequest-host.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverrequest-pause.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-http2-compat-serverrequest-pipe.js",
        COMMON_HTTP2_COMPAT_SERVERREQUEST_PIPE_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverrequest-settimeout.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverrequest-trailers.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-close.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-http2-compat-serverresponse-destroy.js",
        COMMON_COUNTDOWN_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-drain.js"),
    shared_official_batch_case!(
        "test/parallel/test-http2-compat-serverresponse-end-after-statuses-without-body.js"
    ),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-finished.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-flushheaders.js"),
    shared_official_batch_case!(
        "test/parallel/test-http2-compat-serverresponse-headers-after-destroy.js"
    ),
    shared_official_batch_case!(
        "test/parallel/test-http2-compat-serverresponse-headers-send-date.js"
    ),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-settimeout.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-trailers.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-write-early-hints.js"),
    shared_official_batch_case!(
        "test/parallel/test-http2-compat-write-early-hints-invalid-argument-type.js"
    ),
    shared_official_batch_case!(
        "test/parallel/test-http2-compat-write-early-hints-invalid-argument-value.js"
    ),
    shared_official_batch_case!("test/parallel/test-http2-compat-write-head-destroyed.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-aborted.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-http2-compat-client-upload-reject.js",
        COMMON_HTTP2_COMPAT_SERVERREQUEST_PIPE_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-http2-compat-errors.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-expect-continue-check.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-expect-continue.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-expect-handling.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-method-connect.js"),
    shared_official_batch_case!(
        "test/parallel/test-http2-compat-serverresponse-createpushresponse.js"
    ),
    shared_official_batch_case!("test/parallel/test-http2-compat-short-stream-client-server.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-socket-destroy-delayed.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-socket-set.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-socket.js"),
    shared_node20_node22_with_node24_override_case_with_extra!(
        "test/parallel/test-https-agent-create-connection.js",
        "node24/test/parallel/test-https-agent-create-connection.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_node20_node22_batch_case_with_extra!(
        "test/parallel/test-https-agent-disable-session-reuse.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_node20_node22_with_node24_override_case_with_extra!(
        "test/parallel/test-https-agent-servername.js",
        "node24/test/parallel/test-https-agent-servername.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_node20_node22_batch_case_with_extra!(
        "test/parallel/test-https-agent-session-injection.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_node20_node22_with_node24_override_case_with_extra!(
        "test/parallel/test-https-agent-sni.js",
        "node24/test/parallel/test-https-agent-sni.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_node20_node22_batch_case_with_extra!(
        "test/parallel/test-https-agent-sockets-leak.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_node20_node22_batch_case_with_extra!(
        "test/parallel/test-https-client-override-global-agent.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-abortcontroller.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-argument-of-creating.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-byteswritten.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-close.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-max-headers-count.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-request-arguments.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-server-headers-timeout.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-server-request-timeout.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-set-timeout-server.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-simple.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-timeout.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-timeout-server.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-timeout-server-2.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-server-close-all.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-server-close-destroy-timeout.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-server-close-idle.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-socket-options.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-keep-alive-drop-requests.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-server-connections-checking-leak.js",
        COMMON_TLS_KEY_COUNTDOWN_GC_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-client-checkServerIdentity.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-client-reject.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-connecting-to-http.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-drain.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-eof-for-eom.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-host-headers.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-insecure-parse-per-stream.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-max-header-size-per-stream.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-options-boolean-check.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-server-async-dispose.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-truncate.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-selfsigned-no-keycertsign-no-crash.js",
        COMMON_TLS_SESSION_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-client-resume.js",
        COMMON_TLS_SESSION_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-resume-after-renew.js",
        COMMON_TLS_SESSION_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-pfx.js",
        COMMON_TLS_SESSION_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-unix-socket-self-signed.js",
        COMMON_TLS_SESSION_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-strict.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-https-agent-session-reuse.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("node22/test/parallel/test-https-agent-session-reuse.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TLS_KEY_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-https-agent-keylog.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("node22/test/parallel/test-https-agent-keylog.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-https-agent-keylog.js"),
        shared_extra_files: COMMON_TLS_KEY_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-https-hwm.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("node22/test/parallel/test-https-hwm.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-https-hwm.js"),
        shared_extra_files: COMMON_TLS_SESSION_CERT_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case!("test/parallel/test-tls-basic-validations.js"),
    shared_official_batch_case!("test/parallel/test-tls-check-server-identity.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-tls-connect-abort-controller.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-tls-connect-allow-half-open-option.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-tls-connect-hwm-option.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("node22/test/parallel/test-tls-connect-hwm-option.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-tls-connect-hwm-option.js"),
        shared_extra_files: COMMON_TLS_EXTENDED_CERT_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case_with_extra!(
        "test/parallel/test-tls-connect-no-host.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-tls-connect-simple.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-tls-connect-timeout-option.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-tls-options-boolean-check.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-tls-server-parent-constructor-options.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-dgram-bytes-length.js"),
    shared_official_batch_case!("test/parallel/test-dgram-createSocket-type.js"),
    shared_official_batch_case!("test/parallel/test-dgram-send-address-types.js"),
    shared_official_batch_case!("test/parallel/test-dgram-send-bad-arguments.js"),
    shared_official_batch_case!("test/parallel/test-dgram-send-invalid-msg-type.js"),
    shared_official_batch_case!("test/parallel/test-dgram-close-is-not-callback.js"),
    shared_official_batch_case!("test/parallel/test-dgram-send-empty-array.js"),
    shared_official_batch_case!("test/parallel/test-dgram-send-empty-buffer.js"),
    shared_official_batch_case!("test/parallel/test-dgram-address.js"),
    shared_official_batch_case!("test/parallel/test-dgram-bind-default-address.js"),
    shared_official_batch_case!("test/parallel/test-dgram-bind.js"),
    shared_official_batch_case!("test/parallel/test-dgram-close.js"),
    shared_official_batch_case!("test/parallel/test-dgram-listen-after-bind.js"),
    shared_official_batch_case!("test/parallel/test-dgram-ref.js"),
    shared_official_batch_case!("test/parallel/test-dgram-unref.js"),
    shared_official_batch_case!("test/parallel/test-dgram-implicit-bind.js"),
    shared_official_batch_case!("test/parallel/test-dgram-connect.js"),
    shared_official_batch_case!("test/parallel/test-dgram-connect-send-callback-buffer.js"),
    shared_official_batch_case!("test/parallel/test-dgram-connect-send-callback-buffer-length.js"),
    shared_official_batch_case!("test/parallel/test-dgram-connect-send-callback-multi-buffer.js"),
    shared_official_batch_case!("test/parallel/test-dgram-connect-send-default-host.js"),
    shared_official_batch_case!("test/parallel/test-dgram-connect-send-empty-array.js"),
    shared_official_batch_case!("test/parallel/test-dgram-connect-send-empty-buffer.js"),
    shared_official_batch_case!("test/parallel/test-dgram-connect-send-empty-packet.js"),
    shared_official_batch_case!("test/parallel/test-dgram-send-callback-buffer-empty-address.js"),
    shared_official_batch_case!(
        "test/parallel/test-dgram-send-callback-buffer-length-empty-address.js"
    ),
    shared_official_batch_case!("test/parallel/test-dgram-send-callback-buffer-length.js"),
    shared_official_batch_case!("test/parallel/test-dgram-send-callback-buffer.js"),
    shared_official_batch_case!(
        "test/parallel/test-dgram-send-callback-multi-buffer-empty-address.js"
    ),
    shared_official_batch_case!("test/parallel/test-dgram-send-callback-multi-buffer.js"),
    shared_official_batch_case!("test/parallel/test-dgram-send-callback-recursive.js"),
    shared_official_batch_case!("test/parallel/test-dgram-send-cb-quelches-error.js"),
    shared_official_batch_case!("test/parallel/test-dgram-send-default-host.js"),
    shared_official_batch_case!("test/parallel/test-dgram-send-empty-packet.js"),
    shared_official_batch_case!("test/parallel/test-dgram-send-multi-buffer-copy.js"),
    shared_official_batch_case!("test/parallel/test-dgram-send-multi-string-array.js"),
    shared_official_batch_case!("test/parallel/test-dgram-sendto.js"),
    shared_official_batch_case!("test/parallel/test-dgram-abort-closed.js"),
    shared_official_batch_case!("test/parallel/test-dgram-bind-error-repeat.js"),
    shared_official_batch_case!("test/parallel/test-dgram-bind-fd-error.js"),
    shared_official_batch_case!("test/parallel/test-dgram-bind-fd.js"),
    shared_official_batch_case!("test/parallel/test-dgram-bind-socket-close-before-lookup.js"),
    shared_official_batch_case!("test/parallel/test-dgram-blocklist.js"),
    shared_official_batch_case!("test/parallel/test-dgram-close-during-bind.js"),
    shared_official_batch_case!("test/parallel/test-dgram-close-in-listening.js"),
    shared_official_batch_case!("test/parallel/test-dgram-close-signal.js"),
    shared_official_batch_case!("test/parallel/test-dgram-connect-send-multi-buffer-copy.js"),
    shared_official_batch_case!("test/parallel/test-dgram-connect-send-multi-string-array.js"),
    shared_official_batch_case!("test/parallel/test-dgram-create-socket-handle-fd.js"),
    shared_official_batch_case!("test/parallel/test-dgram-create-socket-handle.js"),
    shared_official_batch_case!("test/parallel/test-dgram-custom-lookup.js"),
    shared_official_batch_case!("test/parallel/test-dgram-membership.js"),
    shared_official_batch_case!("test/parallel/test-dgram-msgsize.js"),
    shared_official_batch_case!("test/parallel/test-dgram-multicast-loopback.js"),
    shared_official_batch_case!("test/parallel/test-dgram-multicast-set-interface.js"),
    shared_official_batch_case!("test/parallel/test-dgram-multicast-setTTL.js"),
    shared_official_batch_case!("test/parallel/test-dgram-oob-buffer.js"),
    shared_official_batch_case!("test/parallel/test-dgram-recv-error.js"),
    shared_official_batch_case!("test/parallel/test-dgram-send-error.js"),
    shared_official_batch_case!("test/parallel/test-dgram-send-queue-info.js"),
    shared_official_batch_case!("test/parallel/test-dgram-setBroadcast.js"),
    shared_official_batch_case!("test/parallel/test-dgram-setTTL.js"),
    shared_official_batch_case!("test/parallel/test-dgram-socket-buffer-size.js"),
    shared_official_batch_case!("test/parallel/test-dgram-udp4.js"),
];

fn node_compat_fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/runtime/tests/node_compat_fixtures")
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

// Some async_hooks promise-enable fixtures intentionally count promise hook
// callbacks around already in-flight promises. The default node_compat bundle
// wrapper adds extra Promise/queueMicrotask drains after import, which becomes
// observable noise for those files and obscures the real owner seam.
fn should_skip_default_async_drains_for_fixture(test_relative_path: &str) -> bool {
    matches!(
        test_relative_path,
        "test/parallel/test-async-hooks-disable-during-promise.js"
            | "test/parallel/test-async-hooks-enable-before-promise-resolve.js"
            | "test/parallel/test-async-hooks-enable-during-promise.js"
            | "test/parallel/test-async-hooks-promise-triggerid.js"
            | "test/parallel/test-async-hooks-promise.js"
    )
}

fn should_use_sync_tick_drain_for_fixture(test_relative_path: &str) -> bool {
    matches!(
        test_relative_path,
        "test/parallel/test-async-hooks-disable-during-promise.js"
            | "test/parallel/test-async-hooks-promise-triggerid.js"
            | "test/parallel/test-async-hooks-promise.js"
    )
}

fn should_quiesce_then_require_fixture(test_relative_path: &str) -> bool {
    matches!(
        test_relative_path,
        "test/parallel/test-async-hooks-disable-during-promise.js"
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
        || should_capture_top_level_import_error_for_fixture(test_relative_path);
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
    let use_sync_tick_drain = should_use_sync_tick_drain_for_fixture(test_relative_path);
    let async_drain_script = if use_sync_tick_drain {
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
{gc_setup_script}
if (typeof globalThis.process === "object" && globalThis.process !== null) {{
  globalThis.process.execPath = __nimbusCompatExecPath;
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
  const require = createRequire(import.meta.url);
{invoke_import_guard}
  const common = require("./test/common/index.js");
{async_drain_script}
{postlude_script}
  common.__nimbusAssert?.();
  return {{
    ok: true,
    skipped: false,
    testPath: "{test_relative_path}",
  }};
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
    let fixture_needs_pending_deprecation = fixture_requests_pending_deprecation(test_source);
    let resolved_prelude_behavior =
        prelude_script.and_then(NodeCompatNamedPreludeBehavior::from_script);
    let _interactive_term_guard = matches!(
        resolved_prelude_behavior,
        Some(NodeCompatNamedPreludeBehavior::InteractiveTerminal)
    )
    .then(|| ScopedProcessEnvVar::set("TERM", "xterm-256color"));
    let _pending_deprecation_guard = fixture_needs_pending_deprecation
        .then(|| scoped_node_options_flag("--pending-deprecation"));
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
        mode: NodeCompatBundleMode::Runtime,
    });
    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(runtime_limits_for_node_compat_fixture(
            test_relative_path,
        ))),
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

    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(async {
            runtime
                .invoke_bundle(&RuntimeBundle::new(&bundle_path), &request)
                .await
        });

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
                return Err(format!(
                    "upstream node_compat fixture `{test_relative_path}` exited with non-zero code {exit_code}: {error}"
                ));
            }
            return Err(format!(
                "upstream node_compat fixture `{test_relative_path}` should execute: {error}"
            ));
        }
    };

    if result.get("ok") != Some(&serde_json::json!(true)) {
        return Err(format!(
            "upstream node_compat fixture `{test_relative_path}` returned non-ok payload: {result}"
        ));
    }

    if result.get("testPath") != Some(&serde_json::json!(test_relative_path)) {
        return Err(format!(
            "upstream node_compat fixture `{test_relative_path}` returned mismatched testPath payload: {result}"
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

fn runtime_limits_for_node_compat_fixture(test_relative_path: &str) -> RuntimeLimits {
    let mut limits = RuntimeLimits::application_node22();
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
    let runner_limits =
        runtime_limits_for_node_compat_fixture("test/parallel/test-runner-reporters.js");
    assert_eq!(runner_limits.grants.run, vec!["$runtime_self_exec"]);

    let wasi_limits = runtime_limits_for_node_compat_fixture("test/wasi/test-wasi-stdio.js");
    assert_eq!(wasi_limits.grants.run, vec!["$runtime_self_exec"]);

    let ordinary_limits =
        runtime_limits_for_node_compat_fixture("test/parallel/test-runner-assert.js");
    assert_eq!(
        ordinary_limits.grants.run,
        vec!["$runtime_self_exec"],
        "test-runner fixtures currently opt into the compat self-exec seam as a family",
    );

    let non_respawn_limits =
        runtime_limits_for_node_compat_fixture("test/parallel/test-repl-mode.js");
    assert!(non_respawn_limits.grants.run.is_empty());
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
    let owned_extra_files: Vec<(String, Vec<u8>)> = extra_files
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

fn resolve_seeded_fixture_context(
    lane_name: &str,
    test_relative_path: &str,
) -> std::result::Result<
    (
        NodeCompatLane,
        String,
        String,
        &'static NodeCompatBatchEntry,
        &'static str,
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
    let manifest_fixture_source_path =
        fixture_seed.lane_sources.get(lane_name).ok_or_else(|| {
            format!("seeded manifest fixture `{test_relative_path}` has no `{lane_name}` source")
        })?;
    let batch_entry = family_batch_entries(&family_catalog.family)?
        .iter()
        .find(|entry| entry.test_relative_path == test_relative_path)
        .ok_or_else(|| {
            format!(
                "seeded manifest fixture `{test_relative_path}` is missing from family batch `{}`",
                family_catalog.family
            )
        })?;
    let batch_fixture_source_path = batch_entry
        .fixture_source_path_for_lane(lane)
        .ok_or_else(|| {
            format!(
                "seeded family batch `{}` fixture `{test_relative_path}` has no `{lane_name}` source",
                family_catalog.family
            )
        })?;
    if batch_fixture_source_path != manifest_fixture_source_path {
        return Err(format!(
            "seeded manifest fixture `{test_relative_path}` mismatched `{lane_name}` source: manifest=`{manifest_fixture_source_path}` batch=`{batch_fixture_source_path}`"
        ));
    }
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
            fixture_source_path,
            batch_entry.extra_files_for_lane(lane),
            matches!(lane, NodeCompatLane::Node24),
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
    let test_source = read_node_compat_fixture_text(fixture_source_path);
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
    let fixture_needs_pending_deprecation = fixture_requests_pending_deprecation(&test_source);
    let mut startup_flags = Vec::new();
    if fixture_needs_pending_deprecation {
        startup_flags.push("--pending-deprecation".to_string());
    }
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
        capture_top_level_skip: matches!(lane, NodeCompatLane::Node24),
        lane: Some(lane),
        prelude_script: Some(effective_prelude.as_str()),
        postlude_script: resolved_postlude_behavior.map(NodeCompatNamedPostludeBehavior::script),
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
    let lane_name = node_compat_lane_name(lane);
    let mut passed = 0usize;
    let mut skipped = Vec::new();
    let mut failures = Vec::new();

    for fixture in fixtures {
        if let Some(fixture_source_path) = fixture.fixture_source_path_for_lane(lane) {
            eprintln!(
                "node_compat {batch_name} {lane_name} -> {}",
                fixture.test_relative_path
            );
            let snapshot = NodeCompatHostProcessSnapshot::capture();
            let execution = panic::catch_unwind(AssertUnwindSafe(|| {
                execute_manifested_node_compat_test(
                    fixture.test_relative_path,
                    fixture_source_path,
                    fixture.extra_files_for_lane(lane),
                    matches!(lane, NodeCompatLane::Node24),
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
        "node_compat {batch_name} {lane_name} summary -> passed: {passed}, skipped: {}, failed: {}",
        skipped.len(),
        failures.len()
    );
    if !skipped.is_empty() {
        eprintln!(
            "node_compat {batch_name} {lane_name} skipped fixtures:\n{}",
            skipped.join("\n")
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
    execute_manifested_node_compat_test(
        test_relative_path,
        fixture_source_path,
        extra_files,
        false,
        Some(lane),
        None,
        None,
    )
    .unwrap_or_else(|error| panic!("{error}"));
}

fn run_node_compat_watchpoint(
    test_relative_path: &str,
    fixture_source_path: &str,
    extra_files: &[NodeCompatExtraFixtureEntry],
) {
    execute_manifested_node_compat_test(
        test_relative_path,
        fixture_source_path,
        extra_files,
        false,
        None,
        None,
        None,
    )
    .unwrap_or_else(|error| panic!("{error}"));
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
                    fixture_source_path,
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
            if fixture_source_path != resolved_fixture.fixture_source_path {
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
                    fixture_source_path,
                    batch_entry.extra_files_for_lane(lane),
                    matches!(lane, NodeCompatLane::Node24),
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

const NODE22_STREAM_STATE_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-stream-decoder-objectmode.js",
    "test/parallel/test-stream-push-strings.js",
    "test/parallel/test-stream-readable-error-end.js",
    "test/parallel/test-stream-readable-with-unimplemented-_read.js",
    "test/parallel/test-stream-transform-hwm0.js",
    "test/parallel/test-stream-unshift-read-race.js",
    "test/parallel/test-stream-writable-clear-buffer.js",
    "test/parallel/test-stream-writable-null.js",
    "test/parallel/test-stream-writableState-ending.js",
    "test/parallel/test-stream-writableState-uncorked-bufferedRequestCount.js",
];

const NODE22_STREAM_BUFFERING_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-stream-aliases-legacy.js",
    "test/parallel/test-stream-await-drain-writers-in-synchronously-recursion-write.js",
    "test/parallel/test-stream-backpressure.js",
    "test/parallel/test-stream-big-packet.js",
    "test/parallel/test-stream-big-push.js",
    "test/parallel/test-stream-err-multiple-callback-construction.js",
    "test/parallel/test-stream-pipe-deadlock.js",
    "test/parallel/test-stream-pipe-same-destination-twice.js",
    "test/parallel/test-stream-push-order.js",
    "test/parallel/test-stream-readable-object-multi-push-async.js",
];

const NODE22_TTY_OS_TAIL_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-os-eol.js",
    "test/parallel/test-os-checked-function.js",
    "test/parallel/test-ttywrap-invalid-fd.js",
    "test/parallel/test-ttywrap-stack.js",
];

const NODE22_NETWORKING_PURE_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-dns-get-server.js",
    "test/parallel/test-dns-set-default-order.js",
    "test/parallel/test-dns-default-order-ipv4.js",
    "test/parallel/test-dns-default-order-ipv6.js",
    "test/parallel/test-dns-default-order-verbatim.js",
    "test/parallel/test-net-connect-options-invalid.js",
    "test/parallel/test-net-isip.js",
    "test/parallel/test-net-isipv4.js",
    "test/parallel/test-net-isipv6.js",
    "test/parallel/test-http-agent-getname.js",
    "test/parallel/test-http-agent-close.js",
    "test/parallel/test-http-agent-timeout-option.js",
    "test/parallel/test-http2-util-asserts.js",
    "test/parallel/test-http2-util-nghttp2error.js",
];

const NODE22_NETWORKING_NET_SERVER_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-net-connect-no-arg.js",
    "test/parallel/test-net-listening.js",
    "test/parallel/test-net-listen-close-server.js",
    "test/parallel/test-net-server-close.js",
    "test/parallel/test-net-server-call-listen-multiple-times.js",
    "test/parallel/test-net-server-listen-options.js",
    "test/parallel/test-net-server-listen-options-signal.js",
];

const NODE22_NETWORKING_NET_SOCKET_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-net-after-close.js",
    "test/parallel/test-net-settimeout.js",
    "test/parallel/test-net-can-reset-timeout.js",
    "test/parallel/test-net-socket-close-after-end.js",
    "test/parallel/test-net-socket-connecting.js",
    "test/parallel/test-net-local-address-port.js",
];

const NODE22_NETWORKING_HTTP_REQUEST_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-http-client-defaults.js",
    "test/parallel/test-http-client-get-url.js",
    "test/parallel/test-http-client-request-options.js",
    "test/parallel/test-http-client-upload.js",
    "test/parallel/test-http-client-upload-buf.js",
    "test/parallel/test-http-automatic-headers.js",
    "test/parallel/test-http-client-close-event.js",
];

const NODE22_NETWORKING_HTTP_TIMEOUT_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-http-client-timeout-option.js",
    "test/parallel/test-http-client-set-timeout.js",
    "test/parallel/test-http-client-response-timeout.js",
    "test/parallel/test-http-set-timeout.js",
];

const NODE22_NETWORKING_HTTP_RESPONSE_POSITIVE_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-http-contentLength0.js",
    "test/parallel/test-http-head-request.js",
    "test/parallel/test-http-response-writehead-returns-this.js",
];

const NODE22_NETWORKING_HTTP_RESPONSE_STATE_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-http-response-add-header-after-sent.js",
    "test/parallel/test-http-response-remove-header-after-sent.js",
    "test/parallel/test-http-response-no-headers.js",
    "test/parallel/test-http-response-readable.js",
    "test/parallel/test-http-response-setheaders.js",
    "test/parallel/test-http-response-close.js",
    "test/parallel/test-http-response-cork.js",
    "test/parallel/test-http-response-multi-content-length.js",
    "test/parallel/test-http-head-response-has-no-body.js",
    "test/parallel/test-http-head-response-has-no-body-end.js",
    "test/parallel/test-http-head-response-has-no-body-end-implicit-headers.js",
    "test/parallel/test-http-head-throw-on-response-body-write.js",
    "test/parallel/test-http-status-message.js",
    "test/parallel/test-http-write-head-2.js",
];

const NODE22_NETWORKING_HTTP_RESPONSE_STATE_COUNTDOWN_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-http-write-head-after-set-header.js",
    "test/parallel/test-http-status-code.js",
    "test/parallel/test-http-response-multiheaders.js",
    "test/parallel/test-http-status-reason-invalid-chars.js",
];

const NODE22_NETWORKING_SERVER_NO_ARG_LISTEN_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-http-server-options-incoming-message.js",
    "test/parallel/test-http-server-options-server-response.js",
    "test/parallel/test-net-server-unref-persistent.js",
];

const NODE22_NETWORKING_HTTP_AGENT_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-http-agent-keepalive.js",
    "test/parallel/test-http-agent-keepalive-delay.js",
    "test/parallel/test-http-agent-maxsockets.js",
    "test/parallel/test-http-agent-maxsockets-respected.js",
    "test/parallel/test-http-agent-maxtotalsockets.js",
    "test/parallel/test-http-agent-scheduling.js",
    "test/parallel/test-http-agent-timeout.js",
];

const NODE22_NETWORKING_HTTP_AGENT_LIFECYCLE_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-http-agent-false.js",
    "test/parallel/test-http-agent-no-protocol.js",
    "test/parallel/test-http-agent-null.js",
    "test/parallel/test-http-agent-remove.js",
    "test/parallel/test-http-agent-destroyed-socket.js",
    "test/parallel/test-http-agent-error-on-idle.js",
    "test/parallel/test-http-agent-uninitialized.js",
    "test/parallel/test-http-agent-uninitialized-with-handle.js",
    "test/parallel/test-http-agent-abort-controller.js",
];

const NODE22_NETWORKING_DGRAM_HELPER_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-dgram-bytes-length.js",
    "test/parallel/test-dgram-createSocket-type.js",
    "test/parallel/test-dgram-send-address-types.js",
    "test/parallel/test-dgram-send-bad-arguments.js",
    "test/parallel/test-dgram-send-invalid-msg-type.js",
    "test/parallel/test-dgram-close-is-not-callback.js",
    "test/parallel/test-dgram-send-empty-array.js",
    "test/parallel/test-dgram-send-empty-buffer.js",
];

const NODE22_NETWORKING_DGRAM_BIND_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-dgram-address.js",
    "test/parallel/test-dgram-bind-default-address.js",
    "test/parallel/test-dgram-bind.js",
    "test/parallel/test-dgram-close.js",
    "test/parallel/test-dgram-listen-after-bind.js",
    "test/parallel/test-dgram-ref.js",
    "test/parallel/test-dgram-unref.js",
    "test/parallel/test-dgram-implicit-bind.js",
];

const NODE22_NETWORKING_DGRAM_CONNECT_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-dgram-connect.js",
    "test/parallel/test-dgram-connect-send-callback-buffer.js",
    "test/parallel/test-dgram-connect-send-callback-buffer-length.js",
    "test/parallel/test-dgram-connect-send-callback-multi-buffer.js",
    "test/parallel/test-dgram-connect-send-default-host.js",
    "test/parallel/test-dgram-connect-send-empty-array.js",
    "test/parallel/test-dgram-connect-send-empty-buffer.js",
    "test/parallel/test-dgram-connect-send-empty-packet.js",
];

const NODE22_NETWORKING_DGRAM_SEND_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-dgram-send-callback-buffer-empty-address.js",
    "test/parallel/test-dgram-send-callback-buffer-length-empty-address.js",
    "test/parallel/test-dgram-send-callback-buffer-length.js",
    "test/parallel/test-dgram-send-callback-buffer.js",
    "test/parallel/test-dgram-send-callback-multi-buffer-empty-address.js",
    "test/parallel/test-dgram-send-callback-multi-buffer.js",
    "test/parallel/test-dgram-send-callback-recursive.js",
    "test/parallel/test-dgram-send-cb-quelches-error.js",
    "test/parallel/test-dgram-send-default-host.js",
    "test/parallel/test-dgram-send-empty-packet.js",
    "test/parallel/test-dgram-send-multi-buffer-copy.js",
    "test/parallel/test-dgram-send-multi-string-array.js",
    "test/parallel/test-dgram-sendto.js",
];

const NODE22_NETWORKING_DGRAM_REMAINING_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-dgram-abort-closed.js",
    "test/parallel/test-dgram-bind-error-repeat.js",
    "test/parallel/test-dgram-bind-fd-error.js",
    "test/parallel/test-dgram-bind-fd.js",
    "test/parallel/test-dgram-bind-socket-close-before-lookup.js",
    "test/parallel/test-dgram-blocklist.js",
    "test/parallel/test-dgram-close-during-bind.js",
    "test/parallel/test-dgram-close-in-listening.js",
    "test/parallel/test-dgram-close-signal.js",
    "test/parallel/test-dgram-connect-send-multi-buffer-copy.js",
    "test/parallel/test-dgram-connect-send-multi-string-array.js",
    "test/parallel/test-dgram-create-socket-handle-fd.js",
    "test/parallel/test-dgram-create-socket-handle.js",
    "test/parallel/test-dgram-custom-lookup.js",
    "test/parallel/test-dgram-membership.js",
    "test/parallel/test-dgram-msgsize.js",
    "test/parallel/test-dgram-multicast-loopback.js",
    "test/parallel/test-dgram-multicast-set-interface.js",
    "test/parallel/test-dgram-multicast-setTTL.js",
    "test/parallel/test-dgram-oob-buffer.js",
    "test/parallel/test-dgram-recv-error.js",
    "test/parallel/test-dgram-send-error.js",
    "test/parallel/test-dgram-send-queue-info.js",
    "test/parallel/test-dgram-setBroadcast.js",
    "test/parallel/test-dgram-setTTL.js",
    "test/parallel/test-dgram-socket-buffer-size.js",
    "test/parallel/test-dgram-udp4.js",
];

const NODE22_NETWORKING_DGRAM_LOCAL_PATCH_REGRESSION_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-dgram-close-in-listening.js",
    "test/parallel/test-dgram-connect-send-multi-buffer-copy.js",
    "test/parallel/test-dgram-custom-lookup.js",
    "test/parallel/test-dgram-msgsize.js",
    "test/parallel/test-dgram-multicast-loopback.js",
    "test/parallel/test-dgram-send-error.js",
    "test/parallel/test-dgram-setBroadcast.js",
    "test/parallel/test-dgram-udp4.js",
];

const NODE22_NETWORKING_CRYPTO_GATED_HELPER_BATCH_FIXTURES: &[&str] = &[
    "test/parallel/test-https-agent-constructor.js",
    "test/parallel/test-https-agent-getname.js",
    "test/parallel/test-https-agent.js",
    "test/parallel/test-https-agent-abort-controller.js",
    "test/parallel/test-https-server-options-incoming-message.js",
    "test/parallel/test-https-server-options-server-response.js",
    "test/parallel/test-https-client-get-url.js",
    "test/parallel/test-http2-getpackedsettings.js",
    "test/parallel/test-http2-util-headers-list.js",
    "test/parallel/test-http2-util-update-options-buffer.js",
    "test/parallel/test-http2-misc-util.js",
];

const NODE22_NETWORKING_HTTP2_HEADER_STATUS_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-http2-status-code.js"),
    shared_official_batch_case!("test/parallel/test-http2-status-code-invalid.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-http2-multi-content-length.js",
        COMMON_COUNTDOWN_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-http2-response-splitting.js"),
    shared_official_batch_case!("test/parallel/test-http2-options-server-request.js"),
    shared_official_batch_case!("test/parallel/test-http2-options-server-response.js"),
    shared_official_batch_case!("test/parallel/test-http2-zero-length-header.js"),
    shared_official_batch_case!("test/parallel/test-http2-multiheaders.js"),
    shared_official_batch_case!("test/parallel/test-http2-multiheaders-raw.js"),
];

const NODE22_NETWORKING_HTTP2_COMPAT_REQUEST_RESPONSE_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-end.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-write.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-writehead.js"),
    shared_official_batch_case!(
        "test/parallel/test-http2-compat-serverresponse-writehead-array.js"
    ),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-statuscode.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-statusmessage.js"),
    shared_official_batch_case!(
        "test/parallel/test-http2-compat-serverresponse-statusmessage-property.js"
    ),
    shared_official_batch_case!(
        "test/parallel/test-http2-compat-serverresponse-statusmessage-property-set.js"
    ),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-headers.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverrequest.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverrequest-end.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverrequest-headers.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverrequest-host.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverrequest-pause.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-http2-compat-serverrequest-pipe.js",
        COMMON_HTTP2_COMPAT_SERVERREQUEST_PIPE_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverrequest-settimeout.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverrequest-trailers.js"),
];

const NODE22_NETWORKING_HTTP2_COMPAT_SERVERRESPONSE_LIFECYCLE_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-close.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-http2-compat-serverresponse-destroy.js",
        COMMON_COUNTDOWN_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-drain.js"),
    shared_official_batch_case!(
        "test/parallel/test-http2-compat-serverresponse-end-after-statuses-without-body.js"
    ),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-finished.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-flushheaders.js"),
    shared_official_batch_case!(
        "test/parallel/test-http2-compat-serverresponse-headers-after-destroy.js"
    ),
    shared_official_batch_case!(
        "test/parallel/test-http2-compat-serverresponse-headers-send-date.js"
    ),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-settimeout.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-serverresponse-trailers.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-write-early-hints.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-write-head-destroyed.js"),
];

const NODE22_NETWORKING_HTTP2_COMPAT_REMAINDER_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-http2-compat-aborted.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-http2-compat-client-upload-reject.js",
        COMMON_HTTP2_COMPAT_SERVERREQUEST_PIPE_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-http2-compat-errors.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-expect-continue-check.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-expect-continue.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-expect-handling.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-method-connect.js"),
    shared_official_batch_case!(
        "test/parallel/test-http2-compat-serverresponse-createpushresponse.js"
    ),
    shared_official_batch_case!("test/parallel/test-http2-compat-short-stream-client-server.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-socket-destroy-delayed.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-socket-set.js"),
    shared_official_batch_case!("test/parallel/test-http2-compat-socket.js"),
];

const NODE22_NETWORKING_HTTPS_AGENT_SESSION_BATCH: &[NodeCompatBatchEntry] = &[
    shared_node20_node22_with_node24_override_case_with_extra!(
        "test/parallel/test-https-agent-create-connection.js",
        "node24/test/parallel/test-https-agent-create-connection.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_node20_node22_batch_case_with_extra!(
        "test/parallel/test-https-agent-disable-session-reuse.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_node20_node22_with_node24_override_case_with_extra!(
        "test/parallel/test-https-agent-servername.js",
        "node24/test/parallel/test-https-agent-servername.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_node20_node22_batch_case_with_extra!(
        "test/parallel/test-https-agent-session-injection.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_node20_node22_with_node24_override_case_with_extra!(
        "test/parallel/test-https-agent-sni.js",
        "node24/test/parallel/test-https-agent-sni.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_node20_node22_batch_case_with_extra!(
        "test/parallel/test-https-agent-sockets-leak.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_node20_node22_batch_case_with_extra!(
        "test/parallel/test-https-client-override-global-agent.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
];

const NODE22_NETWORKING_HTTPS_LOCAL_SERVER_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-abortcontroller.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-argument-of-creating.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-byteswritten.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-close.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-max-headers-count.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-request-arguments.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-server-headers-timeout.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-server-request-timeout.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-set-timeout-server.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-simple.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-timeout.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-timeout-server.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-timeout-server-2.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
];

const NODE22_NETWORKING_HTTPS_SERVER_LIFECYCLE_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-server-close-all.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-server-close-destroy-timeout.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-server-close-idle.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-socket-options.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-keep-alive-drop-requests.js",
        COMMON_TLS_KEY_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-server-connections-checking-leak.js",
        COMMON_TLS_KEY_COUNTDOWN_GC_EXTRA_FILES
    ),
];

const NODE22_NETWORKING_HTTPS_CLIENT_SERVER_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-client-checkServerIdentity.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-client-reject.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-connecting-to-http.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-drain.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-eof-for-eom.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-host-headers.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-insecure-parse-per-stream.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-max-header-size-per-stream.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-options-boolean-check.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-server-async-dispose.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-truncate.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
];

const NODE22_NETWORKING_HTTPS_TLS_SESSION_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-selfsigned-no-keycertsign-no-crash.js",
        COMMON_TLS_SESSION_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-client-resume.js",
        COMMON_TLS_SESSION_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-resume-after-renew.js",
        COMMON_TLS_SESSION_CERT_EXTRA_FILES
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-https-agent-session-reuse.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("node22/test/parallel/test-https-agent-session-reuse.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TLS_KEY_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case_with_extra!(
        "test/parallel/test-https-hwm.js",
        COMMON_TLS_SESSION_CERT_EXTRA_FILES
    ),
];

const NODE22_NETWORKING_TLS_LOCAL_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-tls-basic-validations.js"),
    shared_official_batch_case!("test/parallel/test-tls-check-server-identity.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-tls-connect-abort-controller.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-tls-connect-allow-half-open-option.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-tls-connect-hwm-option.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-tls-connect-no-host.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-tls-connect-simple.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-tls-connect-timeout-option.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-tls-options-boolean-check.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-tls-server-parent-constructor-options.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES
    ),
];

const NODE22_NETWORKING_HTTPS_ADDRESS_BOUNDARY_FIXTURES: &[&str] = &[
    "test/parallel/test-https-localaddress-bind-error.js",
    "test/parallel/test-https-connect-address-family.js",
];

const NODE22_NETWORKING_DGRAM_CLUSTER_BOUNDARY_FIXTURES: &[&str] = &[
    "test/parallel/test-dgram-bind-socket-close-before-cluster-reply.js",
    "test/parallel/test-dgram-cluster-bind-error.js",
    "test/parallel/test-dgram-cluster-close-during-bind.js",
    "test/parallel/test-dgram-cluster-close-in-listening.js",
    "test/parallel/test-dgram-exclusive-implicit-bind.js",
    "test/parallel/test-dgram-unref-in-cluster.js",
];

const NODE22_NETWORKING_DGRAM_HOST_PRESET_BOUNDARY_FIXTURES: &[&str] = &[
    "test/parallel/test-dgram-error-message-address.js",
    "test/parallel/test-dgram-ipv6only.js",
    "test/parallel/test-dgram-udp6-link-local-address.js",
    "test/parallel/test-dgram-udp6-send-default-host.js",
];

const NODE22_NLC7_MODULE_COMMONJS_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-module-builtin.js"),
    shared_official_batch_case!("test/parallel/test-module-cache.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-module-create-require.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-module-create-require-multibyte.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-module-isBuiltin.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-module-loading-deprecated.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-module-nodemodulepaths.js"),
    shared_official_batch_case!("test/parallel/test-module-relative-lookup.js"),
    shared_official_batch_case!("test/parallel/test-module-version.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-module-children.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-module-multi-extensions.js"),
    shared_official_batch_case!("test/parallel/test-module-stat.js"),
];

const NODE22_NLC7_ASYNC_LOCAL_STORAGE_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-async-local-storage-bind.js"),
    shared_official_batch_case!("test/parallel/test-async-local-storage-contexts.js"),
    shared_official_batch_case!("test/parallel/test-async-local-storage-deep-stack.js"),
    shared_official_batch_case!("test/parallel/test-async-local-storage-snapshot.js"),
    node22_only_batch_case!(
        "test/parallel/test-async-local-storage-exit-does-not-leak.js",
        "node22/test/parallel/test-async-local-storage-exit-does-not-leak.js"
    ),
];

const NODE22_NLC7_ASYNC_HOOKS_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-async-hooks-asyncresource-constructor.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-constructor.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-enable-disable.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-enable-disable-enable.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-enable-recursive.js"),
    shared_official_batch_case!(
        "test/parallel/test-async-hooks-recursive-stack-runInAsyncScope.js"
    ),
    shared_official_batch_case!("test/parallel/test-async-hooks-run-in-async-scope-this-arg.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-execution-async-resource.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-execution-async-resource-await.js"),
];

const NODE22_NLC7_ASYNC_HOOKS_PROMISE_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-async-hooks-async-await.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-correctly-switch-promise-hook.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-disable-during-promise.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-enable-before-promise-resolve.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-enable-during-promise.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-promise-enable-disable.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-promise-triggerid.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-promise.js"),
];

const NODE22_NLC7_ASYNC_HOOKS_PROMISE_CORE_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-async-hooks-async-await.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-correctly-switch-promise-hook.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-promise-enable-disable.js"),
];

const NLC8_WORKER_MAIN_THREAD_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-async-hooks-disable-during-promise.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-promise-triggerid.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-promise.js"),
    shared_official_batch_case!("test/parallel/test-fs-write-file-sync.js"),
];

const NLC8_WORKER_BASIC_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-worker-type-check.js"),
    shared_official_batch_case!("test/parallel/test-worker.js"),
    shared_official_batch_case!("test/parallel/test-worker-message-channel.js"),
    shared_official_batch_case!("test/parallel/test-worker-message-port.js"),
    shared_official_batch_case!("test/parallel/test-worker-onmessage.js"),
    shared_official_batch_case!("test/parallel/test-worker-ref.js"),
    shared_official_batch_case!("test/parallel/test-worker-hasref.js"),
];

const NLC8_WORKER_BOOTSTRAP_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-worker-execargv.js"),
    shared_official_batch_case!("test/parallel/test-worker-execargv-invalid.js"),
    shared_official_batch_case!("test/parallel/test-worker-process-argv.js"),
    shared_official_batch_case!("test/parallel/test-worker-process-env.js"),
    shared_official_batch_case!("test/parallel/test-worker-process-env-shared.js"),
    shared_official_batch_case!("test/parallel/test-worker-invalid-workerdata.js"),
    shared_official_batch_case!("test/parallel/test-worker-relative-path.js"),
    shared_official_batch_case!("test/parallel/test-worker-unsupported-path.js"),
];

const NLC8_WORKER_CONTRACT_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-worker-type-check.js"),
    shared_official_batch_case!("test/parallel/test-worker-message-port.js"),
];

const NLC8_WORKER_MESSAGE_PORT_BATCH: &[NodeCompatBatchEntry] = &[shared_official_batch_case!(
    "test/parallel/test-worker-message-port.js"
)];

const NLC8_WORKER_MESSAGE_CHANNEL_BATCH: &[NodeCompatBatchEntry] = &[shared_official_batch_case!(
    "test/parallel/test-worker-message-channel.js"
)];

const NLC8_MODULE_COMMONJS_REMAINDER_BATCH: &[NodeCompatBatchEntry] =
    &[shared_official_batch_case_with_extra!(
        "test/parallel/test-module-loading-error.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES
    )];

const NLC8_INSPECTOR_FRONT_EDGE_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-inspector-module.js"),
    shared_official_batch_case!("test/parallel/test-inspector-open.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-inspector-invalid-args.js",
        INSPECTOR_FRONT_EDGE_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-inspector-open-port-integer-overflow.js"),
    shared_official_batch_case!("test/parallel/test-inspector-enabled.js"),
];

const NLC8_V8_HELPER_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-v8-version-tag.js"),
    shared_official_batch_case!("test/parallel/test-v8-deserialize-buffer.js"),
    shared_official_batch_case!("test/parallel/test-v8-serdes.js"),
    shared_official_batch_case!("test/parallel/test-v8-stats.js"),
    shared_official_batch_case!("test/parallel/test-v8-flag-type-check.js"),
];

const NLC8_V8_GREEN_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-v8-version-tag.js"),
    shared_official_batch_case!("test/parallel/test-v8-deserialize-buffer.js"),
    shared_official_batch_case!("test/parallel/test-v8-serdes.js"),
    shared_official_batch_case!("test/parallel/test-v8-flag-type-check.js"),
];

const NLC8_VM_BASIC_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-vm-basic.js"),
    shared_official_batch_case!("test/parallel/test-vm-context.js"),
    shared_official_batch_case!("test/parallel/test-vm-run-in-new-context.js"),
    shared_official_batch_case!("test/parallel/test-vm-strict-mode.js"),
    shared_official_batch_case!("test/parallel/test-vm-not-strict.js"),
    shared_official_batch_case!("test/parallel/test-vm-create-context-arg.js"),
    shared_official_batch_case!("test/parallel/test-inspector-module.js"),
    shared_official_batch_case!("test/parallel/test-inspector-open.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-inspector-invalid-args.js",
        INSPECTOR_FRONT_EDGE_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-inspector-open-port-integer-overflow.js"),
    shared_official_batch_case!("test/parallel/test-inspector-enabled.js"),
];

const NLC8_VM_CONTEXT_REGRESSION_BATCH: &[NodeCompatBatchEntry] = &[
    shared_batch_case!(
        "test/parallel/test-vm-context-regression-script.js",
        "node22/test/parallel/test-vm-context-regression-script.js"
    ),
    shared_batch_case!(
        "test/parallel/test-vm-context-regression-gh1140.js",
        "node22/test/parallel/test-vm-context-regression-gh1140.js"
    ),
    shared_batch_case!(
        "test/parallel/test-vm-context-regression-first-line-stack.js",
        "node22/test/parallel/test-vm-context-regression-first-line-stack.js"
    ),
    shared_batch_case!(
        "test/parallel/test-vm-context-regression-proxy.js",
        "node22/test/parallel/test-vm-context-regression-proxy.js"
    ),
];

const NLC8_VM_CONTEXT_REMAINDER_REGRESSION_BATCH: &[NodeCompatBatchEntry] = &[
    shared_batch_case!(
        "test/parallel/test-vm-context-regression-null-context.js",
        "node22/test/parallel/test-vm-context-regression-null-context.js"
    ),
    shared_batch_case!(
        "test/parallel/test-vm-context-regression-invalid-context-args.js",
        "node22/test/parallel/test-vm-context-regression-invalid-context-args.js"
    ),
    shared_batch_case!(
        "test/parallel/test-vm-context-regression-regexp-throws.js",
        "node22/test/parallel/test-vm-context-regression-regexp-throws.js"
    ),
    shared_batch_case!(
        "test/parallel/test-vm-context-regression-delete.js",
        "node22/test/parallel/test-vm-context-regression-delete.js"
    ),
];

const NLC9_DOMAIN_FOUNDATION_BATCH: &[NodeCompatBatchEntry] = &[
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-add-remove.js",
        "node22/test/parallel/test-domain-add-remove.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-bind-timeout.js",
        "node22/test/parallel/test-domain-bind-timeout.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-ee-error-listener.js",
        "node22/test/parallel/test-domain-ee-error-listener.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-ee-implicit.js",
        "node22/test/parallel/test-domain-ee-implicit.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-ee.js",
        "node22/test/parallel/test-domain-ee.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-enter-exit.js",
        "node22/test/parallel/test-domain-enter-exit.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-from-timer.js",
        "node22/test/parallel/test-domain-from-timer.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-implicit-binding.js",
        "node22/test/parallel/test-domain-implicit-binding.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-intercept.js",
        "node22/test/parallel/test-domain-intercept.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-multiple-errors.js",
        "node22/test/parallel/test-domain-multiple-errors.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-nested.js",
        "node22/test/parallel/test-domain-nested.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-nexttick.js",
        "node22/test/parallel/test-domain-nexttick.js"
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-domain-promise.js",
        node20_fixture_source_path: Some("node22/test/parallel/test-domain-promise.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-domain-promise.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-domain-promise.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-run.js",
        "node22/test/parallel/test-domain-run.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-timer.js",
        "node22/test/parallel/test-domain-timer.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-timers.js",
        "node22/test/parallel/test-domain-timers.js"
    ),
];

const NLC9_CONSTANTS_FOUNDATION_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-constants.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-constants.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-constants.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-constants.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-binding-constants.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-binding-constants.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-binding-constants.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-binding-constants.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-process-constants-noatime.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-process-constants-noatime.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-process-constants-noatime.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-process-constants-noatime.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-os-constants-signals.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-os-constants-signals.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-os-constants-signals.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-os-constants-signals.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-uv-binding-constant.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-uv-binding-constant.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-uv-binding-constant.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-uv-binding-constant.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NLC9_TRACE_EVENTS_FOUNDATION_BATCH: &[NodeCompatBatchEntry] = &[
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-api.js",
        "node22/test/parallel/test-trace-events-api.js"
    ),
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-api.js",
        "node22/test/parallel/test-trace-events-api.js"
    ),
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-binding.js",
        "node22/test/parallel/test-trace-events-binding.js"
    ),
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-bootstrap.js",
        "node22/test/parallel/test-trace-events-bootstrap.js"
    ),
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-category-used.js",
        "node22/test/parallel/test-trace-events-category-used.js"
    ),
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-console.js",
        "node22/test/parallel/test-trace-events-console.js"
    ),
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-dynamic-enable.js",
        "node22/test/parallel/test-trace-events-dynamic-enable.js"
    ),
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-environment.js",
        "node22/test/parallel/test-trace-events-environment.js"
    ),
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-metadata.js",
        "node22/test/parallel/test-trace-events-metadata.js"
    ),
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-none.js",
        "node22/test/parallel/test-trace-events-none.js"
    ),
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-process-exit.js",
        "node22/test/parallel/test-trace-events-process-exit.js"
    ),
];

const NLC9_SYS_FOUNDATION_BATCH: &[NodeCompatBatchEntry] = &[NodeCompatBatchEntry {
    test_relative_path: "test/parallel/test-sys.js",
    node20_fixture_source_path: Some("test/parallel/test-sys.js"),
    node22_fixture_source_path: Some("test/parallel/test-sys.js"),
    node24_fixture_source_path: Some("test/parallel/test-sys.js"),
    shared_extra_files: &[],
    node20_extra_files: &[],
    node22_extra_files: &[],
    node24_extra_files: &[],
}];

const NLC9_SQLITE_NEXT_DB_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/sqlite/next-db.js",
        fixture_source_path: "test/sqlite/next-db.js",
    }];

const NLC9_WASI_VALIDATION_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] =
    &[NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/simple.wasm",
        fixture_source_path: "test/fixtures/simple.wasm",
    }];

const NLC9_WASI_EXECUTION_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/wasi/wasm/main_args.wasm",
        fixture_source_path: "test/wasi/wasm/main_args.wasm",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/wasi/wasm/exitcode.wasm",
        fixture_source_path: "test/wasi/wasm/exitcode.wasm",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/wasi/wasm/stdin.wasm",
        fixture_source_path: "test/wasi/wasm/stdin.wasm",
    },
];

const NLC9_WASI_FILESYSTEM_FOUNDATION_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/wasi.js",
        fixture_source_path: "test/common/wasi.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/child_process.js",
        fixture_source_path: "test/common/child_process.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/wasi-preview-1.js",
        fixture_source_path: "test/fixtures/wasi-preview-1.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/wasi/input.txt",
        fixture_source_path: "test/fixtures/wasi/input.txt",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/wasi/input2.txt",
        fixture_source_path: "test/fixtures/wasi/input2.txt",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/wasi/notadir",
        fixture_source_path: "test/fixtures/wasi/notadir",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/wasi/wasm/main_args.wasm",
        fixture_source_path: "test/wasi/wasm/main_args.wasm",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/wasi/wasm/write_file.wasm",
        fixture_source_path: "test/wasi/wasm/write_file.wasm",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/wasi/wasm/stat.wasm",
        fixture_source_path: "test/wasi/wasm/stat.wasm",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/wasi/wasm/readdir.wasm",
        fixture_source_path: "test/wasi/wasm/readdir.wasm",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/wasi/wasm/notdir.wasm",
        fixture_source_path: "test/wasi/wasm/notdir.wasm",
    },
];

const NLC9_WASI_PREOPEN_IO_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/wasi.js",
        fixture_source_path: "test/common/wasi.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/child_process.js",
        fixture_source_path: "test/common/child_process.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/wasi-preview-1.js",
        fixture_source_path: "test/fixtures/wasi-preview-1.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/wasi/input.txt",
        fixture_source_path: "test/fixtures/wasi/input.txt",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/wasi/input2.txt",
        fixture_source_path: "test/fixtures/wasi/input2.txt",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/wasi/wasm/freopen.wasm",
        fixture_source_path: "test/wasi/wasm/freopen.wasm",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/wasi/wasm/read_file.wasm",
        fixture_source_path: "test/wasi/wasm/read_file.wasm",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/wasi/wasm/read_file_twice.wasm",
        fixture_source_path: "test/wasi/wasm/read_file_twice.wasm",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/wasi/wasm/stdin.wasm",
        fixture_source_path: "test/wasi/wasm/stdin.wasm",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/wasi/wasm/preopen_populates.wasm",
        fixture_source_path: "test/wasi/wasm/preopen_populates.wasm",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/wasi/wasm/fd_prestat_get_refresh.wasm",
        fixture_source_path: "test/wasi/wasm/fd_prestat_get_refresh.wasm",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/wasi/wasm/cant_dotdot.wasm",
        fixture_source_path: "test/wasi/wasm/cant_dotdot.wasm",
    },
];

const NLC9_SQLITE_FOUNDATION_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-sqlite-config.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-sqlite-config.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_INDEX_MJS_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-sqlite-statement-sync.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-sqlite-statement-sync.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-sqlite-template-tag.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-sqlite-template-tag.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_GC_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-sqlite-named-parameters.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-sqlite-named-parameters.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NLC9_WASI_VALIDATION_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-options-validation.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-options-validation.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-initialize-validation.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-initialize-validation.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NLC9_WASI_VALIDATION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-start-validation.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-start-validation.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NLC9_WASI_VALIDATION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NLC9_WASI_EXECUTION_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-not-started.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-not-started.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NLC9_WASI_EXECUTION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-return-on-exit.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-return-on-exit.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NLC9_WASI_EXECUTION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-stdio.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-stdio.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NLC9_WASI_EXECUTION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NLC9_WASI_FILESYSTEM_FOUNDATION_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-main_args.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-main_args.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NLC9_WASI_FILESYSTEM_FOUNDATION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-write_file.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-write_file.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NLC9_WASI_FILESYSTEM_FOUNDATION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-stat.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-stat.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NLC9_WASI_FILESYSTEM_FOUNDATION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-readdir.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-readdir.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NLC9_WASI_FILESYSTEM_FOUNDATION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-notdir.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-notdir.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NLC9_WASI_FILESYSTEM_FOUNDATION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NLC9_WASI_PREOPEN_IO_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-io.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-io.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NLC9_WASI_PREOPEN_IO_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-preopen_populates.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-preopen_populates.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NLC9_WASI_PREOPEN_IO_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-fd_prestat_get_refresh.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-fd_prestat_get_refresh.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NLC9_WASI_PREOPEN_IO_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-cant_dotdot.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-cant_dotdot.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NLC9_WASI_PREOPEN_IO_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NLC9_WASI_IO_SUBCASE_WATCHPOINT_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-freopen-only.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-freopen-only.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NLC9_WASI_PREOPEN_IO_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-read-file-only.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-read-file-only.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NLC9_WASI_PREOPEN_IO_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NLC9_SEA_FOUNDATION_BATCH: &[NodeCompatBatchEntry] = &[NodeCompatBatchEntry {
    test_relative_path: "test/parallel/test-sea-get-asset-keys.js",
    node20_fixture_source_path: None,
    node22_fixture_source_path: Some("test/parallel/test-sea-get-asset-keys.js"),
    node24_fixture_source_path: None,
    shared_extra_files: &[],
    node20_extra_files: &[],
    node22_extra_files: &[],
    node24_extra_files: &[],
}];

const NLC9_REPL_FOUNDATION_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-repl-definecommand.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-repl-definecommand.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_REPL_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-repl-mode.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-repl-mode.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_REPL_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-repl-recoverable.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-repl-recoverable.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_REPL_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-repl-reset-event.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-repl-reset-event.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_REPL_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NLC9_TEST_RUNNER_FOUNDATION_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-aliases.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-aliases.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-typechecking.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-typechecking.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-custom-assertions.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-custom-assertions.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-get-test-context.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-get-test-context.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-assert.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-assert.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NLC9_TEST_RUNNER_CONTEXT_METADATA_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-test-fullname.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-test-fullname.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-test-filepath.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-test-filepath.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NLC9_TEST_RUNNER_RUN_EVENT_METADATA_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-test-id.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-test-id.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_EVENT_METADATA_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-filetest-location.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-filetest-location.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_EVENT_METADATA_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NLC9_TEST_RUNNER_OPTION_VALIDATION_BATCH: &[NodeCompatBatchEntry] = &[NodeCompatBatchEntry {
    test_relative_path: "test/parallel/test-runner-option-validation.js",
    node20_fixture_source_path: None,
    node22_fixture_source_path: Some("test/parallel/test-runner-option-validation.js"),
    node24_fixture_source_path: None,
    shared_extra_files: &[],
    node20_extra_files: &[],
    node22_extra_files: &[],
    node24_extra_files: &[],
}];

const NLC9_TEST_RUNNER_PLAN_BATCH: &[NodeCompatBatchEntry] = &[NodeCompatBatchEntry {
    test_relative_path: "test/parallel/test-runner-plan.mjs",
    node20_fixture_source_path: None,
    node22_fixture_source_path: Some("test/parallel/test-runner-plan.mjs"),
    node24_fixture_source_path: None,
    shared_extra_files: COMMON_TEST_RUNNER_PLAN_EXTRA_FILES,
    node20_extra_files: &[],
    node22_extra_files: &[],
    node24_extra_files: &[],
}];

const NLC9_TEST_RUNNER_RUN_EDGE_BATCH: &[NodeCompatBatchEntry] = &[NodeCompatBatchEntry {
    test_relative_path: "test/parallel/test-runner-enqueue-file-syntax-error.js",
    node20_fixture_source_path: None,
    node22_fixture_source_path: Some("test/parallel/test-runner-enqueue-file-syntax-error.js"),
    node24_fixture_source_path: None,
    shared_extra_files: COMMON_TEST_RUNNER_RUN_EDGE_EXTRA_FILES,
    node20_extra_files: &[],
    node22_extra_files: &[],
    node24_extra_files: &[],
}];

const NLC9_TEST_RUNNER_REPORTERS_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-run-files-undefined.mjs",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-run-files-undefined.mjs"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_RUN_EDGE_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-import-no-scheme.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-import-no-scheme.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_RUN_EDGE_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NLC9_TEST_RUNNER_REPORTER_OUTPUT_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-reporters.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-reporters.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_REPORTERS_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-error-reporter.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-error-reporter.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_REPORTERS_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NLC9_TEST_RUNNER_CLI_OPTIONS_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-cli-concurrency.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-cli-concurrency.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_CLI_OPTIONS_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-cli-timeout.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-cli-timeout.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_CLI_OPTIONS_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NLC9_TEST_RUNNER_CLI_RANDOMIZE_BATCH: &[NodeCompatBatchEntry] = &[NodeCompatBatchEntry {
    test_relative_path: "test/parallel/test-runner-cli-randomize.js",
    node20_fixture_source_path: None,
    node22_fixture_source_path: Some("test/parallel/test-runner-cli-randomize.js"),
    node24_fixture_source_path: None,
    shared_extra_files: COMMON_TEST_RUNNER_CLI_RANDOMIZE_EXTRA_FILES,
    node20_extra_files: &[],
    node22_extra_files: &[],
    node24_extra_files: &[],
}];

const NLC9_TEST_RUNNER_CLI_RERUN_FAILURES_BATCH: &[NodeCompatBatchEntry] =
    &[NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-test-rerun-failures.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-test-rerun-failures.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_CLI_RERUN_FAILURES_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    }];

const NLC9_CLUSTER_WORKER_FOUNDATION_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-cluster-worker-constructor.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-cluster-worker-constructor.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-cluster-worker-init.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-cluster-worker-init.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-cluster-worker-isdead.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-cluster-worker-isdead.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-cluster-worker-isconnected.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-cluster-worker-isconnected.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NLC9_CLUSTER_WORKER_LIFECYCLE_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-cluster-worker-events.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-cluster-worker-events.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-cluster-worker-exit.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-cluster-worker-exit.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-cluster-worker-disconnect.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-cluster-worker-disconnect.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-cluster-worker-forced-exit.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-cluster-worker-forced-exit.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-cluster-worker-kill.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-cluster-worker-kill.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NODE22_NLC7_ZLIB_FOUNDATION_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-zlib-const.js"),
    shared_official_batch_case!("test/parallel/test-zlib-convenience-methods.js"),
    shared_official_batch_case!("test/parallel/test-zlib-create-raw.js"),
    shared_official_batch_case!("test/parallel/test-zlib-deflate-constructors.js"),
    shared_official_batch_case!("test/parallel/test-zlib-deflate-raw-inherits.js"),
    shared_official_batch_case!("test/parallel/test-zlib-empty-buffer.js"),
    shared_official_batch_case!("test/parallel/test-zlib-from-string.js"),
    shared_official_batch_case!("test/parallel/test-zlib-invalid-input.js"),
    shared_official_batch_case!("test/parallel/test-zlib-no-stream.js"),
    shared_official_batch_case!("test/parallel/test-zlib-not-string-or-buffer.js"),
    shared_official_batch_case!("test/parallel/test-zlib-object-write.js"),
    shared_official_batch_case!("test/parallel/test-zlib-zero-byte.js"),
    shared_official_batch_case!("test/parallel/test-zlib-close-after-error.js"),
    shared_official_batch_case!("test/parallel/test-zlib-close-after-write.js"),
    shared_official_batch_case!("test/parallel/test-zlib-close-in-ondata.js"),
    shared_official_batch_case!("test/parallel/test-zlib-destroy-pipe.js"),
    shared_official_batch_case!("test/parallel/test-zlib-destroy.js"),
    shared_official_batch_case!("test/parallel/test-zlib-failed-init.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-flush.js",
        COMMON_PERSON_JPG_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-flush-drain.js"),
    shared_official_batch_case!("test/parallel/test-zlib-flush-flags.js"),
    shared_official_batch_case!("test/parallel/test-zlib-reset-before-write.js"),
    shared_official_batch_case!("test/parallel/test-zlib-write-after-close.js"),
    shared_official_batch_case!("test/parallel/test-zlib-write-after-end.js"),
    shared_official_batch_case!("test/parallel/test-zlib-dictionary.js"),
    shared_official_batch_case!("test/parallel/test-zlib-dictionary-fail.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-from-gzip.js",
        COMMON_ZLIB_GZIP_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-from-concatenated-gzip.js",
        COMMON_ZLIB_GZIP_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-from-gzip-with-trailing-garbage.js"),
    shared_official_batch_case!("test/parallel/test-zlib-premature-end.js"),
    shared_official_batch_case!("test/parallel/test-zlib-truncated.js"),
    shared_official_batch_case!("test/parallel/test-zlib-unzip-one-byte-chunks.js"),
    shared_official_batch_case!("test/parallel/test-zlib-zero-windowBits.js"),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-zlib-brotli-16GB.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("node22/test/parallel/test-zlib-brotli-16GB.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-zlib-brotli-16GB.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case!("test/parallel/test-zlib-brotli-kmaxlength-rangeerror.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-brotli-flush.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-brotli-from-brotli.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-brotli-from-string.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-brotli.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-crc32.js"),
    shared_official_batch_case!("test/parallel/test-zlib-flush-drain-longblock.js"),
    shared_official_batch_case!("test/parallel/test-zlib-flush-write-sync-interleaved.js"),
    shared_official_batch_case!("test/parallel/test-zlib-invalid-arg-value-brotli-compress.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-invalid-input-memory.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-maxOutputLength.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-params.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-random-byte-pipes.js"),
    shared_official_batch_case!("test/parallel/test-zlib-kmaxlength-rangeerror.js"),
    shared_official_batch_case!("test/parallel/test-zlib-sync-no-event.js"),
    shared_official_batch_case!("test/parallel/test-zlib-unused-weak.js"),
    shared_official_batch_case!("test/parallel/test-zlib-write-after-flush.js"),
];

const NODE22_NLC7_ZLIB_STREAM_LIFECYCLE_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-zlib-close-after-error.js"),
    shared_official_batch_case!("test/parallel/test-zlib-close-after-write.js"),
    shared_official_batch_case!("test/parallel/test-zlib-close-in-ondata.js"),
    shared_official_batch_case!("test/parallel/test-zlib-destroy-pipe.js"),
    shared_official_batch_case!("test/parallel/test-zlib-destroy.js"),
    shared_official_batch_case!("test/parallel/test-zlib-failed-init.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-flush.js",
        COMMON_PERSON_JPG_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-flush-drain.js"),
    shared_official_batch_case!("test/parallel/test-zlib-flush-flags.js"),
    shared_official_batch_case!("test/parallel/test-zlib-reset-before-write.js"),
    shared_official_batch_case!("test/parallel/test-zlib-write-after-close.js"),
    shared_official_batch_case!("test/parallel/test-zlib-write-after-end.js"),
    shared_official_batch_case!("test/parallel/test-zlib-dictionary.js"),
    shared_official_batch_case!("test/parallel/test-zlib-dictionary-fail.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-from-gzip.js",
        COMMON_ZLIB_GZIP_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-from-concatenated-gzip.js",
        COMMON_ZLIB_GZIP_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-from-gzip-with-trailing-garbage.js"),
    shared_official_batch_case!("test/parallel/test-zlib-premature-end.js"),
    shared_official_batch_case!("test/parallel/test-zlib-truncated.js"),
    shared_official_batch_case!("test/parallel/test-zlib-unzip-one-byte-chunks.js"),
    shared_official_batch_case!("test/parallel/test-zlib-zero-windowBits.js"),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-zlib-brotli-16GB.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("node22/test/parallel/test-zlib-brotli-16GB.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-zlib-brotli-16GB.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-brotli-flush.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-brotli-from-brotli.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-brotli-from-string.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-brotli.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-crc32.js"),
    shared_official_batch_case!("test/parallel/test-zlib-flush-drain-longblock.js"),
    shared_official_batch_case!("test/parallel/test-zlib-flush-write-sync-interleaved.js"),
    shared_official_batch_case!("test/parallel/test-zlib-invalid-arg-value-brotli-compress.js"),
    shared_official_batch_case!("test/parallel/test-zlib-maxOutputLength.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-params.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-random-byte-pipes.js"),
    shared_official_batch_case!("test/parallel/test-zlib-sync-no-event.js"),
    shared_official_batch_case!("test/parallel/test-zlib-unused-weak.js"),
    shared_official_batch_case!("test/parallel/test-zlib-write-after-flush.js"),
];

const NODE22_NLC7_ZLIB_DECOMPRESSION_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-zlib-dictionary.js"),
    shared_official_batch_case!("test/parallel/test-zlib-dictionary-fail.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-from-gzip.js",
        COMMON_ZLIB_GZIP_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-from-concatenated-gzip.js",
        COMMON_ZLIB_GZIP_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-from-gzip-with-trailing-garbage.js"),
    shared_official_batch_case!("test/parallel/test-zlib-premature-end.js"),
    shared_official_batch_case!("test/parallel/test-zlib-truncated.js"),
    shared_official_batch_case!("test/parallel/test-zlib-unzip-one-byte-chunks.js"),
    shared_official_batch_case!("test/parallel/test-zlib-zero-windowBits.js"),
];

const NODE22_NLC7_ZLIB_BROTLI_AND_CONTROL_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-zlib-brotli-16GB.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("node22/test/parallel/test-zlib-brotli-16GB.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-zlib-brotli-16GB.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case!("test/parallel/test-zlib-brotli-kmaxlength-rangeerror.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-brotli-flush.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-brotli-from-brotli.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-brotli-from-string.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-brotli.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-crc32.js"),
    shared_official_batch_case!("test/parallel/test-zlib-flush-drain-longblock.js"),
    shared_official_batch_case!("test/parallel/test-zlib-flush-write-sync-interleaved.js"),
    shared_official_batch_case!("test/parallel/test-zlib-invalid-arg-value-brotli-compress.js"),
    shared_official_batch_case!("test/parallel/test-zlib-maxOutputLength.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-params.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-random-byte-pipes.js"),
    shared_official_batch_case!("test/parallel/test-zlib-kmaxlength-rangeerror.js"),
    shared_official_batch_case!("test/parallel/test-zlib-sync-no-event.js"),
    shared_official_batch_case!("test/parallel/test-zlib-unused-weak.js"),
    shared_official_batch_case!("test/parallel/test-zlib-write-after-flush.js"),
];

const NODE22_NLC7_CRYPTO_HASH_RANDOM_FOUNDATION_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-crypto-hash-stream-pipe.js"),
    shared_official_batch_case!("test/parallel/test-crypto-from-binary.js"),
    shared_official_batch_case!("test/parallel/test-crypto-secret-keygen.js"),
    shared_official_batch_case!("test/parallel/test-crypto-encoding-validation-error.js"),
    shared_official_batch_case!("test/parallel/test-crypto-hmac.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-hash.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-crypto-getcipherinfo.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-oneshot-hash.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-crypto-random.js"),
    shared_official_batch_case!("test/parallel/test-crypto-randomfillsync-regression.js"),
    shared_official_batch_case!("test/parallel/test-crypto-randomuuid.js"),
    shared_official_batch_case!("test/parallel/test-crypto-update-encoding.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-authenticated-stream.js",
        COMMON_CRYPTO_AUTHENTICATED_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-crypto-aes-wrap.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-cipheriv-decipheriv.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-padding.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-padding-aes256.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-crypto-gcm-explicit-short-tag.js"),
    shared_official_batch_case!("test/parallel/test-crypto-gcm-implicit-short-tag.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-classes.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-lazy-transform-writable.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-stream.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-hkdf.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-pbkdf2.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
];

const NODE22_NLC7_CRYPTO_KDF_AND_STREAM_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-classes.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-lazy-transform-writable.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-stream.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-hkdf.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-pbkdf2.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-scrypt.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-crypto-scrypt.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-crypto-scrypt.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_CRYPTO_HASH_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-errors.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-leak.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-generate-keys.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-group-setters.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-modp2-views.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-modp2.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-odd-key.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-padding.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-shared.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-ecdh-convert-key.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-curves.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-dh-stateless.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-crypto-dh-stateless.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-crypto-dh-stateless.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_CRYPTO_HASH_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NODE22_NLC7_CRYPTO_CIPHER_AND_PADDING_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-cipheriv-decipheriv.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-padding.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-padding-aes256.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-crypto-gcm-explicit-short-tag.js"),
    shared_official_batch_case!("test/parallel/test-crypto-gcm-implicit-short-tag.js"),
];

const NODE22_NLC7_CRYPTO_DH_AND_ECDH_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-errors.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-leak.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-generate-keys.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-group-setters.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-modp2-views.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-modp2.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-odd-key.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-padding.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-shared.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-ecdh-convert-key.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-curves.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-dh-stateless.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-crypto-dh-stateless.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-crypto-dh-stateless.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_CRYPTO_HASH_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NODE20_NLC7_CRYPTO_DH_AND_ECDH_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-errors.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-leak.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-generate-keys.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-group-setters.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-modp2-views.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-modp2.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-odd-key.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-padding.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-shared.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-ecdh-convert-key.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-curves.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-dh-stateless.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-crypto-dh-stateless.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-crypto-dh-stateless.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_CRYPTO_HASH_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NODE22_NLC7_CRYPTO_DH_SAFE_PRIME_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-constructor.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-dh.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("node22/test/parallel/test-crypto-dh.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-crypto-dh.js"),
        shared_extra_files: COMMON_CRYPTO_HASH_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NODE22_NLC7_CRYPTO_DH_CURVES_AND_STATELESS_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-curves.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-dh-stateless.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-crypto-dh-stateless.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-crypto-dh-stateless.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_CRYPTO_HASH_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NODE20_NLC7_CRYPTO_DH_SAFE_PRIME_BATCH: &[NodeCompatBatchEntry] =
    &[shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-constructor.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    )];

const NODE24_NLC7_CRYPTO_DH_STATELESS_SUPPORTED_WATCHPOINT_BATCH: &[NodeCompatBatchEntry] =
    &[NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-dh-stateless.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: None,
        node24_fixture_source_path: Some("node24/test/parallel/test-crypto-dh-stateless.js"),
        shared_extra_files: COMMON_CRYPTO_HASH_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    }];

const NODE20_NLC7_CRYPTO_DH_SUPPORTED_WATCHPOINT_BATCH: &[NodeCompatBatchEntry] =
    &[shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    )];

const NLC7_CRYPTO_AUTHENTICATED_AND_AES_WRAP_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-authenticated-stream.js",
        COMMON_CRYPTO_AUTHENTICATED_EXTRA_FILES
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-authenticated.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("node22/test/parallel/test-crypto-authenticated.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-crypto-authenticated.js"),
        shared_extra_files: COMMON_CRYPTO_AUTHENTICATED_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case!("test/parallel/test-crypto-aes-wrap.js"),
    shared_official_batch_case!("test/parallel/test-crypto-des3-wrap.js"),
];

const NODE20_NLC7_CRYPTO_AUTHENTICATED_SUPPORTED_WATCHPOINT_BATCH: &[NodeCompatBatchEntry] =
    &[NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-authenticated.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-crypto-authenticated.js"),
        node22_fixture_source_path: None,
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_CRYPTO_AUTHENTICATED_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    }];

const NLC7_CRYPTO_XOF_EXTENSION_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-default-shake-lengths.js",
        node20_fixture_source_path: Some(
            "node20/test/parallel/test-crypto-default-shake-lengths.js",
        ),
        node22_fixture_source_path: None,
        node24_fixture_source_path: Some(
            "node24/test/parallel/test-crypto-default-shake-lengths.js",
        ),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-default-shake-lengths-oneshot.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: None,
        node24_fixture_source_path: Some(
            "node24/test/parallel/test-crypto-default-shake-lengths-oneshot.js",
        ),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case!("test/parallel/test-worker-type-check.js"),
    shared_official_batch_case!("test/parallel/test-worker-message-port.js"),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-oneshot-hash-xof.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: None,
        node24_fixture_source_path: Some("node24/test/parallel/test-crypto-oneshot-hash-xof.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const LOADER_CONTEXT_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-module-builtin.js"),
    shared_official_batch_case!("test/parallel/test-module-cache.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-module-create-require.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-module-create-require-multibyte.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-module-isBuiltin.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-module-loading-deprecated.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-module-nodemodulepaths.js"),
    shared_official_batch_case!("test/parallel/test-module-relative-lookup.js"),
    shared_official_batch_case!("test/parallel/test-module-version.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-module-children.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-module-multi-extensions.js"),
    shared_official_batch_case!("test/parallel/test-module-stat.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-module-loading-error.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-module-loading-globalpaths.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-module-main-fail.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-module-main-extension-lookup.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-module-prototype-mutation.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-module-wrap.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-module-wrapper.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-async-local-storage-bind.js"),
    shared_official_batch_case!("test/parallel/test-async-local-storage-contexts.js"),
    shared_official_batch_case!("test/parallel/test-async-local-storage-deep-stack.js"),
    shared_official_batch_case!("test/parallel/test-async-local-storage-snapshot.js"),
    node22_only_batch_case!(
        "test/parallel/test-async-local-storage-exit-does-not-leak.js",
        "node22/test/parallel/test-async-local-storage-exit-does-not-leak.js"
    ),
    shared_official_batch_case!("test/parallel/test-async-hooks-asyncresource-constructor.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-constructor.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-enable-disable.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-enable-disable-enable.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-enable-recursive.js"),
    shared_official_batch_case!(
        "test/parallel/test-async-hooks-recursive-stack-runInAsyncScope.js"
    ),
    shared_official_batch_case!("test/parallel/test-async-hooks-run-in-async-scope-this-arg.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-execution-async-resource.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-execution-async-resource-await.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-async-await.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-correctly-switch-promise-hook.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-disable-during-promise.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-enable-before-promise-resolve.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-enable-during-promise.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-promise-enable-disable.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-promise-triggerid.js"),
    shared_official_batch_case!("test/parallel/test-async-hooks-promise.js"),
    shared_official_batch_case!("test/parallel/test-worker-type-check.js"),
    shared_official_batch_case!("test/parallel/test-worker.js"),
    shared_official_batch_case!("test/parallel/test-worker-message-channel.js"),
    shared_official_batch_case!("test/parallel/test-worker-message-port.js"),
    shared_official_batch_case!("test/parallel/test-worker-onmessage.js"),
    shared_official_batch_case!("test/parallel/test-worker-ref.js"),
    shared_official_batch_case!("test/parallel/test-worker-hasref.js"),
    shared_official_batch_case!("test/parallel/test-worker-execargv.js"),
    shared_official_batch_case!("test/parallel/test-worker-execargv-invalid.js"),
    shared_official_batch_case!("test/parallel/test-worker-process-argv.js"),
    shared_official_batch_case!("test/parallel/test-worker-process-env.js"),
    shared_official_batch_case!("test/parallel/test-worker-process-env-shared.js"),
    shared_official_batch_case!("test/parallel/test-worker-invalid-workerdata.js"),
    shared_official_batch_case!("test/parallel/test-worker-relative-path.js"),
    shared_official_batch_case!("test/parallel/test-worker-unsupported-path.js"),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-add-remove.js",
        "node22/test/parallel/test-domain-add-remove.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-bind-timeout.js",
        "node22/test/parallel/test-domain-bind-timeout.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-ee-error-listener.js",
        "node22/test/parallel/test-domain-ee-error-listener.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-ee-implicit.js",
        "node22/test/parallel/test-domain-ee-implicit.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-ee.js",
        "node22/test/parallel/test-domain-ee.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-enter-exit.js",
        "node22/test/parallel/test-domain-enter-exit.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-from-timer.js",
        "node22/test/parallel/test-domain-from-timer.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-implicit-binding.js",
        "node22/test/parallel/test-domain-implicit-binding.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-intercept.js",
        "node22/test/parallel/test-domain-intercept.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-multiple-errors.js",
        "node22/test/parallel/test-domain-multiple-errors.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-nested.js",
        "node22/test/parallel/test-domain-nested.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-nexttick.js",
        "node22/test/parallel/test-domain-nexttick.js"
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-domain-promise.js",
        node20_fixture_source_path: Some("node22/test/parallel/test-domain-promise.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-domain-promise.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-domain-promise.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-run.js",
        "node22/test/parallel/test-domain-run.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-timer.js",
        "node22/test/parallel/test-domain-timer.js"
    ),
    shared_lane_fixture_batch_case!(
        "test/parallel/test-domain-timers.js",
        "node22/test/parallel/test-domain-timers.js"
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-constants.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-constants.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-constants.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-constants.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-binding-constants.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-binding-constants.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-binding-constants.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-binding-constants.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-process-constants-noatime.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-process-constants-noatime.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-process-constants-noatime.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-process-constants-noatime.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-os-constants-signals.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-os-constants-signals.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-os-constants-signals.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-os-constants-signals.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-uv-binding-constant.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-uv-binding-constant.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-uv-binding-constant.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-uv-binding-constant.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-binding.js",
        "node22/test/parallel/test-trace-events-binding.js"
    ),
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-bootstrap.js",
        "node22/test/parallel/test-trace-events-bootstrap.js"
    ),
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-category-used.js",
        "node22/test/parallel/test-trace-events-category-used.js"
    ),
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-console.js",
        "node22/test/parallel/test-trace-events-console.js"
    ),
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-dynamic-enable.js",
        "node22/test/parallel/test-trace-events-dynamic-enable.js"
    ),
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-environment.js",
        "node22/test/parallel/test-trace-events-environment.js"
    ),
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-metadata.js",
        "node22/test/parallel/test-trace-events-metadata.js"
    ),
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-none.js",
        "node22/test/parallel/test-trace-events-none.js"
    ),
    node22_default_only_batch_case!(
        "test/parallel/test-trace-events-process-exit.js",
        "node22/test/parallel/test-trace-events-process-exit.js"
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-sys.js",
        node20_fixture_source_path: Some("test/parallel/test-sys.js"),
        node22_fixture_source_path: Some("test/parallel/test-sys.js"),
        node24_fixture_source_path: Some("test/parallel/test-sys.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-sqlite-config.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-sqlite-config.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_INDEX_MJS_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-sqlite-statement-sync.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-sqlite-statement-sync.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-sqlite-template-tag.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-sqlite-template-tag.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_GC_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-sqlite-named-parameters.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-sqlite-named-parameters.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-sea-get-asset-keys.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-sea-get-asset-keys.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-repl-definecommand.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-repl-definecommand.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_REPL_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-repl-mode.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-repl-mode.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_REPL_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-repl-recoverable.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-repl-recoverable.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_REPL_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-repl-reset-event.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-repl-reset-event.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_REPL_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-aliases.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-aliases.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-typechecking.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-typechecking.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-custom-assertions.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-custom-assertions.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-get-test-context.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-get-test-context.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-assert.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-assert.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-test-fullname.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-test-fullname.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-test-filepath.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-test-filepath.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-test-id.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-test-id.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_EVENT_METADATA_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-filetest-location.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-filetest-location.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_EVENT_METADATA_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-option-validation.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-option-validation.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-plan.mjs",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-plan.mjs"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_PLAN_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-enqueue-file-syntax-error.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-enqueue-file-syntax-error.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_RUN_EDGE_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-run-files-undefined.mjs",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-run-files-undefined.mjs"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_RUN_EDGE_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-import-no-scheme.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-import-no-scheme.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_RUN_EDGE_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-reporters.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-reporters.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_REPORTERS_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-error-reporter.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-error-reporter.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_REPORTERS_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-cli-concurrency.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-cli-concurrency.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_CLI_OPTIONS_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-cli-timeout.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-cli-timeout.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_CLI_OPTIONS_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-cli-randomize.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-cli-randomize.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_CLI_RANDOMIZE_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-runner-test-rerun-failures.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-runner-test-rerun-failures.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_TEST_RUNNER_CLI_RERUN_FAILURES_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-options-validation.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-options-validation.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-initialize-validation.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-initialize-validation.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NLC9_WASI_VALIDATION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-start-validation.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-start-validation.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NLC9_WASI_VALIDATION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-not-started.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-not-started.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NLC9_WASI_EXECUTION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-return-on-exit.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-return-on-exit.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NLC9_WASI_EXECUTION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-stdio.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-stdio.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NLC9_WASI_EXECUTION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-main_args.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-main_args.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NLC9_WASI_FILESYSTEM_FOUNDATION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-write_file.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-write_file.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NLC9_WASI_FILESYSTEM_FOUNDATION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-stat.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-stat.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NLC9_WASI_FILESYSTEM_FOUNDATION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-readdir.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-readdir.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NLC9_WASI_FILESYSTEM_FOUNDATION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-notdir.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-notdir.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NLC9_WASI_FILESYSTEM_FOUNDATION_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-io.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-io.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NLC9_WASI_PREOPEN_IO_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-preopen_populates.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-preopen_populates.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NLC9_WASI_PREOPEN_IO_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-fd_prestat_get_refresh.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-fd_prestat_get_refresh.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NLC9_WASI_PREOPEN_IO_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/wasi/test-wasi-cant_dotdot.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/wasi/test-wasi-cant_dotdot.js"),
        node24_fixture_source_path: None,
        shared_extra_files: NLC9_WASI_PREOPEN_IO_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-cluster-worker-constructor.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-cluster-worker-constructor.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-cluster-worker-init.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-cluster-worker-init.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-cluster-worker-isdead.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-cluster-worker-isdead.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-cluster-worker-isconnected.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-cluster-worker-isconnected.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-cluster-worker-events.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-cluster-worker-events.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-cluster-worker-exit.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-cluster-worker-exit.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-cluster-worker-disconnect.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-cluster-worker-disconnect.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-cluster-worker-forced-exit.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-cluster-worker-forced-exit.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-cluster-worker-kill.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("test/parallel/test-cluster-worker-kill.js"),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case!("test/parallel/test-zlib-const.js"),
    shared_official_batch_case!("test/parallel/test-zlib-convenience-methods.js"),
    shared_official_batch_case!("test/parallel/test-zlib-create-raw.js"),
    shared_official_batch_case!("test/parallel/test-zlib-deflate-constructors.js"),
    shared_official_batch_case!("test/parallel/test-zlib-deflate-raw-inherits.js"),
    shared_official_batch_case!("test/parallel/test-zlib-empty-buffer.js"),
    shared_official_batch_case!("test/parallel/test-zlib-from-string.js"),
    shared_official_batch_case!("test/parallel/test-zlib-invalid-input.js"),
    shared_official_batch_case!("test/parallel/test-zlib-no-stream.js"),
    shared_official_batch_case!("test/parallel/test-zlib-not-string-or-buffer.js"),
    shared_official_batch_case!("test/parallel/test-zlib-object-write.js"),
    shared_official_batch_case!("test/parallel/test-zlib-zero-byte.js"),
    shared_official_batch_case!("test/parallel/test-zlib-close-after-error.js"),
    shared_official_batch_case!("test/parallel/test-zlib-close-after-write.js"),
    shared_official_batch_case!("test/parallel/test-zlib-close-in-ondata.js"),
    shared_official_batch_case!("test/parallel/test-zlib-destroy-pipe.js"),
    shared_official_batch_case!("test/parallel/test-zlib-destroy.js"),
    shared_official_batch_case!("test/parallel/test-zlib-failed-init.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-flush.js",
        COMMON_PERSON_JPG_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-flush-drain.js"),
    shared_official_batch_case!("test/parallel/test-zlib-flush-flags.js"),
    shared_official_batch_case!("test/parallel/test-zlib-reset-before-write.js"),
    shared_official_batch_case!("test/parallel/test-zlib-write-after-close.js"),
    shared_official_batch_case!("test/parallel/test-zlib-write-after-end.js"),
    shared_official_batch_case!("test/parallel/test-zlib-dictionary.js"),
    shared_official_batch_case!("test/parallel/test-zlib-dictionary-fail.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-from-gzip.js",
        COMMON_ZLIB_GZIP_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-from-concatenated-gzip.js",
        COMMON_ZLIB_GZIP_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-from-gzip-with-trailing-garbage.js"),
    shared_official_batch_case!("test/parallel/test-zlib-premature-end.js"),
    shared_official_batch_case!("test/parallel/test-zlib-truncated.js"),
    shared_official_batch_case!("test/parallel/test-zlib-unzip-one-byte-chunks.js"),
    shared_official_batch_case!("test/parallel/test-zlib-zero-windowBits.js"),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-zlib-brotli-16GB.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("node22/test/parallel/test-zlib-brotli-16GB.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-zlib-brotli-16GB.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-brotli-flush.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-brotli-from-brotli.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-brotli-from-string.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-brotli.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-crc32.js"),
    shared_official_batch_case!("test/parallel/test-zlib-flush-drain-longblock.js"),
    shared_official_batch_case!("test/parallel/test-zlib-flush-write-sync-interleaved.js"),
    shared_official_batch_case!("test/parallel/test-zlib-invalid-arg-value-brotli-compress.js"),
    shared_official_batch_case!("test/parallel/test-zlib-maxOutputLength.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-params.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-random-byte-pipes.js"),
    shared_official_batch_case!("test/parallel/test-zlib-sync-no-event.js"),
    shared_official_batch_case!("test/parallel/test-zlib-unused-weak.js"),
    shared_official_batch_case!("test/parallel/test-zlib-write-after-flush.js"),
    shared_official_batch_case!("test/parallel/test-crypto-hash-stream-pipe.js"),
    shared_official_batch_case!("test/parallel/test-crypto-from-binary.js"),
    shared_official_batch_case!("test/parallel/test-crypto-secret-keygen.js"),
    shared_official_batch_case!("test/parallel/test-crypto-encoding-validation-error.js"),
    shared_official_batch_case!("test/parallel/test-crypto-hmac.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-hash.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-crypto-getcipherinfo.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-oneshot-hash.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-crypto-random.js"),
    shared_official_batch_case!("test/parallel/test-crypto-randomfillsync-regression.js"),
    shared_official_batch_case!("test/parallel/test-crypto-randomuuid.js"),
    shared_official_batch_case!("test/parallel/test-crypto-update-encoding.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-cipheriv-decipheriv.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-padding.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-padding-aes256.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-crypto-gcm-explicit-short-tag.js"),
    shared_official_batch_case!("test/parallel/test-crypto-gcm-implicit-short-tag.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-classes.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-lazy-transform-writable.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-stream.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-hkdf.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-pbkdf2.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-scrypt.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-crypto-scrypt.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-crypto-scrypt.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_CRYPTO_HASH_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-constructor.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-errors.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-leak.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-generate-keys.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-group-setters.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-modp2-views.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-modp2.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-odd-key.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-padding.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-shared.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-dh.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("node22/test/parallel/test-crypto-dh.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-crypto-dh.js"),
        shared_extra_files: COMMON_CRYPTO_HASH_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-ecdh-convert-key.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-default-shake-lengths.js",
        node20_fixture_source_path: Some(
            "node20/test/parallel/test-crypto-default-shake-lengths.js",
        ),
        node22_fixture_source_path: None,
        node24_fixture_source_path: Some(
            "node24/test/parallel/test-crypto-default-shake-lengths.js",
        ),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-default-shake-lengths-oneshot.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: None,
        node24_fixture_source_path: Some(
            "node24/test/parallel/test-crypto-default-shake-lengths-oneshot.js",
        ),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-oneshot-hash-xof.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: None,
        node24_fixture_source_path: Some("node24/test/parallel/test-crypto-oneshot-hash-xof.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case!("test/parallel/test-v8-version-tag.js"),
    shared_official_batch_case!("test/parallel/test-v8-deserialize-buffer.js"),
    shared_official_batch_case!("test/parallel/test-v8-serdes.js"),
    shared_official_batch_case!("test/parallel/test-v8-stats.js"),
    shared_official_batch_case!("test/parallel/test-v8-flag-type-check.js"),
    shared_official_batch_case!("test/parallel/test-vm-basic.js"),
    shared_official_batch_case!("test/parallel/test-vm-context.js"),
    shared_official_batch_case!("test/parallel/test-vm-run-in-new-context.js"),
    shared_official_batch_case!("test/parallel/test-vm-strict-mode.js"),
    shared_official_batch_case!("test/parallel/test-vm-not-strict.js"),
    shared_official_batch_case!("test/parallel/test-vm-create-context-arg.js"),
    shared_official_batch_case!("test/parallel/test-inspector-module.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-inspector-invalid-args.js",
        INSPECTOR_FRONT_EDGE_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-inspector-open.js",
        INSPECTOR_FRONT_EDGE_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-inspector-open-port-integer-overflow.js"),
    shared_official_batch_case!("test/parallel/test-inspector-enabled.js"),
];

// Keep only explicit targeted repros below. Green corpus coverage lives in the
// two manifest-driven batch lanes so the full suite does not execute the same
// fixture bodies twice.

#[test]
#[ignore = "Pinned application-preset restriction: process.env string-key mutation and deletion are intentionally denied outside tooling-owned host surfaces"]
fn node22_process_env_delete_application_preset_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-process-env-delete.js",
        "node22/test/parallel/test-process-env-delete.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned application-preset restriction: process.env string-key mutation and deletion are intentionally denied outside tooling-owned host surfaces"]
fn node20_process_env_delete_application_preset_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-process-env-delete.js",
        "node20/test/parallel/test-process-env-delete.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned official Node20/Node22 assert gap: current runtime still disagrees with the shared test-assert-deep.js circular/deep-diff expectations"]
fn node22_assert_deep_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-assert-deep.js",
        "test/parallel/test-assert-deep.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned official Node20/Node22 assert gap: current runtime still disagrees with the shared test-assert-deep.js circular/deep-diff expectations"]
fn node20_assert_deep_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-assert-deep.js",
        "node20/test/parallel/test-assert-deep.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node22 runtime gap: official test-assert-partial-deep-equal.js currently aborts through a rusty_v8 weak-handle panic in the embedded runtime path"]
fn node22_assert_partial_deep_equal_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-assert-partial-deep-equal.js",
        "test/parallel/test-assert-partial-deep-equal.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned shared Deno-family inspect gap: revoked proxy formatting still throws inside ext/web and blocks test-console-issue-43095.js"]
fn node22_console_issue_43095_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-console-issue-43095.js",
        "test/parallel/test-console-issue-43095.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned shared Deno-family inspect gap: revoked proxy formatting still throws inside ext/web and blocks test-console-issue-43095.js"]
fn node20_console_issue_43095_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-console-issue-43095.js",
        "node20/test/parallel/test-console-issue-43095.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node20 divergence: official v20.20.2 still accepts once(emitter, event, null), while the current runtime matches the newer Node22 invalid-options behavior and rejects null"]
fn node20_events_once_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-events-once.js",
        "node20/test/parallel/test-events-once.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node20 divergence: official v20.20.2 process.features does not expose the Node22-only `typescript` key that Nimbus intentionally keeps in its single Node22-shaped runtime contract"]
fn node20_process_features_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-process-features.js",
        "node20/test/parallel/test-process-features.js",
        &[],
    );
}

#[test]
fn node22_process_finalization_close_fixture() {
    run_manifested_fixture_with_postlude(
        "test/fixtures/process/close.mjs",
        "test/fixtures/process/close.mjs",
        &[],
        r#"
  globalThis.process.emit("exit", globalThis.process.exitCode ?? 0);
"#,
    );
}

#[test]
fn node22_process_finalization_before_exit_fixture() {
    run_manifested_fixture_with_postlude(
        "test/fixtures/process/before-exit.mjs",
        "test/fixtures/process/before-exit.mjs",
        &[],
        r#"
  globalThis.process.emit("beforeExit", globalThis.process.exitCode ?? 0);
  await new Promise((resolve) => setTimeout(resolve, 125));
  globalThis.process.emit("beforeExit", globalThis.process.exitCode ?? 0);
  globalThis.process.emit("exit", globalThis.process.exitCode ?? 0);
"#,
    );
}

#[test]
fn node22_process_finalization_unregister_fixture() {
    run_manifested_fixture_with_postlude(
        "test/fixtures/process/unregister.mjs",
        "test/fixtures/process/unregister.mjs",
        &[],
        r#"
  globalThis.process.emit("exit", globalThis.process.exitCode ?? 0);
"#,
    );
}

#[test]
#[ignore = "Pinned later-family dependency: official test-process-finalization.mjs now runs through the Nimbus sync subprocess harness, and the only remaining failure is different-registry-per-thread.mjs because worker_threads are still owned by a later compatibility family"]
fn node22_process_finalization_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-process-finalization.mjs",
        "node22/test/parallel/test-process-finalization.mjs",
        PROCESS_FINALIZATION_WATCHPOINT_EXTRA_FILES,
    );
}

#[test]
#[ignore = "Pinned Node20 divergence: official v20.20.2 PerformanceResourceTiming#toJSON() omits the Node22-era `deliveryType` and `responseStatus` fields that Nimbus intentionally keeps in its single Node22-shaped runtime contract"]
fn node20_perf_hooks_resourcetiming_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-perf-hooks-resourcetiming.js",
        "node20/test/parallel/test-perf-hooks-resourcetiming.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node20 divergence: official v20.20.2 test-stream-duplex-readable-end.js still probes the older default-highWaterMark flow-control path, while the current runtime matches the later Node22/Node24 explicit-highWaterMark shape"]
fn node20_stream_duplex_readable_end_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-duplex-readable-end.js",
        "node20/test/parallel/test-stream-duplex-readable-end.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node20 divergence: official v20.20.2 test-stream-transform-split-highwatermark.js still expects the older 16 KiB split Transform default highWaterMark, while the current runtime matches the later Node22/Node24 getDefaultHighWaterMark() contract"]
fn node20_stream_transform_split_highwatermark_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-transform-split-highwatermark.js",
        "node20/test/parallel/test-stream-transform-split-highwatermark.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node20 divergence: official v20.20.2 test-stream-transform-split-objectmode.js still expects the older 16 KiB split Transform default highWaterMark, while the current runtime matches the later Node22/Node24 non-Windows 64 KiB contract"]
fn node20_stream_transform_split_objectmode_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-transform-split-objectmode.js",
        "node20/test/parallel/test-stream-transform-split-objectmode.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node20 divergence: official v20.20.2 test-stream-readable-infinite-read.js still depends on the older default Readable highWaterMark accumulation path, while the current runtime matches the later Node22/Node24 explicit-highWaterMark behavior"]
fn node20_stream_readable_infinite_read_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-readable-infinite-read.js",
        "node20/test/parallel/test-stream-readable-infinite-read.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned application-preset path-policy divergence: test-fs-open.js expects ENOENT for an absolute missing host path outside the generated bundle root, while Nimbus intentionally denies that path before raw host open"]
fn node22_fs_open_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-open.js",
        "node22/test/parallel/test-fs-open.js",
        &[],
    );
}

#[test]
fn node22_fs_write_file_flush_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-write-file-flush.js",
        "node22/test/parallel/test-fs-write-file-flush.js",
        &[],
    );
}

#[test]
fn node22_fs_append_file_flush_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-append-file-flush.js",
        "node22/test/parallel/test-fs-append-file-flush.js",
        &[],
    );
}

#[test]
fn node22_fs_watch_abort_signal_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-watch-abort-signal.js",
        "node22/test/parallel/test-fs-watch-abort-signal.js",
        SHARED_FIXTURES_DIR_EXTRA_FILES,
    );
}

#[test]
fn node22_fs_watch_enoent_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-watch-enoent.js",
        "node22/test/parallel/test-fs-watch-enoent.js",
        &[],
    );
}

#[test]
fn node22_fs_watch_recursive_promise_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-watch-recursive-promise.js",
        "node22/test/parallel/test-fs-watch-recursive-promise.js",
        &[],
    );
}

#[test]
fn node22_fs_watch_recursive_add_file_to_new_folder_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-watch-recursive-add-file-to-new-folder.js",
        "node22/test/parallel/test-fs-watch-recursive-add-file-to-new-folder.js",
        &[],
    );
}

#[test]
fn node22_fs_watch_recursive_add_file_to_existing_subfolder_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-watch-recursive-add-file-to-existing-subfolder.js",
        "node22/test/parallel/test-fs-watch-recursive-add-file-to-existing-subfolder.js",
        &[],
    );
}

#[test]
fn node22_fs_watch_recursive_watch_file_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-watch-recursive-watch-file.js",
        "node22/test/parallel/test-fs-watch-recursive-watch-file.js",
        &[],
    );
}

#[test]
fn node22_fs_watch_recursive_symlink_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-watch-recursive-symlink.js",
        "node22/test/parallel/test-fs-watch-recursive-symlink.js",
        &[],
    );
}

#[test]
fn node22_fs_promises_watch_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-promises-watch.js",
        "node22/test/parallel/test-fs-promises-watch.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned application-preset divergence: test-fs-readdir-buffer.js probes /dev outside the generated bundle root, so the runtime intentionally denies that host path instead of claiming broad host-fs parity"]
fn node22_fs_readdir_buffer_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-readdir-buffer.js",
        "node22/test/parallel/test-fs-readdir-buffer.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned application-preset divergence: official test-fs-filehandle-use-after-close.js reopens process.execPath outside the generated bundle root, so the runtime intentionally denies that absolute host path before the later EBADF assertion can run"]
fn node22_fs_filehandle_use_after_close_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-filehandle-use-after-close.js",
        "node22/test/parallel/test-fs-filehandle-use-after-close.js",
        &[],
    );
}

#[test]
#[ignore = "Cross-family follow-up: official test-fs-write-file-sync.js no longer self-skips after the main-thread worker bootstrap fix and is green in the focused NLC8 worker batch, but it has not been re-promoted into the streams/local-io denominator yet"]
fn node22_fs_write_file_sync_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-write-file-sync.js",
        "node22/test/parallel/test-fs-write-file-sync.js",
        &[],
    );
}

#[test]
#[ignore = "Cross-family NLC8 seam: official test-fs-realpath.js no longer self-skips after the main-thread worker bootstrap fix and now fails on a real AlreadyExists symlink/setup path"]
fn node22_fs_realpath_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-realpath.js",
        "node22/test/parallel/test-fs-realpath.js",
        CYCLE_FIXTURES_EXTRA_FILES,
    );
}

#[test]
fn node20_tty_backwards_api_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-tty-backwards-api.js",
        "node20/test/parallel/test-tty-backwards-api.js",
        &[],
    );
}

#[test]
fn node22_tty_backwards_api_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-tty-backwards-api.js",
        "node22/test/parallel/test-tty-backwards-api.js",
        &[],
    );
}

#[test]
fn node22_tty_stdin_end_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-tty-stdin-end.js",
        "node22/test/parallel/test-tty-stdin-end.js",
        &[],
    );
}

#[test]
fn node22_tty_stdin_pipe_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-tty-stdin-pipe.js",
        "node22/test/parallel/test-tty-stdin-pipe.js",
        &[],
    );
}

#[test]
fn node20_tty_stdin_end_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-tty-stdin-end.js",
        "node20/test/parallel/test-tty-stdin-end.js",
        &[],
    );
}

#[test]
fn node20_tty_stdin_pipe_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-tty-stdin-pipe.js",
        "node20/test/parallel/test-tty-stdin-pipe.js",
        &[],
    );
}

#[test]
fn node24_tty_stdin_end_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-tty-stdin-end.js",
        "node24/test/parallel/test-tty-stdin-end.js",
        &[],
    );
}

#[test]
fn node24_tty_stdin_pipe_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-tty-stdin-pipe.js",
        "node24/test/parallel/test-tty-stdin-pipe.js",
        &[],
    );
}

#[test]
fn node24_tty_backwards_api_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-tty-backwards-api.js",
        "node24/test/parallel/test-tty-backwards-api.js",
        &[],
    );
}

#[test]
fn node22_readline_csi_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-readline-csi.js",
        "node22/test/parallel/test-readline-csi.js",
        &[],
    );
}

#[test]
fn node22_readline_carriage_return_between_chunks_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-readline-carriage-return-between-chunks.js",
        "node22/test/parallel/test-readline-carriage-return-between-chunks.js",
        &[],
    );
}

#[test]
fn node22_readline_input_onerror_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-readline-input-onerror.js",
        "node22/test/parallel/test-readline-input-onerror.js",
        &[],
    );
}

#[test]
fn node22_readline_promises_csi_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-readline-promises-csi.mjs",
        "node22/test/parallel/test-readline-promises-csi.mjs",
        NODE22_COMMON_INDEX_MJS_EXTRA_FILES,
    );
}

#[test]
fn node22_stream_add_abort_signal_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-add-abort-signal.js",
        "node22/test/parallel/test-stream-add-abort-signal.js",
        &[],
    );
}

#[test]
fn node22_stream_base_prototype_accessors_enumerability_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-base-prototype-accessors-enumerability.js",
        "node22/test/parallel/test-stream-base-prototype-accessors-enumerability.js",
        &[],
    );
}

#[test]
fn node22_stream_catch_rejections_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-catch-rejections.js",
        "node22/test/parallel/test-stream-catch-rejections.js",
        &[],
    );
}

#[test]
fn node22_stream_compose_operator_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-compose-operator.js",
        "node22/test/parallel/test-stream-compose-operator.js",
        &[],
    );
}

#[test]
fn node22_stream_set_default_hwm_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-set-default-hwm.js",
        "node22/test/parallel/test-stream-set-default-hwm.js",
        &[],
    );
}

#[test]
fn node22_stream_readable_dispose_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-readable-dispose.js",
        "node22/test/parallel/test-stream-readable-dispose.js",
        &[],
    );
}

#[test]
fn node22_stream_readable_from_web_termination_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-readable-from-web-termination.js",
        "node22/test/parallel/test-stream-readable-from-web-termination.js",
        &[],
    );
}

#[test]
fn node22_stream_readable_strategy_option_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-readable-strategy-option.js",
        "node22/test/parallel/test-stream-readable-strategy-option.js",
        &[],
    );
}

#[test]
fn node22_stream_readable_to_web_termination_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-readable-to-web-termination.js",
        "node22/test/parallel/test-stream-readable-to-web-termination.js",
        &[],
    );
}

#[test]
fn node22_stream_state_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-stream-state-batch",
        "node22",
        NODE22_STREAM_STATE_BATCH_FIXTURES,
        &[],
    );
}

#[test]
fn node22_stream_buffering_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-stream-buffering-batch",
        "node22",
        NODE22_STREAM_BUFFERING_BATCH_FIXTURES,
        &[],
    );
}

#[test]
fn node22_tty_os_tail_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-tty-os-tail-batch",
        "node22",
        NODE22_TTY_OS_TAIL_BATCH_FIXTURES,
        &[],
    );
}

#[test]
fn node22_networking_pure_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-pure-batch",
        "node22",
        NODE22_NETWORKING_PURE_BATCH_FIXTURES,
        &[],
    );
}

#[test]
fn node22_networking_net_server_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-net-server-batch",
        "node22",
        NODE22_NETWORKING_NET_SERVER_BATCH_FIXTURES,
        &[],
    );
}

#[test]
fn node22_networking_net_socket_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-net-socket-batch",
        "node22",
        NODE22_NETWORKING_NET_SOCKET_BATCH_FIXTURES,
        &[],
    );
}

#[test]
fn node22_networking_http_request_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-http-request-batch",
        "node22",
        NODE22_NETWORKING_HTTP_REQUEST_BATCH_FIXTURES,
        &[],
    );
}

#[test]
fn node22_networking_http_timeout_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-http-timeout-batch",
        "node22",
        NODE22_NETWORKING_HTTP_TIMEOUT_BATCH_FIXTURES,
        &[],
    );
}

#[test]
fn node22_networking_http_response_positive_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-http-response-positive-batch",
        "node22",
        NODE22_NETWORKING_HTTP_RESPONSE_POSITIVE_BATCH_FIXTURES,
        &[],
    );
}

#[test]
fn node22_networking_http_response_state_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-http-response-state-batch",
        "node22",
        NODE22_NETWORKING_HTTP_RESPONSE_STATE_BATCH_FIXTURES,
        &[],
    );
    run_node_compat_watchpoint_batch(
        "node22-networking-http-response-state-countdown-batch",
        "node22",
        NODE22_NETWORKING_HTTP_RESPONSE_STATE_COUNTDOWN_BATCH_FIXTURES,
        COMMON_COUNTDOWN_EXTRA_FILES,
    );
}

#[test]
fn node22_networking_server_no_arg_listen_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-server-no-arg-listen-batch",
        "node22",
        NODE22_NETWORKING_SERVER_NO_ARG_LISTEN_BATCH_FIXTURES,
        &[],
    );
}

#[test]
fn node22_networking_http_agent_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-http-agent-batch",
        "node22",
        NODE22_NETWORKING_HTTP_AGENT_BATCH_FIXTURES,
        COMMON_COUNTDOWN_EXTRA_FILES,
    );
}

#[test]
fn node22_networking_http_agent_lifecycle_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-http-agent-lifecycle-batch",
        "node22",
        NODE22_NETWORKING_HTTP_AGENT_LIFECYCLE_BATCH_FIXTURES,
        COMMON_COUNTDOWN_EXTRA_FILES,
    );
}

#[test]
fn node22_networking_dgram_helper_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-dgram-helper-batch",
        "node22",
        NODE22_NETWORKING_DGRAM_HELPER_BATCH_FIXTURES,
        &[],
    );
}

#[test]
fn node22_networking_dgram_bind_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-dgram-bind-batch",
        "node22",
        NODE22_NETWORKING_DGRAM_BIND_BATCH_FIXTURES,
        &[],
    );
}

#[test]
fn node22_networking_dgram_connect_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-dgram-connect-batch",
        "node22",
        NODE22_NETWORKING_DGRAM_CONNECT_BATCH_FIXTURES,
        &[],
    );
}

#[test]
fn node22_networking_dgram_send_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-dgram-send-batch",
        "node22",
        NODE22_NETWORKING_DGRAM_SEND_BATCH_FIXTURES,
        &[],
    );
}

#[test]
fn node22_networking_dgram_remaining_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-dgram-remaining-batch",
        "node22",
        NODE22_NETWORKING_DGRAM_REMAINING_BATCH_FIXTURES,
        NODE22_COMMON_UDP_EXTRA_FILES,
    );
}

#[test]
#[ignore = "diagnostic batch for local Deno UDP owner patches"]
fn node22_networking_dgram_local_patch_regression_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-dgram-local-patch-regression-batch",
        "node22",
        NODE22_NETWORKING_DGRAM_LOCAL_PATCH_REGRESSION_BATCH_FIXTURES,
        &[],
    );
}

#[test]
fn node22_networking_crypto_gated_helper_batch_fixture() {
    run_node_compat_watchpoint_batch(
        "node22-networking-crypto-gated-helper-batch",
        "node22",
        NODE22_NETWORKING_CRYPTO_GATED_HELPER_BATCH_FIXTURES,
        COMMON_TLS_KEY_EXTRA_FILES,
    );
}

#[test]
fn node22_networking_http2_header_status_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-networking-http2-header-status-batch",
        NodeCompatLane::Node22,
        NODE22_NETWORKING_HTTP2_HEADER_STATUS_BATCH,
    );
}

#[test]
fn node22_networking_http2_compat_request_response_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-networking-http2-compat-request-response-batch",
        NodeCompatLane::Node22,
        NODE22_NETWORKING_HTTP2_COMPAT_REQUEST_RESPONSE_BATCH,
    );
}

#[test]
fn node22_networking_http2_compat_serverresponse_lifecycle_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-networking-http2-compat-serverresponse-lifecycle-batch",
        NodeCompatLane::Node22,
        NODE22_NETWORKING_HTTP2_COMPAT_SERVERRESPONSE_LIFECYCLE_BATCH,
    );
}

#[test]
fn node22_networking_http2_compat_remainder_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-networking-http2-compat-remainder-batch",
        NodeCompatLane::Node22,
        NODE22_NETWORKING_HTTP2_COMPAT_REMAINDER_BATCH,
    );
}

#[test]
fn node22_networking_https_agent_session_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-networking-https-agent-session-batch",
        NodeCompatLane::Node22,
        NODE22_NETWORKING_HTTPS_AGENT_SESSION_BATCH,
    );
}

#[test]
fn node22_networking_https_local_server_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-networking-https-local-server-batch",
        NodeCompatLane::Node22,
        NODE22_NETWORKING_HTTPS_LOCAL_SERVER_BATCH,
    );
}

#[test]
fn node22_networking_https_server_lifecycle_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-networking-https-server-lifecycle-batch",
        NodeCompatLane::Node22,
        NODE22_NETWORKING_HTTPS_SERVER_LIFECYCLE_BATCH,
    );
}

#[test]
fn node22_networking_https_client_server_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-networking-https-client-server-batch",
        NodeCompatLane::Node22,
        NODE22_NETWORKING_HTTPS_CLIENT_SERVER_BATCH,
    );
}

#[test]
fn node22_networking_https_tls_session_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-networking-https-tls-session-batch",
        NodeCompatLane::Node22,
        NODE22_NETWORKING_HTTPS_TLS_SESSION_BATCH,
    );
}

#[test]
fn node22_networking_tls_local_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-networking-tls-local-batch",
        NodeCompatLane::Node22,
        NODE22_NETWORKING_TLS_LOCAL_BATCH,
    );
}

#[test]
fn node22_nlc7_module_commonjs_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc7-module-commonjs-batch",
        NodeCompatLane::Node22,
        NODE22_NLC7_MODULE_COMMONJS_BATCH,
    );
}

#[test]
fn node22_nlc7_async_local_storage_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc7-async-local-storage-batch",
        NodeCompatLane::Node22,
        NODE22_NLC7_ASYNC_LOCAL_STORAGE_BATCH,
    );
}

#[test]
fn node24_nlc7_async_local_storage_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-nlc7-async-local-storage-batch",
        NodeCompatLane::Node24,
        NODE22_NLC7_ASYNC_LOCAL_STORAGE_BATCH,
    );
}

#[test]
#[ignore = "Pinned Node20 supported-lane divergence: official v20.20.2 test-async-local-storage-exit-does-not-leak.js still expects the old JavaScript AsyncLocalStorage _propagate hook, while the current runtime matches the newer Node22/Node24 implementation shape"]
fn node20_async_local_storage_exit_does_not_leak_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-async-local-storage-exit-does-not-leak.js",
        "node20/test/parallel/test-async-local-storage-exit-does-not-leak.js",
        &[],
    );
}

#[test]
fn node22_nlc7_async_hooks_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc7-async-hooks-batch",
        NodeCompatLane::Node22,
        NODE22_NLC7_ASYNC_HOOKS_BATCH,
    );
}

#[test]
fn node20_nlc7_async_hooks_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-nlc7-async-hooks-batch",
        NodeCompatLane::Node20,
        NODE22_NLC7_ASYNC_HOOKS_BATCH,
    );
}

#[test]
fn node24_nlc7_async_hooks_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-nlc7-async-hooks-batch",
        NodeCompatLane::Node24,
        NODE22_NLC7_ASYNC_HOOKS_BATCH,
    );
}

#[test]
fn node22_nlc7_async_hooks_promise_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc7-async-hooks-promise-batch",
        NodeCompatLane::Node22,
        NODE22_NLC7_ASYNC_HOOKS_PROMISE_BATCH,
    );
}

#[test]
fn node20_nlc7_async_hooks_promise_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-nlc7-async-hooks-promise-batch",
        NodeCompatLane::Node20,
        NODE22_NLC7_ASYNC_HOOKS_PROMISE_BATCH,
    );
}

#[test]
fn node24_nlc7_async_hooks_promise_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-nlc7-async-hooks-promise-batch",
        NodeCompatLane::Node24,
        NODE22_NLC7_ASYNC_HOOKS_PROMISE_BATCH,
    );
}

#[test]
fn node22_nlc7_async_hooks_promise_core_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc7-async-hooks-promise-core-batch",
        NodeCompatLane::Node22,
        NODE22_NLC7_ASYNC_HOOKS_PROMISE_CORE_BATCH,
    );
}

#[test]
fn node20_nlc7_async_hooks_promise_core_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-nlc7-async-hooks-promise-core-batch",
        NodeCompatLane::Node20,
        NODE22_NLC7_ASYNC_HOOKS_PROMISE_CORE_BATCH,
    );
}

#[test]
fn node24_nlc7_async_hooks_promise_core_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-nlc7-async-hooks-promise-core-batch",
        NodeCompatLane::Node24,
        NODE22_NLC7_ASYNC_HOOKS_PROMISE_CORE_BATCH,
    );
}

#[test]
fn node22_nlc8_worker_main_thread_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc8-worker-main-thread-batch",
        NodeCompatLane::Node22,
        NLC8_WORKER_MAIN_THREAD_BATCH,
    );
}

#[test]
fn node20_nlc8_worker_main_thread_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-nlc8-worker-main-thread-batch",
        NodeCompatLane::Node20,
        NLC8_WORKER_MAIN_THREAD_BATCH,
    );
}

#[test]
fn node24_nlc8_worker_main_thread_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-nlc8-worker-main-thread-batch",
        NodeCompatLane::Node24,
        NLC8_WORKER_MAIN_THREAD_BATCH,
    );
}

#[test]
fn node22_nlc8_worker_basic_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc8-worker-basic-batch",
        NodeCompatLane::Node22,
        NLC8_WORKER_BASIC_BATCH,
    );
}

#[test]
fn node20_nlc8_worker_basic_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-nlc8-worker-basic-batch",
        NodeCompatLane::Node20,
        NLC8_WORKER_BASIC_BATCH,
    );
}

#[test]
fn node24_nlc8_worker_basic_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-nlc8-worker-basic-batch",
        NodeCompatLane::Node24,
        NLC8_WORKER_BASIC_BATCH,
    );
}

#[test]
fn node22_nlc8_worker_bootstrap_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc8-worker-bootstrap-batch",
        NodeCompatLane::Node22,
        NLC8_WORKER_BOOTSTRAP_BATCH,
    );
}

#[test]
fn node20_nlc8_worker_bootstrap_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-nlc8-worker-bootstrap-batch",
        NodeCompatLane::Node20,
        NLC8_WORKER_BOOTSTRAP_BATCH,
    );
}

#[test]
fn node24_nlc8_worker_bootstrap_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-nlc8-worker-bootstrap-batch",
        NodeCompatLane::Node24,
        NLC8_WORKER_BOOTSTRAP_BATCH,
    );
}

#[test]
fn node22_nlc8_worker_contract_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc8-worker-contract-batch",
        NodeCompatLane::Node22,
        NLC8_WORKER_CONTRACT_BATCH,
    );
}

#[test]
fn node20_nlc8_worker_contract_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-nlc8-worker-contract-batch",
        NodeCompatLane::Node20,
        NLC8_WORKER_CONTRACT_BATCH,
    );
}

#[test]
fn node24_nlc8_worker_contract_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-nlc8-worker-contract-batch",
        NodeCompatLane::Node24,
        NLC8_WORKER_CONTRACT_BATCH,
    );
}

#[test]
fn node22_nlc8_worker_message_port_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc8-worker-message-port-batch",
        NodeCompatLane::Node22,
        NLC8_WORKER_MESSAGE_PORT_BATCH,
    );
}

#[test]
fn node22_nlc8_worker_message_channel_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc8-worker-message-channel-batch",
        NodeCompatLane::Node22,
        NLC8_WORKER_MESSAGE_CHANNEL_BATCH,
    );
}

#[test]
fn node22_nlc8_worker_onmessage_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-worker-onmessage.js",
        "node22/test/parallel/test-worker-onmessage.js",
        &[],
    );
}

#[test]
fn node22_nlc8_worker_ref_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-worker-ref.js",
        "node22/test/parallel/test-worker-ref.js",
        &[],
    );
}

#[test]
fn node22_nlc8_module_commonjs_remainder_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc8-module-commonjs-remainder-batch",
        NodeCompatLane::Node22,
        NLC8_MODULE_COMMONJS_REMAINDER_BATCH,
    );
}

#[test]
fn node20_nlc8_module_commonjs_remainder_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-nlc8-module-commonjs-remainder-batch",
        NodeCompatLane::Node20,
        NLC8_MODULE_COMMONJS_REMAINDER_BATCH,
    );
}

#[test]
fn node24_nlc8_module_commonjs_remainder_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-nlc8-module-commonjs-remainder-batch",
        NodeCompatLane::Node24,
        NLC8_MODULE_COMMONJS_REMAINDER_BATCH,
    );
}

#[test]
fn node22_nlc8_inspector_front_edge_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc8-inspector-front-edge-batch",
        NodeCompatLane::Node22,
        NLC8_INSPECTOR_FRONT_EDGE_BATCH,
    );
}

#[test]
fn node20_nlc8_inspector_front_edge_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-nlc8-inspector-front-edge-batch",
        NodeCompatLane::Node20,
        NLC8_INSPECTOR_FRONT_EDGE_BATCH,
    );
}

#[test]
fn node24_nlc8_inspector_front_edge_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-nlc8-inspector-front-edge-batch",
        NodeCompatLane::Node24,
        NLC8_INSPECTOR_FRONT_EDGE_BATCH,
    );
}

#[test]
fn node22_nlc8_module_wrapper_regression_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-module-wrapper-regression.js",
        "node22/test/parallel/test-module-wrapper-regression.js",
        &[],
    );
}

#[test]
fn node22_nlc8_module_wrapper_identity_regression_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-module-wrapper-identity-regression.js",
        "node22/test/parallel/test-module-wrapper-identity-regression.js",
        &[],
    );
}

#[test]
fn node22_nlc8_module_wrapper_direct_regression_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-module-wrapper-direct-regression.js",
        "node22/test/parallel/test-module-wrapper-direct-regression.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES,
    );
}

#[test]
fn node22_nlc8_module_wrapper_direct_no_common_regression_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-module-wrapper-direct-no-common-regression.js",
        "node22/test/parallel/test-module-wrapper-direct-no-common-regression.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES,
    );
}

#[test]
fn node22_nlc8_module_wrapper_spawn_regression_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-module-wrapper-spawn-regression.js",
        "node22/test/parallel/test-module-wrapper-spawn-regression.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES,
    );
}

#[test]
fn node22_nlc8_module_wrapper_spawn_require_regression_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-module-wrapper-spawn-require-regression.js",
        "node22/test/parallel/test-module-wrapper-spawn-require-regression.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES,
    );
}

#[test]
fn node22_nlc8_module_wrapper_spawn_wrap_call_regression_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-module-wrapper-spawn-wrap-call-regression.js",
        "node22/test/parallel/test-module-wrapper-spawn-wrap-call-regression.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES,
    );
}

#[test]
fn node22_nlc8_module_wrapper_spawn_node_shape_regression_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-module-wrapper-spawn-node-shape-regression.js",
        "node22/test/parallel/test-module-wrapper-spawn-node-shape-regression.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES,
    );
}

#[test]
fn node22_nlc8_module_wrapper_spawn_newline_wrap_regression_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-module-wrapper-spawn-newline-wrap-regression.js",
        "node22/test/parallel/test-module-wrapper-spawn-newline-wrap-regression.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES,
    );
}

#[test]
fn node22_nlc8_module_wrapper_official_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-module-wrapper.js",
        "node22/test/parallel/test-module-wrapper.js",
        MODULE_COMMONJS_FIXTURES_EXTRA_FILES,
    );
}

#[test]
fn node22_nlc8_vm_basic_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-vm-basic.js",
        "node22/test/parallel/test-vm-basic.js",
        &[],
    );
}

#[test]
fn node22_nlc8_vm_context_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-vm-context.js",
        "node22/test/parallel/test-vm-context.js",
        &[],
    );
}

#[test]
fn node22_nlc8_vm_run_in_new_context_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-vm-run-in-new-context.js",
        "node22/test/parallel/test-vm-run-in-new-context.js",
        &[],
    );
}

#[test]
fn node22_nlc8_vm_context_regression_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc8-vm-context-regression-batch",
        NodeCompatLane::Node22,
        NLC8_VM_CONTEXT_REGRESSION_BATCH,
    );
}

#[test]
fn node22_nlc8_vm_context_remainder_regression_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc8-vm-context-remainder-regression-batch",
        NodeCompatLane::Node22,
        NLC8_VM_CONTEXT_REMAINDER_REGRESSION_BATCH,
    );
}

#[test]
fn node22_nlc8_vm_shared_context_errors_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-vm-context-regression-shared-context-errors.js",
        "node22/test/parallel/test-vm-context-regression-shared-context-errors.js",
        &[],
    );
}

#[test]
fn node22_nlc8_vm_remainder_combined_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-vm-context-regression-remainder-combined.js",
        "node22/test/parallel/test-vm-context-regression-remainder-combined.js",
        &[],
    );
}

#[test]
fn node22_nlc8_vm_official_minus_proxy_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-vm-context-regression-official-minus-proxy.js",
        "node22/test/parallel/test-vm-context-regression-official-minus-proxy.js",
        &[],
    );
}

#[test]
fn node22_nlc8_vm_preamble_plus_proxy_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-vm-context-regression-preamble-plus-proxy.js",
        "node22/test/parallel/test-vm-context-regression-preamble-plus-proxy.js",
        &[],
    );
}

#[test]
fn node22_nlc8_vm_delete_then_proxy_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-vm-context-regression-delete-then-proxy.js",
        "node22/test/parallel/test-vm-context-regression-delete-then-proxy.js",
        &[],
    );
}

#[test]
fn node22_nlc8_vm_shared_errors_plus_proxy_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-vm-context-regression-shared-errors-plus-proxy.js",
        "node22/test/parallel/test-vm-context-regression-shared-errors-plus-proxy.js",
        &[],
    );
}

#[test]
fn node22_nlc8_vm_remainder_plus_proxy_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-vm-context-regression-remainder-plus-proxy.js",
        "node22/test/parallel/test-vm-context-regression-remainder-plus-proxy.js",
        &[],
    );
}

#[test]
fn node22_nlc8_vm_multi_context_plus_proxy_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-vm-context-regression-multi-context-plus-proxy.js",
        "node22/test/parallel/test-vm-context-regression-multi-context-plus-proxy.js",
        &[],
    );
}

#[test]
fn node22_nlc8_v8_helper_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc8-v8-helper-batch",
        NodeCompatLane::Node22,
        NLC8_V8_HELPER_BATCH,
    );
}

#[test]
fn node20_nlc8_v8_helper_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-nlc8-v8-helper-batch",
        NodeCompatLane::Node20,
        NLC8_V8_HELPER_BATCH,
    );
}

#[test]
fn node24_nlc8_v8_helper_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-nlc8-v8-helper-batch",
        NodeCompatLane::Node24,
        NLC8_V8_HELPER_BATCH,
    );
}

#[test]
fn node22_nlc8_v8_green_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc8-v8-green-batch",
        NodeCompatLane::Node22,
        NLC8_V8_GREEN_BATCH,
    );
}

#[test]
fn node20_nlc8_v8_green_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-nlc8-v8-green-batch",
        NodeCompatLane::Node20,
        NLC8_V8_GREEN_BATCH,
    );
}

#[test]
fn node24_nlc8_v8_green_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-nlc8-v8-green-batch",
        NodeCompatLane::Node24,
        NLC8_V8_GREEN_BATCH,
    );
}

#[test]
fn node22_nlc8_vm_basic_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc8-vm-basic-batch",
        NodeCompatLane::Node22,
        NLC8_VM_BASIC_BATCH,
    );
}

#[test]
fn node20_nlc8_vm_basic_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-nlc8-vm-basic-batch",
        NodeCompatLane::Node20,
        NLC8_VM_BASIC_BATCH,
    );
}

#[test]
fn node24_nlc8_vm_basic_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-nlc8-vm-basic-batch",
        NodeCompatLane::Node24,
        NLC8_VM_BASIC_BATCH,
    );
}

#[test]
fn node22_nlc9_domain_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc9-domain-foundation-batch",
        NodeCompatLane::Node22,
        NLC9_DOMAIN_FOUNDATION_BATCH,
    );
}

#[test]
fn node20_nlc9_domain_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-nlc9-domain-foundation-batch",
        NodeCompatLane::Node20,
        NLC9_DOMAIN_FOUNDATION_BATCH,
    );
}

#[test]
fn node24_nlc9_domain_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-nlc9-domain-foundation-batch",
        NodeCompatLane::Node24,
        NLC9_DOMAIN_FOUNDATION_BATCH,
    );
}

#[test]
fn node22_nlc9_domain_promise_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-domain-promise.js",
        "node22/test/parallel/test-domain-promise.js",
        &[],
    );
}

#[test]
fn node22_nlc9_constants_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc9-constants-foundation-batch",
        NodeCompatLane::Node22,
        NLC9_CONSTANTS_FOUNDATION_BATCH,
    );
}

#[test]
fn node20_nlc9_constants_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-nlc9-constants-foundation-batch",
        NodeCompatLane::Node20,
        NLC9_CONSTANTS_FOUNDATION_BATCH,
    );
}

#[test]
fn node24_nlc9_constants_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-nlc9-constants-foundation-batch",
        NodeCompatLane::Node24,
        NLC9_CONSTANTS_FOUNDATION_BATCH,
    );
}

#[test]
fn node22_nlc9_trace_events_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc9-trace-events-foundation-batch",
        NodeCompatLane::Node22,
        NLC9_TRACE_EVENTS_FOUNDATION_BATCH,
    );
}

#[test]
fn node22_nlc9_sys_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc9-sys-foundation-batch",
        NodeCompatLane::Node22,
        NLC9_SYS_FOUNDATION_BATCH,
    );
}

#[test]
fn node20_nlc9_sys_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-nlc9-sys-foundation-batch",
        NodeCompatLane::Node20,
        NLC9_SYS_FOUNDATION_BATCH,
    );
}

#[test]
fn node24_nlc9_sys_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-nlc9-sys-foundation-batch",
        NodeCompatLane::Node24,
        NLC9_SYS_FOUNDATION_BATCH,
    );
}

#[test]
fn node22_nlc9_sqlite_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc9-sqlite-foundation-batch",
        NodeCompatLane::Node22,
        NLC9_SQLITE_FOUNDATION_BATCH,
    );
}

#[test]
#[ignore = "Pinned NLC9 sqlite build-preset watchpoint: test-sqlite.js now narrows to the bundled percentile capability seam because the current bundled SQLCipher sqlite source does not expose percentile() even after the Node-style URI/path and SQLTagStore fixes"]
fn node22_nlc9_sqlite_build_preset_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-sqlite.js",
        "test/parallel/test-sqlite.js",
        NLC9_SQLITE_NEXT_DB_EXTRA_FILES,
    );
}

#[test]
fn node22_nlc9_wasi_validation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc9-wasi-validation-batch",
        NodeCompatLane::Node22,
        NLC9_WASI_VALIDATION_BATCH,
    );
}

#[test]
fn node22_nlc9_wasi_execution_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc9-wasi-execution-batch",
        NodeCompatLane::Node22,
        NLC9_WASI_EXECUTION_BATCH,
    );
}

#[test]
fn node22_nlc9_wasi_filesystem_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc9-wasi-filesystem-foundation-batch",
        NodeCompatLane::Node22,
        NLC9_WASI_FILESYSTEM_FOUNDATION_BATCH,
    );
}

#[test]
fn node22_nlc9_wasi_preopen_io_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc9-wasi-preopen-io-batch",
        NodeCompatLane::Node22,
        NLC9_WASI_PREOPEN_IO_BATCH,
    );
}

#[test]
fn node22_nlc9_wasi_io_subcase_watchpoint_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc9-wasi-io-subcase-watchpoint-batch",
        NodeCompatLane::Node22,
        NLC9_WASI_IO_SUBCASE_WATCHPOINT_BATCH,
    );
}

#[test]
fn node22_nlc9_sea_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc9-sea-foundation-batch",
        NodeCompatLane::Node22,
        NLC9_SEA_FOUNDATION_BATCH,
    );
}

#[test]
fn node22_nlc9_repl_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc9-repl-foundation-batch",
        NodeCompatLane::Node22,
        NLC9_REPL_FOUNDATION_BATCH,
    );
}

#[test]
fn node22_nlc9_test_runner_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc9-test-runner-foundation-batch",
        NodeCompatLane::Node22,
        NLC9_TEST_RUNNER_FOUNDATION_BATCH,
    );
}

#[test]
fn node22_nlc9_test_runner_context_metadata_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc9-test-runner-context-metadata-batch",
        NodeCompatLane::Node22,
        NLC9_TEST_RUNNER_CONTEXT_METADATA_BATCH,
    );
}

#[test]
fn node22_nlc9_test_runner_run_event_metadata_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc9-test-runner-run-event-metadata-batch",
        NodeCompatLane::Node22,
        NLC9_TEST_RUNNER_RUN_EVENT_METADATA_BATCH,
    );
}

#[test]
fn node22_nlc9_test_runner_option_validation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc9-test-runner-option-validation-batch",
        NodeCompatLane::Node22,
        NLC9_TEST_RUNNER_OPTION_VALIDATION_BATCH,
    );
}

#[test]
fn node22_nlc9_test_runner_plan_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc9-test-runner-plan-batch",
        NodeCompatLane::Node22,
        NLC9_TEST_RUNNER_PLAN_BATCH,
    );
}

#[test]
fn node22_nlc9_test_runner_run_edge_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc9-test-runner-run-edge-batch",
        NodeCompatLane::Node22,
        NLC9_TEST_RUNNER_RUN_EDGE_BATCH,
    );
}

#[test]
fn node22_nlc9_test_runner_reporters_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc9-test-runner-reporters-batch",
        NodeCompatLane::Node22,
        NLC9_TEST_RUNNER_REPORTERS_BATCH,
    );
}

#[test]
#[ignore = "Pinned NLC9 node:test/reporters watchpoint: test-runner-run-files-undefined.mjs is now narrowed to the missing node:test/reporters builtin family rather than the earlier eval/input-type harness gap"]
fn node22_nlc9_test_runner_reporters_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-runner-run-files-undefined.mjs",
        "test/parallel/test-runner-run-files-undefined.mjs",
        COMMON_TEST_RUNNER_RUN_EDGE_EXTRA_FILES,
    );
}

#[test]
fn node22_nlc9_test_runner_reporter_output_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc9-test-runner-reporter-output-batch",
        NodeCompatLane::Node22,
        NLC9_TEST_RUNNER_REPORTER_OUTPUT_BATCH,
    );
}

#[test]
fn node22_nlc9_test_runner_cli_options_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc9-test-runner-cli-options-batch",
        NodeCompatLane::Node22,
        NLC9_TEST_RUNNER_CLI_OPTIONS_BATCH,
    );
}

#[test]
fn node22_nlc9_test_runner_cli_randomize_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc9-test-runner-cli-randomize-batch",
        NodeCompatLane::Node22,
        NLC9_TEST_RUNNER_CLI_RANDOMIZE_BATCH,
    );
}

#[test]
fn node22_nlc9_test_runner_cli_rerun_failures_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc9-test-runner-cli-rerun-failures-batch",
        NodeCompatLane::Node22,
        NLC9_TEST_RUNNER_CLI_RERUN_FAILURES_BATCH,
    );
}

#[test]
fn node22_nlc9_cluster_worker_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc9-cluster-worker-foundation-batch",
        NodeCompatLane::Node22,
        NLC9_CLUSTER_WORKER_FOUNDATION_BATCH,
    );
}

#[test]
fn node22_nlc9_cluster_worker_lifecycle_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc9-cluster-worker-lifecycle-batch",
        NodeCompatLane::Node22,
        NLC9_CLUSTER_WORKER_LIFECYCLE_BATCH,
    );
}

#[test]
fn node22_nlc9_trace_events_category_used_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-trace-events-category-used.js",
        "node22/test/parallel/test-trace-events-category-used.js",
        &[],
    );
}

#[test]
fn node22_nlc9_trace_events_dynamic_enable_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-trace-events-dynamic-enable.js",
        "node22/test/parallel/test-trace-events-dynamic-enable.js",
        &[],
    );
}

#[test]
fn node22_nlc7_zlib_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc7-zlib-foundation-batch",
        NodeCompatLane::Node22,
        NODE22_NLC7_ZLIB_FOUNDATION_BATCH,
    );
}

#[test]
fn node20_nlc7_zlib_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-nlc7-zlib-foundation-batch",
        NodeCompatLane::Node20,
        NODE22_NLC7_ZLIB_FOUNDATION_BATCH,
    );
}

#[test]
fn node24_nlc7_zlib_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-nlc7-zlib-foundation-batch",
        NodeCompatLane::Node24,
        NODE22_NLC7_ZLIB_FOUNDATION_BATCH,
    );
}

#[test]
fn node22_nlc7_zlib_stream_lifecycle_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc7-zlib-stream-lifecycle-batch",
        NodeCompatLane::Node22,
        NODE22_NLC7_ZLIB_STREAM_LIFECYCLE_BATCH,
    );
}

#[test]
fn node20_nlc7_zlib_stream_lifecycle_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-nlc7-zlib-stream-lifecycle-batch",
        NodeCompatLane::Node20,
        NODE22_NLC7_ZLIB_STREAM_LIFECYCLE_BATCH,
    );
}

#[test]
fn node24_nlc7_zlib_stream_lifecycle_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-nlc7-zlib-stream-lifecycle-batch",
        NodeCompatLane::Node24,
        NODE22_NLC7_ZLIB_STREAM_LIFECYCLE_BATCH,
    );
}

#[test]
fn node22_nlc7_zlib_decompression_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc7-zlib-decompression-batch",
        NodeCompatLane::Node22,
        NODE22_NLC7_ZLIB_DECOMPRESSION_BATCH,
    );
}

#[test]
fn node20_nlc7_zlib_decompression_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-nlc7-zlib-decompression-batch",
        NodeCompatLane::Node20,
        NODE22_NLC7_ZLIB_DECOMPRESSION_BATCH,
    );
}

#[test]
fn node24_nlc7_zlib_decompression_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-nlc7-zlib-decompression-batch",
        NodeCompatLane::Node24,
        NODE22_NLC7_ZLIB_DECOMPRESSION_BATCH,
    );
}

#[test]
fn node22_nlc7_zlib_brotli_and_control_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc7-zlib-brotli-and-control-batch",
        NodeCompatLane::Node22,
        NODE22_NLC7_ZLIB_BROTLI_AND_CONTROL_BATCH,
    );
}

#[test]
fn node20_nlc7_zlib_brotli_and_control_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-nlc7-zlib-brotli-and-control-batch",
        NodeCompatLane::Node20,
        NODE22_NLC7_ZLIB_BROTLI_AND_CONTROL_BATCH,
    );
}

#[test]
fn node24_nlc7_zlib_brotli_and_control_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-nlc7-zlib-brotli-and-control-batch",
        NodeCompatLane::Node24,
        NODE22_NLC7_ZLIB_BROTLI_AND_CONTROL_BATCH,
    );
}

#[test]
fn node22_nlc7_crypto_hash_random_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc7-crypto-hash-random-foundation-batch",
        NodeCompatLane::Node22,
        NODE22_NLC7_CRYPTO_HASH_RANDOM_FOUNDATION_BATCH,
    );
}

#[test]
fn node20_nlc7_crypto_hash_random_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-nlc7-crypto-hash-random-foundation-batch",
        NodeCompatLane::Node20,
        NODE22_NLC7_CRYPTO_HASH_RANDOM_FOUNDATION_BATCH,
    );
}

#[test]
fn node24_nlc7_crypto_hash_random_foundation_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-nlc7-crypto-hash-random-foundation-batch",
        NodeCompatLane::Node24,
        NODE22_NLC7_CRYPTO_HASH_RANDOM_FOUNDATION_BATCH,
    );
}

#[test]
fn node22_nlc7_crypto_kdf_and_stream_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc7-crypto-kdf-and-stream-batch",
        NodeCompatLane::Node22,
        NODE22_NLC7_CRYPTO_KDF_AND_STREAM_BATCH,
    );
}

#[test]
fn node20_nlc7_crypto_kdf_and_stream_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-nlc7-crypto-kdf-and-stream-batch",
        NodeCompatLane::Node20,
        NODE22_NLC7_CRYPTO_KDF_AND_STREAM_BATCH,
    );
}

#[test]
fn node24_nlc7_crypto_kdf_and_stream_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-nlc7-crypto-kdf-and-stream-batch",
        NodeCompatLane::Node24,
        NODE22_NLC7_CRYPTO_KDF_AND_STREAM_BATCH,
    );
}

#[test]
#[ignore = "Pinned Node24 supported-lane divergence: test-crypto-scrypt.js expects ERR_INCOMPATIBLE_OPTION_PAIR for duplicate short/long option pairs, while the current runtime still throws the older ERR_CRYPTO_SCRYPT_INVALID_PARAMETER shape used by the verified Node22 baseline"]
fn node24_nlc7_crypto_scrypt_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-crypto-scrypt.js",
        "node24/test/parallel/test-crypto-scrypt.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES,
    );
}

#[test]
fn node22_nlc7_crypto_cipher_and_padding_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc7-crypto-cipher-and-padding-batch",
        NodeCompatLane::Node22,
        NODE22_NLC7_CRYPTO_CIPHER_AND_PADDING_BATCH,
    );
}

#[test]
fn node20_nlc7_crypto_cipher_and_padding_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-nlc7-crypto-cipher-and-padding-batch",
        NodeCompatLane::Node20,
        NODE22_NLC7_CRYPTO_CIPHER_AND_PADDING_BATCH,
    );
}

#[test]
fn node24_nlc7_crypto_cipher_and_padding_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-nlc7-crypto-cipher-and-padding-batch",
        NodeCompatLane::Node24,
        NODE22_NLC7_CRYPTO_CIPHER_AND_PADDING_BATCH,
    );
}

#[test]
fn node22_nlc7_crypto_dh_and_ecdh_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc7-crypto-dh-and-ecdh-batch",
        NodeCompatLane::Node22,
        NODE22_NLC7_CRYPTO_DH_AND_ECDH_BATCH,
    );
}

#[test]
fn node20_nlc7_crypto_dh_and_ecdh_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-nlc7-crypto-dh-and-ecdh-batch",
        NodeCompatLane::Node20,
        NODE20_NLC7_CRYPTO_DH_AND_ECDH_BATCH,
    );
}

#[test]
fn node24_nlc7_crypto_dh_and_ecdh_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-nlc7-crypto-dh-and-ecdh-batch",
        NodeCompatLane::Node24,
        NODE22_NLC7_CRYPTO_DH_AND_ECDH_BATCH,
    );
}

#[test]
fn node22_nlc7_crypto_dh_safe_prime_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc7-crypto-dh-safe-prime-batch",
        NodeCompatLane::Node22,
        NODE22_NLC7_CRYPTO_DH_SAFE_PRIME_BATCH,
    );
}

#[test]
fn node20_nlc7_crypto_dh_safe_prime_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-nlc7-crypto-dh-safe-prime-batch",
        NodeCompatLane::Node20,
        NODE20_NLC7_CRYPTO_DH_SAFE_PRIME_BATCH,
    );
}

#[test]
fn node22_nlc7_crypto_dh_curves_and_stateless_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc7-crypto-dh-curves-and-stateless-batch",
        NodeCompatLane::Node22,
        NODE22_NLC7_CRYPTO_DH_CURVES_AND_STATELESS_BATCH,
    );
}

#[test]
fn node20_nlc7_crypto_dh_curves_and_stateless_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-nlc7-crypto-dh-curves-and-stateless-batch",
        NodeCompatLane::Node20,
        NODE22_NLC7_CRYPTO_DH_CURVES_AND_STATELESS_BATCH,
    );
}

#[test]
fn node24_nlc7_crypto_dh_curves_and_stateless_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-nlc7-crypto-dh-curves-and-stateless-batch",
        NodeCompatLane::Node24,
        NODE22_NLC7_CRYPTO_DH_CURVES_AND_STATELESS_BATCH,
    );
}

#[test]
fn node24_nlc7_crypto_dh_safe_prime_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-nlc7-crypto-dh-safe-prime-batch",
        NodeCompatLane::Node24,
        NODE22_NLC7_CRYPTO_DH_SAFE_PRIME_BATCH,
    );
}

#[test]
#[ignore = "Pinned Node24 supported-lane drift: test-crypto-dh-stateless.js still expects ERR_OSSL_FAILED_DURING_DERIVATION on the invalid X25519 public-key case"]
fn node24_nlc7_crypto_dh_stateless_supported_watchpoint_batch() {
    run_node_compat_watchpoint_entry_batch(
        "node24-nlc7-crypto-dh-stateless-supported-watchpoints",
        NodeCompatLane::Node24,
        NODE24_NLC7_CRYPTO_DH_STATELESS_SUPPORTED_WATCHPOINT_BATCH,
    );
}

#[test]
#[ignore = "Pinned Node20 supported-lane divergence: test-crypto-dh.js still expects the older OpenSSL invalid-secret message while the verified Node22 baseline now returns the newer unspecified-validation shape"]
fn node20_nlc7_crypto_dh_supported_watchpoint_batch() {
    run_node_compat_watchpoint_entry_batch(
        "node20-nlc7-crypto-dh-supported-watchpoints",
        NodeCompatLane::Node20,
        NODE20_NLC7_CRYPTO_DH_SUPPORTED_WATCHPOINT_BATCH,
    );
}

#[test]
fn node22_nlc7_crypto_authenticated_and_aes_wrap_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nlc7-crypto-authenticated-and-aes-wrap-batch",
        NodeCompatLane::Node22,
        NLC7_CRYPTO_AUTHENTICATED_AND_AES_WRAP_BATCH,
    );
}

#[test]
fn node20_nlc7_crypto_authenticated_and_aes_wrap_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-nlc7-crypto-authenticated-and-aes-wrap-batch",
        NodeCompatLane::Node20,
        NLC7_CRYPTO_AUTHENTICATED_AND_AES_WRAP_BATCH,
    );
}

#[test]
fn node24_nlc7_crypto_authenticated_and_aes_wrap_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-nlc7-crypto-authenticated-and-aes-wrap-batch",
        NodeCompatLane::Node24,
        NLC7_CRYPTO_AUTHENTICATED_AND_AES_WRAP_BATCH,
    );
}

#[test]
#[ignore = "Pinned Node20 supported-lane divergence: test-crypto-authenticated.js still expects the older deprecation-warning ordering without DEP0182"]
fn node20_nlc7_crypto_authenticated_supported_watchpoint_batch() {
    run_node_compat_watchpoint_entry_batch(
        "node20-nlc7-crypto-authenticated-supported-watchpoints",
        NodeCompatLane::Node20,
        NODE20_NLC7_CRYPTO_AUTHENTICATED_SUPPORTED_WATCHPOINT_BATCH,
    );
}

#[test]
fn node20_nlc7_crypto_xof_extension_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node20-nlc7-crypto-xof-extension-batch",
        NodeCompatLane::Node20,
        NLC7_CRYPTO_XOF_EXTENSION_BATCH,
    );
}

#[test]
fn node24_nlc7_crypto_xof_extension_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node24-nlc7-crypto-xof-extension-batch",
        NodeCompatLane::Node24,
        NLC7_CRYPTO_XOF_EXTENSION_BATCH,
    );
}

#[test]
fn node24_https_hwm_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-https-hwm.js",
        "node24/test/parallel/test-https-hwm.js",
        COMMON_TLS_SESSION_CERT_EXTRA_FILES,
    );
}

#[test]
#[ignore = "Pinned Node20 supported-lane divergence: test-https-hwm.js still times out on the current Node20 lane while the Node22/Node24 official files complete"]
fn node20_https_hwm_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-https-hwm.js",
        "node20/test/parallel/test-https-hwm.js",
        COMMON_TLS_SESSION_CERT_EXTRA_FILES,
    );
}

#[test]
#[ignore = "Pinned Node20 supported-lane divergence: test-tls-connect-hwm-option.js still times out on the current Node20 lane while the Node22/Node24 official files complete"]
fn node20_tls_connect_hwm_option_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-tls-connect-hwm-option.js",
        "node20/test/parallel/test-tls-connect-hwm-option.js",
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES,
    );
}

#[test]
#[ignore = "Pinned NLC6 host/preset boundary batch: these https files currently stop at explicit local-address or IPv6 capability boundaries rather than plain HTTPS semantics"]
fn node22_networking_https_address_boundary_batch_watchpoint() {
    run_node_compat_watchpoint_batch(
        "node22-networking-https-address-boundary-batch",
        "node22",
        NODE22_NETWORKING_HTTPS_ADDRESS_BOUNDARY_FIXTURES,
        COMMON_TLS_EXTENDED_CERT_EXTRA_FILES,
    );
}

#[test]
#[ignore = "Pinned NLC6 cross-family boundary batch: these dgram files currently depend on cluster/child-process script-path behavior rather than plain UDP runtime semantics"]
fn node22_networking_dgram_cluster_boundary_batch_watchpoint() {
    run_node_compat_watchpoint_batch(
        "node22-networking-dgram-cluster-boundary-batch",
        "node22",
        NODE22_NETWORKING_DGRAM_CLUSTER_BOUNDARY_FIXTURES,
        &[],
    );
}

#[test]
#[ignore = "Pinned NLC6 host/preset boundary batch: these dgram files currently depend on external-net or IPv6 capability beyond the current application preset"]
fn node22_networking_dgram_host_preset_boundary_batch_watchpoint() {
    run_node_compat_watchpoint_batch(
        "node22-networking-dgram-host-preset-boundary-batch",
        "node22",
        NODE22_NETWORKING_DGRAM_HOST_PRESET_BOUNDARY_FIXTURES,
        &[],
    );
}

#[test]
#[ignore = "Pinned NLC6 dgram watchpoint: test-dgram-reuseport.js now materializes ../common/udp but blocks in reusePort bind/lifecycle semantics, so it stays explicit until that owner seam is fixed"]
fn node22_dgram_reuseport_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-dgram-reuseport.js",
        "node22/test/parallel/test-dgram-reuseport.js",
        NODE22_COMMON_UDP_EXTRA_FILES,
    );
}

#[test]
#[ignore = "Pinned NLC6 cross-family watchpoint: test-http-agent-reuse-drained-socket-only.js currently blocks in process.report.getReport() and then reaches process.exit(), so it stays explicit as a process/report and embedded-exit dependency rather than a pure http.Agent seam"]
fn node22_http_agent_reuse_drained_socket_only_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-http-agent-reuse-drained-socket-only.js",
        "node22/test/parallel/test-http-agent-reuse-drained-socket-only.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned NLC6/NLC7 boundary watchpoint: test-https-agent-additional-options.js currently reaches the legacy TLSv1.1 secureProtocol path (TLSv1_1_method / minVersion TLSv1.1) that the current rustls-backed TLS owner layer does not negotiate"]
fn node22_https_agent_additional_options_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-https-agent-additional-options.js",
        "node22/test/parallel/test-https-agent-additional-options.js",
        COMMON_TLS_KEY_EXTRA_FILES,
    );
}

#[test]
#[ignore = "Pinned later-family dependency: test-stream-writable-samecb-singletick.js asserts async_hooks TickObject allocation counts, which are owned by the broader async_hooks/task-accounting family rather than the current pure-stream contract"]
fn node22_stream_writable_samecb_singletick_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-writable-samecb-singletick.js",
        "node22/test/parallel/test-stream-writable-samecb-singletick.js",
        &[],
    );
}

#[test]
fn node22_stream_finished_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-finished.js",
        "node22/test/parallel/test-stream-finished.js",
        &[],
    );
}

#[test]
fn node22_stream_pipeline_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-pipeline.js",
        "node22/test/parallel/test-stream-pipeline.js",
        &[],
    );
}

#[test]
fn node22_net_local_address_port_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-net-local-address-port.js",
        "node22/test/parallel/test-net-local-address-port.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node24 supported-lane divergence: test-stream-pipeline.js currently returns an AbortError-style 'The operation was aborted' message where the staged Node24 fixture still expects the inner 'Boom!' pipeline error message"]
fn node24_stream_pipeline_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-stream-pipeline.js",
        "node24/test/parallel/test-stream-pipeline.js",
        &[],
    );
}

#[test]
fn node20_readline_interface_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-readline-interface.js",
        "node20/test/parallel/test-readline-interface.js",
        &[],
    );
}

#[test]
fn node22_readline_interface_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-readline-interface.js",
        "node22/test/parallel/test-readline-interface.js",
        &[],
    );
}

#[test]
fn node24_readline_interface_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-readline-interface.js",
        "node24/test/parallel/test-readline-interface.js",
        &[],
    );
}

#[test]
fn node20_readline_promises_interface_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-readline-promises-interface.js",
        "node20/test/parallel/test-readline-promises-interface.js",
        &[],
    );
}

#[test]
fn node22_readline_promises_interface_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-readline-promises-interface.js",
        "node22/test/parallel/test-readline-promises-interface.js",
        &[],
    );
}

#[test]
fn node24_readline_promises_interface_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-readline-promises-interface.js",
        "node24/test/parallel/test-readline-promises-interface.js",
        &[],
    );
}

#[test]
fn node22_process_load_env_file_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-process-load-env-file.js",
        "node22/test/parallel/test-process-load-env-file.js",
        NODE22_PROCESS_LOAD_ENV_FILE_EXTRA_FILES,
    );
}

#[test]
fn node22_fs_glob_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-glob.mjs",
        "node22/test/parallel/test-fs-glob.mjs",
        NODE22_COMMON_INDEX_MJS_EXTRA_FILES,
    );
}

#[test]
fn node24_fs_glob_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-glob.mjs",
        "node24/test/parallel/test-fs-glob.mjs",
        NODE24_COMMON_INDEX_MJS_EXTRA_FILES,
    );
}

#[test]
fn node22_fs_rmdir_recursive_fixture() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-rmdir-recursive.js",
        "node22/test/parallel/test-fs-rmdir-recursive.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node20 divergence: official v20.20.2 test-fs-stat.js still requires the older JSON.stringify(Stats) field shape that the current runtime no longer preserves while matching the newer Node22/Node24 file contract"]
fn node20_fs_stat_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-stat.js",
        "node20/test/parallel/test-fs-stat.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node24 supported-lane divergence: official v24.15.0 test-fs-constants.js expects a newer constant-surface TypeError gate that Nimbus has not adopted into the current Node22 contract"]
fn node24_fs_constants_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-constants.js",
        "node24/test/parallel/test-fs-constants.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node24 supported-lane divergence: official v24.15.0 test-fs-promises-file-handle-dispose.js now also asserts opendir Dir[Symbol.asyncDispose]() close semantics that the current runtime does not yet match"]
fn node24_fs_promises_file_handle_dispose_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-promises-file-handle-dispose.js",
        "node24/test/parallel/test-fs-promises-file-handle-dispose.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node24 supported-lane divergence: official v24.15.0 test-fs-write-stream.js now also requires fs.close() to be observed when destroying WriteStream directly, while the current Node22 contract still follows the older file semantics"]
fn node24_fs_write_stream_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-write-stream.js",
        "node24/test/parallel/test-fs-write-stream.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node24 supported-lane divergence: official v24.15.0 test-fs-write-stream-autoclose-option.js now also asserts ERR_INVALID_THIS when probing WriteStream.prototype.autoClose, while the current Node22 contract still follows the older surface"]
fn node24_fs_write_stream_autoclose_option_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-write-stream-autoclose-option.js",
        "node24/test/parallel/test-fs-write-stream-autoclose-option.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node24 supported-lane divergence: official v24.15.0 test-fs-symlink.js still expects the newer invalid-type ERR_INVALID_ARG_VALUE contract, while the current runtime intentionally keeps the Node22 ERR_FS_INVALID_SYMLINK_TYPE behavior"]
fn node24_fs_symlink_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-symlink.js",
        "node24/test/parallel/test-fs-symlink.js",
        CYCLE_FIXTURES_EXTRA_FILES,
    );
}

#[test]
#[ignore = "Pinned Node24 supported-lane divergence: official v24.15.0 test-fs-opendir.js now also asserts ERR_INVALID_THIS for newer Dir handle receiver checks, while the current runtime intentionally keeps the Node22 directory-handle surface"]
fn node24_fs_opendir_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-opendir.js",
        "node24/test/parallel/test-fs-opendir.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node24 supported-lane divergence: official v24.15.0 test-fs-promises-watch.js adds maxQueue and overflow option validation that Nimbus has not adopted into the current Node22-based fs.watch contract"]
fn node24_fs_promises_watch_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-fs-promises-watch.js",
        "node24/test/parallel/test-fs-promises-watch.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned shared runtime gap: structuredClone transfer currently leaves ArrayBuffer usable in the embedded runtime, so test-buffer-isascii.js does not raise ERR_INVALID_STATE on detached buffers"]
fn node22_buffer_isascii_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-buffer-isascii.js",
        "node20/test/parallel/test-buffer-isascii.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned shared runtime gap: structuredClone transfer currently leaves ArrayBuffer usable in the embedded runtime, so test-buffer-isascii.js does not raise ERR_INVALID_STATE on detached buffers"]
fn node20_buffer_isascii_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-buffer-isascii.js",
        "node20/test/parallel/test-buffer-isascii.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned shared runtime gap: structuredClone transfer currently leaves ArrayBuffer usable in the embedded runtime, so test-buffer-isutf8.js does not raise ERR_INVALID_STATE on detached buffers"]
fn node22_buffer_isutf8_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-buffer-isutf8.js",
        "node20/test/parallel/test-buffer-isutf8.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned shared runtime gap: structuredClone transfer currently leaves ArrayBuffer usable in the embedded runtime, so test-buffer-isutf8.js does not raise ERR_INVALID_STATE on detached buffers"]
fn node20_buffer_isutf8_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-buffer-isutf8.js",
        "node20/test/parallel/test-buffer-isutf8.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node20 divergence: official v20.20.2 test-buffer-slow.js still exercises SlowBuffer(buffer.kMaxLength), and the embedded runtime hits its 128 MB heap ceiling before Node-style range semantics"]
fn node20_buffer_slow_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-buffer-slow.js",
        "node20/test/parallel/test-buffer-slow.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node22-only path gap: official v22.15.0 expects the post-CVE path.win32.normalize() semantics that preserve the test segment in \\\\? and \\\\. device paths"]
fn node22_path_normalize_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-path-normalize.js",
        "node22/test/parallel/test-path-normalize.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned Node22 path gap: official v22.15.0 expects path.win32.toNamespacedPath('\\\\?\\\\foo') to retain the trailing slash, but the current runtime still returns the older Node20 shape"]
fn node22_path_makelong_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-path-makelong.js",
        "node22/test/parallel/test-path-makelong.js",
        &[],
    );
}

#[test]
#[ignore = "Pinned shared path gap: official Node20/Node22 test-path-resolve.js currently fails because win32.resolve rejects drive-letter-less inputs without a CWD"]
fn node22_path_resolve_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-path-resolve.js",
        "node22/test/parallel/test-path-resolve.js",
        PATH_RESOLVE_EXTRA_FILES,
    );
}

#[test]
#[ignore = "Pinned shared path gap: official Node20/Node22 test-path-resolve.js currently fails because win32.resolve rejects drive-letter-less inputs without a CWD"]
fn node20_path_resolve_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-path-resolve.js",
        "node20/test/parallel/test-path-resolve.js",
        PATH_RESOLVE_EXTRA_FILES,
    );
}

#[test]
#[ignore = "Pinned vendored fixture tracks post-22 url.parse deprecation semantics; official Node22 v22.15.0 has no counterpart"]
fn node22_url_parse_deprecation_watchpoint() {
    run_node_compat_watchpoint(
        "test/parallel/test-url-parse-deprecation.js",
        "test/parallel/test-url-parse-deprecation.js",
        URL_PARSE_DEPRECATION_EXTRA_FILES,
    );
}

#[test]
fn node20_supported_lane_executes_official_core_semantics_subset() {
    run_manifested_subset_for_lane(
        "nlc3-core-semantics",
        NodeCompatLane::Node20,
        CORE_SEMANTICS_BATCH,
    );
}

#[test]
fn node22_default_lane_executes_manifested_core_semantics_subset() {
    run_manifested_subset_for_lane(
        "nlc3-core-semantics",
        NodeCompatLane::Node22,
        CORE_SEMANTICS_BATCH,
    );
}

#[test]
#[ignore = "Node24 supported lane watchpoint: the broad core semantics batch includes known newer-console-clear drift and remains classified until that fixture is promoted green"]
fn node24_supported_lane_core_semantics_watchpoint() {
    run_manifested_subset_for_lane(
        "nlc3-core-semantics",
        NodeCompatLane::Node24,
        CORE_SEMANTICS_BATCH,
    );
}

#[test]
fn node20_supported_lane_executes_official_process_and_timing_subset() {
    run_manifested_subset_for_lane(
        "nlc4-process-and-timing",
        NodeCompatLane::Node20,
        PROCESS_AND_TIMING_BATCH,
    );
}

#[test]
fn node22_default_lane_executes_manifested_process_and_timing_subset() {
    run_manifested_subset_for_lane(
        "nlc4-process-and-timing",
        NodeCompatLane::Node22,
        PROCESS_AND_TIMING_BATCH,
    );
}

#[test]
#[ignore = "Node24 supported lane watchpoint: the broad process/timing batch is classified until each carried fixture is replayed and promoted under the supported-lane gate"]
fn node24_supported_lane_process_and_timing_watchpoint() {
    run_manifested_subset_for_lane(
        "nlc4-process-and-timing",
        NodeCompatLane::Node24,
        PROCESS_AND_TIMING_BATCH,
    );
}

#[test]
fn node20_supported_lane_executes_official_streams_and_local_io_subset() {
    run_manifested_subset_for_lane(
        "nlc5-streams-and-local-io",
        NodeCompatLane::Node20,
        STREAMS_AND_LOCAL_IO_BATCH,
    );
}

#[test]
fn node22_default_lane_executes_manifested_streams_and_local_io_subset() {
    run_manifested_subset_for_lane(
        "nlc5-streams-and-local-io",
        NodeCompatLane::Node22,
        STREAMS_AND_LOCAL_IO_BATCH,
    );
}

#[test]
#[ignore = "Node24 supported lane watchpoint: the broad streams/local-I/O batch is classified until each carried fixture is replayed and promoted under the supported-lane gate"]
fn node24_supported_lane_streams_and_local_io_watchpoint() {
    run_manifested_subset_for_lane(
        "nlc5-streams-and-local-io",
        NodeCompatLane::Node24,
        STREAMS_AND_LOCAL_IO_BATCH,
    );
}

#[test]
fn node20_supported_lane_executes_official_networking_subset() {
    run_manifested_subset_for_lane("nlc6-networking", NodeCompatLane::Node20, NETWORKING_BATCH);
}

#[test]
fn node22_default_lane_executes_manifested_networking_subset() {
    run_manifested_subset_for_lane("nlc6-networking", NodeCompatLane::Node22, NETWORKING_BATCH);
}

#[test]
#[ignore = "Node24 supported lane watchpoint: the broad networking batch is classified until each carried fixture is replayed and promoted under the supported-lane gate"]
fn node24_supported_lane_networking_watchpoint() {
    run_manifested_subset_for_lane("nlc6-networking", NodeCompatLane::Node24, NETWORKING_BATCH);
}

#[test]
fn node20_supported_lane_executes_official_loader_context_subset() {
    run_manifested_subset_for_lane(
        "nlc7-loader-context",
        NodeCompatLane::Node20,
        LOADER_CONTEXT_BATCH,
    );
}

#[test]
fn node22_default_lane_executes_manifested_loader_context_subset() {
    run_manifested_subset_for_lane(
        "nlc7-loader-context",
        NodeCompatLane::Node22,
        LOADER_CONTEXT_BATCH,
    );
}

#[test]
#[ignore = "Node24 supported lane watchpoint: the broad loader/context batch is classified until each carried fixture is replayed and promoted under the supported-lane gate"]
fn node24_supported_lane_loader_context_watchpoint() {
    run_manifested_subset_for_lane(
        "nlc7-loader-context",
        NodeCompatLane::Node24,
        LOADER_CONTEXT_BATCH,
    );
}

#[test]
fn node_compat_supplementary_builtin_completeness_node20() {
    run_manifested_subset_for_lane(
        "ncf3-supplementary-builtin-completeness",
        NodeCompatLane::Node20,
        LOADER_CONTEXT_SUPPLEMENTARY_BATCH,
    );
}

#[test]
fn node_compat_supplementary_builtin_completeness_node22() {
    run_manifested_subset_for_lane(
        "ncf3-supplementary-builtin-completeness",
        NodeCompatLane::Node22,
        LOADER_CONTEXT_SUPPLEMENTARY_BATCH,
    );
}

#[test]
fn node_compat_supplementary_builtin_completeness_node24() {
    run_manifested_subset_for_lane(
        "ncf3-supplementary-builtin-completeness",
        NodeCompatLane::Node24,
        LOADER_CONTEXT_SUPPLEMENTARY_BATCH,
    );
}

#[test]
fn node_compat_supplementary_module_bridge_node20() {
    run_manifested_subset_for_lane(
        "ncf3-supplementary-module-bridge",
        NodeCompatLane::Node20,
        LOADER_CONTEXT_SUPPLEMENTARY_MODULE_BRIDGE_BATCH,
    );
}

#[test]
fn node_compat_supplementary_module_bridge_node22() {
    run_manifested_subset_for_lane(
        "ncf3-supplementary-module-bridge",
        NodeCompatLane::Node22,
        LOADER_CONTEXT_SUPPLEMENTARY_MODULE_BRIDGE_BATCH,
    );
}

#[test]
fn node_compat_supplementary_module_bridge_node24() {
    run_manifested_subset_for_lane(
        "ncf3-supplementary-module-bridge",
        NodeCompatLane::Node24,
        LOADER_CONTEXT_SUPPLEMENTARY_MODULE_BRIDGE_BATCH,
    );
}

#[test]
fn node_compat_supplementary_global_injection_node20() {
    run_manifested_subset_for_lane(
        "ncf3-supplementary-global-injection",
        NodeCompatLane::Node20,
        LOADER_CONTEXT_SUPPLEMENTARY_GLOBAL_INJECTION_BATCH,
    );
}

#[test]
fn node_compat_supplementary_global_injection_node22() {
    run_manifested_subset_for_lane(
        "ncf3-supplementary-global-injection",
        NodeCompatLane::Node22,
        LOADER_CONTEXT_SUPPLEMENTARY_GLOBAL_INJECTION_BATCH,
    );
}

#[test]
fn node_compat_supplementary_global_injection_node24() {
    run_manifested_subset_for_lane(
        "ncf3-supplementary-global-injection",
        NodeCompatLane::Node24,
        LOADER_CONTEXT_SUPPLEMENTARY_GLOBAL_INJECTION_BATCH,
    );
}

#[test]
fn node_compat_supplementary_process_shape_node20() {
    let outcome =
        observe_seeded_fixture_runtime_outcome("node20", "supplementary/process-release-shape.js")
            .expect("supplementary process release shape node20 outcome should resolve");
    assert_eq!(
        outcome.state,
        node_compat_manifest_report::NodeCompatObservedFixtureState::Fail
    );
    let detail = outcome
        .detail
        .expect("node20 supplementary process shape failure should record detail");
    assert!(
        detail.contains("v22.0.0-nimbus") && detail.contains("/^v20\\./"),
        "node20 supplementary process shape should record the cross-lane version drift: {detail}",
    );
}

#[test]
fn node_compat_supplementary_process_shape_node22() {
    let outcome =
        observe_seeded_fixture_runtime_outcome("node22", "supplementary/process-release-shape.js")
            .expect("supplementary process release shape node22 outcome should resolve");
    assert_eq!(
        outcome.state,
        node_compat_manifest_report::NodeCompatObservedFixtureState::Fail
    );
    let detail = outcome
        .detail
        .expect("node22 supplementary process shape failure should record detail");
    assert!(
        detail.contains("undefined !== 'Jod'"),
        "node22 supplementary process shape should record the missing LTS label: {detail}",
    );
}

#[test]
fn node_compat_supplementary_process_shape_node24() {
    let outcome =
        observe_seeded_fixture_runtime_outcome("node24", "supplementary/process-release-shape.js")
            .expect("supplementary process release shape node24 outcome should resolve");
    assert_eq!(
        outcome.state,
        node_compat_manifest_report::NodeCompatObservedFixtureState::Fail
    );
    let detail = outcome
        .detail
        .expect("node24 supplementary process shape failure should record detail");
    assert!(
        detail.contains("v22.0.0-nimbus") && detail.contains("/^v24\\./"),
        "node24 supplementary process shape should record the supported-lane version drift: {detail}",
    );
}

#[test]
fn node_compat_supplementary_runtime_node20() {
    run_manifested_subset_for_lane(
        "ncf3-supplementary-runtime",
        NodeCompatLane::Node20,
        RUNTIME_SUPPLEMENTARY_BATCH,
    );
}

#[test]
fn node_compat_supplementary_runtime_node22() {
    run_manifested_subset_for_lane(
        "ncf3-supplementary-runtime",
        NodeCompatLane::Node22,
        RUNTIME_SUPPLEMENTARY_BATCH,
    );
}

#[test]
fn node_compat_supplementary_runtime_node24() {
    run_manifested_subset_for_lane(
        "ncf3-supplementary-runtime",
        NodeCompatLane::Node24,
        RUNTIME_SUPPLEMENTARY_BATCH,
    );
}

fn assert_signal_lifecycle_watchpoint(lane: &str) {
    let outcome =
        observe_seeded_fixture_runtime_outcome(lane, "supplementary/signal-listener-lifecycle.mjs")
            .expect("supplementary signal lifecycle outcome should resolve");
    assert_eq!(
        outcome.state,
        node_compat_manifest_report::NodeCompatObservedFixtureState::Fail
    );
    let detail = outcome
        .detail
        .expect("supplementary signal lifecycle failure should record detail");
    assert!(
        detail.contains("Deno.addSignalListener is not a function"),
        "signal lifecycle watchpoint should record missing Deno.addSignalListener: {detail}",
    );
}

#[test]
fn node_compat_supplementary_signal_lifecycle_watchpoint_node20() {
    assert_signal_lifecycle_watchpoint("node20");
}

#[test]
fn node_compat_supplementary_signal_lifecycle_watchpoint_node22() {
    assert_signal_lifecycle_watchpoint("node22");
}

#[test]
fn node_compat_supplementary_signal_lifecycle_watchpoint_node24() {
    assert_signal_lifecycle_watchpoint("node24");
}
