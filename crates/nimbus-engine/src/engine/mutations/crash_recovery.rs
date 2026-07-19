//! Exhaustive PPSC5 crash-recovery decision table.
//!
//! The table is deliberately data, rather than a collection of hand-written
//! scenarios. Each closed axis supplies its variants to `ALL`; adding an axis
//! variant changes `CRASH_STATE_COUNT`, so the explicit table cannot keep
//! compiling until the new rows are supplied.

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
    RejectUnassignedDurable,
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
        expected: RecoveryBranch::RejectUnassignedDurable,
    },
    CrashCase {
        name: "unassigned_landed_unpublished_torn",
        state: state!(Unassigned, Landed, NotPublished, Torn),
        expected: RecoveryBranch::RejectUnassignedDurable,
    },
    CrashCase {
        name: "unassigned_landed_published_clean",
        state: state!(Unassigned, Landed, Published, Clean),
        expected: RecoveryBranch::RejectUnassignedDurable,
    },
    CrashCase {
        name: "unassigned_landed_published_torn",
        state: state!(Unassigned, Landed, Published, Torn),
        expected: RecoveryBranch::RejectUnassignedDurable,
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

const RECOVERY_RULES: [RecoveryRule; 10] = [
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
    rule(RecoveryBranch::RejectUnassignedDurable, |s| {
        s.assignment == AssignmentState::Unassigned
            && s.durable_append_n == DurableAppendState::Landed
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
    state.assignment == AssignmentState::Assigned
        && state.durable_append_n == DurableAppendState::Landed
        && state.publish_n_plus_one == PublishState::NotPublished
        && state.tail == tail
}

fn published_prefix(state: CrashState, tail: TailState) -> bool {
    state.assignment == AssignmentState::Assigned
        && state.durable_append_n == DurableAppendState::Landed
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
