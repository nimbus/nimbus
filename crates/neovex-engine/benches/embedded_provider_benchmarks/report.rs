use super::*;

pub(super) fn render_markdown(config: &BenchmarkConfig, report: &BenchmarkReport) -> String {
    let workloads = [
        WorkloadKind::CrudThroughput,
        WorkloadKind::PointReadLatency,
        WorkloadKind::IndexedQueryLatency,
        WorkloadKind::CompositeIndexedQueryLatency,
        WorkloadKind::DurableJournalStreamLatency,
        WorkloadKind::DurableJournalBootstrapLatency,
        WorkloadKind::SubscriptionFanoutLatency,
        WorkloadKind::MixedMultiTenantLoad,
    ]
    .into_iter()
    .filter(|workload| {
        report
            .measurements
            .iter()
            .any(|measurement| measurement.workload == *workload)
    })
    .collect::<Vec<_>>();
    let mut markdown = String::new();
    markdown.push_str("# SQLite Storage Backend Benchmark Report\n\n");
    markdown.push_str("Generated with:\n\n");
    markdown.push_str("```bash\n");
    markdown.push_str(
        "make bench-embedded-providers REPORT=docs/research/sqlite-storage-benchmark-report.md\n",
    );
    markdown.push_str("```\n\n");
    markdown.push_str("## Methodology\n\n");
    markdown.push_str(&format!(
        "- backend order alternates every round inside each workload and lane: round 1 runs `redb -> sqlite`, round 2 runs `sqlite -> redb`, then repeats\n- steady-state warmup rounds: `{STEADY_STATE_WARMUP_ROUNDS}`; steady-state measured rounds: `{STEADY_STATE_MEASURE_ROUNDS}`\n- cold-start warmup rounds: `{COLD_START_WARMUP_ROUNDS}`; cold-start measured rounds: `{COLD_START_MEASURE_ROUNDS}`\n- cold-start read/query/journal lanes seed one canonical on-disk dataset per backend, clone that dataset before each sample, and then time only the fresh open plus first representative execution\n- 95% confidence intervals use a two-sided Student-t interval on mean per-operation latency\n- subscription cold-start includes fresh subscription registration/bootstrap because subscriptions are in-memory and do not survive reopen\n"
    ));
    markdown.push('\n');
    markdown.push_str("## Configuration\n\n");
    markdown.push_str(&format!(
        "- CRUD documents per sample: `{CRUD_DOCUMENTS}`\n- point reads per sample: `{POINT_READ_BATCH_SIZE}` over `{POINT_READ_DOCUMENTS}` seeded documents\n- indexed queries per sample: `{INDEXED_QUERY_BATCH_SIZE}` over `{INDEXED_QUERY_DOCUMENTS}` seeded documents\n- journal dataset size: `{JOURNAL_DOCUMENTS}` writes with stream page limit `{JOURNAL_STREAM_LIMIT}`\n- subscription fan-out count: `{SUBSCRIPTION_FANOUT_COUNT}`\n- mixed-load tenants: `{MIXED_LOAD_TENANTS}` with `{MIXED_LOAD_OPS_PER_TENANT}` ops per tenant per sample\n",
    ));
    if let Some(path) = &config.markdown_output {
        markdown.push_str(&format!("- report path: `{}`\n", path.display()));
    }
    if let Some(workload) = config.workload_filter {
        markdown.push_str(&format!("- workload filter: `{}`\n", workload.label()));
    }
    markdown.push('\n');

    if !workloads.is_empty() {
        let mut overall_sqlite_wins = 0;
        let mut overall_redb_wins = 0;
        markdown.push_str("## Winner Scorecard\n\n");
        markdown.push_str(
            "Winner is determined by higher median ops/s, which is equivalent here to lower\nmedian per-op latency.\n\n",
        );

        for lane in [BenchmarkLane::SteadyState, BenchmarkLane::ColdStart] {
            let mut sqlite_wins = 0;
            let mut redb_wins = 0;
            markdown.push_str(&format!("### {} summary\n\n", lane.label()));
            markdown.push_str("| Workload | SQLite vs redb | Winner |\n");
            markdown.push_str("| --- | ---: | --- |\n");
            for workload in &workloads {
                let redb = measurement_for(report, *workload, lane, EmbeddedProviderKind::Redb);
                let sqlite = measurement_for(report, *workload, lane, EmbeddedProviderKind::Sqlite);
                let ratio = sqlite.stats().median_operations_per_second
                    / redb.stats().median_operations_per_second;
                let winner = if ratio > 1.0 {
                    sqlite_wins += 1;
                    overall_sqlite_wins += 1;
                    "sqlite"
                } else if ratio < 1.0 {
                    redb_wins += 1;
                    overall_redb_wins += 1;
                    "redb"
                } else {
                    "tie"
                };
                markdown.push_str(&format!(
                    "| {} | {:.2}x | {} |\n",
                    workload.label(),
                    ratio,
                    winner
                ));
            }
            markdown.push_str(&format!(
                "| Total lanes won | sqlite {}, redb {} | {} |\n\n",
                sqlite_wins,
                redb_wins,
                overall_winner_label(sqlite_wins, redb_wins)
            ));
        }

        markdown.push_str("### Overall total\n\n");
        markdown.push_str("| Scope | SQLite lanes won | redb lanes won | Overall winner |\n");
        markdown.push_str("| --- | ---: | ---: | --- |\n");
        markdown.push_str(&format!(
            "| All measured lanes | {} | {} | {} |\n\n",
            overall_sqlite_wins,
            overall_redb_wins,
            overall_winner_label(overall_sqlite_wins, overall_redb_wins)
        ));
    }

    for workload in workloads {
        markdown.push_str(&format!("## {}\n\n", workload.label()));
        markdown.push_str(&format!("{}\n\n", workload.notes()));
        for lane in [BenchmarkLane::SteadyState, BenchmarkLane::ColdStart] {
            let redb = measurement_for(report, workload, lane, EmbeddedProviderKind::Redb);
            let sqlite = measurement_for(report, workload, lane, EmbeddedProviderKind::Sqlite);
            let redb_stats = redb.stats();
            let sqlite_stats = sqlite.stats();
            markdown.push_str(&format!("### {} lane\n\n", lane.label()));
            markdown.push_str(&format!("{}\n\n", lane.notes()));
            markdown.push_str(
                "| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |\n",
            );
            markdown.push_str("| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |\n");
            markdown.push_str(&format!(
                "| redb | {} | {} | {} | {} | {} | {:.2}% | {} | {:.2} |\n",
                redb_stats.sample_count,
                format_duration(redb_stats.median_per_operation),
                format_duration(redb_stats.p95_per_operation),
                format_duration(redb_stats.mean_per_operation),
                format_duration(redb_stats.stddev_per_operation),
                redb_stats.cv_percent,
                format_confidence_interval(
                    redb_stats.ci95_low_per_operation,
                    redb_stats.ci95_high_per_operation,
                ),
                redb_stats.median_operations_per_second,
            ));
            markdown.push_str(&format!(
                "| sqlite | {} | {} | {} | {} | {} | {:.2}% | {} | {:.2} |\n\n",
                sqlite_stats.sample_count,
                format_duration(sqlite_stats.median_per_operation),
                format_duration(sqlite_stats.p95_per_operation),
                format_duration(sqlite_stats.mean_per_operation),
                format_duration(sqlite_stats.stddev_per_operation),
                sqlite_stats.cv_percent,
                format_confidence_interval(
                    sqlite_stats.ci95_low_per_operation,
                    sqlite_stats.ci95_high_per_operation,
                ),
                sqlite_stats.median_operations_per_second,
            ));
            markdown.push_str(&format!(
                "SQLite vs redb on the {} lane: `{:.2}x` median ops/s, `{:.2}x` median per-op latency\n\n",
                lane.label().to_lowercase(),
                sqlite_stats.median_operations_per_second / redb_stats.median_operations_per_second,
                duration_ratio(
                    redb_stats.median_per_operation,
                    sqlite_stats.median_per_operation,
                ),
            ));
        }

        if let Some(plan) = report
            .sqlite_query_plans
            .iter()
            .find(|plan| plan.workload == workload)
        {
            markdown.push_str("### SQLite EXPLAIN QUERY PLAN\n\n");
            markdown.push_str(
                "Captured against the seeded SQLite benchmark dataset for this workload.\n\n",
            );
            markdown.push_str("```sql\n");
            markdown.push_str(plan.statement.trim());
            markdown.push_str("\n```\n\n");
            markdown.push_str("```text\n");
            for detail in &plan.detail_rows {
                markdown.push_str(detail);
                markdown.push('\n');
            }
            markdown.push_str("```\n\n");
        }
    }

    markdown
}

fn measurement_for(
    report: &BenchmarkReport,
    workload: WorkloadKind,
    lane: BenchmarkLane,
    backend: EmbeddedProviderKind,
) -> &WorkloadMeasurement {
    report
        .measurements
        .iter()
        .find(|measurement| {
            measurement.workload == workload
                && measurement.lane == lane
                && measurement.backend == backend
        })
        .expect("benchmark measurement should exist")
}

fn overall_winner_label(sqlite_wins: usize, redb_wins: usize) -> &'static str {
    use std::cmp::Ordering::*;

    match sqlite_wins.cmp(&redb_wins) {
        Greater => "sqlite",
        Less => "redb",
        Equal => "tie",
    }
}
