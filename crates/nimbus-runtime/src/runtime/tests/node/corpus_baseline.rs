// Recorded expectations for the upstream Node compatibility corpus.
//
// The corpus is aspirational. It vendors the whole upstream Node test suite for
// four lanes, and large parts of it exercise APIs that the runtime does not
// implement yet. A lane that requires every fixture to pass can therefore never
// go green, and a real regression cannot be told apart from the permanent
// background failure. Measured on 2026-09-16, run 35095026629 had 294 failing
// tests and zero successful runs in all retained history.
//
// This seam records which fixtures are known to fail, and compares each
// observed result with that record:
//
//   recorded failure, observed failure -> known gap, the lane stays green
//   recorded failure, observed pass    -> failure, the record must shrink
//   no record,        observed failure -> failure, a real regression
//
// The record only shrinks through a reviewed change, so progress stays visible
// and cannot reverse without a reviewer seeing it. The record never states a
// wish: an entry is only valid because a real run produced it.
//
// Contract and refresh procedure: docs/private/operating/node-compat-nightly.md.

/// Repository-relative location of the recorded baseline.
const NODE_COMPAT_CORPUS_BASELINE_PATH: &str = "tests/runtime/node/expectations/corpus-baseline.json";

/// Names a file that receives one JSON object per fixture attempt.
///
/// nextest runs each test in its own process, so an in-memory buffer would not
/// survive. Each process appends instead, and the aggregation step merges the
/// partitions.
const NODE_COMPAT_OBSERVED_RESULTS_ENV: &str = "NIMBUS_NODE_COMPAT_OBSERVED_RESULTS";

/// Baseline key for a fixture that runs without a declared lane.
const NODE_COMPAT_LANELESS_BASELINE_KEY: &str = "unspecified";

#[derive(Debug, serde::Deserialize)]
struct NodeCompatCorpusBaselineDocument {
    #[serde(default)]
    lanes: std::collections::BTreeMap<String, Vec<NodeCompatCorpusBaselineEntry>>,
}

#[derive(Debug, serde::Deserialize)]
struct NodeCompatCorpusBaselineEntry {
    test_relative_path: String,
    reason: String,
}

#[derive(Debug, Default)]
struct NodeCompatCorpusBaseline {
    /// lane key -> fixture path -> reason
    lanes: std::collections::HashMap<String, std::collections::HashMap<String, String>>,
}

impl NodeCompatCorpusBaseline {
    fn recorded_gap_reason(&self, lane_key: &str, test_relative_path: &str) -> Option<&str> {
        self.lanes
            .get(lane_key)
            .and_then(|fixtures| fixtures.get(test_relative_path))
            .map(String::as_str)
    }

    fn entry_count(&self) -> usize {
        self.lanes.values().map(std::collections::HashMap::len).sum()
    }
}

fn node_compat_corpus_baseline() -> &'static NodeCompatCorpusBaseline {
    static BASELINE: std::sync::OnceLock<NodeCompatCorpusBaseline> = std::sync::OnceLock::new();
    BASELINE.get_or_init(|| {
        let path = node_compat_repo_root().join(NODE_COMPAT_CORPUS_BASELINE_PATH);
        let raw = std::fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!(
                "the node_compat corpus baseline at {} should be readable: {error}",
                path.display()
            )
        });
        let document: NodeCompatCorpusBaselineDocument = serde_json::from_str(&raw)
            .unwrap_or_else(|error| {
                panic!(
                    "the node_compat corpus baseline at {} should parse: {error}",
                    path.display()
                )
            });
        let mut baseline = NodeCompatCorpusBaseline::default();
        for (lane_key, entries) in document.lanes {
            let fixtures = baseline.lanes.entry(lane_key.clone()).or_default();
            for entry in entries {
                if let Some(previous) =
                    fixtures.insert(entry.test_relative_path.clone(), entry.reason)
                {
                    panic!(
                        "the node_compat corpus baseline records `{}` twice for lane `{lane_key}`: {previous}",
                        entry.test_relative_path
                    );
                }
            }
        }
        baseline
    })
}

fn node_compat_baseline_lane_key(lane: Option<NodeCompatLane>) -> &'static str {
    lane.map_or(NODE_COMPAT_LANELESS_BASELINE_KEY, node_compat_lane_name)
}

/// The full module path of the running Rust test, as libtest names the thread.
fn node_compat_current_test_path() -> String {
    std::thread::current()
        .name()
        .unwrap_or("unknown")
        .to_string()
}

/// The bare function name of the running Rust test.
///
/// `tests/runtime/node/expectations/rust-watchpoints.json` keys its entries on
/// this form, so an observed result must use it too. The full module path would
/// never match, and `detect_unexpected_passes` would silently find nothing.
fn node_compat_current_test_name() -> String {
    let path = node_compat_current_test_path();
    path.rsplit("::").next().unwrap_or(&path).to_string()
}

/// Resolve the observed-results path that `NIMBUS_NODE_COMPAT_OBSERVED_RESULTS`
/// names.
///
/// Cargo and nextest run a test binary with its working directory set to the
/// package root, so a relative path resolves under `crates/nimbus-runtime`
/// rather than the repository root. A caller that sets
/// `target/node-compat/observed/partition-0.jsonl` means the workspace
/// `target/`, which is also where the CI upload step looks, so an unresolved
/// relative path writes the shard where nothing collects it.
///
/// That failure is silent by construction: the tests still run, the file is
/// still created, and only the artifact is empty. Anchoring a relative path to
/// the repository root keeps the writer and its reader on the same file.
fn node_compat_observed_results_path(configured: &Path) -> PathBuf {
    if configured.is_absolute() {
        return configured.to_path_buf();
    }
    node_compat_repo_root().join(configured)
}

fn record_node_compat_observed_result(
    lane_key: &str,
    test_relative_path: &str,
    decision: &NodeCompatReconciliation,
) {
    let Some(path) = std::env::var_os(NODE_COMPAT_OBSERVED_RESULTS_ENV) else {
        return;
    };
    let path = node_compat_observed_results_path(Path::new(&path));
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut record = serde_json::json!({
        "lane": lane_key,
        "test_relative_path": test_relative_path,
        "test_name": node_compat_current_test_name(),
        "rust_test_path": node_compat_current_test_path(),
        "outcome": decision.outcome_label(),
    });
    if let Some(detail) = decision.detail() {
        // Keep the line bounded. The full text is already in the failure output
        // and in the diagnostic artifact.
        let trimmed: String = detail.chars().take(500).collect();
        record["detail"] = serde_json::Value::String(trimmed);
    }
    let mut line = record.to_string();
    line.push('\n');

    use std::io::Write as _;
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(mut file) => {
            if let Err(error) = file.write_all(line.as_bytes()) {
                eprintln!(
                    "node_compat could not append an observed result to {}: {error}",
                    path.display()
                );
            }
        }
        Err(error) => eprintln!(
            "node_compat could not open the observed-results file {}: {error}",
            path.display()
        ),
    }
}

/// Compares one observed fixture result with the recorded baseline.
///
/// This is the only place that decides whether a fixture result fails its Rust
/// test. Every corpus execution passes through
/// `execute_upstream_node_compat_test_with_extra_files`, which calls this.
fn reconcile_node_compat_fixture_result(
    lane: Option<NodeCompatLane>,
    test_relative_path: &str,
    result: std::result::Result<NodeCompatFixtureOutcome, String>,
) -> std::result::Result<NodeCompatFixtureOutcome, String> {
    let lane_key = node_compat_baseline_lane_key(lane);
    let recorded_reason = node_compat_corpus_baseline().recorded_gap_reason(lane_key, test_relative_path);
    let decision = decide_node_compat_fixture_result(lane_key, test_relative_path, recorded_reason, result);
    record_node_compat_observed_result(lane_key, test_relative_path, &decision);
    decision.into_result()
}

/// What the baseline says about one observed result.
#[derive(Debug)]
enum NodeCompatReconciliation {
    /// Not recorded, and it passed or skipped.
    Clean(NodeCompatFixtureOutcome),
    /// Recorded, and it still fails. The lane stays green.
    KnownGap { detail: String },
    /// Recorded, but it passed. The record must shrink before the lane is green.
    UnexpectedPass { message: String },
    /// Not recorded, and it failed. This is the regression signal.
    Regression { error: String },
}

impl NodeCompatReconciliation {
    fn outcome_label(&self) -> &'static str {
        match self {
            Self::Clean(outcome) if outcome.skipped => "skipped",
            Self::Clean(_) => "passed",
            Self::KnownGap { .. } => "known_gap",
            Self::UnexpectedPass { .. } => "unexpected_pass",
            Self::Regression { .. } => "failed",
        }
    }

    fn detail(&self) -> Option<&str> {
        match self {
            Self::Clean(_) => None,
            Self::KnownGap { detail } => Some(detail),
            Self::UnexpectedPass { message } => Some(message),
            Self::Regression { error } => Some(error),
        }
    }

    fn into_result(self) -> std::result::Result<NodeCompatFixtureOutcome, String> {
        match self {
            Self::Clean(outcome) => Ok(outcome),
            Self::KnownGap { .. } => Ok(NodeCompatFixtureOutcome {
                skipped: false,
                known_gap: true,
            }),
            Self::UnexpectedPass { message } => Err(message),
            Self::Regression { error } => Err(error),
        }
    }
}

/// The reconciliation policy, with no file access and no I/O.
fn decide_node_compat_fixture_result(
    lane_key: &str,
    test_relative_path: &str,
    recorded_reason: Option<&str>,
    result: std::result::Result<NodeCompatFixtureOutcome, String>,
) -> NodeCompatReconciliation {
    match (recorded_reason, result) {
        // A recorded gap that still fails. The lane stays green, and the batch
        // summary reports it, so the gap stays visible.
        (Some(reason), Err(error)) => {
            eprintln!("node_compat known gap {lane_key} {test_relative_path}: {reason}");
            NodeCompatReconciliation::KnownGap { detail: error }
        }
        // A recorded gap that now passes. This is good news, and it must be
        // recorded before the lane goes green again, or the baseline drifts
        // back into fiction.
        (Some(reason), Ok(_)) => NodeCompatReconciliation::UnexpectedPass {
            message: format!(
                "upstream node_compat fixture `{test_relative_path}` is recorded as a known gap \
                 for lane `{lane_key}` but it passed. Remove the entry from \
                 {NODE_COMPAT_CORPUS_BASELINE_PATH} so the improvement is recorded. \
                 Recorded reason: {reason}"
            ),
        },
        // No record and a failure. This is the regression signal that the lane
        // exists to give.
        (None, Err(error)) => NodeCompatReconciliation::Regression { error },
        (None, Ok(outcome)) => NodeCompatReconciliation::Clean(outcome),
    }
}

#[cfg(test)]
fn node_compat_passing_outcome() -> NodeCompatFixtureOutcome {
    NodeCompatFixtureOutcome {
        skipped: false,
        known_gap: false,
    }
}

#[test]
fn node_compat_corpus_baseline_parses_and_reports_its_size() {
    let baseline = node_compat_corpus_baseline();
    eprintln!(
        "node_compat corpus baseline: {} lanes, {} recorded gaps",
        baseline.lanes.len(),
        baseline.entry_count()
    );
    for lane_key in baseline.lanes.keys() {
        assert!(
            lane_key == NODE_COMPAT_LANELESS_BASELINE_KEY
                || node_compat_lane_from_manifest_name(lane_key).is_ok(),
            "baseline lane key `{lane_key}` is not a known lane"
        );
    }
}

#[test]
fn node_compat_corpus_baseline_entries_name_a_vendored_fixture() {
    let fixture_root = node_compat_fixture_root();
    let mut missing = Vec::new();
    for (lane_key, fixtures) in &node_compat_corpus_baseline().lanes {
        if lane_key == NODE_COMPAT_LANELESS_BASELINE_KEY {
            continue;
        }
        for test_relative_path in fixtures.keys() {
            let fixture = fixture_root.join(lane_key).join(test_relative_path);
            if !fixture.exists() {
                missing.push(format!("{lane_key}/{test_relative_path}"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "the corpus baseline records {} fixtures that are not vendored:\n{}",
        missing.len(),
        missing.join("\n")
    );
}

#[test]
fn node_compat_unrecorded_failure_stays_a_regression() {
    let decision = decide_node_compat_fixture_result(
        "node20",
        "test/parallel/test-example.js",
        None,
        Err("boom".to_string()),
    );
    assert_eq!(decision.outcome_label(), "failed");
    assert_eq!(decision.into_result().err().as_deref(), Some("boom"));
}

#[test]
fn node_compat_unrecorded_pass_stays_a_pass() {
    let decision = decide_node_compat_fixture_result(
        "node20",
        "test/parallel/test-example.js",
        None,
        Ok(node_compat_passing_outcome()),
    );
    assert_eq!(decision.outcome_label(), "passed");
    let outcome = decision.into_result().expect("an unrecorded pass stays green");
    assert!(!outcome.known_gap);
    assert!(!outcome.skipped);
}

#[test]
fn node_compat_recorded_failure_keeps_the_lane_green() {
    let decision = decide_node_compat_fixture_result(
        "node22",
        "test/parallel/test-example.js",
        Some("node:vm compileFunction is not implemented"),
        Err("runtime JavaScript error: TypeError".to_string()),
    );
    assert_eq!(decision.outcome_label(), "known_gap");
    let outcome = decision
        .into_result()
        .expect("a recorded gap must not fail the lane");
    assert!(outcome.known_gap, "the outcome must report itself as a gap");
    assert!(!outcome.skipped, "a gap is not a skip");
}

#[test]
fn node_compat_recorded_fixture_that_passes_fails_the_lane() {
    let decision = decide_node_compat_fixture_result(
        "node24",
        "test/parallel/test-example.js",
        Some("node:vm compileFunction is not implemented"),
        Ok(node_compat_passing_outcome()),
    );
    assert_eq!(decision.outcome_label(), "unexpected_pass");
    let error = decision
        .into_result()
        .expect_err("an unexpected pass must fail the lane");
    assert!(
        error.contains("but it passed"),
        "the message must name the unexpected pass: {error}"
    );
    assert!(
        error.contains(NODE_COMPAT_CORPUS_BASELINE_PATH),
        "the message must tell the reader which file to edit: {error}"
    );
}

#[test]
fn node_compat_observed_results_append_one_json_line_per_fixture() {
    let _suite = acquire_runtime_suite_lock_blocking();
    let tempdir = tempfile::tempdir().expect("a temp dir should be available");
    let path = tempdir.path().join("nested/observed.jsonl");
    // SAFETY: the runtime suite serializes env mutation through this guard.
    let _guard = ScopedProcessEnvVar::set(
        NODE_COMPAT_OBSERVED_RESULTS_ENV,
        path.to_str().expect("a UTF-8 temp path"),
    );

    for (lane_key, fixture, decision) in [
        (
            "node20",
            "test/parallel/test-one.js",
            decide_node_compat_fixture_result(
                "node20",
                "test/parallel/test-one.js",
                None,
                Ok(node_compat_passing_outcome()),
            ),
        ),
        (
            "node22",
            "test/parallel/test-two.js",
            decide_node_compat_fixture_result(
                "node22",
                "test/parallel/test-two.js",
                Some("recorded"),
                Err("still failing".to_string()),
            ),
        ),
    ] {
        record_node_compat_observed_result(lane_key, fixture, &decision);
    }

    let raw = std::fs::read_to_string(&path).expect("the observed-results file should exist");
    let lines: Vec<&str> = raw.lines().collect();
    assert_eq!(lines.len(), 2, "one line per fixture attempt: {raw}");

    let first: serde_json::Value = serde_json::from_str(lines[0]).expect("line 1 is JSON");
    assert_eq!(first["lane"], "node20");
    assert_eq!(first["test_relative_path"], "test/parallel/test-one.js");
    assert_eq!(first["outcome"], "passed");
    let test_name = first["test_name"].as_str().expect("the Rust test name is recorded");
    assert!(
        !test_name.contains("::"),
        "the watchpoint catalog keys on the bare function name, not the module path: {test_name}"
    );
    assert!(
        first["rust_test_path"]
            .as_str()
            .is_some_and(|path| path.ends_with(test_name)),
        "the full module path is recorded beside the bare name"
    );

    let second: serde_json::Value = serde_json::from_str(lines[1]).expect("line 2 is JSON");
    assert_eq!(second["lane"], "node22");
    assert_eq!(second["outcome"], "known_gap");
    assert_eq!(second["detail"], "still failing");
}

#[test]
fn node_compat_observed_results_stay_silent_without_the_env_var() {
    let _suite = acquire_runtime_suite_lock_blocking();
    // SAFETY: the runtime suite serializes env mutation through this guard.
    let _guard = ScopedProcessEnvVar::unset(NODE_COMPAT_OBSERVED_RESULTS_ENV);
    let decision = decide_node_compat_fixture_result(
        "node20",
        "test/parallel/test-one.js",
        None,
        Ok(node_compat_passing_outcome()),
    );
    // The call must not panic and must not create a file anywhere.
    record_node_compat_observed_result("node20", "test/parallel/test-one.js", &decision);
}

/// The CI upload step collects `target/node-compat/observed` from the
/// repository root. A relative shard path must therefore land there, and not
/// under the crate directory that Cargo makes the working directory.
#[test]
fn node_compat_relative_observed_results_anchor_to_the_repo_root() {
    let resolved = node_compat_observed_results_path(Path::new(
        "target/node-compat/observed/partition-0.jsonl",
    ));
    assert_eq!(
        resolved,
        node_compat_repo_root().join("target/node-compat/observed/partition-0.jsonl"),
        "a relative shard path must resolve against the repo root"
    );
    assert!(
        !resolved.starts_with(runtime_crate_root().join("target")),
        "a relative shard path must not land under the crate target directory"
    );
}

/// An absolute path is already unambiguous, so it is used as given.
#[test]
fn node_compat_absolute_observed_results_are_left_alone() {
    let absolute = std::env::temp_dir().join("nimbus-node-compat-observed.jsonl");
    assert_eq!(
        node_compat_observed_results_path(&absolute),
        absolute,
        "an absolute shard path must be used verbatim"
    );
}
