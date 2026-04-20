use super::*;

#[path = "workloads/crud.rs"]
mod crud;
#[path = "workloads/journal.rs"]
mod journal;
#[path = "workloads/reads.rs"]
mod reads;
#[path = "workloads/subscription.rs"]
mod subscription;
#[path = "workloads/tenant.rs"]
mod tenant;

pub(crate) use self::crud::benchmark_crud_throughput;
pub(crate) use self::journal::{
    benchmark_durable_journal_bootstrap_latency, benchmark_durable_journal_stream_latency,
};
pub(crate) use self::reads::{
    benchmark_composite_indexed_query_latency, benchmark_indexed_query_latency,
    benchmark_point_read_latency,
};
pub(crate) use self::subscription::{
    benchmark_subscription_bootstrap_catchup_latency, benchmark_subscription_fanout_latency,
};
pub(crate) use self::tenant::{
    benchmark_mixed_multi_tenant_load, benchmark_tenant_lifecycle_latency,
};
