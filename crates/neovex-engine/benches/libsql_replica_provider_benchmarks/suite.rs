use super::*;

pub(super) async fn run_suite(
    config: &BenchmarkConfig,
    environment: &BenchmarkEnvironment,
) -> BenchResult<BenchmarkReport> {
    let mut report = BenchmarkReport::default();
    let run = async {
        if should_run_workload(config, WorkloadKind::CrudThroughput) {
            benchmark_crud_throughput(environment, &mut report).await?;
        }
        if should_run_workload(config, WorkloadKind::PointReadLatency) {
            benchmark_point_read_latency(environment, &mut report).await?;
        }
        if should_run_workload(config, WorkloadKind::IndexedQueryLatency) {
            benchmark_indexed_query_latency(environment, &mut report).await?;
        }
        if should_run_workload(config, WorkloadKind::CompositeIndexedQueryLatency) {
            benchmark_composite_indexed_query_latency(environment, &mut report).await?;
        }
        if should_run_workload(config, WorkloadKind::MixedMultiTenantLoad) {
            benchmark_mixed_multi_tenant_load(environment, &mut report).await?;
        }
        if should_run_workload(config, WorkloadKind::BarrierRefreshLatency) {
            benchmark_barrier_refresh_latency(environment, &mut report).await?;
        }
        if should_run_workload(config, WorkloadKind::PeerCatchUpLatency) {
            benchmark_peer_catch_up_latency(environment, &mut report).await?;
        }
        Ok::<(), Box<dyn std::error::Error>>(())
    }
    .await;
    cleanup_registered_libsql_replica_providers().await;
    run?;
    Ok(report)
}

fn should_run_workload(config: &BenchmarkConfig, workload: WorkloadKind) -> bool {
    config
        .workload_filter
        .is_none_or(|selected| selected == workload)
}
