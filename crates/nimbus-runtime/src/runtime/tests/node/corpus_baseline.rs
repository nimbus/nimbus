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
    /// Where the fixture is vendored, relative to the fixture root.
    ///
    /// The runtime path inside the bundle is not where the fixture lives on
    /// disk. A lane vendors some fixtures under its own directory, the shared
    /// tree vendors others once at the root, and a regression fixture can carry
    /// a different file name. Guessing from `test_relative_path` gets all three
    /// wrong, so the seam records the path it actually read.
    ///
    /// The field is required. A fixture with no vendored source is a synthetic
    /// probe of Nimbus behavior rather than a measurement of upstream
    /// compatibility, and the baseline may not absorb one.
    fixture_source_relative_path: String,
}

/// One recorded gap: why the fixture fails, and where it is vendored.
#[derive(Debug)]
struct NodeCompatRecordedGap {
    reason: String,
    fixture_source_relative_path: String,
}

/// Which fixture ran, and where its source came from.
///
/// The two paths always travel together, and neither one derives from the
/// other, so the pair is named once. A call site that knows the runtime path
/// cannot then forget to say where the source came from.
#[derive(Debug, Clone, Copy)]
struct NodeCompatFixtureIdentity<'a> {
    /// The path the fixture takes inside the runtime bundle.
    test_relative_path: &'a str,
    /// Where the fixture is vendored, relative to the fixture root, or `None`
    /// when the test supplies its own source.
    vendored_source: Option<&'a str>,
}

impl<'a> NodeCompatFixtureIdentity<'a> {
    /// A fixture read from the vendored corpus. Only this kind is recordable.
    fn vendored(test_relative_path: &'a str, vendored_source: &'a str) -> Self {
        Self {
            test_relative_path,
            vendored_source: Some(vendored_source),
        }
    }

    /// A test that supplies its own source. It probes Nimbus behavior rather
    /// than upstream compatibility, so the baseline may never absorb it.
    fn synthetic(test_relative_path: &'a str) -> Self {
        Self {
            test_relative_path,
            vendored_source: None,
        }
    }
}

#[derive(Debug, Default)]
struct NodeCompatCorpusBaseline {
    /// lane key -> fixture path -> recorded gap
    lanes: std::collections::HashMap<
        String,
        std::collections::HashMap<String, NodeCompatRecordedGap>,
    >,
}

impl NodeCompatCorpusBaseline {
    fn recorded_gap_reason(&self, lane_key: &str, test_relative_path: &str) -> Option<&str> {
        self.lanes
            .get(lane_key)
            .and_then(|fixtures| fixtures.get(test_relative_path))
            .map(|gap| gap.reason.as_str())
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
                let gap = NodeCompatRecordedGap {
                    reason: entry.reason,
                    fixture_source_relative_path: entry.fixture_source_relative_path,
                };
                if let Some(previous) = fixtures.insert(entry.test_relative_path.clone(), gap) {
                    panic!(
                        "the node_compat corpus baseline records `{}` twice for lane `{lane_key}`: {}",
                        entry.test_relative_path, previous.reason
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

thread_local! {
    /// The batch that is running fixtures on this thread, if any.
    static NODE_COMPAT_ACTIVE_BATCH: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

/// Marks every fixture result on this thread as part of one batch.
///
/// A batch runs hundreds of fixtures inside a single Rust test. nextest kills a
/// test that outruns its timeout, and the fixtures the batch already recorded
/// still reach the artifact, so a killed batch shrinks the measurement without
/// failing it. The next run then reaches further into the same batch and
/// reports the fixtures behind the old kill point as fresh regressions.
///
/// The mark pairs those records with the completion record that the batch
/// writes when its loop ends. A batch that was killed wrote no completion
/// record, and the aggregator refuses the measurement instead of trimming it.
struct NodeCompatBatchScope {
    key: String,
}

impl NodeCompatBatchScope {
    /// Opens a batch and records that it started.
    ///
    /// The key names the Rust test as well as the batch and the lane, because
    /// several tests run the same batch for different lanes and the aggregator
    /// pairs a start with exactly one end. The sequence number separates two
    /// batches that one test opens in turn.
    fn start(batch_name: &str, lane_name: &str) -> Self {
        static NEXT_SEQUENCE: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(1);
        let key = format!(
            "{}::{batch_name}/{lane_name}#{}",
            node_compat_current_test_path(),
            NEXT_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        NODE_COMPAT_ACTIVE_BATCH.with(|active| {
            let mut active = active.borrow_mut();
            // One batch at a time. A nested batch would take the mark from the
            // outer one, and the fixtures after the inner batch would then be
            // recorded as belonging to no batch at all.
            assert!(
                active.is_none(),
                "node_compat batch `{key}` started inside batch `{}`",
                active.as_deref().unwrap_or_default()
            );
            *active = Some(key.clone());
        });
        record_node_compat_batch_start(&key);
        Self { key }
    }

    /// Records that the fixture loop ran to its end.
    ///
    /// This is a method rather than a drop, because a drop also runs on an
    /// early exit and the record must mean that the loop did not take one.
    fn finish(self, fixture_count: usize) {
        record_node_compat_batch_completion(&self.key, fixture_count);
    }
}

impl Drop for NodeCompatBatchScope {
    fn drop(&mut self) {
        NODE_COMPAT_ACTIVE_BATCH.with(|active| *active.borrow_mut() = None);
    }
}

fn node_compat_active_batch() -> Option<String> {
    NODE_COMPAT_ACTIVE_BATCH.with(|active| active.borrow().clone())
}

/// Appends one record to the observed-results shard.
///
/// The env var names the shard. Without it, nothing is recorded, which is what
/// a plain local `cargo test` does.
fn append_node_compat_observed_record(record: &serde_json::Value) {
    let Some(path) = std::env::var_os(NODE_COMPAT_OBSERVED_RESULTS_ENV) else {
        return;
    };
    let path = node_compat_observed_results_path(Path::new(&path));
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        let _ = std::fs::create_dir_all(parent);
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

/// Records that one batch started.
///
/// A batch that the test runner kills before its first fixture writes nothing
/// else, so without this record the batch leaves no trace at all, and the
/// aggregator cannot tell a killed batch from a batch that does not exist.
fn record_node_compat_batch_start(batch_key: &str) {
    append_node_compat_observed_record(&serde_json::json!({
        "kind": "batch_start",
        "batch": batch_key,
        "test_name": node_compat_current_test_name(),
        "rust_test_path": node_compat_current_test_path(),
    }));
}

/// Records that one batch ran its fixture list to the end.
///
/// The count is what the aggregator compares with the fixture records that
/// carry the same batch key, so a batch that stopped early fails the
/// measurement instead of shrinking it.
fn record_node_compat_batch_completion(batch_key: &str, fixture_count: usize) {
    append_node_compat_observed_record(&serde_json::json!({
        "kind": "batch_complete",
        "batch": batch_key,
        "fixture_count": fixture_count,
        "test_name": node_compat_current_test_name(),
        "rust_test_path": node_compat_current_test_path(),
    }));
}

fn record_node_compat_observed_result(
    lane_key: &str,
    fixture: NodeCompatFixtureIdentity<'_>,
    decision: &NodeCompatReconciliation,
) {
    let mut record = serde_json::json!({
        "lane": lane_key,
        "test_relative_path": fixture.test_relative_path,
        "test_name": node_compat_current_test_name(),
        "rust_test_path": node_compat_current_test_path(),
        "outcome": decision.outcome_label(),
    });
    if let Some(source) = fixture.vendored_source {
        // Only a fixture read from the vendored tree carries this. A test that
        // supplies its own inline source is a Nimbus probe, and leaving the
        // field out is what stops the baseline from absorbing it.
        record["fixture_source_relative_path"] = serde_json::Value::String(source.to_string());
    }
    if let Some(detail) = decision.detail() {
        // Keep the line bounded. The full text is already in the failure output
        // and in the diagnostic artifact.
        let trimmed: String = detail.chars().take(500).collect();
        record["detail"] = serde_json::Value::String(trimmed);
    }
    if let Some(batch_key) = node_compat_active_batch() {
        // Only a fixture that a batch executed carries this. The aggregator
        // pairs it with the batch completion record.
        record["batch"] = serde_json::Value::String(batch_key);
    }
    append_node_compat_observed_record(&record);
}

/// Compares one observed fixture result with the recorded baseline.
///
/// This is the only place that decides whether a fixture result fails its Rust
/// test. Every corpus execution passes through
/// `execute_upstream_node_compat_test_with_extra_files`, which calls this.
fn reconcile_node_compat_fixture_result(
    lane: Option<NodeCompatLane>,
    fixture: NodeCompatFixtureIdentity<'_>,
    result: std::result::Result<NodeCompatFixtureOutcome, String>,
) -> std::result::Result<NodeCompatFixtureOutcome, String> {
    let lane_key = node_compat_baseline_lane_key(lane);
    let recorded_reason =
        node_compat_corpus_baseline().recorded_gap_reason(lane_key, fixture.test_relative_path);
    let decision = decide_node_compat_fixture_result(
        lane_key,
        fixture.test_relative_path,
        recorded_reason,
        result,
    );
    record_node_compat_observed_result(lane_key, fixture, &decision);
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
            Self::KnownGap { detail } => Ok(NodeCompatFixtureOutcome {
                skipped: false,
                known_gap_detail: Some(detail),
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
        known_gap_detail: None,
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
        for (test_relative_path, gap) in fixtures {
            if gap.fixture_source_relative_path.trim().is_empty() {
                missing.push(format!(
                    "{lane_key}/{test_relative_path}: names no vendored source"
                ));
                continue;
            }
            if !fixture_root.join(&gap.fixture_source_relative_path).exists() {
                missing.push(format!(
                    "{lane_key}/{test_relative_path}: {} is not vendored",
                    gap.fixture_source_relative_path
                ));
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
    assert!(!outcome.is_known_gap());
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
    assert_eq!(
        outcome.known_gap_detail.as_deref(),
        Some("runtime JavaScript error: TypeError"),
        "a recorded gap keeps the reason the run observed, not a placeholder"
    );
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
            NodeCompatFixtureIdentity::vendored(
                "test/parallel/test-one.js",
                "node20/test/parallel/test-one.js",
            ),
            decide_node_compat_fixture_result(
                "node20",
                "test/parallel/test-one.js",
                None,
                Ok(node_compat_passing_outcome()),
            ),
        ),
        (
            "node22",
            NodeCompatFixtureIdentity::vendored(
                "test/parallel/test-two.js",
                "node22/test/parallel/test-two.js",
            ),
            decide_node_compat_fixture_result(
                "node22",
                "test/parallel/test-two.js",
                Some("recorded"),
                Err("still failing".to_string()),
            ),
        ),
        (
            "node24",
            NodeCompatFixtureIdentity::synthetic("test/parallel/__nimbus-probe.js"),
            decide_node_compat_fixture_result(
                "node24",
                "test/parallel/__nimbus-probe.js",
                None,
                Err("probe failed".to_string()),
            ),
        ),
    ] {
        record_node_compat_observed_result(lane_key, fixture, &decision);
    }

    let raw = std::fs::read_to_string(&path).expect("the observed-results file should exist");
    let lines: Vec<&str> = raw.lines().collect();
    assert_eq!(lines.len(), 3, "one line per fixture attempt: {raw}");

    let first: serde_json::Value = serde_json::from_str(lines[0]).expect("line 1 is JSON");
    assert_eq!(first["lane"], "node20");
    assert_eq!(first["test_relative_path"], "test/parallel/test-one.js");
    assert_eq!(first["outcome"], "passed");
    assert!(
        first.get("batch").is_none(),
        "a fixture outside a batch carries no batch key: {first}"
    );
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
    assert_eq!(
        second["fixture_source_relative_path"],
        "node22/test/parallel/test-two.js",
        "the record must name where the fixture is vendored"
    );

    // A synthetic probe names no vendored source. The absent field is what
    // keeps `refresh` from recording it, so the writer must leave it out
    // rather than invent a path.
    let third: serde_json::Value = serde_json::from_str(lines[2]).expect("line 3 is JSON");
    assert_eq!(third["outcome"], "failed");
    assert!(
        third.get("fixture_source_relative_path").is_none(),
        "a test that supplies its own source must record no vendored path: {third}"
    );
}

#[test]
fn node_compat_batch_records_bracket_the_fixtures_they_measure() {
    let _suite = acquire_runtime_suite_lock_blocking();
    let tempdir = tempfile::tempdir().expect("a temp dir should be available");
    let path = tempdir.path().join("observed.jsonl");
    // SAFETY: the runtime suite serializes env mutation through this guard.
    let _guard = ScopedProcessEnvVar::set(
        NODE_COMPAT_OBSERVED_RESULTS_ENV,
        path.to_str().expect("a UTF-8 temp path"),
    );

    {
        let scope = NodeCompatBatchScope::start("streams", "node20");
        record_node_compat_observed_result(
            "node20",
            NodeCompatFixtureIdentity::vendored(
                "test/parallel/test-one.js",
                "node20/test/parallel/test-one.js",
            ),
            &decide_node_compat_fixture_result(
                "node20",
                "test/parallel/test-one.js",
                None,
                Ok(node_compat_passing_outcome()),
            ),
        );
        scope.finish(1);
    }

    // The scope ended, so a later fixture belongs to no batch.
    record_node_compat_observed_result(
        "node20",
        NodeCompatFixtureIdentity::vendored(
            "test/parallel/test-two.js",
            "node20/test/parallel/test-two.js",
        ),
        &decide_node_compat_fixture_result(
            "node20",
            "test/parallel/test-two.js",
            None,
            Ok(node_compat_passing_outcome()),
        ),
    );

    let raw = std::fs::read_to_string(&path).expect("the observed-results file should exist");
    let lines: Vec<serde_json::Value> = raw
        .lines()
        .map(|line| serde_json::from_str(line).expect("each line is JSON"))
        .collect();
    assert_eq!(lines.len(), 4, "start, fixture, completion, fixture: {raw}");

    assert_eq!(lines[0]["kind"], "batch_start");
    let batch_key = lines[0]["batch"].as_str().expect("the start names a batch");
    // Two tests run the same batch for different lanes, so the key names the
    // test as well.
    assert!(
        batch_key.contains("streams/node20")
            && batch_key.contains("node_compat_batch_records_bracket_the_fixtures_they_measure"),
        "the key names the test, the batch, and the lane: {batch_key}"
    );

    assert_eq!(lines[1]["test_relative_path"], "test/parallel/test-one.js");
    assert_eq!(
        lines[1]["batch"], batch_key,
        "a fixture inside a batch names the batch that measured it"
    );

    assert_eq!(lines[2]["kind"], "batch_complete");
    assert_eq!(lines[2]["batch"], batch_key);
    assert_eq!(
        lines[2]["fixture_count"], 1,
        "the completion record counts the fixtures the loop executed"
    );

    assert_eq!(lines[3]["test_relative_path"], "test/parallel/test-two.js");
    assert!(
        lines[3].get("batch").is_none(),
        "the scope ended, so this fixture belongs to no batch: {}",
        lines[3]
    );
}

#[test]
#[should_panic(expected = "started inside batch")]
fn node_compat_a_nested_batch_is_refused() {
    let _outer = NodeCompatBatchScope::start("outer", "node20");
    let _inner = NodeCompatBatchScope::start("inner", "node20");
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
    record_node_compat_observed_result(
        "node20",
        NodeCompatFixtureIdentity::vendored(
            "test/parallel/test-one.js",
            "node20/test/parallel/test-one.js",
        ),
        &decision,
    );
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
