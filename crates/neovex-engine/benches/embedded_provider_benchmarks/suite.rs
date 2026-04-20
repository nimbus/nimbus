use super::config::{BenchmarkConfig, WorkloadKind};
use super::models::BenchmarkReport;
use super::workloads::{
    benchmark_composite_indexed_query_latency, benchmark_crud_throughput,
    benchmark_durable_journal_bootstrap_latency, benchmark_durable_journal_stream_latency,
    benchmark_indexed_query_latency, benchmark_mixed_multi_tenant_load,
    benchmark_point_read_latency, benchmark_subscription_fanout_latency,
};
use super::*;

pub(super) async fn run_suite(config: &BenchmarkConfig) -> BenchResult<BenchmarkReport> {
    let mut report = BenchmarkReport::default();
    if should_run_workload(config, WorkloadKind::CrudThroughput) {
        report.extend(run_workload(WorkloadKind::CrudThroughput, benchmark_crud_throughput).await?);
    }
    if should_run_workload(config, WorkloadKind::PointReadLatency) {
        report.extend(
            run_workload(WorkloadKind::PointReadLatency, benchmark_point_read_latency).await?,
        );
    }
    if should_run_workload(config, WorkloadKind::IndexedQueryLatency) {
        report.extend(
            run_workload(
                WorkloadKind::IndexedQueryLatency,
                benchmark_indexed_query_latency,
            )
            .await?,
        );
    }
    if should_run_workload(config, WorkloadKind::CompositeIndexedQueryLatency) {
        report.extend(
            run_workload(
                WorkloadKind::CompositeIndexedQueryLatency,
                benchmark_composite_indexed_query_latency,
            )
            .await?,
        );
    }
    if should_run_workload(config, WorkloadKind::DurableJournalStreamLatency) {
        report.extend(
            run_workload(
                WorkloadKind::DurableJournalStreamLatency,
                benchmark_durable_journal_stream_latency,
            )
            .await?,
        );
    }
    if should_run_workload(config, WorkloadKind::DurableJournalBootstrapLatency) {
        report.extend(
            run_workload(
                WorkloadKind::DurableJournalBootstrapLatency,
                benchmark_durable_journal_bootstrap_latency,
            )
            .await?,
        );
    }
    if should_run_workload(config, WorkloadKind::SubscriptionFanoutLatency) {
        report.extend(
            run_workload(
                WorkloadKind::SubscriptionFanoutLatency,
                benchmark_subscription_fanout_latency,
            )
            .await?,
        );
    }
    if should_run_workload(config, WorkloadKind::MixedMultiTenantLoad) {
        report.extend(
            run_workload(
                WorkloadKind::MixedMultiTenantLoad,
                benchmark_mixed_multi_tenant_load,
            )
            .await?,
        );
    }
    Ok(report)
}

fn should_run_workload(config: &BenchmarkConfig, workload: WorkloadKind) -> bool {
    config
        .workload_filter
        .is_none_or(|selected| selected == workload)
}

async fn run_workload<F, Fut>(
    workload: WorkloadKind,
    run: F,
) -> BenchResult<super::models::WorkloadOutcome>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = BenchResult<super::models::WorkloadOutcome>>,
{
    eprintln!("starting {}", workload.label());
    let started = Instant::now();
    let outcome = run().await?;
    eprintln!("finished {} in {:?}", workload.label(), started.elapsed());
    Ok(outcome)
}
