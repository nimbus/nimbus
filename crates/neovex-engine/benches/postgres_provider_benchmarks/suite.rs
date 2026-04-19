use super::*;

pub(super) async fn run_suite(
    config: &BenchmarkConfig,
    environment: &BenchmarkEnvironment,
) -> BenchResult<BenchmarkReport> {
    let mut report = BenchmarkReport::default();
    if should_run_workload(config, WorkloadKind::CrudThroughput) {
        benchmark_crud_throughput(config, environment, &mut report).await?;
    }
    if should_run_workload(config, WorkloadKind::PointReadLatency) {
        benchmark_point_read_latency(config, environment, &mut report).await?;
    }
    if should_run_workload(config, WorkloadKind::IndexedQueryLatency) {
        benchmark_indexed_query_latency(config, environment, &mut report).await?;
    }
    if should_run_workload(config, WorkloadKind::CompositeIndexedQueryLatency) {
        benchmark_composite_indexed_query_latency(config, environment, &mut report).await?;
    }
    if should_run_workload(config, WorkloadKind::DurableJournalStreamLatency) {
        benchmark_durable_journal_stream_latency(config, environment, &mut report).await?;
    }
    if should_run_workload(config, WorkloadKind::DurableJournalBootstrapLatency) {
        benchmark_durable_journal_bootstrap_latency(config, environment, &mut report).await?;
    }
    if should_run_workload(config, WorkloadKind::SubscriptionBootstrapCatchupLatency) {
        benchmark_subscription_bootstrap_catchup_latency(config, environment, &mut report).await?;
    }
    if should_run_workload(config, WorkloadKind::SubscriptionFanoutLatency) {
        benchmark_subscription_fanout_latency(config, environment, &mut report).await?;
    }
    if should_run_workload(config, WorkloadKind::MixedMultiTenantLoad) {
        benchmark_mixed_multi_tenant_load(config, environment, &mut report).await?;
    }
    if should_run_workload(config, WorkloadKind::TenantLifecycleLatency) {
        benchmark_tenant_lifecycle_latency(config, environment, &mut report).await?;
    }
    report.pool_pressure = Some(observe_pool_pressure(environment).await?);
    cleanup_registered_postgres_providers().await;
    Ok(report)
}

fn should_run_workload(config: &BenchmarkConfig, workload: WorkloadKind) -> bool {
    config
        .workload_filter
        .is_none_or(|selected| selected == workload)
}
