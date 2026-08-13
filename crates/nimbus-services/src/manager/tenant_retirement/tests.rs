use std::collections::BTreeMap;
use std::num::NonZeroU64;
use std::sync::Arc;

use nimbus_core::{TenantId, WorkloadId};
use nimbus_network::{
    NetworkAddressFamily, NetworkAttachmentCapabilitySet, NetworkBindRealmKind,
    NetworkCapabilityRequirements, NetworkControlPlaneLocality, NetworkEndpointCapabilitySet,
    NetworkExposure, NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
    NetworkLifecycleCapabilitySet, NetworkManagementMode, NetworkPortAssignmentMode,
    NetworkResourceGeneration, NetworkSovereigntyRequirements, PortProtocol,
};
use nimbus_sandbox::SandboxBackend;
use nimbus_tenant::TenantIsolationContext;
use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, DesiredWorkloadKind, DesiredWorkloadState, NodeIdentity,
    WorkloadActivationIntent, WorkloadAdmissionEvidence, WorkloadExecutableEncoding,
    WorkloadExecutableIntent, WorkloadGeneration, WorkloadNetworkIntent,
    WorkloadNetworkPlanContent, WorkloadNetworkPlanIdentity, WorkloadProvisionSourceEvidence,
    WorkloadProvisionSourceGeneration, WorkloadProvisionSourceIdentity,
    WorkloadProvisionSourceKind, WorkloadProvisionSourceResourceVersion, WorkloadPublicationIntent,
    WorkloadSagaIntent, WorkloadSagaKey, WorkloadSagaPhase, WorkloadSagaRecord,
};

use super::*;
use crate::SessionTarget;
use crate::manager::tests::{
    StubSandboxBackend, StubServiceDefinitionCatalog, execution_reference_for_handle,
    image_service_backend, reserve_standalone_source, standalone_resource_spec,
};
use crate::manager::types::TenantSandboxResourceKey;

struct Fixture {
    manager: ServiceManager,
    backend: Arc<StubSandboxBackend>,
    tenant_id: TenantId,
}

impl Fixture {
    fn with_sources() -> Self {
        let tenant_id = TenantId::new("tenant").expect("tenant id should validate");
        let backend = Arc::new(StubSandboxBackend::new(1));
        let manager = ServiceManager::new(
            Arc::new(StubServiceDefinitionCatalog {
                launches: BTreeMap::new(),
            }),
            backend.kind(),
        );
        let definition = manager
            .create_service_definition(
                &tenant_id,
                "worker",
                image_service_backend("worker", "registry.example.com/worker:1"),
                BTreeMap::new(),
            )
            .expect("service definition should create");
        let mut service_handle =
            backend.sandbox_handle(&tenant_id, "worker", nimbus_sandbox::SandboxStatus::Ready);
        let service_execution =
            execution_reference_for_handle(&mut service_handle, definition.generation, 0);
        manager
            .project_service_definition_execution_observation(
                &tenant_id,
                "worker",
                definition.generation,
                &definition.resource_version,
                &service_execution,
                service_handle,
            )
            .expect("service observation should project");

        let sandbox = reserve_standalone_source(
            &manager,
            &tenant_id,
            "task",
            "worker-profile",
            standalone_resource_spec(&tenant_id, "task"),
            BTreeMap::new(),
        );
        let mut sandbox_handle =
            backend.sandbox_handle(&tenant_id, "task", nimbus_sandbox::SandboxStatus::Ready);
        let sandbox_execution =
            execution_reference_for_handle(&mut sandbox_handle, sandbox.generation, 0);
        manager
            .project_sandbox_resource_execution_observation(
                &tenant_id,
                &sandbox.id,
                sandbox.generation,
                &sandbox.resource_version,
                &sandbox_execution,
                sandbox_handle,
            )
            .expect("sandbox observation should project");

        manager
            .create_service_definition(
                &tenant_id,
                "idle",
                image_service_backend("idle", "registry.example.com/idle:1"),
                BTreeMap::new(),
            )
            .expect("unstarted definition should create");

        Self {
            manager,
            backend,
            tenant_id,
        }
    }

    fn claim(&self) -> TenantSourceRetirementSnapshot {
        self.manager
            .claim_tenant_source_retirement(
                &self.tenant_id,
                NonZeroU64::new(7).expect("fixture incarnation is nonzero"),
            )
            .expect("tenant source retirement should claim")
    }
}

#[test]
fn tenant_retirement_barrier_replays_exactly_and_rejects_crossed_incarnation() {
    let fixture = Fixture::with_sources();
    let first = fixture.claim();
    let replay = fixture.claim();
    assert_eq!(replay, first);
    assert_eq!(first.claim().tenant_id(), &fixture.tenant_id);
    assert_eq!(first.claim().tenant_incarnation().get(), 7);
    assert_eq!(first.sources().len(), 3);
    assert_eq!(
        first
            .sources()
            .iter()
            .filter(|source| source.has_observation())
            .count(),
        2
    );

    let crossed = fixture.manager.claim_tenant_source_retirement(
        &fixture.tenant_id,
        NonZeroU64::new(8).expect("fixture incarnation is nonzero"),
    );
    assert!(matches!(crossed, Err(Error::Conflict { .. })));
    assert_eq!(fixture.claim(), first);
    assert!(
        fixture
            .manager
            .release_tenant_source_retirement(first.claim())
            .is_err()
    );
    assert_eq!(fixture.claim(), first);
}

#[test]
fn fresh_manager_restores_exact_durable_barrier_before_admission() {
    let original = Fixture::with_sources();
    let snapshot = original.claim();
    let record = TenantRetirementRecord::new(
        original.tenant_id.clone(),
        snapshot.claim().tenant_incarnation(),
        snapshot.sources().to_vec(),
    )
    .expect("durable retirement should validate");

    let fresh = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::new(),
        }),
        original.backend.kind(),
    );
    assert_eq!(
        fresh
            .restore_tenant_source_retirement(&record)
            .expect("fresh manager should restore the durable barrier"),
        snapshot
    );
    assert_eq!(
        fresh
            .restore_tenant_source_retirement(&record)
            .expect("exact restoration should replay"),
        snapshot
    );
    assert!(matches!(
        fresh.create_service_definition(
            &original.tenant_id,
            "late",
            image_service_backend("late", "registry.example.com/late:1"),
            BTreeMap::new(),
        ),
        Err(Error::Conflict { .. })
    ));

    let crossed = TenantRetirementRecord::new(
        original.tenant_id.clone(),
        NonZeroU64::new(8).expect("fixture incarnation is nonzero"),
        snapshot.sources().to_vec(),
    )
    .expect("crossed durable retirement should validate alone");
    assert!(matches!(
        fresh.restore_tenant_source_retirement(&crossed),
        Err(Error::Conflict { .. })
    ));
    assert_eq!(original.backend.retirement_effect_counts(), (0, 0, 0));
}

#[test]
fn restored_finalized_barrier_releases_only_after_exact_terminal_progress() {
    let original = Fixture::with_sources();
    let snapshot = original.claim();
    let record = TenantRetirementRecord::new(
        original.tenant_id.clone(),
        snapshot.claim().tenant_incarnation(),
        snapshot.sources().to_vec(),
    )
    .unwrap()
    .advance(TenantRetirementPhase::ChildrenRecorded)
    .unwrap()
    .advance(TenantRetirementPhase::SourcesFinalized)
    .unwrap();
    let fresh = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::new(),
        }),
        original.backend.kind(),
    );
    let restored = fresh
        .restore_tenant_source_retirement(&record)
        .expect("finalized durable progress should restore a finalized barrier");
    fresh
        .release_tenant_source_retirement(restored.claim())
        .expect("the exact finalized durable barrier should release");
    fresh
        .create_service_definition(
            &original.tenant_id,
            "recreated",
            image_service_backend("recreated", "registry.example.com/recreated:1"),
            BTreeMap::new(),
        )
        .expect("admission should reopen after exact release");
    assert_eq!(original.backend.retirement_effect_counts(), (0, 0, 0));
}

#[test]
fn restore_rejects_current_source_created_after_durable_snapshot() {
    let original = Fixture::with_sources();
    let snapshot = original.claim();
    let record = TenantRetirementRecord::new(
        original.tenant_id.clone(),
        snapshot.claim().tenant_incarnation(),
        snapshot.sources().to_vec(),
    )
    .unwrap();
    original
        .manager
        .state
        .lock()
        .expect("manager lock should not be poisoned")
        .tenant_source_retirements
        .clear();
    original
        .manager
        .create_service_definition(
            &original.tenant_id,
            "late",
            image_service_backend("late", "registry.example.com/late:1"),
            BTreeMap::new(),
        )
        .expect("fixture should create a source after the captured snapshot");

    assert!(matches!(
        original.manager.restore_tenant_source_retirement(&record),
        Err(Error::PreconditionFailed(_))
    ));
    assert_eq!(original.backend.retirement_effect_counts(), (0, 0, 0));
}

#[test]
fn tenant_retirement_barrier_retains_snapshot_failure_and_keeps_admission_closed() {
    let fixture = Fixture::with_sources();
    let crossed_source = crate::SandboxResourceSource::new(
        fixture.tenant_id.clone(),
        "worker",
        "overlapping-profile",
        standalone_resource_spec(&fixture.tenant_id, "worker"),
        1,
        1,
        BTreeMap::new(),
    );
    fixture
        .manager
        .state
        .lock()
        .expect("manager lock should not be poisoned")
        .sandbox_resource_sources
        .insert(
            TenantSandboxResourceKey::new(&fixture.tenant_id, "worker"),
            crossed_source,
        );

    let first = fixture.manager.claim_tenant_source_retirement(
        &fixture.tenant_id,
        NonZeroU64::new(7).expect("fixture incarnation is nonzero"),
    );
    assert!(matches!(first, Err(Error::Internal(_))));
    let replay = fixture.manager.claim_tenant_source_retirement(
        &fixture.tenant_id,
        NonZeroU64::new(7).expect("fixture incarnation is nonzero"),
    );
    assert!(matches!(replay, Err(Error::PreconditionFailed(_))));
    let crossed = fixture.manager.claim_tenant_source_retirement(
        &fixture.tenant_id,
        NonZeroU64::new(8).expect("fixture incarnation is nonzero"),
    );
    assert!(matches!(crossed, Err(Error::Conflict { .. })));
    assert!(matches!(
        fixture.manager.create_service_definition(
            &fixture.tenant_id,
            "late-service",
            image_service_backend("late-service", "registry.example.com/late:1"),
            BTreeMap::new(),
        ),
        Err(Error::Conflict { .. })
    ));
    assert_eq!(fixture.backend.retirement_effect_counts(), (0, 0, 0));
}

#[test]
fn sandbox_backed_service_and_standalone_sandbox_cannot_share_workload_id() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should validate");

    let service_first = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::new(),
        }),
        nimbus_sandbox::SandboxBackendKind::Krun,
    );
    service_first
        .create_service_definition(
            &tenant_id,
            "worker",
            image_service_backend("worker", "registry.example.com/worker:1"),
            BTreeMap::new(),
        )
        .expect("sandbox-backed service should create");
    let before = manager_state_fingerprint(&service_first);
    assert!(matches!(
        service_first.prepare_standalone_sandbox_provision_source(
            &tenant_id,
            "worker",
            "worker-profile",
            standalone_resource_spec(&tenant_id, "worker"),
            BTreeMap::new(),
        ),
        Err(Error::Conflict { .. })
    ));
    assert_eq!(manager_state_fingerprint(&service_first), before);
    assert_eq!(
        service_first
            .claim_tenant_source_retirement(
                &tenant_id,
                NonZeroU64::new(1).expect("fixture incarnation is nonzero"),
            )
            .expect("one admitted source should produce a coherent retirement snapshot")
            .sources()
            .len(),
        1
    );

    let standalone_first = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::new(),
        }),
        nimbus_sandbox::SandboxBackendKind::Krun,
    );
    reserve_standalone_source(
        &standalone_first,
        &tenant_id,
        "worker",
        "worker-profile",
        standalone_resource_spec(&tenant_id, "worker"),
        BTreeMap::new(),
    );
    let before = manager_state_fingerprint(&standalone_first);
    assert!(matches!(
        standalone_first.create_service_definition(
            &tenant_id,
            "worker",
            image_service_backend("worker", "registry.example.com/worker:1"),
            BTreeMap::new(),
        ),
        Err(Error::Conflict { .. })
    ));
    assert_eq!(manager_state_fingerprint(&standalone_first), before);
    assert_eq!(
        standalone_first
            .claim_tenant_source_retirement(
                &tenant_id,
                NonZeroU64::new(1).expect("fixture incarnation is nonzero"),
            )
            .expect("one admitted source should produce a coherent retirement snapshot")
            .sources()
            .len(),
        1
    );

    let catalog_service = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::from([(
                "worker".to_owned(),
                image_service_backend("worker", "registry.example.com/catalog-worker:1"),
            )]),
        }),
        nimbus_sandbox::SandboxBackendKind::Krun,
    );
    assert!(matches!(
        catalog_service.prepare_standalone_sandbox_provision_source(
            &tenant_id,
            "worker",
            "worker-profile",
            standalone_resource_spec(&tenant_id, "worker"),
            BTreeMap::new(),
        ),
        Err(Error::Conflict { .. })
    ));

    let update_to_sandbox = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::new(),
        }),
        nimbus_sandbox::SandboxBackendKind::Krun,
    );
    let built_in = update_to_sandbox
        .create_service_definition(
            &tenant_id,
            "worker",
            crate::ServiceBackend::built_in("browser"),
            BTreeMap::new(),
        )
        .expect("non-workload service may share the logical name before it becomes sandbox-backed");
    reserve_standalone_source(
        &update_to_sandbox,
        &tenant_id,
        "worker",
        "worker-profile",
        standalone_resource_spec(&tenant_id, "worker"),
        BTreeMap::new(),
    );
    let before = manager_state_fingerprint(&update_to_sandbox);
    assert!(matches!(
        update_to_sandbox.update_service_definition(
            &tenant_id,
            "worker",
            built_in.generation,
            image_service_backend("worker", "registry.example.com/worker:1"),
            BTreeMap::new(),
        ),
        Err(Error::Conflict { .. })
    ));
    assert_eq!(manager_state_fingerprint(&update_to_sandbox), before);
}

#[test]
fn standalone_reservation_rechecks_sandbox_service_name_collision() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should validate");
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::new(),
        }),
        nimbus_sandbox::SandboxBackendKind::Krun,
    );
    let prepared = manager
        .prepare_standalone_sandbox_provision_source(
            &tenant_id,
            "worker",
            "worker-profile",
            standalone_resource_spec(&tenant_id, "worker"),
            BTreeMap::new(),
        )
        .expect("standalone source should prepare before the colliding service exists");
    let decision = TenantIsolationContext::system(tenant_id.clone(), "stale-standalone")
        .with_deployment_generation(prepared.source().generation)
        .admit_decision(prepared.policy_input().clone())
        .expect("standalone source should admit");
    manager
        .create_service_definition(
            &tenant_id,
            "worker",
            image_service_backend("worker", "registry.example.com/worker:1"),
            BTreeMap::new(),
        )
        .expect("sandbox-backed service should win before stale reservation");
    let before = manager_state_fingerprint(&manager);

    assert!(matches!(
        manager.reserve_standalone_sandbox_provision_source(&decision, prepared),
        Err(Error::Conflict { .. })
    ));
    assert_eq!(manager_state_fingerprint(&manager), before);
    assert_eq!(
        manager
            .claim_tenant_source_retirement(
                &tenant_id,
                NonZeroU64::new(1).expect("fixture incarnation is nonzero"),
            )
            .expect("the winning service source should remain coherent")
            .sources()
            .len(),
        1
    );
}

#[tokio::test]
async fn tenant_retirement_barrier_rejects_all_source_and_session_admission_without_mutation() {
    let fixture = Fixture::with_sources();
    let prepared_sandbox = fixture
        .manager
        .prepare_standalone_sandbox_provision_source(
            &fixture.tenant_id,
            "late-task",
            "worker-profile",
            standalone_resource_spec(&fixture.tenant_id, "late-task"),
            BTreeMap::new(),
        )
        .expect("pre-barrier sandbox source should prepare");
    let sandbox_decision =
        TenantIsolationContext::system(fixture.tenant_id.clone(), "tenant-retirement-test")
            .with_deployment_generation(prepared_sandbox.source().generation)
            .admit_decision(prepared_sandbox.policy_input().clone())
            .expect("pre-barrier sandbox source should admit");
    let prepared_service = fixture
        .manager
        .prepare_sandbox_service_provision_source(&fixture.tenant_id, "worker")
        .expect("pre-barrier service source should prepare");
    let service_decision =
        TenantIsolationContext::system(fixture.tenant_id.clone(), "tenant-retirement-test")
            .with_deployment_generation(prepared_service.definition().generation)
            .admit_decision(prepared_service.policy_input().clone())
            .expect("pre-barrier service source should admit");

    fixture.claim();
    let before = manager_state_fingerprint(&fixture.manager);

    assert!(matches!(
        fixture.manager.create_service_definition(
            &fixture.tenant_id,
            "late-service",
            image_service_backend("late-service", "registry.example.com/late:1"),
            BTreeMap::new(),
        ),
        Err(Error::Conflict { .. })
    ));
    let worker = fixture
        .manager
        .service_definition_for_tenant(&fixture.tenant_id, "worker")
        .expect("worker definition should remain");
    assert!(matches!(
        fixture.manager.update_service_definition(
            &fixture.tenant_id,
            "worker",
            worker.generation,
            image_service_backend("worker", "registry.example.com/worker:2"),
            BTreeMap::new(),
        ),
        Err(Error::Conflict { .. })
    ));
    assert!(
        fixture
            .manager
            .prepare_standalone_sandbox_provision_source(
                &fixture.tenant_id,
                "after-barrier",
                "worker-profile",
                standalone_resource_spec(&fixture.tenant_id, "after-barrier"),
                BTreeMap::new(),
            )
            .is_err()
    );
    assert!(
        fixture
            .manager
            .prepare_sandbox_service_provision_source(&fixture.tenant_id, "worker")
            .is_err()
    );
    assert!(
        fixture
            .manager
            .reserve_standalone_sandbox_provision_source(&sandbox_decision, prepared_sandbox)
            .is_err()
    );
    assert!(
        fixture
            .manager
            .reserve_sandbox_service_provision_source(&service_decision, prepared_service)
            .is_err()
    );
    assert!(
        fixture
            .manager
            .open_session_async(
                &fixture.tenant_id,
                SessionTarget::Sandbox {
                    id: "task".to_owned(),
                },
                vec!["stdio".to_owned()],
                None,
            )
            .await
            .is_err()
    );

    assert_eq!(manager_state_fingerprint(&fixture.manager), before);
    assert_eq!(fixture.backend.retirement_effect_counts(), (0, 0, 0));
}

#[tokio::test]
async fn tenant_retirement_finalizer_removes_complete_sources_and_sessions_without_effects() {
    let fixture = Fixture::with_sources();
    fixture
        .manager
        .open_session_async(
            &fixture.tenant_id,
            SessionTarget::Sandbox {
                id: "task".to_owned(),
            },
            vec!["stdio".to_owned()],
            None,
        )
        .await
        .expect("pre-barrier session should open");
    let other = TenantId::new("tenant-retirement-other").expect("tenant id should validate");
    fixture
        .manager
        .create_service_definition(
            &other,
            "other-service",
            crate::ServiceBackend::built_in("browser"),
            BTreeMap::new(),
        )
        .expect("other tenant definition should create");

    let snapshot = fixture.claim();
    let records = observed_records(&snapshot);
    fixture
        .manager
        .finalize_tenant_sources_after_recorded(snapshot.claim(), &records)
        .expect("complete terminal inventory should finalize");
    fixture
        .manager
        .finalize_tenant_sources_after_recorded(snapshot.claim(), &records)
        .expect("finalization should replay while Engine finish is pending");

    assert!(
        fixture
            .manager
            .service_definitions_for_tenant(&fixture.tenant_id)
            .is_empty()
    );
    assert!(
        fixture
            .manager
            .list_sandbox_resource_snapshots_for_tenant(&fixture.tenant_id)
            .is_empty()
    );
    assert!(
        fixture
            .manager
            .list_sessions_for_tenant(&fixture.tenant_id)
            .is_empty()
    );
    assert!(
        fixture
            .manager
            .service_definition_for_tenant(&other, "other-service")
            .is_some()
    );
    assert_eq!(fixture.claim(), snapshot);
    assert!(matches!(
        fixture.manager.create_service_definition(
            &fixture.tenant_id,
            "premature-reuse",
            image_service_backend("premature-reuse", "registry.example.com/reuse:1"),
            BTreeMap::new(),
        ),
        Err(Error::Conflict { .. })
    ));
    let crossed_claim = TenantSourceRetirementClaim {
        tenant_id: fixture.tenant_id.clone(),
        tenant_incarnation: NonZeroU64::new(8).expect("fixture incarnation is nonzero"),
    };
    assert!(
        fixture
            .manager
            .release_tenant_source_retirement(&crossed_claim)
            .is_err()
    );
    assert_eq!(fixture.claim(), snapshot);
    fixture
        .manager
        .release_tenant_source_retirement(snapshot.claim())
        .expect("exact post-Engine release should clear the barrier");
    fixture
        .manager
        .create_service_definition(
            &fixture.tenant_id,
            "recreated",
            image_service_backend("recreated", "registry.example.com/recreated:1"),
            BTreeMap::new(),
        )
        .expect("admission should reopen only after exact release");
    assert_eq!(fixture.backend.retirement_effect_counts(), (0, 0, 0));
}

#[test]
fn tenant_retirement_finalizer_rejects_incomplete_crossed_duplicate_and_nonterminal_inventory() {
    assert_finalization_rejected(|_| Vec::new());
    assert_finalization_rejected(|snapshot| {
        let mut records = observed_records(snapshot);
        records.push(records[0].clone());
        records
    });
    assert_finalization_rejected(|snapshot| {
        vec![record_for_source(
            &TenantId::new("crossed-tenant").expect("tenant id should validate"),
            snapshot
                .sources()
                .iter()
                .find(|source| source.has_observation())
                .expect("fixture has an observed source"),
            DesiredWorkloadState::Stopped,
            None,
            None,
        )]
    });
    assert_finalization_rejected(|snapshot| {
        let source = snapshot
            .sources()
            .iter()
            .find(|source| source.has_observation())
            .expect("fixture has an observed source");
        vec![record_for_source(
            snapshot.claim().tenant_id(),
            source,
            DesiredWorkloadState::Running,
            None,
            None,
        )]
    });
    assert_finalization_rejected(|snapshot| {
        let source = snapshot
            .sources()
            .iter()
            .find(|source| source.has_observation())
            .expect("fixture has an observed source");
        vec![record_for_source(
            snapshot.claim().tenant_id(),
            source,
            DesiredWorkloadState::Stopped,
            Some(source.source_generation().as_u64() + 1),
            Some("crossed-resource-version"),
        )]
    });
    assert_finalization_rejected(|snapshot| {
        let observed = snapshot
            .sources()
            .iter()
            .find(|source| source.has_observation())
            .expect("fixture has an observed source");
        let orphan = TenantRetirementSource::new(
            WorkloadProvisionSourceIdentity::sandbox_backed_service("orphan")
                .expect("orphan identity should validate"),
            observed.source_generation(),
            observed.resource_version().clone(),
            true,
        );
        vec![record_for_source(
            snapshot.claim().tenant_id(),
            &orphan,
            DesiredWorkloadState::Stopped,
            None,
            None,
        )]
    });

    let fixture = Fixture::with_sources();
    let snapshot = fixture.claim();
    let records = observed_records(&snapshot);
    let crossed_claim = TenantSourceRetirementClaim {
        tenant_id: fixture.tenant_id.clone(),
        tenant_incarnation: NonZeroU64::new(8).expect("fixture incarnation is nonzero"),
    };
    let before = manager_state_fingerprint(&fixture.manager);
    assert!(
        fixture
            .manager
            .finalize_tenant_sources_after_recorded(&crossed_claim, &records)
            .is_err()
    );
    assert_eq!(manager_state_fingerprint(&fixture.manager), before);
    assert_eq!(fixture.backend.retirement_effect_counts(), (0, 0, 0));
}

fn assert_finalization_rejected(
    records: impl FnOnce(&TenantSourceRetirementSnapshot) -> Vec<WorkloadSagaRecord>,
) {
    let fixture = Fixture::with_sources();
    let snapshot = fixture.claim();
    let records = records(&snapshot);
    let before = manager_state_fingerprint(&fixture.manager);
    assert!(
        fixture
            .manager
            .finalize_tenant_sources_after_recorded(snapshot.claim(), &records)
            .is_err()
    );
    assert_eq!(manager_state_fingerprint(&fixture.manager), before);
    assert_eq!(fixture.claim(), snapshot);
    assert_eq!(fixture.backend.retirement_effect_counts(), (0, 0, 0));
}

fn observed_records(snapshot: &TenantSourceRetirementSnapshot) -> Vec<WorkloadSagaRecord> {
    snapshot
        .sources()
        .iter()
        .filter(|source| source.has_observation())
        .map(|source| {
            record_for_source(
                snapshot.claim().tenant_id(),
                source,
                DesiredWorkloadState::Stopped,
                None,
                None,
            )
        })
        .collect()
}

fn record_for_source(
    tenant_id: &TenantId,
    source: &TenantRetirementSource,
    desired_state: DesiredWorkloadState,
    generation: Option<u64>,
    resource_version: Option<&str>,
) -> WorkloadSagaRecord {
    let generation = generation.unwrap_or(source.source_generation().as_u64());
    let resource_version = resource_version.unwrap_or(source.resource_version().as_str());
    let executable = WorkloadExecutableIntent::new(
        WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1,
        format!(r#"{{"source":"{}"}}"#, source.identity().stable_name()),
    )
    .expect("fixture executable should validate");
    let source_evidence = match source.identity().kind() {
        WorkloadProvisionSourceKind::StandaloneSandbox => {
            WorkloadProvisionSourceEvidence::standalone_sandbox(
                source.identity().clone(),
                WorkloadProvisionSourceGeneration::new(generation),
                WorkloadProvisionSourceResourceVersion::new(resource_version)
                    .expect("fixture resource version should validate"),
                executable.content_digest(),
                nimbus_network::NetworkProviderId::for_registration_key("fixture-attachment"),
                nimbus_workloads::WorkloadExecutionProviderId::for_registration_key(
                    "fixture-execution",
                ),
            )
        }
        WorkloadProvisionSourceKind::SandboxBackedService => {
            WorkloadProvisionSourceEvidence::sandbox_backed_service(
                source.identity().clone(),
                WorkloadProvisionSourceGeneration::new(generation),
                WorkloadProvisionSourceResourceVersion::new(resource_version)
                    .expect("fixture resource version should validate"),
                executable.content_digest(),
                nimbus_network::NetworkProviderId::for_registration_key("fixture-attachment"),
                nimbus_workloads::WorkloadExecutionProviderId::for_registration_key(
                    "fixture-execution",
                ),
            )
        }
    }
    .expect("fixture source evidence should validate");
    let (activation, publication) = match desired_state {
        DesiredWorkloadState::Running => (
            WorkloadActivationIntent::ActivateWhenAttached,
            WorkloadPublicationIntent::Withheld,
        ),
        DesiredWorkloadState::Stopped => (
            WorkloadActivationIntent::PrepareOnly,
            WorkloadPublicationIntent::Withheld,
        ),
    };
    let key = WorkloadSagaKey::new(
        tenant_id.clone(),
        WorkloadId::new(source.identity().stable_name())
            .expect("fixture workload id should validate"),
    );
    let intent = WorkloadSagaIntent::new_without_automatic_restart(
        match source.identity().kind() {
            WorkloadProvisionSourceKind::StandaloneSandbox => DesiredWorkloadKind::Sandbox,
            WorkloadProvisionSourceKind::SandboxBackedService => DesiredWorkloadKind::Service,
        },
        desired_state,
        WorkloadGeneration::new(generation),
        executable,
        source_evidence,
        WorkloadNetworkIntent::new(compiled_network_plan(
            tenant_id,
            source.identity().stable_name(),
            generation,
            activation,
            publication,
        )),
        activation,
        publication,
        WorkloadAdmissionEvidence::new(
            format!("tid_{}", "1".repeat(64))
                .try_into()
                .expect("fixture decision id should validate"),
            format!("twu_{}", "2".repeat(64))
                .try_into()
                .expect("fixture workload uid should validate"),
            NodeIdentity::new("tenant-retirement-node").expect("fixture node should validate"),
        ),
    )
    .expect("fixture intent should validate");
    let record = WorkloadSagaRecord::new(key, intent).expect("fixture record should validate");
    if desired_state == DesiredWorkloadState::Stopped {
        assert_eq!(record.phase(), WorkloadSagaPhase::Recorded);
    }
    record
}

fn compiled_network_plan(
    tenant_id: &TenantId,
    stable_name: &str,
    generation: u64,
    activation: WorkloadActivationIntent,
    publication: WorkloadPublicationIntent,
) -> CompiledWorkloadNetworkPlan {
    let identity = WorkloadNetworkPlanIdentity::new(
        tenant_id.clone(),
        stable_name,
        NetworkResourceGeneration::new(generation),
    )
    .expect("fixture network identity should validate");
    let attachment =
        NetworkAttachmentCapabilitySet::new(NetworkManagementMode::NimbusHostManaged, [], []);
    let endpoint = NetworkEndpointCapabilitySet::new(
        [NetworkAddressFamily::Ipv4],
        [NetworkBindRealmKind::Host],
        [NetworkExposure::Loopback],
        [PortProtocol::Tcp],
        [NetworkPortAssignmentMode::ProviderAssigned],
    );
    let ingress = NetworkIngressCapabilitySet::new([]);
    let forwarding = NetworkForwardingCapabilitySet::new([]);
    let lifecycle = NetworkLifecycleCapabilitySet::new([]);
    let requirements = NetworkCapabilityRequirements::new(
        attachment,
        endpoint,
        ingress,
        forwarding,
        nimbus_network::NetworkLifecycleRequirements::new(lifecycle.clone(), lifecycle),
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let content = WorkloadNetworkPlanContent::new(
        identity,
        requirements,
        None,
        None,
        None,
        [],
        [],
        [],
        activation,
        publication,
    )
    .expect("fixture network content should validate");
    CompiledWorkloadNetworkPlan::from_content(content)
        .expect("fixture compiled network plan should validate")
}

fn manager_state_fingerprint(manager: &ServiceManager) -> String {
    let state = manager
        .state
        .lock()
        .expect("manager lock should not be poisoned");
    format!(
        "{:?}",
        (
            &state.service_definition_observations,
            &state.definitions,
            &state.sandbox_resource_sources,
            &state.sandbox_resource_observations,
            &state.sessions,
            &state.session_channels,
            &state.source_retirement_claims,
            &state.tenant_source_retirements,
            &state.service_resolution_withdrawals,
            state.next_definition_version,
            state.next_session_version,
        )
    )
}
