//! Background TTL sweeper task.
//!
//! Transport-side glue only: on a fixed cadence it asks `nimbus-dynamodb` to
//! reclaim expired items across every bound tenant's tables (the sweep logic
//! and per-item REMOVE stream capture live in the adapter). The engine has no
//! TTL concept, so this adapter-owned task is what makes DynamoDB TTL actually
//! delete (DynamoDB Local never does).

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nimbus_dynamodb::AccessKeyRegistry;
use nimbus_engine::Service;
use tokio::time::{MissedTickBehavior, interval};
use tracing::{info, warn};

/// Current UNIX time in whole seconds (the unit DynamoDB TTL attributes use).
fn now_epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

/// Sweep expired items on `period` until the task is aborted. Per-tenant errors
/// are logged and never stop the schedule.
pub async fn run_ttl_sweeper(
    service: Arc<Service>,
    access_keys: Arc<AccessKeyRegistry>,
    period: Duration,
) {
    info!("DynamoDB TTL sweeper started (interval {:?})", period);
    let mut ticker = interval(period);
    // A slow sweep must not let missed ticks pile into a burst.
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        let now = now_epoch_seconds();
        let (swept, errors) = nimbus_dynamodb::sweep_all_tenants(&service, &access_keys, now);
        if swept > 0 {
            info!("DynamoDB TTL sweep reclaimed {swept} expired item(s)");
        }
        for (tenant, error) in errors {
            warn!("DynamoDB TTL sweep failed for tenant {tenant:?}: {error}");
        }
    }
}
