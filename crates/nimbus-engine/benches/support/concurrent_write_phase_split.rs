#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PhaseTotals {
    pub(crate) journal_batch_size_sum: u64,
    pub(crate) journal_batch_count: u64,
    pub(crate) prepare_nanos: u64,
    pub(crate) conflict_check_nanos: u64,
    pub(crate) apply_nanos: u64,
    pub(crate) publish_nanos: u64,
    pub(crate) durable_append_nanos: u64,
    pub(crate) window_prepare_total: u64,
    pub(crate) storage_prepare_total: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PhaseSplit {
    journal_batch_size_sum: u64,
    journal_batch_count: u64,
    plan_cpu_nanos: u64,
    conflict_check_nanos: u64,
    apply_nanos: u64,
    fsync_append_nanos: u64,
    window_prepare_total: u64,
    storage_prepare_total: u64,
}

impl PhaseSplit {
    pub(crate) fn between(before: PhaseTotals, after: PhaseTotals) -> Self {
        Self {
            journal_batch_size_sum: after
                .journal_batch_size_sum
                .saturating_sub(before.journal_batch_size_sum),
            journal_batch_count: after
                .journal_batch_count
                .saturating_sub(before.journal_batch_count),
            plan_cpu_nanos: after.prepare_nanos.saturating_sub(before.prepare_nanos),
            conflict_check_nanos: after
                .conflict_check_nanos
                .saturating_sub(before.conflict_check_nanos),
            apply_nanos: after
                .apply_nanos
                .saturating_sub(before.apply_nanos)
                .saturating_add(after.publish_nanos.saturating_sub(before.publish_nanos)),
            fsync_append_nanos: after
                .durable_append_nanos
                .saturating_sub(before.durable_append_nanos),
            window_prepare_total: after
                .window_prepare_total
                .saturating_sub(before.window_prepare_total),
            storage_prepare_total: after
                .storage_prepare_total
                .saturating_sub(before.storage_prepare_total),
        }
    }

    fn total_nanos(self) -> u64 {
        self.plan_cpu_nanos
            .saturating_add(self.conflict_check_nanos)
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

    fn average_batch_size(self) -> f64 {
        if self.journal_batch_count == 0 {
            0.0
        } else {
            self.journal_batch_size_sum as f64 / self.journal_batch_count as f64
        }
    }
}

pub(crate) fn render_phase_split_section(rows: &[(usize, PhaseSplit)]) -> String {
    if rows.is_empty() {
        return String::new();
    }

    let mut out = String::from("\n## Commit phase split\n\n");
    out.push_str(
        "Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.\n\n",
    );
    out.push_str(
        "| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |\n",
    );
    out.push_str("|---|---|---|---|---|---|---|---|\n");
    for (n, split) in rows {
        out.push_str(&format!(
            "| {n} | {:.2} | {}/{} | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.3} ms |\n",
            split.average_batch_size(),
            split.window_prepare_total,
            split.storage_prepare_total,
            split.share(split.plan_cpu_nanos),
            split.share(split.conflict_check_nanos),
            split.share(split.apply_nanos),
            split.share(split.fsync_append_nanos),
            split.total_nanos() as f64 / 1_000_000.0,
        ));
    }
    out
}
