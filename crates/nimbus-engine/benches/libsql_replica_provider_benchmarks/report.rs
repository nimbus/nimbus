use super::common::{duration_ratio, format_confidence_interval, format_duration};
use super::config::{BenchmarkConfig, BenchmarkLane, WorkloadKind};
use super::models::{BenchmarkReport, MeasuredBackend, WorkloadMeasurement};
use super::*;

pub(super) fn render_markdown(config: &BenchmarkConfig, report: &BenchmarkReport) -> String {
    let contrast_workloads = [
        WorkloadKind::CrudThroughput,
        WorkloadKind::PointReadLatency,
        WorkloadKind::IndexedQueryLatency,
        WorkloadKind::CompositeIndexedQueryLatency,
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
    let operational_workloads = [
        WorkloadKind::BarrierRefreshLatency,
        WorkloadKind::PeerCatchUpLatency,
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
    markdown.push_str("# Replica-Connected SQLite Provider Benchmark Report\n\n");
    markdown.push_str("Generated with:\n\n");
    markdown.push_str("```bash\n");
    markdown.push_str("NIMBUS_LIBSQL_URL='http://127.0.0.1:18080' \\\n");
    markdown.push_str("NIMBUS_LIBSQL_ADMIN_URL='http://127.0.0.1:18081' \\\n");
    markdown.push_str("make bench-libsql-replica-provider");
    if !config.workload_filters.is_empty() {
        let workload_values = config
            .workload_filters
            .iter()
            .map(|workload| workload.cli_value())
            .collect::<Vec<_>>()
            .join(" ");
        markdown.push_str(" WORKLOADS='");
        markdown.push_str(&workload_values);
        markdown.push('\'');
    }
    if config.local_cache_encryption.is_enabled() {
        markdown.push_str(" ENCRYPTION=");
        markdown.push_str(config.local_cache_encryption.cli_value());
    }
    markdown.push_str(" REPORT=");
    markdown.push_str(
        config
            .markdown_output
            .as_deref()
            .unwrap_or_else(|| {
                Path::new("docs/research/libsql-replica-provider-benchmark-report.md")
            })
            .to_string_lossy()
            .as_ref(),
    );
    markdown.push('\n');
    markdown.push_str("```\n\n");
    markdown.push_str("## Methodology\n\n");
    markdown.push_str(&format!(
        "- local cache encryption mode: `{}`\n- steady-state lane compares embedded `sqlite` against `libsql replica` with alternating backend order\n- cold-start lane compares fresh service open plus the first representative execution for embedded `sqlite` and `libsql replica`\n- replica-operational lane measures the real freshness contract shipped today: same-service barrier refresh after a remote-primary write, plus peer catch-up / delegated-write visibility through the provider poll worker\n- steady-state warmup rounds: `{}`; steady-state measured rounds: `{}`\n- cold-start warmup rounds: `{}`; cold-start measured rounds: `{}`\n- replica-operational warmup rounds: `{}`; replica-operational measured rounds: `{}`\n- 95% confidence intervals use a two-sided Student-t interval on mean per-operation latency\n- encryption-enabled runs use one benchmark-only 32-byte master key file per benchmark process so local redb control state and replica-cache SQLite files both reopen through the same manifest-backed path during the benchmark\n",
        config.local_cache_encryption.cli_value(),
        BenchmarkLane::SteadyState.warmup_rounds(),
        BenchmarkLane::SteadyState.measure_rounds(),
        BenchmarkLane::ColdStart.warmup_rounds(),
        BenchmarkLane::ColdStart.measure_rounds(),
        BenchmarkLane::ReplicaOperational.warmup_rounds(),
        BenchmarkLane::ReplicaOperational.measure_rounds(),
    ));
    markdown.push('\n');
    markdown.push_str("## Configuration\n\n");
    markdown.push_str(&format!(
        "- CRUD documents per sample: `{CRUD_DOCUMENTS}`\n- point reads per sample: `{POINT_READ_BATCH_SIZE}` over `{POINT_READ_DOCUMENTS}` seeded documents\n- indexed queries per sample: `{INDEXED_QUERY_BATCH_SIZE}` over `{INDEXED_QUERY_DOCUMENTS}` seeded documents\n- mixed-load tenants: `{MIXED_LOAD_TENANTS}` with `{MIXED_LOAD_OPS_PER_TENANT}` ops per tenant per sample\n- peer catch-up timeout: `{}` with `{}` polling interval\n",
        PEER_CATCH_UP_TIMEOUT_SECS,
        format_duration(Duration::from_millis(PEER_CATCH_UP_POLL_INTERVAL_MS)),
    ));
    markdown.push_str(&format!(
        "- local cache encryption posture: `{}`\n- local cache encryption notes: {}\n",
        config.local_cache_encryption.label(),
        config.local_cache_encryption.notes(),
    ));
    if let Some(path) = &config.markdown_output {
        markdown.push_str(&format!("- report path: `{}`\n", path.display()));
    }
    if !config.workload_filters.is_empty() {
        let workload_labels = config
            .workload_filters
            .iter()
            .map(|workload| workload.label())
            .collect::<Vec<_>>()
            .join(", ");
        markdown.push_str(&format!("- workload filter: `{workload_labels}`\n"));
    }
    markdown.push('\n');

    if !contrast_workloads.is_empty() {
        let mut overall_sqlite_wins = 0;
        let mut overall_replica_wins = 0;
        markdown.push_str("## SQLite Contrast Scorecard\n\n");
        markdown.push_str(
            "Winner is determined by higher median ops/s, which is equivalent here to lower median per-op latency.\n\n",
        );
        for lane in [BenchmarkLane::SteadyState, BenchmarkLane::ColdStart] {
            let mut sqlite_wins = 0;
            let mut replica_wins = 0;
            markdown.push_str(&format!("### {} summary\n\n", lane.label()));
            markdown.push_str("| Workload | libsql replica vs sqlite | Winner |\n");
            markdown.push_str("| --- | ---: | --- |\n");
            for workload in &contrast_workloads {
                let sqlite = measurement_for(report, *workload, lane, MeasuredBackend::Sqlite);
                let replica =
                    measurement_for(report, *workload, lane, MeasuredBackend::LibsqlReplica);
                let ratio = replica.stats().median_operations_per_second
                    / sqlite.stats().median_operations_per_second;
                let winner = if ratio > 1.0 {
                    replica_wins += 1;
                    overall_replica_wins += 1;
                    "libsql replica"
                } else if ratio < 1.0 {
                    sqlite_wins += 1;
                    overall_sqlite_wins += 1;
                    "sqlite"
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
                "| Total lanes won | libsql replica {}, sqlite {} | {} |\n\n",
                replica_wins,
                sqlite_wins,
                overall_winner_label(replica_wins, sqlite_wins, "libsql replica", "sqlite")
            ));
        }
        markdown.push_str("### Overall total\n\n");
        markdown
            .push_str("| Scope | libsql replica lanes won | sqlite lanes won | Overall winner |\n");
        markdown.push_str("| --- | ---: | ---: | --- |\n");
        markdown.push_str(&format!(
            "| All contrast lanes | {} | {} | {} |\n\n",
            overall_replica_wins,
            overall_sqlite_wins,
            overall_winner_label(
                overall_replica_wins,
                overall_sqlite_wins,
                "libsql replica",
                "sqlite"
            )
        ));
    }

    for workload in &contrast_workloads {
        markdown.push_str(&format!("## {}\n\n", workload.label()));
        markdown.push_str(&format!("{}\n\n", workload.notes()));
        render_lane_table(
            &mut markdown,
            report,
            *workload,
            BenchmarkLane::SteadyState,
            &[MeasuredBackend::Sqlite, MeasuredBackend::LibsqlReplica],
        );
        render_lane_table(
            &mut markdown,
            report,
            *workload,
            BenchmarkLane::ColdStart,
            &[MeasuredBackend::Sqlite, MeasuredBackend::LibsqlReplica],
        );
    }

    if !operational_workloads.is_empty() {
        markdown.push_str("## Replica Freshness Drills\n\n");
        markdown.push_str(
            "These lanes are the operational readiness gate for the shipped replica contract. They are intentionally replica-only because embedded SQLite has no corresponding remote-primary barrier or peer catch-up path.\n\n",
        );
        markdown.push_str("| Drill | Samples | Median latency | P95 latency | Mean latency | 95% CI of mean | Result |\n");
        markdown.push_str("| --- | ---: | ---: | ---: | ---: | --- | --- |\n");
        for workload in &operational_workloads {
            let measurement = measurement_for(
                report,
                *workload,
                BenchmarkLane::ReplicaOperational,
                MeasuredBackend::LibsqlReplica,
            );
            let stats = measurement.stats();
            markdown.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | pass |\n",
                workload.label(),
                stats.sample_count,
                format_duration(stats.median_per_operation),
                format_duration(stats.p95_per_operation),
                format_duration(stats.mean_per_operation),
                format_confidence_interval(
                    stats.ci95_low_per_operation,
                    stats.ci95_high_per_operation,
                ),
            ));
        }
        markdown.push('\n');
    }

    markdown.push_str("## Operator Assumptions\n\n");
    markdown.push_str(
        "- Replica-connected SQLite tenant persistence is benchmarked with the global usage/control path still local and redb-backed.\n- The live freshness contract in this first slice is provider-owned cache refresh or poll-driven catch-up, not an ad hoc direct-primary query bypass from planner code.\n- The peer catch-up drill is the delegated-write readiness check for this family: one authoritative remote primary accepts the write, and another service becomes fresh only after the provider poll worker re-establishes journal/cache proof.\n",
    );

    markdown
}

fn render_lane_table(
    markdown: &mut String,
    report: &BenchmarkReport,
    workload: WorkloadKind,
    lane: BenchmarkLane,
    backends: &[MeasuredBackend],
) {
    markdown.push_str(&format!("### {} lane\n\n", lane.label()));
    markdown.push_str(&format!("{}\n\n", lane.notes()));
    markdown.push_str(
        "| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |\n",
    );
    markdown.push_str("| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |\n");
    for backend in backends {
        let measurement = measurement_for(report, workload, lane, *backend);
        let stats = measurement.stats();
        markdown.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {:.2}% | {} | {:.2} |\n",
            backend.label(),
            stats.sample_count,
            format_duration(stats.median_per_operation),
            format_duration(stats.p95_per_operation),
            format_duration(stats.mean_per_operation),
            format_duration(stats.stddev_per_operation),
            stats.cv_percent,
            format_confidence_interval(
                stats.ci95_low_per_operation,
                stats.ci95_high_per_operation,
            ),
            stats.median_operations_per_second,
        ));
    }
    markdown.push('\n');
    if backends.len() == 2 {
        let left = measurement_for(report, workload, lane, backends[0]);
        let right = measurement_for(report, workload, lane, backends[1]);
        let left_stats = left.stats();
        let right_stats = right.stats();
        markdown.push_str(&format!(
            "{} vs {} on the {} lane: `{:.2}x` median ops/s, `{:.2}x` median per-op latency\n\n",
            right.backend.label(),
            left.backend.label(),
            lane.label().to_lowercase(),
            right_stats.median_operations_per_second / left_stats.median_operations_per_second,
            duration_ratio(
                left_stats.median_per_operation,
                right_stats.median_per_operation
            ),
        ));
    }
}

fn measurement_for(
    report: &BenchmarkReport,
    workload: WorkloadKind,
    lane: BenchmarkLane,
    backend: MeasuredBackend,
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

fn overall_winner_label(
    primary_wins: usize,
    secondary_wins: usize,
    primary_label: &'static str,
    secondary_label: &'static str,
) -> &'static str {
    use std::cmp::Ordering::*;

    match primary_wins.cmp(&secondary_wins) {
        Greater => primary_label,
        Less => secondary_label,
        Equal => "tie",
    }
}
