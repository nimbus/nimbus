use nimbus_core::{Mutation, TableName};

use super::*;

fn scheduled_mutation() -> Mutation {
    Mutation::Insert {
        table: TableName::new("scheduled_tasks").expect("table name should parse"),
        id: None,
        fields: serde_json::Map::from_iter([("title".to_string(), json!("scheduled"))]),
    }
}

#[test]
fn mutation_execution_unit_run_at_preserves_requested_past_target() {
    let data_dir = tempdir().expect("scheduler execution-unit tempdir should build");
    let wall = Arc::new(ManualWallClock::new(Timestamp(10_000)));
    let engine = Arc::new(
        Engine::new_with_simulation(data_dir.path(), wall, Arc::new(NoopFaultInjector))
            .expect("engine should create"),
    );
    let tenant_id = TenantId::new("execution-unit-run-at").expect("tenant id should parse");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    let execution_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should begin");

    execution_unit
        .schedule_mutation_at(scheduled_mutation(), 1_000)
        .expect("past absolute schedule should stage");
    execution_unit
        .commit()
        .expect("schedule should commit atomically with its parent");

    let jobs = engine
        .list_scheduled_jobs(&tenant_id)
        .expect("scheduled jobs should list");
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].run_at, Timestamp(1_000));
}

#[test]
fn mutation_execution_unit_run_at_rolls_back_with_parent() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("execution-unit-run-at-rollback", Engine::create_tenant);

    let execution_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should begin");
    execution_unit
        .schedule_mutation_at(scheduled_mutation(), 1_000)
        .expect("absolute schedule should stage");
    drop(execution_unit);

    assert!(
        engine
            .list_scheduled_jobs(&tenant_id)
            .expect("scheduled jobs should list")
            .is_empty(),
        "dropping the parent transaction must discard its staged schedule"
    );
}
