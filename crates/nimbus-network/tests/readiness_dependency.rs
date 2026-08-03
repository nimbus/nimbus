use std::collections::BTreeSet;
use std::num::NonZeroU16;

use nimbus_network::{
    LocalPortLeaseAuthority, NetworkAttachmentCapabilitySet, NetworkCapabilityRequirements,
    NetworkCondition, NetworkConditionKind, NetworkConditionState, NetworkControlPlaneLocality,
    NetworkEndpointCapabilitySet, NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
    NetworkLeaseEpoch, NetworkLifecycleCapabilitySet, NetworkManagementMode, NetworkPlan,
    NetworkPlanContentDigest, NetworkPlanId, NetworkProviderHandle, NetworkProviderId,
    NetworkReadinessDependency, NetworkReadinessEvaluationError, NetworkReadinessEvidence,
    NetworkReadinessRequirement, NetworkResourceGeneration, NetworkResourceVersion,
    NetworkSovereigntyRequirements, PortBindClaim, PortBindRealm, PortBindTarget,
    PortBindingProvenance, PortBindingSpec, PortBoundEndpoint, PortExposure, PortLeaseAccounting,
    PortLeaseBinding, PortLeaseEffectScope, PortLeaseFence, PortLeaseLifetimeGuard, PortLeasePhase,
    PortLeaseRecoveryAttempt, PortLeaseRequest, PortProtocol, PortPublicationIntent,
    PortRequestMode,
};

const PORT: u16 = 41_473;

struct ActiveLease {
    _root: tempfile::TempDir,
    authority: LocalPortLeaseAuthority,
    request: PortLeaseRequest,
    record: nimbus_network::PortLeaseRecord,
    lifetime: PortLeaseLifetimeGuard,
}

#[test]
fn direct_and_pep_required_plans_have_distinct_digests() {
    let requirement = pep_requirement(provider_id());
    let direct = plan_with_requirements([]);
    let pep_required = plan_with_requirements([requirement]);

    assert_ne!(
        direct.digest(),
        pep_required.digest(),
        "a proxy-required plan must not share desired identity with a direct plan"
    );
    assert_eq!(
        pep_required.digest().to_string(),
        "e3653495e8aa1fcb5a622ef24f7dbfa6bf0553455c4d56cfd7e511d49f4c00ed",
        "the PEP-required desired digest is a pinned wire contract"
    );
}

#[test]
fn equal_generation_changed_readiness_requirement_is_a_content_conflict() {
    let current = plan_with_requirements([pep_requirement(provider_id())]);
    let candidate = plan_with_requirements([pep_requirement(foreign_provider_id())]);

    assert!(matches!(
        current.classify_update(&candidate),
        Err(nimbus_network::NetworkPlanUpdateError::EqualGenerationContentConflict {
            generation
        }) if generation == NetworkResourceGeneration::new(7)
    ));
}

#[test]
fn exact_current_dependency_and_evidence_satisfy_the_required_plan() {
    let active = active_lease();
    let requirement = pep_requirement(provider_id());
    let plan = plan_with_requirements([requirement.clone()]);
    let dependency = dependency(&plan, requirement, &active.record);
    let evidence = ready_evidence(dependency.clone());

    plan.evaluate_readiness(
        std::slice::from_ref(&dependency),
        std::slice::from_ref(&evidence),
    )
    .expect("one exact current dependency and true evidence should satisfy");
}

#[test]
fn dependency_constructor_rejects_requirement_absent_from_plan() {
    let active = active_lease();
    let requirement = pep_requirement(provider_id());
    let plan = plan_with_requirements([]);
    let version = NetworkResourceVersion::for_plan(
        &plan,
        requirement.resource_id().clone(),
        active.record.request().lease_epoch(),
    );

    assert!(matches!(
        NetworkReadinessDependency::new(
            &plan,
            requirement,
            version,
            &active.record,
            active.lifetime.lifetime(),
        ),
        Err(nimbus_network::NetworkReadinessDependencyError::RequirementNotInPlan)
    ));
}

#[test]
fn provider_distinct_requirements_evaluate_independently() {
    let active = active_lease();
    let local_requirement = pep_requirement(provider_id());
    let foreign_requirement = pep_requirement(foreign_provider_id());
    let plan = plan_with_requirements([local_requirement.clone(), foreign_requirement.clone()]);
    let foreign_record = mutate_record(&active.record, |wire| {
        wire["binding"]["provider_handle"]["provider_id"] =
            serde_json::json!(foreign_provider_id().to_string());
    });
    let local_dependency = dependency(&plan, local_requirement, &active.record);
    let foreign_dependency = dependency(&plan, foreign_requirement.clone(), &foreign_record);
    let local_evidence = ready_evidence(local_dependency.clone());
    let foreign_evidence = ready_evidence(foreign_dependency.clone());

    plan.evaluate_readiness(
        &[local_dependency.clone(), foreign_dependency.clone()],
        &[local_evidence.clone(), foreign_evidence],
    )
    .expect("provider-distinct desired requirements should satisfy independently");

    let foreign_not_ready = NetworkReadinessEvidence::new(
        foreign_dependency,
        NetworkCondition::new(NetworkConditionKind::Ready, NetworkConditionState::False),
    )
    .expect("matching false evidence should remain an honest observation");
    assert!(matches!(
        plan.evaluate_readiness(
            &[local_dependency, foreign_not_ready.dependency().clone()],
            &[local_evidence, foreign_not_ready],
        ),
        Err(NetworkReadinessEvaluationError::ConditionUnsatisfied {
            requirement,
            state: NetworkConditionState::False,
        }) if requirement == foreign_requirement
    ));
}

#[test]
fn old_lifetime_evidence_does_not_satisfy_a_replacement_lifetime() {
    let active = active_lease();
    let requirement = pep_requirement(provider_id());
    let plan = plan_with_requirements([requirement.clone()]);
    let old_dependency = dependency(&plan, requirement.clone(), &active.record);
    let old_evidence = ready_evidence(old_dependency);
    let old_lifetime = active.lifetime.lifetime();
    let old_binding = active
        .record
        .binding()
        .cloned()
        .expect("active fixture should retain exact binding evidence");

    let ActiveLease {
        _root,
        authority,
        request,
        lifetime,
        ..
    } = active;
    drop(lifetime);
    let PortLeaseRecoveryAttempt::Acquired(recovery) = authority
        .recover_dead_lifetime(&request)
        .expect("dead process lifetime should be recoverable")
    else {
        panic!("released lifetime lock must yield recovery authority");
    };
    authority
        .mark_cleanup_pending_after_owner_death(&request, &recovery)
        .expect("dead lifetime should enter cleanup pending");
    authority
        .prepare_rebind_process_bound_after_owner_death(&request, &recovery)
        .expect("dead process-bound listener should retain its exact slot");
    drop(recovery);

    let replacement_claim = bind_claim("replacement-attempt");
    let replacement_lifetime = authority
        .claim_bind_with_lifetime(
            &request,
            None,
            replacement_claim.clone(),
            PortLeaseEffectScope::ProcessBound,
        )
        .expect("replacement should claim a higher process lifetime");
    assert!(
        replacement_lifetime.lifetime().generation() > old_lifetime.generation(),
        "replacement must fence the dead process generation"
    );
    let replacement_record = authority
        .adopt_claimed_and_activate_with_lifetime(
            &request,
            None,
            &replacement_claim,
            old_binding,
            &replacement_lifetime,
        )
        .expect("replacement should activate under its exact lifetime");
    let replacement_dependency = dependency(&plan, requirement, &replacement_record);

    plan.evaluate_readiness(
        std::slice::from_ref(&replacement_dependency),
        std::slice::from_ref(&old_evidence),
    )
    .expect_err("old-lifetime readiness evidence must not satisfy the replacement lifetime");

    drop(replacement_lifetime);
    drop(_root);
}

#[test]
fn duplicate_and_conflicting_evidence_are_rejected() {
    let active = active_lease();
    let requirement = pep_requirement(provider_id());
    let plan = plan_with_requirements([requirement.clone()]);
    let dependency = dependency(&plan, requirement, &active.record);
    let ready = ready_evidence(dependency.clone());
    let conflicting = NetworkReadinessEvidence::new(
        dependency.clone(),
        NetworkCondition::new(NetworkConditionKind::Ready, NetworkConditionState::False),
    )
    .expect("matching false evidence should be a valid honest observation");

    plan.evaluate_readiness(
        std::slice::from_ref(&dependency),
        &[ready.clone(), ready.clone()],
    )
    .expect_err("duplicate evidence must fail closed");
    plan.evaluate_readiness(std::slice::from_ref(&dependency), &[ready, conflicting])
        .expect_err("conflicting evidence must fail closed");
}

#[test]
fn duplicate_desired_requirements_are_rejected() {
    let requirement = pep_requirement(provider_id());
    NetworkPlan::new(
        plan_id(),
        NetworkResourceGeneration::new(7),
        NetworkPlanContentDigest::sha256(b"nnc4.5-readiness-dependency"),
        capability_requirements(),
    )
    .with_readiness_requirements([requirement.clone(), requirement])
    .expect_err("duplicate desired requirements must not canonicalize silently");
}

#[test]
fn requirement_order_is_canonical_on_construction_and_wire() {
    let first = pep_requirement(provider_id());
    let second = NetworkReadinessRequirement::new(
        foreign_listener_id().into(),
        provider_id(),
        NetworkConditionKind::Ready,
    );
    let forward = plan_with_requirements([first.clone(), second.clone()]);
    let reversed = plan_with_requirements([second, first]);

    assert_eq!(forward, reversed);
    assert_eq!(forward.digest(), reversed.digest());
    assert_eq!(
        serde_json::to_vec(&forward).expect("canonical plan should serialize"),
        serde_json::to_vec(&reversed).expect("reordered plan should serialize identically")
    );
}

#[test]
fn dependency_constructor_rejects_every_inexact_authority_field() {
    let active = active_lease();
    let requirement = pep_requirement(provider_id());
    let plan = plan_with_requirements([requirement.clone()]);
    let version = NetworkResourceVersion::for_plan(
        &plan,
        requirement.resource_id().clone(),
        active.record.request().lease_epoch(),
    );
    let lifetime = active.lifetime.lifetime();

    let wrong_plan = mutate_version(&version, |wire| {
        wire["plan_id"] = serde_json::json!("netplan_01ARZ3NDEKTSV4RRFFQ69G5FAW");
    });
    assert!(matches!(
        NetworkReadinessDependency::new(
            &plan,
            requirement.clone(),
            wrong_plan,
            &active.record,
            lifetime,
        ),
        Err(nimbus_network::NetworkReadinessDependencyError::PlanIdentityMismatch)
    ));

    let wrong_resource = mutate_version(&version, |wire| {
        wire["resource_id"]["id"] = serde_json::json!(foreign_listener_id().to_string());
    });
    assert!(matches!(
        NetworkReadinessDependency::new(
            &plan,
            requirement.clone(),
            wrong_resource,
            &active.record,
            lifetime,
        ),
        Err(nimbus_network::NetworkReadinessDependencyError::ResourceIdentityMismatch)
    ));

    let wrong_generation = mutate_version(&version, |wire| {
        wire["generation"] = serde_json::json!(8);
    });
    assert!(matches!(
        NetworkReadinessDependency::new(
            &plan,
            requirement.clone(),
            wrong_generation,
            &active.record,
            lifetime,
        ),
        Err(
            nimbus_network::NetworkReadinessDependencyError::PlanGenerationMismatch {
                expected,
                candidate,
            }
        ) if expected == NetworkResourceGeneration::new(7)
            && candidate == NetworkResourceGeneration::new(8)
    ));

    let wrong_digest = mutate_version(&version, |wire| {
        wire["plan_digest"] =
            serde_json::json!("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");
    });
    assert!(matches!(
        NetworkReadinessDependency::new(
            &plan,
            requirement.clone(),
            wrong_digest,
            &active.record,
            lifetime,
        ),
        Err(nimbus_network::NetworkReadinessDependencyError::PlanDigestMismatch)
    ));

    let inactive = mutate_record(&active.record, |wire| {
        wire["phase"] = serde_json::json!("binding");
    });
    assert!(matches!(
        NetworkReadinessDependency::new(
            &plan,
            requirement.clone(),
            version.clone(),
            &inactive,
            lifetime,
        ),
        Err(
            nimbus_network::NetworkReadinessDependencyError::LeaseNotActive {
                phase: PortLeasePhase::Binding,
            }
        )
    ));

    let wrong_owner = mutate_record(&active.record, |wire| {
        wire["request"]["owner_id"]["id"] = serde_json::json!(foreign_listener_id().to_string());
    });
    assert!(matches!(
        NetworkReadinessDependency::new(
            &plan,
            requirement.clone(),
            version.clone(),
            &wrong_owner,
            lifetime,
        ),
        Err(nimbus_network::NetworkReadinessDependencyError::LeaseOwnerMismatch)
    ));

    let wrong_lease_generation = mutate_record(&active.record, |wire| {
        wire["request"]["generation"] = serde_json::json!(8);
    });
    assert!(matches!(
        NetworkReadinessDependency::new(
            &plan,
            requirement.clone(),
            version.clone(),
            &wrong_lease_generation,
            lifetime,
        ),
        Err(
            nimbus_network::NetworkReadinessDependencyError::LeaseGenerationMismatch {
                expected,
                candidate,
            }
        ) if expected == NetworkResourceGeneration::new(7)
            && candidate == NetworkResourceGeneration::new(8)
    ));

    let wrong_epoch = mutate_record(&active.record, |wire| {
        wire["request"]["lease_epoch"] = serde_json::json!(12);
    });
    assert!(matches!(
        NetworkReadinessDependency::new(
            &plan,
            requirement.clone(),
            version.clone(),
            &wrong_epoch,
            lifetime,
        ),
        Err(nimbus_network::NetworkReadinessDependencyError::LeaseEpochMismatch)
    ));

    let missing_binding = mutate_record(&active.record, |wire| {
        wire["binding"] = serde_json::Value::Null;
    });
    assert!(matches!(
        NetworkReadinessDependency::new(
            &plan,
            requirement.clone(),
            version.clone(),
            &missing_binding,
            lifetime,
        ),
        Err(nimbus_network::NetworkReadinessDependencyError::MissingBinding)
    ));

    let wrong_provider = mutate_record(&active.record, |wire| {
        wire["binding"]["provider_handle"]["provider_id"] =
            serde_json::json!(foreign_provider_id().to_string());
    });
    assert!(matches!(
        NetworkReadinessDependency::new(
            &plan,
            requirement.clone(),
            version.clone(),
            &wrong_provider,
            lifetime,
        ),
        Err(nimbus_network::NetworkReadinessDependencyError::ProviderMismatch)
    ));

    let missing_lifetime = mutate_record(&active.record, |wire| {
        wire["active_lifetime"] = serde_json::Value::Null;
    });
    assert!(matches!(
        NetworkReadinessDependency::new(
            &plan,
            requirement.clone(),
            version.clone(),
            &missing_lifetime,
            lifetime,
        ),
        Err(nimbus_network::NetworkReadinessDependencyError::MissingLifetime)
    ));

    let wrong_lifetime = mutate_lifetime(lifetime, |wire| {
        wire["generation"] = serde_json::json!(lifetime.generation().as_u64() + 1);
    });
    assert!(matches!(
        NetworkReadinessDependency::new(
            &plan,
            requirement,
            version,
            &active.record,
            wrong_lifetime,
        ),
        Err(nimbus_network::NetworkReadinessDependencyError::LifetimeMismatch)
    ));
}

#[test]
fn missing_false_unknown_and_foreign_evidence_fail_closed() {
    let active = active_lease();
    let requirement = pep_requirement(provider_id());
    let plan = plan_with_requirements([requirement.clone()]);
    let dependency = dependency(&plan, requirement, &active.record);

    assert!(matches!(
        plan.evaluate_readiness(&[], &[]),
        Err(NetworkReadinessEvaluationError::MissingDependency { .. })
    ));
    assert!(matches!(
        plan.evaluate_readiness(std::slice::from_ref(&dependency), &[]),
        Err(NetworkReadinessEvaluationError::MissingEvidence { .. })
    ));

    for state in [NetworkConditionState::False, NetworkConditionState::Unknown] {
        let observation = NetworkReadinessEvidence::new(
            dependency.clone(),
            NetworkCondition::new(NetworkConditionKind::Ready, state),
        )
        .expect("honest non-true evidence should construct");
        assert!(matches!(
            plan.evaluate_readiness(
                std::slice::from_ref(&dependency),
                std::slice::from_ref(&observation),
            ),
            Err(NetworkReadinessEvaluationError::ConditionUnsatisfied {
                state: observed,
                ..
            }) if observed == state
        ));
    }

    let foreign = mutate_dependency(&dependency, |wire| {
        let foreign = foreign_listener_id().to_string();
        wire["requirement"]["resource_id"]["id"] = serde_json::json!(foreign);
        wire["version"]["resource_id"]["id"] = serde_json::json!(foreign_listener_id().to_string());
    });
    let foreign = ready_evidence(foreign);
    assert!(matches!(
        plan.evaluate_readiness(
            std::slice::from_ref(&dependency),
            std::slice::from_ref(&foreign),
        ),
        Err(NetworkReadinessEvaluationError::ForeignEvidence { .. })
    ));
}

#[test]
fn evidence_rejects_every_stale_or_substituted_fence() {
    let active = active_lease();
    let requirement = pep_requirement(provider_id());
    let plan = plan_with_requirements([requirement.clone()]);
    let dependency = dependency(&plan, requirement, &active.record);

    let cases = [
        (
            mutate_dependency(&dependency, |wire| {
                wire["version"]["plan_id"] =
                    serde_json::json!("netplan_01ARZ3NDEKTSV4RRFFQ69G5FAW");
            }),
            NetworkReadinessEvaluationError::PlanIdentityMismatch,
        ),
        (
            mutate_dependency(&dependency, |wire| {
                wire["version"]["generation"] = serde_json::json!(6);
            }),
            NetworkReadinessEvaluationError::StaleGeneration {
                desired: NetworkResourceGeneration::new(7),
                candidate: NetworkResourceGeneration::new(6),
            },
        ),
        (
            mutate_dependency(&dependency, |wire| {
                wire["version"]["generation"] = serde_json::json!(8);
            }),
            NetworkReadinessEvaluationError::FutureGeneration {
                desired: NetworkResourceGeneration::new(7),
                candidate: NetworkResourceGeneration::new(8),
            },
        ),
        (
            mutate_dependency(&dependency, |wire| {
                wire["version"]["plan_digest"] = serde_json::json!(
                    "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
                );
            }),
            NetworkReadinessEvaluationError::PlanDigestMismatch,
        ),
        (
            mutate_dependency(&dependency, |wire| {
                wire["version"]["lease_epoch"] = serde_json::json!(12);
            }),
            NetworkReadinessEvaluationError::LeaseEpochMismatch,
        ),
        (
            mutate_dependency(&dependency, |wire| {
                wire["port_lease_id"] = serde_json::json!(
                    nimbus_network::PortLeaseId::for_listener(&foreign_listener_id()).to_string()
                );
            }),
            NetworkReadinessEvaluationError::PortLeaseMismatch,
        ),
        (
            mutate_dependency(&dependency, |wire| {
                wire["requirement"]["provider_id"] =
                    serde_json::json!(foreign_provider_id().to_string());
            }),
            NetworkReadinessEvaluationError::ProviderMismatch,
        ),
        (
            mutate_dependency(&dependency, |wire| {
                wire["lifetime"]["generation"] =
                    serde_json::json!(dependency.lifetime().generation().as_u64() + 1);
            }),
            NetworkReadinessEvaluationError::LifetimeMismatch,
        ),
    ];

    for (candidate, expected) in cases {
        let observation = ready_evidence(candidate);
        assert_eq!(
            plan.evaluate_readiness(
                std::slice::from_ref(&dependency),
                std::slice::from_ref(&observation),
            ),
            Err(expected)
        );
    }
}

#[test]
fn projection_loss_does_not_mutate_desired_or_durable_authority() {
    let active = active_lease();
    let requirement = pep_requirement(provider_id());
    let plan = plan_with_requirements([requirement.clone()]);
    let dependency = dependency(&plan, requirement, &active.record);
    let before_plan = serde_json::to_vec(&plan).expect("plan should serialize");
    let before_dependency = serde_json::to_vec(&dependency).expect("dependency should serialize");
    let before_lease = serde_json::to_vec(&active.record).expect("lease should serialize");

    assert!(matches!(
        plan.evaluate_readiness(std::slice::from_ref(&dependency), &[]),
        Err(NetworkReadinessEvaluationError::MissingEvidence { .. })
    ));
    assert_eq!(
        serde_json::to_vec(&plan).expect("plan should remain serializable"),
        before_plan
    );
    assert_eq!(
        serde_json::to_vec(&dependency).expect("dependency should remain serializable"),
        before_dependency
    );
    assert_eq!(
        serde_json::to_vec(&active.record).expect("lease should remain serializable"),
        before_lease
    );
}

fn plan_with_requirements(
    requirements: impl IntoIterator<Item = NetworkReadinessRequirement>,
) -> NetworkPlan {
    NetworkPlan::new(
        plan_id(),
        NetworkResourceGeneration::new(7),
        NetworkPlanContentDigest::sha256(b"nnc4.5-readiness-dependency"),
        capability_requirements(),
    )
    .with_readiness_requirements(requirements)
    .expect("distinct readiness requirements should form one canonical plan")
}

fn dependency(
    plan: &NetworkPlan,
    requirement: NetworkReadinessRequirement,
    record: &nimbus_network::PortLeaseRecord,
) -> NetworkReadinessDependency {
    let version = NetworkResourceVersion::for_plan(
        plan,
        requirement.resource_id().clone(),
        record.request().lease_epoch(),
    );
    NetworkReadinessDependency::new(
        plan,
        requirement,
        version,
        record,
        record
            .active_lifetime()
            .expect("active fixture should retain an exact lifetime"),
    )
    .expect("exact active listener state should form a durable dependency")
}

fn ready_evidence(dependency: NetworkReadinessDependency) -> NetworkReadinessEvidence {
    NetworkReadinessEvidence::new(
        dependency,
        NetworkCondition::new(NetworkConditionKind::Ready, NetworkConditionState::True),
    )
    .expect("matching ready evidence should construct")
}

fn active_lease() -> ActiveLease {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let request = port_lease_request();
    authority
        .reserve(request.clone())
        .expect("fixture request should reserve");
    let claim = bind_claim("initial-attempt");
    let lifetime = authority
        .claim_bind_with_lifetime(
            &request,
            None,
            claim.clone(),
            PortLeaseEffectScope::ProcessBound,
        )
        .expect("fixture should claim a process lifetime");
    let record = authority
        .adopt_claimed_and_activate_with_lifetime(
            &request,
            None,
            &claim,
            binding("initial-binding"),
            &lifetime,
        )
        .expect("fixture should activate under its exact lifetime");
    assert_eq!(record.phase(), PortLeasePhase::Active);
    ActiveLease {
        _root: root,
        authority,
        request,
        record,
        lifetime,
    }
}

fn port_lease_request() -> PortLeaseRequest {
    PortLeaseRequest::new(
        nimbus_network::PortLeaseId::for_listener(&listener_id()),
        listener_id().into(),
        None,
        PortLeaseFence::new(
            NetworkResourceGeneration::new(7),
            NetworkLeaseEpoch::new(11),
        ),
        PortLeaseAccounting::HostInternal,
        PortPublicationIntent::Unpublished,
        PortBindingSpec::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_wildcard(),
            PortExposure::Private,
            PortRequestMode::Exact(nonzero_port()),
        ),
    )
}

fn binding(opaque: &str) -> PortLeaseBinding {
    PortLeaseBinding::new(
        PortBoundEndpoint::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_wildcard(),
            nonzero_port(),
        )
        .expect("fixture endpoint should validate"),
        PortBindingProvenance::NimbusOwned,
        provider_handle(opaque),
    )
}

fn bind_claim(opaque: &str) -> PortBindClaim {
    PortBindClaim::new(provider_handle(opaque))
}

fn provider_handle(opaque: &str) -> NetworkProviderHandle {
    NetworkProviderHandle::new(provider_id(), opaque)
        .expect("fixture provider handle should validate")
}

fn pep_requirement(provider_id: NetworkProviderId) -> NetworkReadinessRequirement {
    NetworkReadinessRequirement::new(
        listener_id().into(),
        provider_id,
        NetworkConditionKind::Ready,
    )
}

fn capability_requirements() -> NetworkCapabilityRequirements {
    NetworkCapabilityRequirements::new(
        NetworkAttachmentCapabilitySet::new(
            NetworkManagementMode::NimbusHostManaged,
            BTreeSet::new(),
            BTreeSet::new(),
        ),
        NetworkEndpointCapabilitySet::new(
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
        ),
        NetworkIngressCapabilitySet::new(BTreeSet::new()),
        NetworkForwardingCapabilitySet::new(BTreeSet::new()),
        NetworkLifecycleCapabilitySet::new(BTreeSet::new()),
        NetworkSovereigntyRequirements::new(
            NetworkControlPlaneLocality::LocalOnly,
            BTreeSet::new(),
            true,
        ),
    )
}

fn plan_id() -> NetworkPlanId {
    "netplan_01ARZ3NDEKTSV4RRFFQ69G5FAV"
        .parse()
        .expect("fixture plan ID should parse")
}

fn listener_id() -> nimbus_network::ListenerId {
    "netlistener_01ARZ3NDEKTSV4RRFFQ69G5FAV"
        .parse()
        .expect("fixture listener ID should parse")
}

fn provider_id() -> NetworkProviderId {
    NetworkProviderId::for_registration_key("nimbus-sandbox.egress-pep")
}

fn foreign_provider_id() -> NetworkProviderId {
    NetworkProviderId::for_registration_key("fixture.foreign-egress-pep")
}

fn foreign_listener_id() -> nimbus_network::ListenerId {
    "netlistener_01ARZ3NDEKTSV4RRFFQ69G5FAW"
        .parse()
        .expect("foreign fixture listener ID should parse")
}

fn mutate_version(
    version: &NetworkResourceVersion,
    mutate: impl FnOnce(&mut serde_json::Value),
) -> NetworkResourceVersion {
    mutate_wire(version, mutate)
}

fn mutate_record(
    record: &nimbus_network::PortLeaseRecord,
    mutate: impl FnOnce(&mut serde_json::Value),
) -> nimbus_network::PortLeaseRecord {
    mutate_wire(record, mutate)
}

fn mutate_lifetime(
    lifetime: nimbus_network::PortLeaseLifetime,
    mutate: impl FnOnce(&mut serde_json::Value),
) -> nimbus_network::PortLeaseLifetime {
    mutate_wire(&lifetime, mutate)
}

fn mutate_dependency(
    dependency: &NetworkReadinessDependency,
    mutate: impl FnOnce(&mut serde_json::Value),
) -> NetworkReadinessDependency {
    mutate_wire(dependency, mutate)
}

fn mutate_wire<T>(value: &T, mutate: impl FnOnce(&mut serde_json::Value)) -> T
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    let mut wire = serde_json::to_value(value).expect("fixture should serialize");
    mutate(&mut wire);
    serde_json::from_value(wire).expect("mutated fixture should retain a valid wire shape")
}

fn nonzero_port() -> NonZeroU16 {
    NonZeroU16::new(PORT).expect("fixture port should be nonzero")
}
