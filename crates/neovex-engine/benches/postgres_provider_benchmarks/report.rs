use super::*;

pub(super) fn render_markdown(config: &BenchmarkConfig, report: &BenchmarkReport) -> String {
    let workloads = [
        WorkloadKind::CrudThroughput,
        WorkloadKind::PointReadLatency,
        WorkloadKind::IndexedQueryLatency,
        WorkloadKind::CompositeIndexedQueryLatency,
        WorkloadKind::DurableJournalStreamLatency,
        WorkloadKind::DurableJournalBootstrapLatency,
        WorkloadKind::SubscriptionBootstrapCatchupLatency,
        WorkloadKind::SubscriptionFanoutLatency,
        WorkloadKind::MixedMultiTenantLoad,
        WorkloadKind::TenantLifecycleLatency,
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
    markdown.push_str("# Postgres Provider Benchmark Report\n\n");
    markdown.push_str("Generated with:\n\n");
    markdown.push_str("```bash\n");
    markdown.push_str(
        "NEOVEX_BENCH_POSTGRES_URL='<connection-string>' make bench-postgres-provider REPORT=docs/research/postgres-provider-benchmark-report.md\n",
    );
    markdown.push_str("```\n\n");
    markdown.push_str("## Methodology\n\n");
    markdown.push_str(&format!(
        "- steady-state lane compares `sqlite` against `postgres (loopback)` with alternating backend order\n- cold-start lane compares `sqlite` against `postgres (loopback)` and includes fresh service open plus the first representative execution\n- RTT-sensitive lane compares `postgres (loopback)` against `postgres (injected RTT)` using a local TCP proxy that delays each forwarded chunk by `{}`\n- RTT-sensitive lanes use reduced representative sample sizes documented below so network sensitivity stays measurable without turning the readiness gate into an hours-long run\n- steady-state warmup rounds: `{}`; steady-state measured rounds: `{}`\n- cold-start warmup rounds: `{}`; cold-start measured rounds: `{}`\n- RTT warmup rounds: `{}`; RTT measured rounds: `{}`\n- 95% confidence intervals use a two-sided Student-t interval on mean per-operation latency\n",
        format_duration(config.rtt_delay),
        BenchmarkLane::SteadyState.warmup_rounds(),
        BenchmarkLane::SteadyState.measure_rounds(),
        BenchmarkLane::ColdStart.warmup_rounds(),
        BenchmarkLane::ColdStart.measure_rounds(),
        BenchmarkLane::RttSensitive.warmup_rounds(),
        BenchmarkLane::RttSensitive.measure_rounds(),
    ));
    markdown.push('\n');
    markdown.push_str("## Configuration\n\n");
    markdown.push_str(&format!(
        "- CRUD documents per steady/cold sample: `{CRUD_DOCUMENTS}`; RTT sample: `{CRUD_RTT_DOCUMENTS}`\n- point reads per steady/cold sample: `{POINT_READ_BATCH_SIZE}` over `{POINT_READ_DOCUMENTS}` seeded documents; RTT sample: `{POINT_READ_RTT_BATCH_SIZE}`\n- indexed queries per steady/cold sample: `{INDEXED_QUERY_BATCH_SIZE}` over `{INDEXED_QUERY_DOCUMENTS}` seeded documents; RTT sample: `{INDEXED_QUERY_RTT_BATCH_SIZE}`\n- journal dataset size: `{JOURNAL_DOCUMENTS}` writes with stream page limit `{JOURNAL_STREAM_LIMIT}`\n- subscription fan-out count: `{SUBSCRIPTION_FANOUT_COUNT}`\n- mixed-load steady/cold sample: `{MIXED_LOAD_TENANTS}` tenants with `{MIXED_LOAD_OPS_PER_TENANT}` ops per tenant; RTT sample: `{MIXED_LOAD_RTT_TENANTS}` tenants with `{MIXED_LOAD_RTT_OPS_PER_TENANT}` ops per tenant\n- standard Postgres pool config for benchmark fixtures: `min_connections=1`, `max_connections=4`\n- pool-pressure observation: `min_connections=1`, `max_connections={POOL_PRESSURE_MAX_CONNECTIONS}`, `{POOL_PRESSURE_TASKS}` concurrent workers running pure point reads\n- notification model assumption: one additional Postgres listener connection per live service process, outside the measured pool\n- control-plane assumption: tenant persistence may be Postgres-backed while the global usage/control path remains local redb\n",
    ));
    if workloads.contains(&WorkloadKind::TenantLifecycleLatency) {
        markdown.push_str(
            "- tenant-lifecycle sqlite contrast uses same-service open verification because the embedded redb control plane is single-open within one process; the Postgres lane uses a distinct peer service\n",
        );
    }
    if let Some(path) = &config.markdown_output {
        markdown.push_str(&format!("- report path: `{}`\n", path.display()));
    }
    if let Some(workload) = config.workload_filter {
        markdown.push_str(&format!("- workload filter: `{}`\n", workload.label()));
    }
    markdown.push('\n');

    if !workloads.is_empty() {
        let mut overall_postgres_wins = 0;
        let mut overall_sqlite_wins = 0;
        markdown.push_str("## SQLite Contrast Scorecard\n\n");
        markdown.push_str(
            "Winner is determined by higher median ops/s, which is equivalent here to lower median per-op latency.\n\n",
        );
        for lane in [BenchmarkLane::SteadyState, BenchmarkLane::ColdStart] {
            let mut postgres_wins = 0;
            let mut sqlite_wins = 0;
            markdown.push_str(&format!("### {} summary\n\n", lane.label()));
            markdown.push_str("| Workload | Postgres vs sqlite | Winner |\n");
            markdown.push_str("| --- | ---: | --- |\n");
            for workload in &workloads {
                let sqlite = measurement_for(report, *workload, lane, MeasuredBackend::Sqlite);
                let postgres =
                    measurement_for(report, *workload, lane, MeasuredBackend::PostgresLoopback);
                let ratio = postgres.stats().median_operations_per_second
                    / sqlite.stats().median_operations_per_second;
                let winner = if ratio > 1.0 {
                    postgres_wins += 1;
                    overall_postgres_wins += 1;
                    "postgres"
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
                "| Total lanes won | postgres {}, sqlite {} | {} |\n\n",
                postgres_wins,
                sqlite_wins,
                overall_contrast_winner_label(postgres_wins, sqlite_wins)
            ));
        }
        markdown.push_str("### Overall total\n\n");
        markdown.push_str("| Scope | Postgres lanes won | sqlite lanes won | Overall winner |\n");
        markdown.push_str("| --- | ---: | ---: | --- |\n");
        markdown.push_str(&format!(
            "| Loopback contrast lanes | {} | {} | {} |\n\n",
            overall_postgres_wins,
            overall_sqlite_wins,
            overall_contrast_winner_label(overall_postgres_wins, overall_sqlite_wins)
        ));
    }

    markdown.push_str("## RTT Sensitivity Scorecard\n\n");
    markdown.push_str("| Workload | Injected RTT vs loopback latency | Interpretation |\n");
    markdown.push_str("| --- | ---: | --- |\n");
    for workload in &workloads {
        let loopback = measurement_for(
            report,
            *workload,
            BenchmarkLane::RttSensitive,
            MeasuredBackend::PostgresLoopback,
        );
        let injected = measurement_for(
            report,
            *workload,
            BenchmarkLane::RttSensitive,
            MeasuredBackend::PostgresInjectedRtt,
        );
        let inflation = injected.stats().median_per_operation.as_secs_f64()
            / loopback
                .stats()
                .median_per_operation
                .as_secs_f64()
                .max(f64::MIN_POSITIVE);
        markdown.push_str(&format!(
            "| {} | {:.2}x | {} |\n",
            workload.label(),
            inflation,
            if inflation > 1.0 {
                "higher is worse; this is the steady-state sensitivity to non-zero RTT"
            } else {
                "at or below parity in this proxy setup"
            }
        ));
    }
    markdown.push('\n');

    for workload in workloads {
        markdown.push_str(&format!("## {}\n\n", workload.label()));
        markdown.push_str(&format!("{}\n\n", workload.notes()));
        render_lane_table(
            &mut markdown,
            report,
            workload,
            BenchmarkLane::SteadyState,
            &[MeasuredBackend::Sqlite, MeasuredBackend::PostgresLoopback],
        );
        render_lane_table(
            &mut markdown,
            report,
            workload,
            BenchmarkLane::ColdStart,
            &[MeasuredBackend::Sqlite, MeasuredBackend::PostgresLoopback],
        );
        render_lane_table(
            &mut markdown,
            report,
            workload,
            BenchmarkLane::RttSensitive,
            &[
                MeasuredBackend::PostgresLoopback,
                MeasuredBackend::PostgresInjectedRtt,
            ],
        );
    }

    if let Some(pool_pressure) = &report.pool_pressure {
        markdown.push_str("## Pool Pressure Observation\n\n");
        markdown.push_str(
            "This observation intentionally constrains the Postgres provider pool to expose head-of-line behavior and verify that active pooled backends remain bounded.\n\n",
        );
        markdown.push_str("| Samples | Max pooled backends observed | Configured max connections | Concurrent workers | Median sample latency | P95 sample latency | Mean sample latency |\n");
        markdown.push_str("| ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n");
        markdown.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n\n",
            pool_pressure.sample_count,
            pool_pressure.max_backends_observed,
            pool_pressure.configured_max_connections,
            pool_pressure.concurrent_tasks,
            format_duration(pool_pressure.median_sample_latency),
            format_duration(pool_pressure.p95_sample_latency),
            format_duration(pool_pressure.mean_sample_latency),
        ));
        if let Some(steady_mixed) = maybe_measurement_for(
            report,
            WorkloadKind::MixedMultiTenantLoad,
            BenchmarkLane::SteadyState,
            MeasuredBackend::PostgresLoopback,
        ) {
            let inflation = pool_pressure.median_sample_latency.as_secs_f64()
                / steady_mixed
                    .stats()
                    .median_per_operation
                    .as_secs_f64()
                    .max(f64::MIN_POSITIVE);
            markdown.push_str(&format!(
                "Relative to the unconstrained steady-state Postgres mixed-load lane, the bounded-pool observation shows `{:.2}x` higher median end-to-end sample latency while pooled backend count remained capped at `{}`.\n\n",
                inflation,
                pool_pressure.max_backends_observed,
            ));
        }
    }

    markdown.push_str("## Operator Assumptions\n\n");
    markdown.push_str(
        "- Postgres tenant persistence is benchmarked with the global usage/control path still local and redb-backed.\n- The service-path benchmark includes provider-owned pooling, typed construction, scheduler/journal semantics, and the provider hint-listener wake path, but notifications remain wake hints rather than the authoritative journal contract.\n- Companion operational drills for reconnect recovery, restart recovery, transient backend termination, unloaded-tenant scheduler wake, and tenant cleanup are covered by focused storage/engine verification and recorded in `/Users/jack/src/github.com/agentstation/neovex/docs/plans/archive/postgres-storage-provider-plan.md`.\n",
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

fn maybe_measurement_for(
    report: &BenchmarkReport,
    workload: WorkloadKind,
    lane: BenchmarkLane,
    backend: MeasuredBackend,
) -> Option<&WorkloadMeasurement> {
    report.measurements.iter().find(|measurement| {
        measurement.workload == workload
            && measurement.lane == lane
            && measurement.backend == backend
    })
}

fn overall_contrast_winner_label(postgres_wins: usize, sqlite_wins: usize) -> &'static str {
    use std::cmp::Ordering::*;

    match postgres_wins.cmp(&sqlite_wins) {
        Greater => "postgres",
        Less => "sqlite",
        Equal => "tie",
    }
}
