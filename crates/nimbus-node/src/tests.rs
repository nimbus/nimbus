use nimbus_core::{Error, PrincipalContext, Result, TenantId};
use nimbus_network::EndpointProtocol;
use nimbus_runtime::{RuntimeLimits, RuntimePolicy};
use nimbus_sandbox::SandboxResourceCharge;
use nimbus_workloads::{
    LocalEnforcementBinding, NodeIdentity, TenantCredentialProjectionRequest,
    TenantCredentialProjectionScope, TenantEgressReloadRequest, TenantFinalizerRecord,
    TenantPolicyArea, TenantPolicyLifecycle, TenantWorkloadDeletionState, TenantWorkloadGeneration,
    TenantWorkloadSpec, policy_lifecycle,
};

use super::*;
use nimbus_tenant::{
    RuntimeIsolationTier, TenantImagePolicyDecision, TenantIsolationContext,
    TenantIsolationDecision, TenantIsolationEventKind, TenantIsolationMode,
    TenantIsolationPolicyInput, TenantNetworkEndpointDecision, TenantNetworkPolicyDecision,
    TenantQuotaPolicyDecision, TenantSecretPolicyDecision, TenantServiceGrantPolicyDecision,
    TenantStoragePolicyDecision, TenantVolumePolicyDecision, WorkloadAttributes, WorkloadLocation,
};

fn principal_with_tenant_claim(tenant: &str) -> PrincipalContext {
    PrincipalContext {
        authenticated: true,
        claims: serde_json::Map::from_iter([(
            "tenant_id".to_string(),
            serde_json::Value::String(tenant.to_string()),
        )]),
        verified_claims: serde_json::Map::new(),
    }
}

fn admitted_decision(
    workload_name: &str,
    invocation_id: &str,
    generation: u64,
    node_id: &str,
) -> TenantIsolationDecision {
    let tenant_id = TenantId::new("tenant-a").expect("tenant id should parse");
    let context = TenantIsolationContext::application(
        tenant_id,
        principal_with_tenant_claim("tenant-a"),
        "convex.runtime",
    )
    .with_deployment_generation(generation)
    .with_workload_location(
        WorkloadLocation::new()
            .with_node_id(node_id)
            .with_machine_id("machine-a"),
    );
    let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
    let workload = WorkloadAttributes::runtime_function(
        workload_name,
        RuntimeIsolationTier::InProcessUntrusted,
    )
    .with_invocation_id(invocation_id);
    let input = TenantIsolationPolicyInput::new(workload)
        .with_runtime_policy(
            &context,
            &policy,
            RuntimeIsolationTier::InProcessUntrusted,
            TenantIsolationMode::Production,
        )
        .with_services(TenantServiceGrantPolicyDecision::new(["db", "cache"]))
        .with_network(TenantNetworkPolicyDecision::new([
            TenantNetworkEndpointDecision::new(
                "db",
                "postgres",
                EndpointProtocol::Tcp,
                "127.0.0.1",
                15432,
            )
            .with_guest_port(5432),
        ]))
        .with_storage(TenantStoragePolicyDecision::namespace("tenant-a-storage"))
        .with_volumes(TenantVolumePolicyDecision::new(["cache"]))
        .with_image(TenantImagePolicyDecision::digest_pinned(
            "registry.example.com/app@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ))
        .with_secrets(TenantSecretPolicyDecision::handles(["prod/db/password"]))
        .with_quotas(TenantQuotaPolicyDecision::default().with_sandbox_charge(
            SandboxResourceCharge {
                active_sandboxes: 1,
                vcpus: 1,
                memory_bytes: 512 * 1024 * 1024,
                disk_bytes: 10 * 1024 * 1024 * 1024,
                log_bytes: 64 * 1024 * 1024,
            },
        ));

    context
        .admit_decision(input)
        .expect("decision should admit matching tenant authority")
}

fn binding_with_credentials() -> LocalEnforcementBinding {
    let decision = admitted_decision("messages:send", "invoke-1", 7, "node-a");
    let spec = TenantWorkloadSpec::from_decision(&decision)
        .expect("spec should materialize from decision")
        .with_admitted_credential_scopes([TenantCredentialProjectionScope::new(
            "vault", "runtime",
        )
        .expect("scope should parse")]);
    LocalEnforcementBinding::from_spec(spec)
}

fn assert_error_contains<T: std::fmt::Debug>(result: Result<T>, expected: &str) {
    let error = result.expect_err("operation should fail closed");
    assert!(
        error.to_string().contains(expected),
        "expected error containing `{expected}`, got `{error}`"
    );
}

#[test]
fn binding_materializes_decision_derived_spec_and_projections() {
    let decision = admitted_decision("messages:send", "invoke-1", 7, "node-a");
    let binding = LocalEnforcementBinding::from_decision(&decision)
        .expect("binding should materialize from admitted decision");
    let spec = binding.spec();

    assert_eq!(spec.decision_id(), decision.id());
    assert_eq!(spec.tenant_id(), decision.tenant_id());
    assert_eq!(spec.surface(), decision.surface());
    assert_eq!(spec.authority_class(), decision.authority_class());
    assert_eq!(spec.generation().as_u64(), 7);
    assert_eq!(
        spec.assigned_node_id()
            .expect("node assignment should be present")
            .as_str(),
        "node-a"
    );
    assert!(
        spec.workload_uid().as_str().starts_with("twu_"),
        "workload UID should be derived, not caller supplied"
    );
    assert_eq!(
        binding.storage_access().namespace_name(),
        "tenant-a-storage"
    );
    binding
        .storage_access()
        .ensure_tenant_matches(decision.tenant_id(), "storage projection")
        .expect("storage projection should match admitted tenant");
    assert_error_contains(
        binding.storage_access().ensure_tenant_matches(
            &TenantId::new("tenant-b").expect("tenant id should parse"),
            "storage projection",
        ),
        "authorized tenant tenant-a",
    );
    assert_eq!(
        binding
            .service_access("db")
            .expect("db service should be admitted")
            .service_name(),
        "db"
    );
    assert_error_contains(binding.service_access("not-admitted"), "did not authorize");

    let evidence = binding.system_evidence_projection();
    assert_eq!(evidence.decision_id(), decision.id());
    assert_eq!(evidence.tenant_id(), decision.tenant_id());
    assert_eq!(evidence.surface(), "convex.runtime");
    assert_eq!(evidence.authority_class(), "application");
    assert_eq!(evidence.generation().as_u64(), 7);
    assert_eq!(evidence.workload_uid(), spec.workload_uid());
    assert!(
        evidence.workload_subject().contains("messages%3Asend"),
        "system evidence should use the admitted workload subject"
    );
    assert!(
        evidence
            .redacted_fields()
            .contains(&"raw_credentials".to_string()),
        "system evidence projection should preserve redaction metadata"
    );
}

#[test]
fn lifecycle_evidence_audit_events_keep_high_cardinality_ids_out_of_metric_labels() {
    let binding = binding_with_credentials();
    let spec = binding.spec();
    let authorizer = NodeStatusAuthorizer;
    let projection = binding.system_evidence_projection();
    let lifecycle = TenantWorkloadLifecycleEvidence::for_observed_unit(
        HostLifecycleBackendKind::SystemdTransientUnit,
        &SystemdUnitName::new("nimbus-tw-highcard.service").expect("unit should parse"),
        HostLifecycleStatusReason::Running,
    )
    .with_job_path("/org/freedesktop/systemd1/job/9001")
    .expect("job path evidence should parse")
    .with_process_id(424_242)
    .with_cgroup_path("/system.slice/nimbus-tw-highcard.service")
    .expect("cgroup evidence should parse")
    .with_journal_selectors([
        HostLifecycleJournalSelectorEvidence::new("_SYSTEMD_UNIT", "nimbus-tw-highcard.service")
            .expect("journal selector should parse"),
        HostLifecycleJournalSelectorEvidence::new("NIMBUS_WORKLOAD_ID", "tw_highcard")
            .expect("journal selector should parse"),
    ]);
    let node_ids = TenantNodeObservationIds::new()
        .with_node_lease_id("lease-node-a-0001")
        .expect("lease id should parse")
        .with_heartbeat_id("heartbeat-node-a-0002")
        .expect("heartbeat id should parse");
    let diagnostics = TenantWorkloadDiagnostics::new()
        .with_backend_capabilities([SystemdTransientCapabilities::available()
            .without_dbus()
            .to_backend_capabilities()])
        .with_actionable_failure_reason(
            "install systemd with D-Bus and transient service unit support",
        )
        .expect("diagnostic reason should parse");
    let status = authorizer
        .authorize(
            spec,
            TenantWorkloadStatusPatch::observed_status(spec)
                .with_phase(TenantWorkloadPhase::Running)
                .with_lifecycle_evidence(lifecycle)
                .with_node_observation_ids(node_ids)
                .with_diagnostics(diagnostics)
                .with_evidence_correlation_ids([
                    "nimbus-tw-highcard.service",
                    "/org/freedesktop/systemd1/job/9001",
                ]),
        )
        .expect("status with lifecycle evidence should authorize");

    let labels = serde_json::to_string(&status.metric_labels()).expect("labels serialize");
    assert!(labels.contains("systemd_transient_unit"));
    assert!(labels.contains("running"));
    for high_cardinality in [
        spec.decision_id().as_str(),
        spec.workload_uid().as_str(),
        "nimbus-tw-highcard.service",
        "/org/freedesktop/systemd1/job/9001",
        "/system.slice/nimbus-tw-highcard.service",
        "424242",
        "lease-node-a-0001",
        "heartbeat-node-a-0002",
    ] {
        assert!(
            !labels.contains(high_cardinality),
            "metric labels must not carry high-cardinality evidence `{high_cardinality}`: {labels}"
        );
    }

    let event = status.lifecycle_audit_event(&projection);
    let event_json = serde_json::to_string(&event).expect("audit event should serialize");
    for evidence in [
        spec.decision_id().as_str(),
        "nimbus-tw-highcard.service",
        "/org/freedesktop/systemd1/job/9001",
        "/system.slice/nimbus-tw-highcard.service",
        "424242",
        "lease-node-a-0001",
        "heartbeat-node-a-0002",
    ] {
        assert!(
            event_json.contains(evidence),
            "audit event should retain high-cardinality evidence `{evidence}`: {event_json}"
        );
    }
    assert_eq!(event.kind(), TenantIsolationEventKind::LifecycleStatus);
    assert_eq!(event.decision_id(), Some(spec.decision_id().as_str()));
}

#[test]
fn node_status_authorizer_accepts_observed_status_for_assigned_node() {
    let binding = binding_with_credentials();
    let spec = binding.spec();
    let authorizer = NodeStatusAuthorizer;
    let running_false = TenantWorkloadCondition::new(
        TenantWorkloadConditionType::Running,
        TenantWorkloadConditionStatus::False,
        "Starting",
    )
    .expect("condition should build");
    let running_true = TenantWorkloadCondition::new(
        TenantWorkloadConditionType::Running,
        TenantWorkloadConditionStatus::True,
        "Started",
    )
    .expect("condition should build")
    .with_message("runtime reported ready")
    .expect("message should build");
    let usage = TenantObservedResourceUsage {
        active_sandboxes: 9,
        cpu_millis: 42,
        memory_bytes: 900 * 1024 * 1024,
        disk_bytes: 8 * 1024 * 1024,
        log_bytes: 1024,
    };
    let patch = TenantWorkloadStatusPatch::observed_status(spec)
        .with_phase(TenantWorkloadPhase::Running)
        .with_conditions([running_false, running_true])
        .with_observed_usage(usage.clone())
        .with_evidence_correlation_ids(["evt-1"]);

    let status = authorizer
        .authorize(spec, patch)
        .expect("assigned node status should be accepted");

    assert_eq!(status.workload_uid(), spec.workload_uid());
    assert_eq!(status.observed_generation(), spec.generation());
    assert_eq!(status.decision_id(), spec.decision_id());
    assert_eq!(status.writer_node_id().as_str(), "node-a");
    assert_eq!(status.phase(), TenantWorkloadPhase::Running);
    assert_eq!(status.target(), TenantWorkloadStatusPatchTarget::Status);
    assert_eq!(status.observed_usage(), &usage);
    assert_eq!(status.evidence_correlation_ids(), &["evt-1".to_string()]);
    assert_eq!(
        status.conditions().len(),
        1,
        "conditions should merge by type"
    );
    assert_eq!(
        status.conditions()[0].status(),
        TenantWorkloadConditionStatus::True
    );
    assert_eq!(status.conditions()[0].reason(), "Started");
    assert_eq!(
        status.conditions()[0].message(),
        Some("runtime reported ready")
    );
}

#[test]
fn node_status_authorizer_rejects_wrong_node_stale_generation_and_desired_mutations() {
    let binding = binding_with_credentials();
    let spec = binding.spec();
    let authorizer = NodeStatusAuthorizer;

    assert_error_contains(
        authorizer.authorize(
            spec,
            TenantWorkloadStatusPatch::observed_status(spec).with_writer_node_id(Some(
                NodeIdentity::new("node-b").expect("node should parse"),
            )),
        ),
        "assigned to node node-a",
    );
    assert_error_contains(
        authorizer.authorize(
            spec,
            TenantWorkloadStatusPatch::observed_status(spec)
                .with_observed_generation(TenantWorkloadGeneration::new(6)),
        ),
        "referenced generation 6",
    );
    let other_decision = admitted_decision("messages:list", "invoke-1", 7, "node-a");
    assert_error_contains(
        authorizer.authorize(
            spec,
            TenantWorkloadStatusPatch::observed_status(spec)
                .with_decision_id(other_decision.id().clone()),
        ),
        "referenced decision",
    );
    assert_error_contains(
        authorizer.authorize(
            spec,
            TenantWorkloadStatusPatch::observed_status(spec).with_writer_node_id(None),
        ),
        "did not include a writer node",
    );

    for target in [
        TenantWorkloadStatusPatchTarget::Spec,
        TenantWorkloadStatusPatchTarget::Labels,
        TenantWorkloadStatusPatchTarget::Policy,
        TenantWorkloadStatusPatchTarget::Grants,
        TenantWorkloadStatusPatchTarget::QuotaHardLimits,
        TenantWorkloadStatusPatchTarget::Placement,
        TenantWorkloadStatusPatchTarget::Credentials,
        TenantWorkloadStatusPatchTarget::Admission,
        TenantWorkloadStatusPatchTarget::DeletionAuthority,
        TenantWorkloadStatusPatchTarget::UserData,
    ] {
        assert_error_contains(
            authorizer.authorize(spec, TenantWorkloadStatusPatch::for_target(spec, target)),
            "desired state",
        );
    }
}

#[test]
fn credential_projection_requires_admitted_scope_node_generation_invocation_and_redaction() {
    let binding = binding_with_credentials();
    let spec = binding.spec();
    let request = TenantCredentialProjectionRequest::node_mediated(
        spec,
        NodeIdentity::new("node-a").expect("node should parse"),
        "vault",
        "runtime",
    )
    .expect("credential request should build");
    let projection = binding
        .authorize_credential_projection(&request)
        .expect("matching credential projection should be admitted");

    assert_eq!(projection.workload_uid(), spec.workload_uid());
    assert_eq!(projection.generation(), spec.generation());
    assert_eq!(projection.decision_id(), spec.decision_id());
    assert_eq!(projection.scope().provider(), "vault");
    assert_eq!(projection.scope().audience(), "runtime");
    assert_eq!(
        projection.workload_subject(),
        spec.workload_identity().subject()
    );
    assert!(
        projection
            .redacted_fields()
            .contains(&"raw_credentials".to_string())
    );

    assert_error_contains(
        binding.authorize_credential_projection(
            &TenantCredentialProjectionRequest::node_mediated(
                spec,
                NodeIdentity::new("node-a").expect("node should parse"),
                "vault",
                "wrong-audience",
            )
            .expect("credential request should build"),
        ),
        "did not admit provider `vault` with audience `wrong-audience`",
    );
    assert_error_contains(
        binding.authorize_credential_projection(
            &TenantCredentialProjectionRequest::node_mediated(
                spec,
                NodeIdentity::new("node-b").expect("node should parse"),
                "vault",
                "runtime",
            )
            .expect("credential request should build"),
        ),
        "assigned to node node-a",
    );
    assert_error_contains(
        binding.authorize_credential_projection(
            &TenantCredentialProjectionRequest::node_mediated(
                spec,
                NodeIdentity::new("node-a").expect("node should parse"),
                "vault",
                "runtime",
            )
            .expect("credential request should build")
            .with_generation(TenantWorkloadGeneration::new(6)),
        ),
        "referenced generation 6",
    );
    assert_error_contains(
        binding.authorize_credential_projection(
            &TenantCredentialProjectionRequest::node_mediated(
                spec,
                NodeIdentity::new("node-a").expect("node should parse"),
                "vault",
                "runtime",
            )
            .expect("credential request should build")
            .with_runtime_invocation_id(Some("wrong-invocation".to_string())),
        ),
        "referenced invocation",
    );
    assert_error_contains(
        binding.authorize_credential_projection(
            &TenantCredentialProjectionRequest::node_mediated(
                spec,
                NodeIdentity::new("node-a").expect("node should parse"),
                "vault",
                "runtime",
            )
            .expect("credential request should build")
            .without_redaction_metadata(),
        ),
        "missing redaction metadata",
    );
    assert_error_contains(
        binding.authorize_credential_projection(
            &TenantCredentialProjectionRequest::node_mediated(
                spec,
                NodeIdentity::new("node-a").expect("node should parse"),
                "vault",
                "runtime",
            )
            .expect("credential request should build")
            .with_echo_back_workload_subject("spiffe://attacker"),
        ),
        "echo back a subject",
    );

    let no_grant_binding = LocalEnforcementBinding::from_decision(&admitted_decision(
        "messages:send",
        "invoke-1",
        7,
        "node-a",
    ))
    .expect("binding should materialize");
    assert_error_contains(
        no_grant_binding.authorize_credential_projection(
            &TenantCredentialProjectionRequest::node_mediated(
                no_grant_binding.spec(),
                NodeIdentity::new("node-a").expect("node should parse"),
                "vault",
                "runtime",
            )
            .expect("credential request should build"),
        ),
        "did not admit provider",
    );
}

#[test]
fn deletion_and_quota_state_stay_server_owned_while_cleanup_progress_is_observed() {
    let binding = binding_with_credentials();
    let mut spec = binding
        .spec()
        .clone()
        .mark_deleting_server_owned([TenantFinalizerRecord::new(
            "local_enforcement",
            "sandbox-cleanup",
        )
        .expect("finalizer should parse")]);
    let authorizer = NodeStatusAuthorizer;
    let usage = TenantObservedResourceUsage {
        memory_bytes: u64::MAX,
        ..TenantObservedResourceUsage::default()
    };
    let cleanup_status = authorizer
        .authorize(
            &spec,
            TenantWorkloadStatusPatch::for_target(
                &spec,
                TenantWorkloadStatusPatchTarget::CleanupProgress,
            )
            .with_phase(TenantWorkloadPhase::Deleting)
            .with_cleanup_progress(
                TenantWorkloadCleanupProgress::new()
                    .with_pending_finalizers([TenantFinalizerRecord::new(
                        "local_enforcement",
                        "sandbox-cleanup",
                    )
                    .expect("finalizer should parse")])
                    .with_retained_bytes(u64::MAX),
            )
            .with_observed_usage(usage),
        )
        .expect("cleanup progress is observed status");

    assert_eq!(cleanup_status.phase(), TenantWorkloadPhase::Deleting);
    assert_eq!(
        cleanup_status.target(),
        TenantWorkloadStatusPatchTarget::CleanupProgress
    );
    assert_eq!(
        cleanup_status
            .cleanup_progress()
            .expect("cleanup progress should be present")
            .retained_bytes(),
        u64::MAX
    );
    assert_eq!(
        spec.resources().admitted_quotas(),
        binding.spec().resources().admitted_quotas(),
        "observed usage must not mutate admitted hard-limit policy"
    );
    match spec.deletion() {
        TenantWorkloadDeletionState::Deleting { finalizers } => {
            assert_eq!(finalizers.len(), 1);
            assert_eq!(finalizers[0].owner(), "local_enforcement");
        }
        TenantWorkloadDeletionState::Active => panic!("spec should be deleting"),
    }

    assert_error_contains(
        authorizer.authorize(
            &spec,
            TenantWorkloadStatusPatch::for_target(
                &spec,
                TenantWorkloadStatusPatchTarget::DeletionAuthority,
            ),
        ),
        "desired state",
    );
    assert_error_contains(
        authorizer.authorize(
            &spec,
            TenantWorkloadStatusPatch::for_target(
                &spec,
                TenantWorkloadStatusPatchTarget::QuotaHardLimits,
            ),
        ),
        "desired state",
    );
    assert_error_contains(
        authorizer.authorize(
            &spec,
            TenantWorkloadStatusPatch::observed_status(&spec)
                .with_cleanup_progress(TenantWorkloadCleanupProgress::new().with_retained_bytes(1)),
        ),
        "cleanup progress",
    );

    spec = spec.mark_deleting_server_owned(Vec::<TenantFinalizerRecord>::new());
    match spec.deletion() {
        TenantWorkloadDeletionState::Deleting { finalizers } => {
            assert!(finalizers.is_empty());
        }
        TenantWorkloadDeletionState::Active => panic!("spec should remain server-owned deleting"),
    }
}

#[test]
fn egress_reload_and_policy_lifecycle_require_admitted_binding_identity() {
    let binding = binding_with_credentials();
    let spec = binding.spec();
    binding
        .authorize_egress_reload(&TenantEgressReloadRequest::for_spec(spec))
        .expect("matching egress reload should be authorized");

    let other_decision = admitted_decision("messages:list", "invoke-1", 7, "node-a");
    assert_error_contains(
        binding.authorize_egress_reload(
            &TenantEgressReloadRequest::for_spec(spec)
                .with_decision_id(other_decision.id().clone()),
        ),
        "referenced decision",
    );

    assert_eq!(
        policy_lifecycle(TenantPolicyArea::Filesystem),
        TenantPolicyLifecycle::RecreateRequired
    );
    assert_eq!(
        policy_lifecycle(TenantPolicyArea::Placement),
        TenantPolicyLifecycle::RecreateRequired
    );
    assert_eq!(
        policy_lifecycle(TenantPolicyArea::HostBridgeGrants),
        TenantPolicyLifecycle::DynamicReload
    );
    assert_eq!(
        policy_lifecycle(TenantPolicyArea::DeletionFinalizerState),
        TenantPolicyLifecycle::ServerOwnedTransition
    );
}

#[test]
fn malformed_local_enforcement_identifiers_fail_closed() {
    assert!(matches!(
        NodeIdentity::new("  "),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        TenantCredentialProjectionScope::new("vault", ""),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        TenantFinalizerRecord::new("", "cleanup"),
        Err(Error::InvalidInput(_))
    ));
}
