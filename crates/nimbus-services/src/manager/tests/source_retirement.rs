use nimbus_workloads::{WorkloadGeneration, WorkloadSagaRevision};

use super::*;

fn dynamic_service() -> (ServiceManager, TenantId, ServiceDefinition) {
    let tenant_id = TenantId::new("tenant").expect("tenant ID should validate");
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::new(),
        }),
        Arc::new(StubSandboxBackend::new(1)),
    );
    let definition = manager
        .create_service_definition(
            &tenant_id,
            "worker",
            image_service_backend("worker", "registry.example.com/worker:1"),
            BTreeMap::new(),
        )
        .expect("dynamic service definition should create");
    (manager, tenant_id, definition)
}

#[test]
fn exact_retirement_retry_adopts_the_newer_durable_saga_fence() {
    let (manager, tenant_id, definition) = dynamic_service();
    let first = manager
        .claim_service_definition_retirement(
            &tenant_id,
            "worker",
            definition.generation,
            &definition.resource_version,
            WorkloadSourceRetirementOperation::Stop,
            WorkloadGeneration::new(4),
            WorkloadSagaRevision::new(9),
        )
        .expect("first exact source claim should succeed");
    let advanced = manager
        .advance_source_retirement_claim_saga_fence(
            &first,
            WorkloadGeneration::new(5),
            WorkloadSagaRevision::new(0),
        )
        .expect("a new saga generation may reset its revision");

    let retry = manager
        .claim_service_definition_retirement(
            &tenant_id,
            "worker",
            definition.generation,
            &definition.resource_version,
            WorkloadSourceRetirementOperation::Stop,
            WorkloadGeneration::new(5),
            WorkloadSagaRevision::new(0),
        )
        .expect("an exact retry must adopt the advanced claim");
    assert_eq!(retry, advanced);

    let stale_retry = manager
        .claim_service_definition_retirement(
            &tenant_id,
            "worker",
            definition.generation,
            &definition.resource_version,
            WorkloadSourceRetirementOperation::Stop,
            WorkloadGeneration::new(4),
            WorkloadSagaRevision::new(9),
        )
        .expect("a stale exact retry must retain the newer claim");
    assert_eq!(stale_retry, advanced);

    let stale_high_revision = manager
        .claim_service_definition_retirement(
            &tenant_id,
            "worker",
            definition.generation,
            &definition.resource_version,
            WorkloadSourceRetirementOperation::Stop,
            WorkloadGeneration::new(4),
            WorkloadSagaRevision::new(11),
        )
        .expect("an older generation remains stale even with a higher revision");
    assert_eq!(stale_high_revision, advanced);

    let revised = manager
        .advance_source_retirement_claim_saga_fence(
            &advanced,
            WorkloadGeneration::new(5),
            WorkloadSagaRevision::new(1),
        )
        .expect("revision should advance within one generation");
    let backward_same_generation = manager.advance_source_retirement_claim_saga_fence(
        &revised,
        WorkloadGeneration::new(5),
        WorkloadSagaRevision::new(0),
    );
    assert!(matches!(
        backward_same_generation,
        Err(Error::PreconditionFailed(_))
    ));
}

#[test]
fn crossed_retirement_operation_cannot_adopt_an_existing_source_claim() {
    let (manager, tenant_id, definition) = dynamic_service();
    manager
        .claim_service_definition_retirement(
            &tenant_id,
            "worker",
            definition.generation,
            &definition.resource_version,
            WorkloadSourceRetirementOperation::Stop,
            WorkloadGeneration::new(4),
            WorkloadSagaRevision::new(9),
        )
        .expect("first exact source claim should succeed");

    let crossed = manager.claim_service_definition_retirement(
        &tenant_id,
        "worker",
        definition.generation,
        &definition.resource_version,
        WorkloadSourceRetirementOperation::DeleteDefinition { force: true },
        WorkloadGeneration::new(5),
        WorkloadSagaRevision::new(10),
    );
    assert!(matches!(crossed, Err(Error::Conflict { .. })));
    assert_eq!(
        manager
            .service_definition_for_tenant(&tenant_id, "worker")
            .expect("crossed claim must retain the desired source"),
        definition
    );
}

#[test]
fn retirement_claim_fences_source_update_and_start_reservation() {
    let (manager, tenant_id, definition) = dynamic_service();
    manager
        .claim_service_definition_retirement(
            &tenant_id,
            "worker",
            definition.generation,
            &definition.resource_version,
            WorkloadSourceRetirementOperation::Stop,
            WorkloadGeneration::new(4),
            WorkloadSagaRevision::new(9),
        )
        .expect("source claim should succeed");

    let prepared = manager
        .prepare_sandbox_service_provision_source(&tenant_id, "worker")
        .expect("read-only source preparation should remain available");
    let decision = TenantIsolationContext::system(tenant_id.clone(), "service.provision")
        .with_deployment_generation(definition.generation)
        .admit_decision(prepared.policy_input().clone())
        .expect("fixture source should admit");
    let reserved = manager.reserve_sandbox_service_provision_source(&decision, prepared);
    assert!(matches!(reserved, Err(Error::Conflict { .. })));

    let updated = manager.update_service_definition(
        &tenant_id,
        "worker",
        definition.generation,
        image_service_backend("worker", "registry.example.com/worker:2"),
        BTreeMap::new(),
    );
    assert!(matches!(updated, Err(Error::Conflict { .. })));
    assert_eq!(
        manager
            .service_definition_for_tenant(&tenant_id, "worker")
            .expect("retirement fence must retain the exact desired source"),
        definition
    );
}

#[test]
fn advanced_claim_cannot_finalize_unstarted_source_stop() {
    let (manager, tenant_id, definition) = dynamic_service();
    let claim = manager
        .claim_service_definition_retirement(
            &tenant_id,
            "worker",
            definition.generation,
            &definition.resource_version,
            WorkloadSourceRetirementOperation::Stop,
            WorkloadGeneration::new(0),
            WorkloadSagaRevision::new(0),
        )
        .expect("lower-bound source claim should succeed");
    let advanced = manager
        .advance_source_retirement_claim_saga_fence(
            &claim,
            WorkloadGeneration::new(1),
            WorkloadSagaRevision::new(2),
        )
        .expect("durable saga progress should advance the claim");

    assert!(matches!(
        manager.finalize_unstarted_source_stop(&advanced),
        Err(Error::PreconditionFailed(_))
    ));
    let prepared = manager
        .prepare_sandbox_service_provision_source(&tenant_id, "worker")
        .expect("rejected finalization must retain source bytes");
    let decision = TenantIsolationContext::system(tenant_id.clone(), "service.provision")
        .with_deployment_generation(definition.generation)
        .admit_decision(prepared.policy_input().clone())
        .expect("retained fixture source should admit");
    assert!(matches!(
        manager.reserve_sandbox_service_provision_source(&decision, prepared),
        Err(Error::Conflict { .. })
    ));
    assert_eq!(
        manager
            .service_definition_for_tenant(&tenant_id, "worker")
            .expect("advanced claim must retain the exact desired source"),
        definition
    );
}

#[test]
fn advanced_claim_cannot_finalize_unstarted_definition_deletion() {
    let (manager, tenant_id, definition) = dynamic_service();
    let claim = manager
        .claim_service_definition_retirement(
            &tenant_id,
            "worker",
            definition.generation,
            &definition.resource_version,
            WorkloadSourceRetirementOperation::DeleteDefinition { force: true },
            WorkloadGeneration::new(0),
            WorkloadSagaRevision::new(0),
        )
        .expect("lower-bound deletion claim should succeed");
    let advanced = manager
        .advance_source_retirement_claim_saga_fence(
            &claim,
            WorkloadGeneration::new(1),
            WorkloadSagaRevision::new(2),
        )
        .expect("durable saga progress should advance the claim");

    assert!(matches!(
        manager.finalize_unstarted_service_definition_deletion(&advanced),
        Err(Error::PreconditionFailed(_))
    ));
    assert_eq!(
        manager
            .service_definition_for_tenant(&tenant_id, "worker")
            .expect("advanced claim must retain the exact desired definition"),
        definition
    );
    let crossed = manager.claim_service_definition_retirement(
        &tenant_id,
        "worker",
        definition.generation,
        &definition.resource_version,
        WorkloadSourceRetirementOperation::Stop,
        WorkloadGeneration::new(0),
        WorkloadSagaRevision::new(0),
    );
    assert!(matches!(crossed, Err(Error::Conflict { .. })));
}

#[test]
fn definition_finalization_rejects_crossed_source_or_session_set() {
    let (manager, tenant_id, definition) = dynamic_service();
    let claim = manager
        .claim_service_definition_retirement(
            &tenant_id,
            "worker",
            definition.generation,
            &definition.resource_version,
            WorkloadSourceRetirementOperation::DeleteDefinition { force: true },
            WorkloadGeneration::new(4),
            WorkloadSagaRevision::new(9),
        )
        .expect("exact deletion claim should succeed");
    super::super::source_retirement::authenticate_definition_finalization(&definition, &[], &claim)
        .expect("exact source and captured session set should authenticate");

    let mut crossed_definition = definition.clone();
    crossed_definition.resource_version = "crossed-resource-version".to_owned();
    assert!(matches!(
        super::super::source_retirement::authenticate_definition_finalization(
            &crossed_definition,
            &[],
            &claim,
        ),
        Err(Error::PreconditionFailed(_))
    ));
    assert!(matches!(
        super::super::source_retirement::authenticate_definition_finalization(
            &definition,
            &["late-session".to_owned()],
            &claim,
        ),
        Err(Error::PreconditionFailed(_))
    ));
    assert_eq!(
        manager
            .service_definition_for_tenant(&tenant_id, "worker")
            .expect("rejected finalization inputs must retain desired source"),
        definition
    );
}
