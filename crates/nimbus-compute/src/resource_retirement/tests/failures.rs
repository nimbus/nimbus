use std::sync::Arc;

use nimbus_engine::Engine;
use nimbus_network::{
    LocalNetworkManager, NetworkControlPlaneLocality, NetworkSovereigntyRequirements,
};
use nimbus_workloads::{WorkloadSagaPhase, WorkloadSagaStore};
use tempfile::tempdir;

use super::super::ComputeResourceRetirementError;
use super::support::{
    NextCasFault, RetirementHarness, SERVICE_NAME, key, max_generation_record, provider_realm,
    provision_capabilities, run_async_test,
};
use crate::config::control_plane::ControlPlaneConfig;
use crate::config::deployment::DeploymentConfig;
use crate::config::node_services::NodeServicesConfig;
use crate::config::runtime::RuntimeGovernorConfig;
use crate::state::{ComputeState, ComputeStateConfig, ComputeWorkloadComposition};
use crate::workload_projection::ServiceManagerWorkloadProjectionSink;
use crate::workload_provision_source::ServiceManagerWorkloadProvisionSourceAuthority;
use crate::workload_saga::{
    WorkloadProvisionSourceAuthority, WorkloadRestartCapabilityRegistry,
    sandbox_execution_provider_id,
};

#[test]
fn native_stop_without_teardown_composition_fails_before_source_or_effect() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.declare_service();
        let temp = tempdir().expect("fixture directory should build");
        let engine = Arc::new(Engine::new(temp.path()).expect("fixture engine should build"));
        let (provider_reports, selection) = provider_realm();
        let network_manager =
            LocalNetworkManager::open(temp.path().join("network"), provider_reports)
                .expect("fixture network manager should build");
        let source_authority: Arc<dyn WorkloadProvisionSourceAuthority> = Arc::new(
            ServiceManagerWorkloadProvisionSourceAuthority::new(harness.manager.clone()),
        );
        let store: Arc<dyn WorkloadSagaStore> = harness.store.clone();
        let before_source = harness
            .manager
            .service_definition_for_tenant(harness.context.tenant_id(), SERVICE_NAME);
        let before_store = harness.store.counts();
        let before_provider = harness.provision_provider.call_count();
        let state = ComputeState::from_config(ComputeStateConfig {
            engine,
            workload_composition: ComputeWorkloadComposition::Managed {
                network_manager,
                local_node: crate::embedded_local_node_identity(),
                capability_selection: Box::new(selection.clone()),
                execution_provider_id: sandbox_execution_provider_id(
                    nimbus_sandbox::SandboxBackendKind::Krun,
                ),
                sovereignty: NetworkSovereigntyRequirements::new(
                    NetworkControlPlaneLocality::LocalOnly,
                    [],
                    true,
                ),
                saga_store: store,
                source_authority,
                provision_capabilities: Box::new(provision_capabilities(
                    harness.provision_provider.clone(),
                    &selection,
                )),
                restart_capabilities: Box::new(
                    WorkloadRestartCapabilityRegistry::new([])
                        .expect("empty restart capabilities should validate"),
                ),
                teardown_capabilities: None,
                desire_admission_guard: None,
                projection_sink: Arc::new(ServiceManagerWorkloadProjectionSink::new(
                    harness.manager.clone(),
                )),
            },
            deployment: DeploymentConfig::default(),
            control_plane: ControlPlaneConfig::router_options_default(),
            node_services: NodeServicesConfig::default()
                .with_service_manager(harness.manager.clone()),
            runtime: RuntimeGovernorConfig::default(),
        });

        assert!(
            state.workload_provisioner().is_none(),
            "partial managed composition must expose no provision authority"
        );
        assert!(state.resource_provisioner().is_err());
        let error = match state.resource_retirer() {
            Ok(_) => panic!("missing exact teardown composition must fail closed"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("exact teardown composition"));
        assert_eq!(harness.store.counts(), before_store);
        assert_eq!(harness.provision_provider.call_count(), before_provider);
        assert_eq!(harness.teardown_provider.call_count(), 0);
        assert_eq!(
            harness
                .manager
                .service_definition_for_tenant(harness.context.tenant_id(), SERVICE_NAME,),
            before_source,
            "missing composition must not change the desired source bytes"
        );
    });
}

#[test]
fn cancellation_before_source_claim_makes_zero_store_source_or_provider_mutation() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.declare_service();
        harness.start_service().await;
        harness.reset_retirement_evidence();
        let before_record = harness.store.record(&key(SERVICE_NAME));
        let before_source = harness
            .manager
            .service_definition_for_tenant(harness.context.tenant_id(), SERVICE_NAME);
        let before_store = harness.store.counts();

        let unpolled = harness
            .retire
            .submit_service_teardown(&harness.context, SERVICE_NAME);
        drop(unpolled);

        assert_eq!(harness.store.counts(), before_store);
        assert_eq!(harness.store.record(&key(SERVICE_NAME)), before_record);
        assert_eq!(
            harness
                .manager
                .service_definition_for_tenant(harness.context.tenant_id(), SERVICE_NAME,),
            before_source
        );
        assert!(!harness.service_source_is_fenced());
        assert_eq!(harness.teardown_provider.call_count(), 0);
    });
}

#[test]
fn native_stop_unresolved_submission_makes_zero_provider_calls() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.declare_service();
        harness.start_service().await;
        harness.reset_retirement_evidence();
        harness
            .store
            .fail_next_cas(NextCasFault::AmbiguousBeforeApply);

        let error = harness
            .retire
            .submit_service_teardown(&harness.context, SERVICE_NAME)
            .await
            .expect_err("unresolved stopped-successor submission must fail closed");

        assert!(matches!(
            error,
            ComputeResourceRetirementError::Ingress(
                crate::workload_saga::WorkloadSagaIngressError::Saga(
                    nimbus_workloads::WorkloadSagaStoreError::Ambiguous
                )
            )
        ));
        assert_eq!(harness.teardown_provider.call_count(), 0);
        assert_eq!(
            harness.store.record(&key(SERVICE_NAME)).phase(),
            WorkloadSagaPhase::Observed
        );

        harness
            .retire
            .submit_service_teardown(&harness.context, SERVICE_NAME)
            .await
            .expect("exact retry should adopt the retained claim after ambiguous submission");
        assert_eq!(
            harness.store.record(&key(SERVICE_NAME)).phase(),
            WorkloadSagaPhase::Recorded
        );
        assert_eq!(harness.teardown_provider.call_count(), 5);
        assert!(!harness.service_source_is_fenced());
    });
}

#[test]
fn generation_overflow_fails_before_source_store_or_provider_effect() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.declare_service();
        harness.start_service().await;
        let maximum = max_generation_record(&harness.store.record(&key(SERVICE_NAME)));
        harness.store.replace(maximum);
        harness.reset_retirement_evidence();
        let before_cas = harness.store.counts().1;

        let error = harness
            .retire
            .submit_service_teardown(&harness.context, SERVICE_NAME)
            .await
            .expect_err("maximum generation must fail before retirement submission");

        assert!(matches!(
            error,
            ComputeResourceRetirementError::GenerationOverflow
        ));
        assert_eq!(harness.store.counts().1, before_cas);
        assert_eq!(harness.teardown_provider.call_count(), 0);
        assert!(
            !harness.service_source_is_fenced(),
            "overflow preflight must not leave a services-owned retirement claim"
        );
    });
}

#[test]
fn missing_saga_with_provider_observation_fails_closed() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.declare_service();
        harness.start_service().await;
        harness.store.remove(&key(SERVICE_NAME));
        harness.reset_retirement_evidence();

        let error = harness
            .retire
            .submit_service_teardown(&harness.context, SERVICE_NAME)
            .await
            .expect_err("provider observation without durable saga truth must fail closed");

        assert!(matches!(
            error,
            ComputeResourceRetirementError::ObservationWithoutSaga
        ));
        assert_eq!(harness.teardown_provider.call_count(), 0);
        assert!(
            harness
                .manager
                .service_definition_observation_for_tenant(
                    harness.context.tenant_id(),
                    SERVICE_NAME
                )
                .is_some(),
            "fail-closed handling must preserve the observed projection for recovery"
        );
    });
}
