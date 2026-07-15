#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PhaseTotals {
    pub(crate) prepare_nanos: u64,
    pub(crate) conflict_check_nanos: u64,
    pub(crate) apply_nanos: u64,
    pub(crate) publish_nanos: u64,
    pub(crate) durable_append_nanos: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PhaseSplit {
    plan_cpu_nanos: u64,
    apply_nanos: u64,
    fsync_append_nanos: u64,
}

impl PhaseSplit {
    pub(crate) fn between(before: PhaseTotals, after: PhaseTotals) -> Self {
        Self {
            plan_cpu_nanos: after
                .prepare_nanos
                .saturating_sub(before.prepare_nanos)
                .saturating_add(
                    after
                        .conflict_check_nanos
                        .saturating_sub(before.conflict_check_nanos),
                ),
            apply_nanos: after
                .apply_nanos
                .saturating_sub(before.apply_nanos)
                .saturating_add(after.publish_nanos.saturating_sub(before.publish_nanos)),
            fsync_append_nanos: after
                .durable_append_nanos
                .saturating_sub(before.durable_append_nanos),
        }
    }

    fn total_nanos(self) -> u64 {
        self.plan_cpu_nanos
            .saturating_add(self.apply_nanos)
            .saturating_add(self.fsync_append_nanos)
    }

    fn share(self, nanos: u64) -> f64 {
        let total = self.total_nanos();
        if total == 0 {
            0.0
        } else {
            nanos as f64 / total as f64 * 100.0
        }
    }
}

pub(crate) fn render_phase_split_section(rows: &[(usize, PhaseSplit)]) -> String {
    if rows.is_empty() {
        return String::new();
    }

    let mut out = String::from("\n## Under-gate phase split\n\n");
    out.push_str(
        "Shares use measured-round committer wall time: `plan-CPU = prepare + conflict-check`; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`.\n\n",
    );
    out.push_str("| N | plan-CPU | apply | fsync/append | measured under-gate |\n");
    out.push_str("|---|---|---|---|---|\n");
    for (n, split) in rows {
        out.push_str(&format!(
            "| {n} | {:.1}% | {:.1}% | {:.1}% | {:.3} ms |\n",
            split.share(split.plan_cpu_nanos),
            split.share(split.apply_nanos),
            split.share(split.fsync_append_nanos),
            split.total_nanos() as f64 / 1_000_000.0,
        ));
    }
    out
}
