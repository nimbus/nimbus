use nimbus_core::{ManualMonotonicClock, MonotonicClock, WallClock};
use nimbus_storage::NoopFaultInjector;

use super::*;

#[test]
fn engine_simulation_constructs_all_runtimes_with_the_supplied_monotonic_clock() {
    let data_dir = tempdir().expect("clock construction tempdir should build");
    let wall = Arc::new(ManualWallClock::new(Timestamp(50_000)));
    let monotonic = Arc::new(ManualMonotonicClock::new());
    let engine = Engine::new_with_simulation_clocks(
        data_dir.path(),
        wall.clone(),
        monotonic.clone(),
        Arc::new(NoopFaultInjector),
    )
    .expect("engine should construct with independent clocks");
    let alpha = TenantId::new("clock-alpha").expect("tenant id should parse");
    let beta = TenantId::new("clock-beta").expect("tenant id should parse");
    engine
        .create_tenant(alpha.clone())
        .expect("alpha tenant should create");
    engine
        .create_tenant(beta.clone())
        .expect("beta tenant should create");

    let initial = monotonic.now();
    assert_eq!(
        engine
            .tenant_runtime_for_testing(&alpha)
            .expect("alpha runtime should load")
            .monotonic_now(),
        initial
    );
    assert_eq!(
        engine
            .tenant_runtime_for_testing(&beta)
            .expect("beta runtime should load")
            .monotonic_now(),
        initial
    );

    wall.set(Timestamp(1));
    let advanced = monotonic.advance(Duration::from_secs(3));
    let gamma = TenantId::new("clock-gamma").expect("tenant id should parse");
    engine
        .create_tenant(gamma.clone())
        .expect("gamma tenant should create after monotonic advancement");

    for tenant_id in [&alpha, &beta, &gamma] {
        assert_eq!(
            engine
                .tenant_runtime_for_testing(tenant_id)
                .expect("runtime should remain registered")
                .monotonic_now(),
            advanced
        );
    }
    assert_eq!(wall.now(), Timestamp(1));
}
