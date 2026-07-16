use super::concurrent_write_phase_split::{PhaseSplit, PhaseTotals, render_phase_split_section};

#[test]
fn default_phase_split_section_is_byte_empty() {
    assert_eq!(render_phase_split_section(&[]), "");
}

#[test]
fn phase_split_section_reports_plan_conflict_apply_and_fsync_shares() {
    let split = PhaseSplit::between(
        PhaseTotals::default(),
        PhaseTotals {
            journal_batch_size_sum: 16,
            journal_batch_count: 2,
            prepare_nanos: 1_000_000,
            conflict_check_nanos: 1_000_000,
            apply_nanos: 2_000_000,
            publish_nanos: 1_000_000,
            durable_append_nanos: 5_000_000,
        },
    );
    let report = render_phase_split_section(&[(8, split)]);

    assert!(report.contains("## Under-gate phase split"));
    assert!(report.contains("| N | avg effective batch | plan-CPU | conflict-check | apply |"));
    assert!(report.contains("| 8 | 8.00 | 10.0% | 10.0% | 30.0% | 50.0% | 10.000 ms |"));
}

#[test]
fn phase_split_delta_separates_prepare_from_conflict_check() {
    let before = PhaseTotals {
        journal_batch_size_sum: 3,
        journal_batch_count: 3,
        prepare_nanos: 10,
        conflict_check_nanos: 20,
        apply_nanos: 30,
        publish_nanos: 5,
        durable_append_nanos: 40,
    };
    let after = PhaseTotals {
        journal_batch_size_sum: 7,
        journal_batch_count: 7,
        prepare_nanos: 25,
        conflict_check_nanos: 45,
        apply_nanos: 65,
        publish_nanos: 5,
        durable_append_nanos: 85,
    };

    // Deltas: prepare 15, conflict-check 25, apply(+publish) 35, fsync 45 → total 120.
    let report = render_phase_split_section(&[(1, PhaseSplit::between(before, after))]);
    assert!(report.contains("| 1 | 1.00 | 12.5% | 20.8% | 29.2% | 37.5% | 0.000 ms |"));
}
