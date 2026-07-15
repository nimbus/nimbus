use super::concurrent_write_phase_split::{PhaseSplit, PhaseTotals, render_phase_split_section};

#[test]
fn default_phase_split_section_is_byte_empty() {
    assert_eq!(render_phase_split_section(&[]), "");
}

#[test]
fn phase_split_section_reports_plan_apply_and_fsync_shares() {
    let split = PhaseSplit::between(
        PhaseTotals::default(),
        PhaseTotals {
            prepare_nanos: 1_000_000,
            conflict_check_nanos: 1_000_000,
            apply_nanos: 2_000_000,
            publish_nanos: 1_000_000,
            durable_append_nanos: 5_000_000,
        },
    );
    let report = render_phase_split_section(&[(8, split)]);

    assert!(report.contains("## Under-gate phase split"));
    assert!(report.contains("| N | plan-CPU | apply | fsync/append |"));
    assert!(report.contains("| 8 | 20.0% | 30.0% | 50.0% | 10.000 ms |"));
}

#[test]
fn phase_split_delta_combines_prepare_and_conflict_check() {
    let before = PhaseTotals {
        prepare_nanos: 10,
        conflict_check_nanos: 20,
        apply_nanos: 30,
        publish_nanos: 5,
        durable_append_nanos: 40,
    };
    let after = PhaseTotals {
        prepare_nanos: 25,
        conflict_check_nanos: 45,
        apply_nanos: 65,
        publish_nanos: 5,
        durable_append_nanos: 85,
    };

    let report = render_phase_split_section(&[(1, PhaseSplit::between(before, after))]);
    assert!(report.contains("| 1 | 33.3% | 29.2% | 37.5% | 0.000 ms |"));
}
