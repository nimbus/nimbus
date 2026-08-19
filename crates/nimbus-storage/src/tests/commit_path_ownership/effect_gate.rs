//! Checks the writer ownership matrix against the source it describes.
//!
//! The matrix alone would only be a list. This gate makes it a contract: it
//! reads `sql/store_core.rs` as text, classifies every `SqlStoreCore` method,
//! and requires that the two agree. A new writer with no row fails. A row for a
//! writer that no longer exists fails. A row whose effects contradict the
//! writer's own body fails. Reading source rather than registering writers at
//! run time is deliberate — several writers are provider-feature-gated, and the
//! bare `cargo test -p nimbus-storage` the plan verifier runs does not compile
//! them, so a runtime registry would report success over an empty set.

use super::effect_matrix::{
    Admission, CatalogEffect, Condition, DocumentEffect, IndexEffect, JournalEffect, Lease, MATRIX,
    Outcome, SchedulerEffect, Shape, TriggerEffect, VersionEffect, WatermarkEffect, WriterEffects,
};

/// The write-transaction primitives. They open a transaction for other writers
/// and own no effect of their own, so they carry no row.
const PRIMITIVES: [&str; 2] = ["execute_write", "execute_write_cancellable"];

/// Bodiless `SqlStoreCore` methods that only read.
const PROVIDER_READERS: [&str; 4] = [
    "retention_floor",
    "journal_progress",
    "read_durable_journal_from",
    "export_materialized_journal_snapshot",
];

/// Bodiless `SqlStoreCore` methods that mutate. Source cannot tell: the body
/// lives in a provider or in a feature-gated free function, so the split is
/// pinned here and every bodiless method must appear in exactly one pin list.
const PROVIDER_MUTATORS: [&str; 5] = [
    "append_durable_records_batch",
    "apply_durable_records_batch",
    "replay_durable_records_batch",
    "fenced_append_and_apply_durable_records_batch_cancellable",
    "recover_durable_journal",
];

/// Declared outcome for each return payload, so an outcome cannot drift from
/// the signature it claims to describe.
const OUTCOME_BY_PAYLOAD: [(&str, Outcome); 9] = [
    ("()", Outcome::Unit),
    ("bool", Outcome::Boolean),
    ("CommitEntry", Outcome::CommitEntry),
    ("Option<CommitEntry>", Outcome::OptionalCommitEntry),
    ("(CommitEntry, Document)", Outcome::CommitAndRemovedDocument),
    (
        "Option<(CommitEntry, Document)>",
        Outcome::OptionalCommitAndRemovedDocument,
    ),
    ("Vec<ScheduledJob>", Outcome::ClaimedJobs),
    ("JournalProgress", Outcome::JournalProgress),
    ("RetentionGcSummary", Outcome::RetentionSummary),
];

/// Floors, so the scan cannot silently shrink to nothing and report success.
const DIRECT_WRITER_FLOOR: usize = 26;
const CORE_WRITER_FLOOR: usize = 44;
const EXTERNAL_WRITER_FLOOR: usize = 10;

/// One dropped effect: the column it removes, and the edit that removes it.
type EffectOmission = (&'static str, fn(&mut WriterEffects));

#[derive(Clone, Debug, PartialEq, Eq)]
enum ScannedShape {
    Direct,
    Composes(Vec<String>),
    Bodiless,
    Reader,
}

#[derive(Clone, Debug)]
struct CoreMethod {
    name: String,
    signature: String,
    body: String,
    shape: ScannedShape,
}

struct CoreScan {
    methods: Vec<CoreMethod>,
    writers: Vec<String>,
}

fn repo_root() -> std::path::PathBuf {
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(std::path::PathBuf::from)
        .expect("CARGO_MANIFEST_DIR should be set by Cargo/nextest for nimbus-storage tests");
    manifest
        .parent()
        .and_then(std::path::Path::parent)
        .expect("crates/nimbus-storage sits two levels below the repository root")
        .to_path_buf()
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Split `SqlStoreCore` into its methods, with comments removed so a doc
/// comment mentioning `execute_write` cannot classify a reader as a writer.
fn scan_core() -> CoreScan {
    let path = repo_root()
        .join("crates/nimbus-storage/src/sql/store_core.rs")
        .to_path_buf();
    let source = std::fs::read_to_string(&path)
        .expect("sql/store_core.rs must exist — it owns the shared store writers");

    let lines: Vec<&str> = source.lines().collect();
    let start = lines
        .iter()
        .position(|line| line.starts_with("pub(crate) trait SqlStoreCore"))
        .expect("SqlStoreCore must exist in sql/store_core.rs; the ownership scan anchors on it");
    let end = lines
        .iter()
        .enumerate()
        .position(|(index, line)| index > start && *line == "}")
        .expect("SqlStoreCore must close at column zero");

    let mut chunks: Vec<(String, Vec<&str>)> = Vec::new();
    for line in &lines[start + 1..end] {
        if let Some(name) = line.strip_prefix("    fn ") {
            let name = name
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .next()
                .unwrap_or_default()
                .to_string();
            chunks.push((name, Vec::new()));
        }
        if let Some(current) = chunks.last_mut() {
            current.1.push(line);
        }
    }

    let names: Vec<String> = chunks.iter().map(|(name, _)| name.clone()).collect();
    let mut methods = Vec::new();
    for (name, chunk) in chunks {
        let code: String = chunk
            .iter()
            .filter(|line| !line.trim_start().starts_with("//"))
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        let brace = code.find('{');
        let semicolon = code.find(';');
        let bodiless = match (brace, semicolon) {
            (Some(brace), Some(semicolon)) => semicolon < brace,
            (None, Some(_)) => true,
            _ => false,
        };
        let (signature, body) = match (bodiless, brace) {
            (false, Some(brace)) => (
                collapse_whitespace(&code[..brace]),
                code[brace..].to_string(),
            ),
            _ => (
                collapse_whitespace(code.split(';').next().unwrap_or(&code)),
                String::new(),
            ),
        };
        let shape = if bodiless {
            ScannedShape::Bodiless
        } else if body.contains("execute_write") {
            ScannedShape::Direct
        } else {
            let delegates: Vec<String> = names
                .iter()
                .filter(|other| **other != name && body.contains(&format!("self.{other}(")))
                .cloned()
                .collect();
            if delegates.is_empty() {
                ScannedShape::Reader
            } else {
                ScannedShape::Composes(delegates)
            }
        };
        methods.push(CoreMethod {
            name,
            signature,
            body,
            shape,
        });
    }

    let mut writers: Vec<String> = methods
        .iter()
        .filter(|method| match &method.shape {
            ScannedShape::Direct => true,
            ScannedShape::Bodiless => PROVIDER_MUTATORS.contains(&method.name.as_str()),
            _ => false,
        })
        .map(|method| method.name.clone())
        .collect();

    // A composing method is a writer when it can reach one, however deep the
    // forwarding chain runs: `insert_with_indexes` reaches `insert_once`
    // through `insert`.
    loop {
        let mut grew = false;
        for method in &methods {
            if writers.contains(&method.name) {
                continue;
            }
            if let ScannedShape::Composes(delegates) = &method.shape
                && delegates.iter().any(|delegate| writers.contains(delegate))
            {
                writers.push(method.name.clone());
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }

    CoreScan { methods, writers }
}

fn method<'a>(scan: &'a CoreScan, name: &str) -> Option<&'a CoreMethod> {
    scan.methods.iter().find(|method| method.name == name)
}

fn row<'a>(matrix: &'a [WriterEffects], writer: &str) -> Option<&'a WriterEffects> {
    matrix.iter().find(|row| row.writer == writer)
}

/// The declared outcome for a scanned signature, and whether the signature is
/// fenced. `None` means the return type is one this gate does not know, which
/// is itself a violation: a new return shape must be pinned here.
fn scanned_outcome(signature: &str) -> Option<(Outcome, bool)> {
    let arrow = signature.find("->")?;
    let mut tail = signature[arrow + 2..].trim();
    if let Some(where_clause) = tail.find(" where ") {
        tail = tail[..where_clause].trim();
    }
    let (payload, fenced) = match tail.strip_prefix("CommitterLeaseResult<") {
        Some(inner) => (inner.strip_suffix('>')?, true),
        None => (tail.strip_prefix("Result<")?.strip_suffix('>')?, false),
    };
    OUTCOME_BY_PAYLOAD
        .iter()
        .find(|(text, _)| *text == payload.trim())
        .map(|(_, outcome)| (*outcome, fenced))
}

/// Effects a writer's own body proves it has. Every rule is one-directional —
/// evidence in the source implies a declaration — because the effects a writer
/// gets from the shared commit sequence leave no token in its body. That
/// asymmetry is the whole reason the direct path could drift.
fn evidence_violations(declaration: &WriterEffects, body: &str) -> Vec<String> {
    let mut found = Vec::new();
    let writer = declaration.writer;
    let mut require = |holds: bool, message: String| {
        if !holds {
            found.push(format!("{writer}: {message}"));
        }
    };

    if body.contains("begin_scheduled_execution") || body.contains("execution_dedup(") {
        require(
            declaration.admission == Admission::Deduplicated,
            "guards a scheduled-execution id but does not declare Admission::Deduplicated".into(),
        );
    }
    if body.contains("ExecutionDedup::NotDeduplicated") {
        require(
            declaration.admission == Admission::AlwaysAdmitted,
            "declares ExecutionDedup::NotDeduplicated but not Admission::AlwaysAdmitted".into(),
        );
    }
    if body.contains("advance_fenced_committer_lease")
        || body.contains("validate_fenced_committer_lease")
        || body.contains("LeaseEffect::Fenced")
    {
        require(
            declaration.lease == Lease::Fenced,
            "validates a committer lease but does not declare Lease::Fenced".into(),
        );
    }

    // A point write commits through the shared sequence, so its journal, index
    // and version effects are real even though the body never names them.
    let point = [
        ("insert_document", DocumentEffect::PointInsert),
        ("update_document_validated", DocumentEffect::PointUpdate),
        ("delete_document_validated", DocumentEffect::PointDelete),
    ];
    for (token, expected) in point {
        if body.contains(token) {
            require(
                declaration.document == expected,
                format!("calls {token} but declares {:?}", declaration.document),
            );
            require(
                declaration.index == IndexEffect::MaintainedWithDocument,
                format!("writes a document through {token} without declaring index maintenance"),
            );
            require(
                declaration.version == VersionEffect::RetainsNewVersion,
                format!("writes a document through {token} without declaring a retained version"),
            );
            require(
                declaration.journal == JournalEffect::CommitEntryFromBufferedWrites,
                format!("writes a document through {token} without declaring a journal effect"),
            );
        }
    }
    if body.contains("update_document_validated") || body.contains("delete_document_validated") {
        require(
            declaration.condition == Condition::CallerValidator,
            "runs a caller validator but does not declare Condition::CallerValidator".into(),
        );
    }
    if body.contains("DocumentWrites::PreparedDurableRecord") {
        require(
            declaration.document == DocumentEffect::PreparedRecord,
            "carries a prepared durable record but does not declare it".into(),
        );
        require(
            declaration.journal == JournalEffect::PreparedRecord,
            "carries a prepared durable record without the matching journal effect".into(),
        );
    }
    if body.contains("DocumentWrites::ResolvedExecutionUnit") {
        require(
            declaration.document == DocumentEffect::ResolvedExecutionUnit,
            "commits a resolved execution unit but does not declare it".into(),
        );
    }
    if body.contains("prune_retained_versions") {
        require(
            declaration.version == VersionEffect::PrunesRetained,
            "prunes retained versions but does not declare it".into(),
        );
        require(
            declaration.index == IndexEffect::PrunedWithVersions,
            "prunes retained versions without declaring the index effect".into(),
        );
    }
    if body.contains("schedule_ops_effect") {
        require(
            declaration.scheduler == SchedulerEffect::ResolvedOps,
            "applies resolved schedule operations but does not declare them".into(),
        );
    }
    let scheduler = [
        ("insert_scheduled_job", SchedulerEffect::JobInserted),
        ("claim_due_jobs", SchedulerEffect::JobsClaimed),
        ("complete_scheduled_job", SchedulerEffect::JobCompleted),
        ("cancel_scheduled_job", SchedulerEffect::JobCancelled),
        (
            "record_scheduled_job_result",
            SchedulerEffect::JobResultRecorded,
        ),
        ("save_cron_job", SchedulerEffect::CronSaved),
        ("delete_cron_job", SchedulerEffect::CronDeleted),
        (
            "recover_running_jobs",
            SchedulerEffect::RunningJobsRecovered,
        ),
    ];
    for (token, expected) in scheduler {
        if body.contains(&format!("transaction.{token}(")) {
            require(
                declaration.scheduler == expected,
                format!(
                    "writes scheduler state through {token} but declares {:?}",
                    declaration.scheduler
                ),
            );
        }
    }
    let trigger = [
        (
            "materialize_trigger_invocations",
            TriggerEffect::InvocationsMaterialized,
        ),
        ("save_trigger_invocation", TriggerEffect::InvocationSaved),
    ];
    for (token, expected) in trigger {
        if body.contains(&format!("transaction.{token}(")) {
            require(
                declaration.trigger == expected,
                format!(
                    "writes trigger state through {token} but declares {:?}",
                    declaration.trigger
                ),
            );
        }
    }
    let catalog = [
        ("replace_table_schema", CatalogEffect::TableSchemaReplaced),
        ("delete_table_schema", CatalogEffect::TableSchemaDeleted),
    ];
    for (token, expected) in catalog {
        if body.contains(&format!("transaction.{token}(")) {
            require(
                declaration.catalog == expected,
                format!(
                    "writes catalog state through {token} but declares {:?}",
                    declaration.catalog
                ),
            );
        }
    }
    if body.contains("WatermarkEffect::AdvancedByRecordApply") {
        require(
            declaration.watermark == WatermarkEffect::AdvancedByRecordApply,
            "advances the applied watermark but does not declare it".into(),
        );
    }
    if body.contains("WatermarkEffect::NotAdvanced") {
        require(
            declaration.watermark == WatermarkEffect::NotAdvanced,
            "declares WatermarkEffect::NotAdvanced in source but not in the matrix".into(),
        );
    }
    if body.contains("JournalEffect::CommitEntryFromBufferedWrites") {
        require(
            declaration.journal == JournalEffect::CommitEntryFromBufferedWrites,
            "appends a commit entry from buffered writes but does not declare it".into(),
        );
    }
    if body.contains("TriggerOriginEffect::Explicit") {
        require(
            declaration.trigger == TriggerEffect::OriginExplicitOrDefault,
            "can carry an explicit trigger origin but does not declare it".into(),
        );
    }

    found
}

/// True when a row claims no effect at all. A writer that touches storage and
/// declares twelve no-ops has said nothing, which is the silence this matrix
/// exists to remove.
fn declares_nothing(declaration: &WriterEffects) -> bool {
    declaration.admission == Admission::AlwaysAdmitted
        && declaration.lease == Lease::NotFenced
        && declaration.condition == Condition::Unconditional
        && declaration.document == DocumentEffect::None
        && declaration.index == IndexEffect::None
        && declaration.version == VersionEffect::None
        && declaration.catalog == CatalogEffect::None
        && declaration.scheduler == SchedulerEffect::None
        && declaration.trigger == TriggerEffect::None
        && declaration.journal == JournalEffect::None
        && declaration.watermark == WatermarkEffect::NotAdvanced
}

/// Every effect a composing writer declares must come from a delegate it
/// actually calls. The outcome is excluded: a forwarder narrows `Option` away,
/// and its own signature already checks it.
fn composition_violations(
    declaration: &WriterEffects,
    delegates: &[&'static str],
    matrix: &[WriterEffects],
) -> Vec<String> {
    let sources: Vec<&WriterEffects> = delegates
        .iter()
        .filter_map(|delegate| row(matrix, delegate))
        .collect();
    if sources.is_empty() {
        return vec![format!(
            "{}: composes {delegates:?} but none of them has a matrix row",
            declaration.writer
        )];
    }
    let mut found = Vec::new();
    let mut require = |holds: bool, effect: &str| {
        if !holds {
            found.push(format!(
                "{}: declares a {effect} effect that no delegate in {delegates:?} declares",
                declaration.writer
            ));
        }
    };
    require(
        sources.iter().any(|s| s.admission == declaration.admission),
        "admission",
    );
    require(
        sources.iter().any(|s| s.lease == declaration.lease),
        "lease",
    );
    require(
        sources.iter().any(|s| s.condition == declaration.condition),
        "condition",
    );
    require(
        sources.iter().any(|s| s.document == declaration.document),
        "document",
    );
    require(
        sources.iter().any(|s| s.index == declaration.index),
        "index",
    );
    require(
        sources.iter().any(|s| s.version == declaration.version),
        "version",
    );
    require(
        sources.iter().any(|s| s.catalog == declaration.catalog),
        "catalog",
    );
    require(
        sources.iter().any(|s| s.scheduler == declaration.scheduler),
        "scheduler",
    );
    require(
        sources.iter().any(|s| s.trigger == declaration.trigger),
        "trigger",
    );
    require(
        sources.iter().any(|s| s.journal == declaration.journal),
        "journal",
    );
    require(
        sources.iter().any(|s| s.watermark == declaration.watermark),
        "watermark",
    );
    found
}

/// The whole gate as one pure function, so the mutation test below can run it
/// over a deliberately damaged matrix and prove it fails.
fn ownership_violations(matrix: &[WriterEffects], scan: &CoreScan) -> Vec<String> {
    let mut found = Vec::new();

    for method in &scan.methods {
        if method.shape != ScannedShape::Bodiless {
            continue;
        }
        let pins = [
            PRIMITIVES.contains(&method.name.as_str()),
            PROVIDER_READERS.contains(&method.name.as_str()),
            PROVIDER_MUTATORS.contains(&method.name.as_str()),
        ]
        .into_iter()
        .filter(|pinned| *pinned)
        .count();
        if pins != 1 {
            found.push(format!(
                "{}: a SqlStoreCore method with no default body must be pinned in exactly one of \
                 PRIMITIVES, PROVIDER_READERS or PROVIDER_MUTATORS (pinned in {pins})",
                method.name
            ));
        }
    }
    for pinned in PRIMITIVES
        .iter()
        .chain(PROVIDER_READERS.iter())
        .chain(PROVIDER_MUTATORS.iter())
    {
        if method(scan, pinned).is_none() {
            found.push(format!(
                "{pinned} is pinned in effect_gate.rs but no longer exists in SqlStoreCore"
            ));
        }
    }

    for writer in &scan.writers {
        let rows = matrix.iter().filter(|row| row.writer == *writer).count();
        if rows != 1 {
            found.push(format!(
                "{writer} writes storage and has {rows} matrix rows; every writer declares its \
                 effects exactly once"
            ));
        }
    }

    for declaration in matrix {
        if let Shape::External { path, symbol } = declaration.shape {
            let full = repo_root().join(path);
            match std::fs::read_to_string(&full) {
                Ok(source) if source.contains(symbol) => {}
                Ok(_) => found.push(format!(
                    "{}: {path} no longer contains {symbol}",
                    declaration.writer
                )),
                Err(error) => found.push(format!(
                    "{}: cannot read {path}: {error}",
                    declaration.writer
                )),
            }
            if declares_nothing(declaration) {
                found.push(format!(
                    "{}: declares no effect at all; a writer that touches storage must name at \
                     least one",
                    declaration.writer
                ));
            }
            continue;
        }

        let Some(method) = method(scan, declaration.writer) else {
            found.push(format!(
                "{}: has a matrix row but is not a SqlStoreCore method",
                declaration.writer
            ));
            continue;
        };
        if !scan.writers.contains(&method.name) {
            found.push(format!(
                "{}: has a matrix row but the scan classifies it as a reader or a primitive",
                declaration.writer
            ));
        }
        match (&declaration.shape, &method.shape) {
            (Shape::Direct, ScannedShape::Direct) => {}
            (Shape::ProviderBodied, ScannedShape::Bodiless) => {}
            (Shape::Composes(declared), ScannedShape::Composes(scanned)) => {
                let mut declared: Vec<&str> = declared.to_vec();
                let mut scanned_writers: Vec<&str> = scanned
                    .iter()
                    .filter(|delegate| scan.writers.contains(delegate))
                    .map(String::as_str)
                    .collect();
                declared.sort_unstable();
                scanned_writers.sort_unstable();
                if declared != scanned_writers {
                    found.push(format!(
                        "{}: declares delegates {declared:?} but calls {scanned_writers:?}",
                        declaration.writer
                    ));
                }
            }
            (declared, scanned) => found.push(format!(
                "{}: declares shape {declared:?} but the source reads as {scanned:?}",
                declaration.writer
            )),
        }

        match scanned_outcome(&method.signature) {
            Some((outcome, fenced)) => {
                if outcome != declaration.outcome {
                    found.push(format!(
                        "{}: returns {outcome:?} but declares {:?}",
                        declaration.writer, declaration.outcome
                    ));
                }
                let declared_fenced = declaration.lease == Lease::Fenced;
                if fenced != declared_fenced {
                    found.push(format!(
                        "{}: CommitterLeaseResult in the signature is {fenced} but Lease::Fenced \
                         in the matrix is {declared_fenced}",
                        declaration.writer
                    ));
                }
            }
            None => found.push(format!(
                "{}: returns a shape this gate does not know; pin it in OUTCOME_BY_PAYLOAD so the \
                 outcome column stays checked",
                declaration.writer
            )),
        }

        if declaration.writer.starts_with("fenced_") && declaration.lease != Lease::Fenced {
            found.push(format!(
                "{}: is named as a fenced writer but does not declare Lease::Fenced",
                declaration.writer
            ));
        }

        if declares_nothing(declaration) {
            found.push(format!(
                "{}: declares no effect at all; a writer that touches storage must name at least \
                 one",
                declaration.writer
            ));
        }

        if let ScannedShape::Direct = method.shape {
            found.extend(evidence_violations(declaration, &method.body));
        }
        if let Shape::Composes(delegates) = declaration.shape {
            found.extend(composition_violations(declaration, delegates, matrix));
        }
    }

    found
}

/// SIC3, condition 7: every client and internal storage writer is inventoried
/// in one checked matrix, not only the three composite SQL commit paths.
#[test]
fn all_storage_writers_declare_their_commit_effects() {
    let scan = scan_core();

    let direct = scan
        .methods
        .iter()
        .filter(|method| method.shape == ScannedShape::Direct)
        .count();
    assert!(
        direct >= DIRECT_WRITER_FLOOR,
        "the ownership scan found only {direct} direct writers in SqlStoreCore; the scan is broken \
         and would report success over an empty set"
    );
    assert!(
        scan.writers.len() >= CORE_WRITER_FLOOR,
        "the ownership scan classified only {} SqlStoreCore methods as writers",
        scan.writers.len()
    );
    let external = MATRIX
        .iter()
        .filter(|row| matches!(row.shape, Shape::External { .. }))
        .count();
    assert!(
        external >= EXTERNAL_WRITER_FLOOR,
        "the matrix names only {external} writers outside SqlStoreCore; the SIC0 census lists the \
         engine committer routes, object metadata, committer lease, trigger candidates, table \
         lifecycle, resource paths, replica cache and usage store"
    );

    println!(
        "ownership scan: {} SqlStoreCore methods, {direct} direct writers, {} writers total, \
         {} matrix rows ({external} outside SqlStoreCore)",
        scan.methods.len(),
        scan.writers.len(),
        MATRIX.len()
    );

    let violations = ownership_violations(MATRIX, &scan);
    assert!(
        violations.is_empty(),
        "storage writer ownership is incomplete. Every writer declares admission, lease, \
         condition, document, index, version, catalog, scheduler, trigger, journal, watermark and \
         outcome in crates/nimbus-storage/src/tests/commit_path_ownership/effect_matrix.rs, and \
         each declaration is checked against the writer's own source. Violations: {violations:#?}"
    );
}

/// SIC3, condition 8: an effect cannot be omitted. Each mutation below removes
/// exactly one thing a writer declares and proves the gate reports it, which is
/// what stops this matrix from being a list that agrees with itself.
#[test]
fn omitted_commit_effect_fails_the_ownership_matrix() {
    let scan = scan_core();
    assert!(
        ownership_violations(MATRIX, &scan).is_empty(),
        "the mutation test needs a clean baseline"
    );

    let index = MATRIX
        .iter()
        .position(|row| row.writer == "insert_once")
        .expect("insert_once is a direct point writer and must be in the matrix");

    let without_row: Vec<WriterEffects> = MATRIX
        .iter()
        .enumerate()
        .filter(|(position, _)| *position != index)
        .map(|(_, row)| *row)
        .collect();
    let violations = ownership_violations(&without_row, &scan);
    assert!(
        violations.iter().any(|text| text.contains("insert_once")),
        "removing a writer's row must fail the gate, got {violations:#?}"
    );

    let mutations: [EffectOmission; 5] = [
        ("journal", |row| row.journal = JournalEffect::None),
        ("index", |row| row.index = IndexEffect::None),
        ("version", |row| row.version = VersionEffect::None),
        ("document", |row| row.document = DocumentEffect::None),
        ("admission", |row| row.admission = Admission::AlwaysAdmitted),
    ];
    for (effect, omit) in mutations {
        let mut damaged = MATRIX.to_vec();
        omit(&mut damaged[index]);
        let violations = ownership_violations(&damaged, &scan);
        assert!(
            violations.iter().any(|text| text.contains("insert_once")),
            "omitting the {effect} effect of insert_once must fail the gate, got {violations:#?}"
        );
    }

    let mut stale = MATRIX.to_vec();
    stale[index].writer = "insert_once_that_no_longer_exists";
    let violations = ownership_violations(&stale, &scan);
    assert!(
        violations
            .iter()
            .any(|text| text.contains("insert_once_that_no_longer_exists")),
        "a row for a writer that does not exist must fail the gate, got {violations:#?}"
    );

    let outcome_index = MATRIX
        .iter()
        .position(|row| row.writer == "cancel_scheduled_job")
        .expect("cancel_scheduled_job must be in the matrix");
    let mut wrong_outcome = MATRIX.to_vec();
    wrong_outcome[outcome_index].outcome = Outcome::Unit;
    let violations = ownership_violations(&wrong_outcome, &scan);
    assert!(
        violations
            .iter()
            .any(|text| text.contains("cancel_scheduled_job")),
        "an outcome that contradicts the writer's return type must fail the gate, got \
         {violations:#?}"
    );

    let schema_index = MATRIX
        .iter()
        .position(|row| row.writer == "replace_table_schema")
        .expect("replace_table_schema must be in the matrix");
    let mut silent = MATRIX.to_vec();
    silent[schema_index].catalog = CatalogEffect::None;
    let violations = ownership_violations(&silent, &scan);
    assert!(
        violations
            .iter()
            .any(|text| text.contains("replace_table_schema")),
        "a row that declares twelve no-ops must fail the gate, got {violations:#?}"
    );
}
