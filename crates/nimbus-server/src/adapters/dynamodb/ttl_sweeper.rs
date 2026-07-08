//! Background TTL sweeper task.
//!
//! Transport-side glue only: on a fixed cadence it asks `nimbus-dynamodb` to
//! reclaim expired items across every bound tenant's tables (the sweep logic
//! and per-item REMOVE stream capture live in the adapter). The engine has no
//! TTL concept, so this adapter-owned task is what makes DynamoDB TTL actually
//! delete (DynamoDB Local never does).

use std::sync::Arc;
use std::time::Duration;

use nimbus_dynamodb::AccessKeyRegistry;
use nimbus_engine::Engine;
use tokio::time::{MissedTickBehavior, interval};
use tracing::{info, warn};

/// Current UNIX time in whole seconds (the unit DynamoDB TTL attributes use).
///
/// Pure plumbing: this loop is not itself unit-tested (it's a real
/// `tokio::spawn` background task on a real `tokio::time::interval`); the
/// clock-dependent unit that *is* tested is `sweep_all_tenants`, which
/// already takes `now: i64` as an explicit parameter. No struct here would
/// benefit from holding an injected `Arc<dyn Clock>`.
fn now_epoch_seconds() -> i64 {
    nimbus_core::clock::system_now_secs() as i64
}

/// Sweep expired items on `period` until the task is aborted. Per-tenant errors
/// are logged and never stop the schedule.
pub async fn run_ttl_sweeper(
    engine: Arc<Engine>,
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
        let (swept, errors) = nimbus_dynamodb::sweep_all_tenants(&engine, &access_keys, now);
        if swept > 0 {
            info!("DynamoDB TTL sweep reclaimed {swept} expired item(s)");
        }
        for (tenant, error) in errors {
            warn!("DynamoDB TTL sweep failed for tenant {tenant:?}: {error}");
        }
    }
}
