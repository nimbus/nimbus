//! Differential test for the six single-document write preparers.
//!
//! Each case feeds the same logical mutation, principal, schema, and stored
//! state to every preparer, stamps the result as sequence assignment would,
//! and compares the durable `WriteOp` and the selected index work. Nothing is
//! committed, so every preparer observes the same stored state.

use nimbus_core::{
    AtomicWrite, AtomicWriteBatch, DocumentId, DocumentLocator, DocumentPath, Error,
    IndexDefinition, IndexState, Mutation, PrincipalContext, ResourcePathBinding, Result,
    SequenceNumber, TableName, Timestamp, WriteKey, WriteOp, WritePrecondition, WriteSetMode,
};
use nimbus_testing::EngineFixture;
use serde_json::json;

use crate::Engine;
use crate::tenant::TenantRuntime;
use crate::test_support::{
    messages_schema, messages_table, owner_write_policy, principal_with_subject,
};

use super::direct::prepare_direct_write_for_testing;
use super::inline_reprepare::rebuild_write_for_testing;
use super::journal::prepare_queued_write_for_testing;
use super::prepared::{InlineRepreparePlan, PreparedCommit};
use super::window_prepare::prepare_single_document_write_from_window;
use super::write_log::WindowDocumentState;

const TABLE: &str = "mr66_tasks";
const MISSING_TABLE: &str = "mr66_absent";
const UNSCHEMATIZED_TABLE: &str = "mr66_free";
const ASSIGNED_SEQUENCE: SequenceNumber = SequenceNumber(1_000);
const ASSIGNED_AT: Timestamp = Timestamp(1_700_000_000_000);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Site {
    Window,
    Direct,
    Journal,
    Reprepare,
    Staging,
    Batch,
}

const SITES: [Site; 6] = [
    Site::Window,
    Site::Direct,
    Site::Journal,
    Site::Reprepare,
    Site::Staging,
    Site::Batch,
];

#[derive(Debug, Clone)]
enum Outcome {
    /// The preparer built one write. `None` indexes means the preparer selects
    /// index work later than prepare, so that comparison does not apply.
    Write {
        write: Box<WriteOp>,
        indexes: Option<Vec<IndexDefinition>>,
    },
    /// The preparer accepted the mutation and staged no durable write.
    NoWrite,
    /// The preparer rejected the mutation with this error variant.
    Error(String),
    /// The preparer has no input for this state and defers to another site.
    Declined,
}

impl Outcome {
    fn agrees_with(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Write {
                    write: left,
                    indexes: left_indexes,
                },
                Self::Write {
                    write: right,
                    indexes: right_indexes,
                },
            ) => {
                left == right
                    && match (left_indexes, right_indexes) {
                        (Some(left), Some(right)) => left == right,
                        _ => true,
                    }
            }
            (Self::NoWrite, Self::NoWrite) => true,
            (Self::Error(left), Self::Error(right)) => left == right,
            _ => false,
        }
    }

    fn summary(&self) -> String {
        match self {
            Self::Write { write, indexes } => {
                let binding = write
                    .resource_path_binding
                    .as_ref()
                    .map(|binding| format!("{:?}", binding.document_path))
                    .unwrap_or_else(|| "none".to_string());
                let indexes = indexes
                    .as_ref()
                    .map(|indexes| {
                        format!(
                            "{:?}",
                            indexes.iter().map(|index| &index.name).collect::<Vec<_>>()
                        )
                    })
                    .unwrap_or_else(|| "not selected at prepare".to_string());
                format!(
                    "write {:?} table_id={:?} binding={binding} indexes={indexes} previous={:?} current={:?}",
                    write.op_type,
                    write.table_id,
                    write.previous.as_ref().map(|document| &document.fields),
                    write.current.as_ref().map(|document| &document.fields),
                )
            }
            Self::NoWrite => "no write".to_string(),
            Self::Error(variant) => format!("error {variant}"),
            Self::Declined => "declined".to_string(),
        }
    }
}

fn error_variant(error: &Error) -> String {
    let debug = format!("{error:?}");
    let end = debug.find(['(', ' ', '{']).unwrap_or(debug.len());
    let variant = &debug[..end];
    match error {
        Error::Conflict { retryable, .. } => format!("{variant}(retryable={retryable})"),
        _ => variant.to_string(),
    }
}

fn outcome(result: Result<Outcome>) -> Outcome {
    result.unwrap_or_else(|error| Outcome::Error(error_variant(&error)))
}

struct Case {
    name: &'static str,
    mutation: Mutation,
    /// The window preparer must not decline this case, so the comparison
    /// covers all six sites.
    window_prepares: bool,
    /// Outcomes that a commit path owns outside the shared preparer. Each
    /// pinned site must produce exactly this outcome and leaves the comparison.
    path_owned: Vec<PathOwned>,
}

struct PathOwned {
    site: Site,
    expected: Outcome,
    reason: &'static str,
}

impl Case {
    fn path_owned(mut self, site: Site, expected: Outcome, reason: &'static str) -> Self {
        self.path_owned.push(PathOwned {
            site,
            expected,
            reason,
        });
        self
    }
}

const INSERT_CONFLICT_AT_SERIAL_STEP: &str = "the serial step sees the existing document in its \
     retained image; the storage commit rejects the other single-document inserts with the same \
     terminal conflict";
const BATCH_CREATE_CONTRACT: &str =
    "a batch create follows the Firestore create contract and reports AlreadyExists";
const STAGED_NO_OP_ELIDED: &str =
    "the execution-unit buffer drops a staged write whose image equals the original";

fn table(name: &str) -> TableName {
    messages_table(name)
}

fn id(key: &str) -> DocumentId {
    DocumentId::from_key(key).expect("document id should parse")
}

fn fields(
    entries: impl IntoIterator<Item = (&'static str, serde_json::Value)>,
) -> serde_json::Map<String, serde_json::Value> {
    entries
        .into_iter()
        .map(|(name, value)| (name.to_string(), value))
        .collect()
}

fn insert(
    table_name: &str,
    key: &str,
    entries: serde_json::Map<String, serde_json::Value>,
) -> Mutation {
    Mutation::Insert {
        table: table(table_name),
        id: Some(id(key)),
        fields: entries,
    }
}

fn update(
    table_name: &str,
    key: &str,
    patch: serde_json::Map<String, serde_json::Value>,
) -> Mutation {
    Mutation::Update {
        table: table(table_name),
        id: id(key),
        patch,
    }
}

fn delete(table_name: &str, key: &str) -> Mutation {
    Mutation::Delete {
        table: table(table_name),
        id: id(key),
    }
}

fn cases() -> Vec<Case> {
    let case = |name, mutation, window_prepares| Case {
        name,
        mutation,
        window_prepares,
        path_owned: Vec::new(),
    };
    vec![
        case(
            "insert_valid",
            insert(
                TABLE,
                "fresh",
                fields([("owner", json!("alice")), ("body", json!("new"))]),
            ),
            true,
        ),
        case(
            "insert_schema_invalid",
            insert(TABLE, "invalid", fields([("owner", json!("alice"))])),
            true,
        ),
        case(
            "insert_denied",
            insert(
                TABLE,
                "denied",
                fields([("owner", json!("bob")), ("body", json!("new"))]),
            ),
            true,
        ),
        case(
            "insert_existing_id",
            insert(
                TABLE,
                "seed",
                fields([("owner", json!("alice")), ("body", json!("again"))]),
            ),
            true,
        )
        .path_owned(
            Site::Reprepare,
            Outcome::Error("Conflict(retryable=false)".to_string()),
            INSERT_CONFLICT_AT_SERIAL_STEP,
        )
        .path_owned(
            Site::Batch,
            Outcome::Error("AlreadyExists".to_string()),
            BATCH_CREATE_CONTRACT,
        ),
        case(
            "insert_unschematized_table",
            insert(
                UNSCHEMATIZED_TABLE,
                "free",
                fields([("anything", json!(1))]),
            ),
            false,
        ),
        case(
            "update_valid",
            update(TABLE, "seed", fields([("body", json!("two"))])),
            true,
        ),
        case(
            "update_schema_invalid",
            update(TABLE, "seed", fields([("body", json!(5))])),
            true,
        ),
        case(
            "update_denied",
            update(TABLE, "seed-bob", fields([("body", json!("two"))])),
            true,
        ),
        case(
            "update_noop",
            update(TABLE, "seed", fields([("body", json!("one"))])),
            true,
        )
        .path_owned(Site::Staging, Outcome::NoWrite, STAGED_NO_OP_ELIDED)
        .path_owned(Site::Batch, Outcome::NoWrite, STAGED_NO_OP_ELIDED),
        case(
            "update_bound",
            update(TABLE, "seed-bound", fields([("body", json!("two"))])),
            true,
        ),
        case(
            "update_missing_document",
            update(TABLE, "absent", fields([("body", json!("two"))])),
            false,
        ),
        case(
            "update_missing_table",
            update(MISSING_TABLE, "seed", fields([("body", json!("two"))])),
            false,
        ),
        case("delete_valid", delete(TABLE, "seed"), true),
        case("delete_denied", delete(TABLE, "seed-bob"), true),
        case("delete_bound", delete(TABLE, "seed-bound"), true),
        case("delete_missing_document", delete(TABLE, "absent"), false),
        case("delete_missing_table", delete(MISSING_TABLE, "seed"), false),
    ]
}

fn bound_seed_binding() -> ResourcePathBinding {
    ResourcePathBinding::new(
        DocumentLocator::new(table(TABLE), id("seed-bound")),
        DocumentPath::from_segments(["projects", "p1", "tasks", "seed-bound"])
            .expect("document path should parse"),
    )
}

fn seed(engine: &std::sync::Arc<Engine>, tenant_id: &nimbus_core::TenantId) {
    let by_owner = IndexDefinition {
        id: nimbus_core::IndexId::new(),
        name: "by_owner".to_string(),
        fields: vec!["owner".to_string()],
        state: IndexState::Enabled,
    };
    engine
        .set_table_schema(
            tenant_id,
            messages_schema(TABLE, vec![by_owner], Some(owner_write_policy())),
        )
        .expect("schema should apply");
    let alice = principal_with_subject("alice");
    let bob = principal_with_subject("bob");
    engine
        .insert_document_with(
            tenant_id,
            table(TABLE),
            Some(id("seed")),
            fields([("owner", json!("alice")), ("body", json!("one"))]),
            crate::MutationActor::with_principal(&alice),
        )
        .expect("alice seed should insert");
    engine
        .insert_document_with(
            tenant_id,
            table(TABLE),
            Some(id("seed-bob")),
            fields([("owner", json!("bob")), ("body", json!("one"))]),
            crate::MutationActor::with_principal(&bob),
        )
        .expect("bob seed should insert");
    let batch = AtomicWriteBatch::new(vec![AtomicWrite::Set {
        key: WriteKey::from(bound_seed_binding()),
        document: fields([("owner", json!("alice")), ("body", json!("bound"))]),
        typed_fields: Default::default(),
        mode: WriteSetMode::Overwrite,
        precondition: WritePrecondition::default(),
        transforms: Vec::new(),
    }])
    .expect("seed batch should build");
    engine
        .begin_mutation_execution_unit(tenant_id.clone(), alice)
        .expect("seed execution unit should begin")
        .execute_atomic_write_batch(batch)
        .expect("bound seed should commit");
}

fn mutation_key(mutation: &Mutation) -> (&TableName, &DocumentId) {
    match mutation {
        Mutation::Insert {
            table,
            id: Some(id),
            ..
        }
        | Mutation::Update { table, id, .. }
        | Mutation::Delete { table, id } => (table, id),
        Mutation::Insert { id: None, .. } => panic!("differential cases use explicit ids"),
    }
}

fn single_write(writes: Vec<(WriteOp, Vec<IndexDefinition>)>) -> Result<Outcome> {
    match <[(WriteOp, Vec<IndexDefinition>); 1]>::try_from(writes) {
        Ok([(write, indexes)]) => Ok(Outcome::Write {
            write: Box::new(write),
            indexes: Some(indexes),
        }),
        Err(writes) if writes.is_empty() => Ok(Outcome::NoWrite),
        Err(writes) => Err(Error::Internal(format!(
            "one mutation staged {} writes",
            writes.len()
        ))),
    }
}

fn journal_shaped_write(snapshot_sequence: SequenceNumber, write: WriteOp) -> Result<WriteOp> {
    let record = PreparedCommit::for_journal(snapshot_sequence, vec![write], None)
        .into_record(ASSIGNED_SEQUENCE, ASSIGNED_AT)?;
    record
        .writes
        .into_iter()
        .next()
        .ok_or_else(|| Error::Internal("journal record lost its write".to_string()))
}

fn prepare_at(
    site: Site,
    engine: &std::sync::Arc<Engine>,
    runtime: &TenantRuntime,
    tenant_id: &nimbus_core::TenantId,
    mutation: &Mutation,
    principal: &PrincipalContext,
) -> Outcome {
    match site {
        Site::Window => outcome((|| {
            let Some(prepared) =
                prepare_single_document_write_from_window(runtime, mutation, principal)?
            else {
                return Ok(Outcome::Declined);
            };
            Ok(Outcome::Write {
                write: Box::new(journal_shaped_write(
                    prepared.snapshot_sequence,
                    prepared.write,
                )?),
                indexes: Some(prepared.indexes),
            })
        })()),
        Site::Direct => outcome(
            prepare_direct_write_for_testing(
                runtime,
                mutation.clone(),
                principal,
                ASSIGNED_SEQUENCE,
                ASSIGNED_AT,
            )
            .map(|(write, indexes)| Outcome::Write {
                write: Box::new(write),
                indexes: Some(indexes),
            }),
        ),
        Site::Journal => outcome(
            prepare_queued_write_for_testing(
                runtime,
                mutation.clone(),
                principal.clone(),
                ASSIGNED_SEQUENCE,
                ASSIGNED_AT,
            )
            .map(|write| Outcome::Write {
                write: Box::new(write),
                indexes: None,
            }),
        ),
        Site::Reprepare => outcome((|| {
            // The serial step rebuilds from the latest retained image of the
            // document. The stored state supplies the same image here.
            let (table, document_id) = mutation_key(mutation);
            let snapshot = runtime.store.read_snapshot()?;
            let table_id = match snapshot.table_id(table)? {
                Some(table_id) => table_id,
                None => match runtime.prepared_table_id_if_known(table) {
                    Some(table_id) => table_id,
                    None => return Ok(Outcome::Declined),
                },
            };
            let base = WindowDocumentState {
                sequence: snapshot.applied_sequence()?,
                table_id,
                document: snapshot.get(table, document_id)?,
                resource_path_binding: snapshot.resource_path_binding(&DocumentLocator::new(
                    table.clone(),
                    document_id.clone(),
                ))?,
            };
            let plan = InlineRepreparePlan {
                mutation: mutation.clone(),
                principal: principal.clone(),
                schema: runtime.schema(),
            };
            let (write, indexes) = rebuild_write_for_testing(&plan, &base)?;
            Ok(Outcome::Write {
                write: Box::new(journal_shaped_write(base.sequence, write)?),
                indexes: Some(indexes),
            })
        })()),
        Site::Staging => outcome((|| {
            let unit =
                engine.begin_mutation_execution_unit(tenant_id.clone(), principal.clone())?;
            match mutation.clone() {
                Mutation::Insert { table, id, fields } => {
                    unit.insert_document_with_id(table, id, fields)?;
                }
                Mutation::Update { table, id, patch } => {
                    unit.update_document(table, id, patch)?;
                }
                Mutation::Delete { table, id } => unit.delete_document(table, id)?,
            }
            single_write(unit.assigned_writes_for_testing(ASSIGNED_SEQUENCE, ASSIGNED_AT)?)
        })()),
        Site::Batch => outcome((|| {
            let write = match mutation.clone() {
                Mutation::Insert { table, id, fields } => AtomicWrite::Set {
                    key: WriteKey::from(DocumentLocator::new(
                        table,
                        id.expect("differential cases use explicit ids"),
                    )),
                    document: fields,
                    typed_fields: Default::default(),
                    mode: WriteSetMode::Create,
                    precondition: WritePrecondition::default(),
                    transforms: Vec::new(),
                },
                Mutation::Update { table, id, patch } => AtomicWrite::Patch {
                    key: WriteKey::from(DocumentLocator::new(table, id)),
                    field_patch: patch,
                    typed_fields: Default::default(),
                    mask: Vec::new(),
                    precondition: WritePrecondition::exists(true),
                    transforms: Vec::new(),
                },
                Mutation::Delete { table, id } => AtomicWrite::Delete {
                    key: WriteKey::from(DocumentLocator::new(table, id)),
                    precondition: WritePrecondition::default(),
                    missing_ok: false,
                },
            };
            let unit =
                engine.begin_mutation_execution_unit(tenant_id.clone(), principal.clone())?;
            unit.stage_atomic_write_batch(AtomicWriteBatch::new(vec![write])?)?;
            single_write(unit.assigned_writes_for_testing(ASSIGNED_SEQUENCE, ASSIGNED_AT)?)
        })()),
    }
}

/// Returns a report line for every case where two preparers disagree, or
/// where a pinned path-owned outcome changed.
fn divergences(case: &Case, outcomes: &[(Site, Outcome)]) -> Option<String> {
    let mut pinned_mismatches = Vec::new();
    let mut classes: Vec<(Outcome, Vec<Site>)> = Vec::new();
    for (site, outcome) in outcomes {
        if let Some(owned) = case.path_owned.iter().find(|owned| owned.site == *site) {
            if !owned.expected.agrees_with(outcome) {
                pinned_mismatches.push(format!(
                    "\n  {site:?} pinned to {} ({}) but produced {}",
                    owned.expected.summary(),
                    owned.reason,
                    outcome.summary()
                ));
            }
            continue;
        }
        if matches!(outcome, Outcome::Declined) {
            continue;
        }
        match classes
            .iter_mut()
            .find(|(class, _)| class.agrees_with(outcome))
        {
            Some((_, sites)) => sites.push(*site),
            None => classes.push((outcome.clone(), vec![*site])),
        }
    }
    let window_declined = outcomes
        .iter()
        .any(|(site, outcome)| *site == Site::Window && matches!(outcome, Outcome::Declined));
    if classes.len() <= 1
        && !(case.window_prepares && window_declined)
        && pinned_mismatches.is_empty()
    {
        return None;
    }
    let mut report = format!("{}: {} distinct outcomes", case.name, classes.len());
    for mismatch in &pinned_mismatches {
        report.push_str(mismatch);
    }
    if case.window_prepares && window_declined {
        report.push_str(" (window declined a case that it must prepare)");
    }
    let summaries = classes
        .iter()
        .map(|(class, _)| class.summary())
        .collect::<Vec<_>>();
    for (index, (class, sites)) in classes.iter().enumerate() {
        report.push_str(&format!("\n  {sites:?} => {}", summaries[index]));
        if summaries
            .iter()
            .enumerate()
            .any(|(other, summary)| other != index && summary == &summaries[index])
        {
            report.push_str(&format!("\n    full: {class:?}"));
        }
    }
    Some(report)
}

#[test]
fn prepare_write_op_sites_agree_on_every_case() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("mr66-differential", Engine::create_tenant);
    seed(&engine, &tenant_id);
    let runtime = engine
        .tenant_runtime_for_testing(&tenant_id)
        .expect("tenant runtime should load");
    let principal = principal_with_subject("alice");

    let mut report = Vec::new();
    for case in cases() {
        let outcomes = SITES
            .iter()
            .map(|site| {
                (
                    *site,
                    prepare_at(
                        *site,
                        &engine,
                        &runtime,
                        &tenant_id,
                        &case.mutation,
                        &principal,
                    ),
                )
            })
            .collect::<Vec<_>>();
        report.extend(divergences(&case, &outcomes));
    }
    assert!(
        report.is_empty(),
        "write preparers diverge:\n{}",
        report.join("\n")
    );
}
