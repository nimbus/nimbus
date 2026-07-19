//! Exhaustive PPSC5 crash-recovery decision table.
//!
//! The table is deliberately data, rather than a collection of hand-written
//! scenarios. Each closed axis supplies its variants to `ALL`; adding an axis
//! variant changes `CRASH_STATE_COUNT`, so the explicit table cannot keep
//! compiling until the new rows are supplied.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use nimbus_core::{
    Error, SeededIdSource, SequenceNumber, TableName, TenantEventKind, TenantEventRecord, TenantId,
    Timestamp,
};
use nimbus_storage::{FaultInjector, FaultPoint, ManualClock, NoopFaultInjector};
use serde_json::json;
use tempfile::tempdir;
use tokio::time::timeout;

use crate::Engine;
use crate::engine::execution_units::{Fault, labels};

macro_rules! closed_axis {
    ($name:ident { $($variant:ident),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum $name {
            $($variant),+
        }

        impl $name {
            const ALL: [Self; [$(stringify!($variant)),+].len()] = [$(Self::$variant),+];
        }
    };
}

closed_axis!(AssignmentState {
    Unassigned,
    Assigned,
});
closed_axis!(DurableAppendState { Missing, Landed });
closed_axis!(PublishState {
    NotPublished,
    Published,
});
closed_axis!(TailState { Clean, Torn });

const CRASH_STATE_COUNT: usize = AssignmentState::ALL.len()
    * DurableAppendState::ALL.len()
    * PublishState::ALL.len()
    * TailState::ALL.len();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CrashState {
    assignment: AssignmentState,
    durable_append_n: DurableAppendState,
    publish_n_plus_one: PublishState,
    tail: TailState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecoveryBranch {
    CleanUnassignedPrefix,
    TornUnassignedPrefix,
    CleanAssignedSuffixDiscard,
    TornAssignedSuffixDiscard,
    CleanDurableTailReplay,
    TornDurablePrefixReplay,
    CleanPublishedPrefix,
    TornPublishedPrefix,
    RejectInteriorSequenceHole,
}

#[derive(Debug, Clone, Copy)]
struct CrashCase {
    name: &'static str,
    state: CrashState,
    expected: RecoveryBranch,
}

macro_rules! state {
    ($assignment:ident, $durable:ident, $publish:ident, $tail:ident) => {
        CrashState {
            assignment: AssignmentState::$assignment,
            durable_append_n: DurableAppendState::$durable,
            publish_n_plus_one: PublishState::$publish,
            tail: TailState::$tail,
        }
    };
}

// Reviewable at a glance: 2 × 2 × 2 × 2 = 16 explicit crash states.
const CRASH_CASES: [CrashCase; CRASH_STATE_COUNT] = [
    CrashCase {
        name: "unassigned_missing_unpublished_clean",
        state: state!(Unassigned, Missing, NotPublished, Clean),
        expected: RecoveryBranch::CleanUnassignedPrefix,
    },
    CrashCase {
        name: "unassigned_missing_unpublished_torn",
        state: state!(Unassigned, Missing, NotPublished, Torn),
        expected: RecoveryBranch::TornUnassignedPrefix,
    },
    CrashCase {
        name: "unassigned_missing_published_clean",
        state: state!(Unassigned, Missing, Published, Clean),
        expected: RecoveryBranch::RejectInteriorSequenceHole,
    },
    CrashCase {
        name: "unassigned_missing_published_torn",
        state: state!(Unassigned, Missing, Published, Torn),
        expected: RecoveryBranch::RejectInteriorSequenceHole,
    },
    CrashCase {
        name: "unassigned_landed_unpublished_clean",
        state: state!(Unassigned, Landed, NotPublished, Clean),
        expected: RecoveryBranch::CleanDurableTailReplay,
    },
    CrashCase {
        name: "unassigned_landed_unpublished_torn",
        state: state!(Unassigned, Landed, NotPublished, Torn),
        expected: RecoveryBranch::TornDurablePrefixReplay,
    },
    CrashCase {
        name: "unassigned_landed_published_clean",
        state: state!(Unassigned, Landed, Published, Clean),
        expected: RecoveryBranch::CleanPublishedPrefix,
    },
    CrashCase {
        name: "unassigned_landed_published_torn",
        state: state!(Unassigned, Landed, Published, Torn),
        expected: RecoveryBranch::TornPublishedPrefix,
    },
    CrashCase {
        name: "assigned_missing_unpublished_clean",
        state: state!(Assigned, Missing, NotPublished, Clean),
        expected: RecoveryBranch::CleanAssignedSuffixDiscard,
    },
    CrashCase {
        name: "assigned_missing_unpublished_torn",
        state: state!(Assigned, Missing, NotPublished, Torn),
        expected: RecoveryBranch::TornAssignedSuffixDiscard,
    },
    CrashCase {
        name: "assigned_missing_published_clean",
        state: state!(Assigned, Missing, Published, Clean),
        expected: RecoveryBranch::RejectInteriorSequenceHole,
    },
    CrashCase {
        name: "assigned_missing_published_torn",
        state: state!(Assigned, Missing, Published, Torn),
        expected: RecoveryBranch::RejectInteriorSequenceHole,
    },
    CrashCase {
        name: "assigned_landed_unpublished_clean",
        state: state!(Assigned, Landed, NotPublished, Clean),
        expected: RecoveryBranch::CleanDurableTailReplay,
    },
    CrashCase {
        name: "assigned_landed_unpublished_torn",
        state: state!(Assigned, Landed, NotPublished, Torn),
        expected: RecoveryBranch::TornDurablePrefixReplay,
    },
    CrashCase {
        name: "assigned_landed_published_clean",
        state: state!(Assigned, Landed, Published, Clean),
        expected: RecoveryBranch::CleanPublishedPrefix,
    },
    CrashCase {
        name: "assigned_landed_published_torn",
        state: state!(Assigned, Landed, Published, Torn),
        expected: RecoveryBranch::TornPublishedPrefix,
    },
];

#[derive(Debug, Clone, Copy)]
struct RecoveryRule {
    branch: RecoveryBranch,
    matches: fn(CrashState) -> bool,
}

const RECOVERY_RULES: [RecoveryRule; 9] = [
    rule(RecoveryBranch::CleanUnassignedPrefix, |s| {
        unassigned_prefix(s, TailState::Clean)
    }),
    rule(RecoveryBranch::TornUnassignedPrefix, |s| {
        unassigned_prefix(s, TailState::Torn)
    }),
    rule(RecoveryBranch::CleanAssignedSuffixDiscard, |s| {
        assigned_missing(s, TailState::Clean)
    }),
    rule(RecoveryBranch::TornAssignedSuffixDiscard, |s| {
        assigned_missing(s, TailState::Torn)
    }),
    rule(RecoveryBranch::CleanDurableTailReplay, |s| {
        durable_unpublished(s, TailState::Clean)
    }),
    rule(RecoveryBranch::TornDurablePrefixReplay, |s| {
        durable_unpublished(s, TailState::Torn)
    }),
    rule(RecoveryBranch::CleanPublishedPrefix, |s| {
        published_prefix(s, TailState::Clean)
    }),
    rule(RecoveryBranch::TornPublishedPrefix, |s| {
        published_prefix(s, TailState::Torn)
    }),
    rule(RecoveryBranch::RejectInteriorSequenceHole, |s| {
        s.durable_append_n == DurableAppendState::Missing
            && s.publish_n_plus_one == PublishState::Published
    }),
];

const fn rule(branch: RecoveryBranch, matches: fn(CrashState) -> bool) -> RecoveryRule {
    RecoveryRule { branch, matches }
}

fn unassigned_prefix(state: CrashState, tail: TailState) -> bool {
    state.assignment == AssignmentState::Unassigned
        && state.durable_append_n == DurableAppendState::Missing
        && state.publish_n_plus_one == PublishState::NotPublished
        && state.tail == tail
}

fn assigned_missing(state: CrashState, tail: TailState) -> bool {
    state.assignment == AssignmentState::Assigned
        && state.durable_append_n == DurableAppendState::Missing
        && state.publish_n_plus_one == PublishState::NotPublished
        && state.tail == tail
}

fn durable_unpublished(state: CrashState, tail: TailState) -> bool {
    state.durable_append_n == DurableAppendState::Landed
        && state.publish_n_plus_one == PublishState::NotPublished
        && state.tail == tail
}

fn published_prefix(state: CrashState, tail: TailState) -> bool {
    state.durable_append_n == DurableAppendState::Landed
        && state.publish_n_plus_one == PublishState::Published
        && state.tail == tail
}

fn classify(state: CrashState) -> RecoveryBranch {
    let matches = RECOVERY_RULES
        .iter()
        .filter(|rule| (rule.matches)(state))
        .map(|rule| rule.branch)
        .collect::<Vec<_>>();
    assert_eq!(
        matches.len(),
        1,
        "crash recovery state {state:?} matched {matches:?}; expected exactly one recovery branch"
    );
    matches[0]
}

#[test]
fn crash_recovery_decision_table_is_exhaustive_and_single_match() {
    let mut enumerated = Vec::new();
    for assignment in AssignmentState::ALL {
        for durable_append_n in DurableAppendState::ALL {
            for publish_n_plus_one in PublishState::ALL {
                for tail in TailState::ALL {
                    enumerated.push(CrashState {
                        assignment,
                        durable_append_n,
                        publish_n_plus_one,
                        tail,
                    });
                }
            }
        }
    }
    assert_eq!(enumerated.len(), CRASH_STATE_COUNT);

    for state in enumerated {
        let rows = CRASH_CASES
            .iter()
            .filter(|case| case.state == state)
            .collect::<Vec<_>>();
        assert_eq!(
            rows.len(),
            1,
            "crash state {state:?} has {} explicit table rows: {rows:?}",
            rows.len()
        );
        let case = rows[0];
        assert_eq!(
            classify(state),
            case.expected,
            "crash case {} selected the wrong recovery branch for {state:?}",
            case.name
        );
    }
}

#[derive(Default)]
struct TornTailFaultInjector {
    armed: AtomicBool,
    fired: AtomicBool,
}

impl TornTailFaultInjector {
    fn arm(&self) {
        self.armed.store(true, Ordering::Release);
    }
}

impl FaultInjector for TornTailFaultInjector {
    fn check(&self, point: FaultPoint) -> nimbus_core::Result<()> {
        if point == FaultPoint::JournalAppendBeforeDurableFlush
            && self.armed.load(Ordering::Acquire)
            && !self.fired.swap(true, Ordering::AcqRel)
        {
            return Err(Error::Internal(
                "injected torn journal tail before durable flush".to_string(),
            ));
        }
        Ok(())
    }
}

fn barrier_record(sequence: SequenceNumber, case_name: &str) -> TenantEventRecord {
    TenantEventRecord::from_events(
        sequence,
        Timestamp(50_000 + sequence.0),
        vec![TenantEventKind::Barrier {
            label: format!("crash-matrix-{case_name}"),
        }],
    )
    .expect("matrix barrier record should be valid")
}

async fn insert_case_document(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    case_name: &str,
) -> nimbus_core::Result<()> {
    timeout(
        Duration::from_secs(10),
        engine.insert_document_async(
            tenant_id.clone(),
            TableName::new("crash_matrix").expect("matrix table should be valid"),
            serde_json::Map::from_iter([("case".to_string(), json!(case_name))]),
        ),
    )
    .await
    .unwrap_or_else(|_| panic!("crash case {case_name}: mutation did not finish within 10s"))
    .map(|_| ())
}

fn assert_contiguous_prefix(case_name: &str, records: &[TenantEventRecord]) {
    for (index, record) in records.iter().enumerate() {
        let expected = u64::try_from(index).unwrap_or(u64::MAX).saturating_add(1);
        assert_eq!(
            record.sequence,
            SequenceNumber(expected),
            "crash case {case_name}: interior sequence hole reached durable storage at row {index}: {records:?}"
        );
    }
}

async fn run_crash_case(case: CrashCase) {
    assert_eq!(
        classify(case.state),
        case.expected,
        "crash case {} must enter its declared single recovery branch",
        case.name
    );

    let data_dir = tempdir().expect("crash matrix data directory should create");
    let storage_faults = Arc::new(TornTailFaultInjector::default());
    let engine = Arc::new(
        Engine::new_with_simulation_and_id_source(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(50_000))),
            storage_faults.clone(),
            Arc::new(SeededIdSource::new(50_000)),
        )
        .expect("crash matrix engine should create"),
    );
    let tenant_id = TenantId::new(format!("matrix-{}", case.name))
        .expect("crash matrix tenant id should be valid");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("crash matrix tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor must not add unrelated matrix records");
    insert_case_document(&engine, &tenant_id, "stable-prefix")
        .await
        .expect("stable prefix should commit");

    let runtime = engine
        .registered_runtime_for_testing(&tenant_id)
        .expect("matrix runtime should remain registered before injection");
    let baseline = runtime
        .store()
        .journal_progress()
        .expect("baseline journal progress should read");
    assert_eq!(baseline.durable_head, baseline.applied_head);
    let sequence_n = SequenceNumber(baseline.durable_head.0.saturating_add(1));

    match (
        case.state.assignment,
        case.state.durable_append_n,
        case.state.publish_n_plus_one,
    ) {
        (AssignmentState::Unassigned, DurableAppendState::Missing, _) => {}
        (AssignmentState::Unassigned, DurableAppendState::Landed, publish) => {
            runtime
                .store()
                .append_durable_records_batch(&[barrier_record(sequence_n, case.name)])
                .expect("unassigned durable record should model persisted crash evidence");
            if publish == PublishState::Published {
                runtime
                    .store()
                    .apply_durable_records_batch(&[barrier_record(sequence_n, case.name)])
                    .expect("published crash evidence should apply");
            }
        }
        (AssignmentState::Assigned, DurableAppendState::Missing, _) => {
            if case.state.tail == TailState::Torn {
                storage_faults.arm();
            } else {
                engine.commit_fault_handle_for_testing().inject(
                    labels::JOURNAL_ASSIGN_AFTER_STAGE,
                    Fault::Error(Error::Internal(
                        "injected crash after sequence assignment".to_string(),
                    )),
                );
            }
            insert_case_document(&engine, &tenant_id, case.name)
                .await
                .expect_err("assigned-but-missing crash point must fail the writer");
        }
        (AssignmentState::Assigned, DurableAppendState::Landed, publish) => {
            let label = match publish {
                PublishState::NotPublished => labels::DURABLE_BEFORE_PUBLISH,
                PublishState::Published => labels::POST_PUBLISH_PRE_FANOUT,
            };
            engine.commit_fault_handle_for_testing().inject(
                label,
                Fault::Error(Error::Internal(format!("injected crash for {}", case.name))),
            );
            insert_case_document(&engine, &tenant_id, case.name)
                .await
                .expect_err("durable crash point must fail the in-flight writer");
        }
    }

    if case.state.durable_append_n == DurableAppendState::Missing
        && case.state.publish_n_plus_one == PublishState::Published
    {
        let hole = barrier_record(SequenceNumber(sequence_n.0.saturating_add(1)), case.name);
        let error = runtime
            .store()
            .append_durable_records_batch(&[hole])
            .expect_err("publish N+1 without durable N must be rejected before storage");
        assert!(
            error.to_string().contains("expected sequence"),
            "crash case {}: interior-hole rejection should name the expected sequence: {error}",
            case.name
        );
    }

    if case.state.tail == TailState::Torn && !storage_faults.fired.load(Ordering::Acquire) {
        let progress = runtime
            .store()
            .journal_progress()
            .expect("pre-tail progress should read");
        storage_faults.arm();
        let error = runtime
            .store()
            .append_durable_records_batch(&[barrier_record(
                SequenceNumber(progress.durable_head.0.saturating_add(1)),
                case.name,
            )])
            .expect_err("torn-tail row must inject failure before durable flush");
        assert!(
            error.to_string().contains("torn journal tail"),
            "crash case {}: torn-tail fault should remain diagnostic: {error}",
            case.name
        );
    }

    let crashed_progress = runtime
        .store()
        .journal_progress()
        .expect("crashed journal progress should read");
    let expected_durable = match case.state.durable_append_n {
        DurableAppendState::Missing => baseline.durable_head,
        DurableAppendState::Landed => sequence_n,
    };
    let expected_applied = match (case.state.durable_append_n, case.state.publish_n_plus_one) {
        (DurableAppendState::Landed, PublishState::Published) => sequence_n,
        _ => baseline.applied_head,
    };
    assert_eq!(
        crashed_progress.durable_head, expected_durable,
        "crash case {} entered the wrong durable recovery branch",
        case.name
    );
    assert_eq!(
        crashed_progress.applied_head, expected_applied,
        "crash case {} entered the wrong publish recovery branch",
        case.name
    );
    let crashed_records = runtime
        .store()
        .read_durable_journal_from(SequenceNumber(1))
        .expect("crashed durable prefix should read");
    assert_contiguous_prefix(case.name, &crashed_records);

    timeout(Duration::from_secs(10), engine.quiesce())
        .await
        .unwrap_or_else(|_| panic!("crash case {}: engine quiesce exceeded 10s", case.name));
    drop(runtime);
    drop(engine);

    let recovered = Arc::new(
        Engine::new_with_simulation_and_id_source(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(60_000))),
            Arc::new(NoopFaultInjector),
            Arc::new(SeededIdSource::new(60_000)),
        )
        .expect("recovery engine should reopen"),
    );
    timeout(
        Duration::from_secs(10),
        recovered.get_existing_tenant_async_for_testing(&tenant_id),
    )
    .await
    .unwrap_or_else(|_| panic!("crash case {}: tenant recovery exceeded 10s", case.name))
    .expect("tenant runtime should recover");
    recovered
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("recovered trigger cursor should stop");
    let recovered_runtime = recovered
        .registered_runtime_for_testing(&tenant_id)
        .expect("recovered runtime should be registered");
    let recovered_progress = recovered_runtime
        .store()
        .journal_progress()
        .expect("recovered progress should read");
    assert_eq!(
        recovered_progress.durable_head, recovered_progress.applied_head,
        "crash case {}: recovery must apply the complete durable prefix",
        case.name
    );
    let recovered_records = recovered_runtime
        .store()
        .read_durable_journal_from(SequenceNumber(1))
        .expect("recovered durable prefix should read");
    assert_contiguous_prefix(case.name, &recovered_records);

    let recovered_horizon = recovered_progress.durable_head;
    insert_case_document(&recovered, &tenant_id, "after-recovery")
        .await
        .expect("post-recovery mutation should commit");
    let final_records = recovered_runtime
        .store()
        .read_durable_journal_from(SequenceNumber(1))
        .expect("final durable journal should read");
    assert_contiguous_prefix(case.name, &final_records);
    assert!(
        final_records
            .last()
            .is_some_and(|record| record.sequence > recovered_horizon),
        "crash case {}: sequence horizon {recovered_horizon} was reused after recovery: {final_records:?}",
        case.name
    );
}

macro_rules! crash_state_test {
    ($name:ident, $row:expr) => {
        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn $name() {
            run_crash_case(CRASH_CASES[$row]).await;
        }
    };
}

crash_state_test!(crash_state_unassigned_missing_unpublished_clean, 0);
crash_state_test!(crash_state_unassigned_missing_unpublished_torn, 1);
crash_state_test!(crash_state_unassigned_missing_published_clean, 2);
crash_state_test!(crash_state_unassigned_missing_published_torn, 3);
crash_state_test!(crash_state_unassigned_landed_unpublished_clean, 4);
crash_state_test!(crash_state_unassigned_landed_unpublished_torn, 5);
crash_state_test!(crash_state_unassigned_landed_published_clean, 6);
crash_state_test!(crash_state_unassigned_landed_published_torn, 7);
crash_state_test!(crash_state_assigned_missing_unpublished_clean, 8);
crash_state_test!(crash_state_assigned_missing_unpublished_torn, 9);
crash_state_test!(crash_state_assigned_missing_published_clean, 10);
crash_state_test!(crash_state_assigned_missing_published_torn, 11);
crash_state_test!(crash_state_assigned_landed_unpublished_clean, 12);
crash_state_test!(crash_state_assigned_landed_unpublished_torn, 13);
crash_state_test!(crash_state_assigned_landed_published_clean, 14);
crash_state_test!(crash_state_assigned_landed_published_torn, 15);
