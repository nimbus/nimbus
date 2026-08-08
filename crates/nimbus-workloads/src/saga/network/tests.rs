use nimbus_core::{TenantId, WorkloadId};
use nimbus_network::{
    NetworkAttachmentCapabilitySet, NetworkCapabilityRequirements, NetworkControlPlaneLocality,
    NetworkEndpointCapabilitySet, NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
    NetworkLifecycleCapabilitySet, NetworkManagementMode, NetworkProviderId,
    NetworkResourceGeneration, NetworkSovereigntyRequirements, PublishedEndpointId,
};
use nimbus_tenant::TenantIsolationDecisionId;
use serde_json::json;

use super::*;
use crate::WorkloadExecutionProviderId;
use crate::{
    DesiredWorkloadKind, DesiredWorkloadState, NodeIdentity, TenantWorkloadUid,
    WorkloadActivationIntent, WorkloadAdmissionEvidence, WorkloadEffectReferences,
    WorkloadExecutableEncoding, WorkloadExecutableIntent, WorkloadGeneration,
    WorkloadNetworkDependencyListenerBlueprint, WorkloadNetworkPlanContent,
    WorkloadNetworkPlanIdentity, WorkloadPhaseDetail, WorkloadProvisionSourceEvidence,
    WorkloadProvisionSourceGeneration, WorkloadProvisionSourceIdentity,
    WorkloadProvisionSourceResourceVersion, WorkloadPublicationIntent,
    WorkloadPublicationReference, WorkloadSagaError, WorkloadSagaIntentUpdate, WorkloadSagaKey,
    WorkloadSagaPhase, WorkloadSagaRecord,
};

fn tenant(label: &str) -> TenantId {
    TenantId::new(label).expect("fixture tenant should validate")
}

fn digest_text(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

fn compiled_plan(
    tenant_id: &TenantId,
    workload_label: &str,
    generation: u64,
    activation: WorkloadActivationIntent,
    publication: WorkloadPublicationIntent,
    seed: u8,
) -> CompiledWorkloadNetworkPlan {
    let identity = WorkloadNetworkPlanIdentity::new(
        tenant_id.clone(),
        workload_label,
        NetworkResourceGeneration::new(generation),
    )
    .expect("network identity should validate");
    let requirements = NetworkCapabilityRequirements::new(
        NetworkAttachmentCapabilitySet::new(NetworkManagementMode::NimbusHostManaged, [], []),
        NetworkEndpointCapabilitySet::new([], [], [], [], []),
        NetworkIngressCapabilitySet::new([]),
        NetworkForwardingCapabilitySet::new([]),
        nimbus_network::NetworkLifecycleRequirements::new(
            NetworkLifecycleCapabilitySet::new([]),
            NetworkLifecycleCapabilitySet::new([]),
        ),
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let dependency = WorkloadNetworkDependencyListenerBlueprint::new(
        &identity,
        format!("dependency-{seed}"),
        NetworkProviderId::for_registration_key(&format!("provider-{seed}")),
    )
    .expect("network dependency should validate");
    let content = WorkloadNetworkPlanContent::new(
        identity,
        requirements,
        None,
        None,
        None,
        [],
        [],
        [dependency],
        activation,
        publication,
    )
    .expect("network content should validate");
    CompiledWorkloadNetworkPlan::from_content(content).expect("network plan should compile")
}

fn saga_intent(
    tenant_id: &TenantId,
    workload_label: &str,
    workload_generation: u64,
    network_generation: u64,
    activation: WorkloadActivationIntent,
    publication: WorkloadPublicationIntent,
    seed: u8,
) -> Result<WorkloadSagaIntent, WorkloadSagaError> {
    let executable = WorkloadExecutableIntent::new(
        WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1,
        format!(r#"{{"fixtureSeed":{seed}}}"#),
    )?;
    let source = WorkloadProvisionSourceEvidence::standalone_sandbox(
        WorkloadProvisionSourceIdentity::standalone_sandbox(workload_label, workload_label)?,
        WorkloadProvisionSourceGeneration::new(workload_generation),
        WorkloadProvisionSourceResourceVersion::new(format!("fixture-{seed}"))?,
        executable.content_digest(),
        NetworkProviderId::for_registration_key(&format!("provider-{seed}")),
        WorkloadExecutionProviderId::for_registration_key(&format!("execution-{seed}")),
    )?;
    WorkloadSagaIntent::new(
        DesiredWorkloadKind::Sandbox,
        DesiredWorkloadState::Running,
        WorkloadGeneration::new(workload_generation),
        executable,
        source,
        WorkloadNetworkIntent::new(compiled_plan(
            tenant_id,
            workload_label,
            network_generation,
            activation,
            publication,
            seed,
        )),
        activation,
        publication,
        WorkloadAdmissionEvidence::new(
            format!("tid_{}", digest_text(seed))
                .try_into()
                .expect("decision should validate"),
            format!("twu_{}", digest_text(seed))
                .try_into()
                .expect("workload uid should validate"),
            NodeIdentity::new(format!("node-{seed}")).expect("node should validate"),
        ),
    )
}

#[test]
fn network_intent_retains_the_complete_compiled_plan() {
    let intent = saga_intent(
        &tenant("tenant-network-carrier"),
        "workload-network-carrier",
        7,
        7,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
        3,
    )
    .expect("intent should validate");
    let wire = serde_json::to_value(intent.network()).expect("network intent should serialize");
    let missing = [
        "/plan/requirements",
        "/plan/readiness_requirements",
        "/content/identity",
        "/content/listeners",
    ]
    .into_iter()
    .filter(|pointer| wire.pointer(pointer).is_none())
    .collect::<Vec<_>>();

    assert!(
        missing.is_empty(),
        "carrier must retain complete desired network state: {missing:?}"
    );
}

#[test]
fn crossed_workload_and_network_generation_is_rejected() {
    let result = saga_intent(
        &tenant("tenant-network-generation"),
        "workload-network-generation",
        7,
        8,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
        0x83,
    );

    assert!(
        matches!(result, Err(WorkloadSagaError::InvalidIntent(message)) if message == "network generation must match workload generation"),
        "crossed generation must fail before durable intent: {result:?}"
    );
}

#[test]
fn complete_carrier_round_trips_and_derives_its_tuple_at_u64_max() {
    let compiled = compiled_plan(
        &tenant("tenant-network-max"),
        "workload-network-max",
        u64::MAX,
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
        4,
    );
    let expected = compiled.clone();
    let intent = WorkloadNetworkIntent::new(compiled);
    let wire = serde_json::to_value(&intent).expect("carrier should serialize");

    assert_eq!(
        wire.pointer("/plan/generation"),
        Some(&json!(u64::MAX.to_string()))
    );
    assert_eq!(
        wire.pointer("/content/identity/generation"),
        Some(&json!(u64::MAX.to_string()))
    );
    assert_eq!(intent.plan_id(), expected.plan().plan_id());
    assert_eq!(intent.generation(), expected.plan().generation());
    assert_eq!(intent.digest(), expected.plan().digest());
    assert_eq!(
        serde_json::from_value::<WorkloadNetworkIntent>(wire)
            .expect("carrier should deserialize")
            .into_compiled_plan(),
        expected
    );
}

#[test]
fn carrier_strictly_rejects_tuple_partial_unknown_and_non_decimal_shapes() {
    let intent = WorkloadNetworkIntent::new(compiled_plan(
        &tenant("tenant-network-strict"),
        "workload-network-strict",
        9,
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
        5,
    ));
    let exact = serde_json::to_value(&intent).expect("carrier should serialize");
    let tuple = json!({
        "planId": intent.plan_id(),
        "generation": intent.generation().as_u64().to_string(),
        "digest": intent.digest(),
    });
    let candidates = [
        tuple,
        json!({"digest": intent.digest()}),
        json!({"plan": exact["plan"].clone()}),
        json!({"content": exact["content"].clone()}),
        serde_json::Value::Null,
    ];
    for candidate in candidates {
        assert!(serde_json::from_value::<WorkloadNetworkIntent>(candidate).is_err());
    }

    let mut unknown = exact.clone();
    unknown["unknown"] = json!(true);
    assert!(serde_json::from_value::<WorkloadNetworkIntent>(unknown).is_err());

    let mut numeric_generation = exact.clone();
    numeric_generation["plan"]["generation"] = json!(9);
    assert!(serde_json::from_value::<WorkloadNetworkIntent>(numeric_generation).is_err());

    let mut crossed_digest = exact;
    crossed_digest["plan"]["content_digest"] = json!("00".repeat(32));
    assert!(serde_json::from_value::<WorkloadNetworkIntent>(crossed_digest).is_err());
}

#[test]
fn carrier_strictly_rejects_duplicate_fields_at_every_generation_boundary() {
    let intent = WorkloadNetworkIntent::new(compiled_plan(
        &tenant("tenant-network-duplicate-wire"),
        "workload-network-duplicate-wire",
        9,
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
        5,
    ));
    let exact = serde_json::to_value(&intent).expect("carrier should serialize");
    let plan = serde_json::to_string(&exact["plan"]).expect("plan should serialize");
    let content = serde_json::to_string(&exact["content"]).expect("content should serialize");
    let identity =
        serde_json::to_string(&exact["content"]["identity"]).expect("identity should serialize");
    let duplicate_generation = format!("{{\"generation\":\"9\",{}", &plan[1..]);
    let duplicate_identity_generation = format!("{{\"generation\":\"9\",{}", &identity[1..]);
    let duplicate_content_format = format!("{{\"formatVersion\":1,{}", &content[1..]);
    let content_with_duplicate_generation =
        content.replacen(&identity, &duplicate_identity_generation, 1);
    let candidates = [
        format!("{{\"plan\":{plan},\"plan\":{plan},\"content\":{content}}}"),
        format!("{{\"plan\":{plan},\"content\":{content},\"content\":{content}}}"),
        format!("{{\"plan\":{duplicate_generation},\"content\":{content}}}"),
        format!("{{\"plan\":{plan},\"content\":{duplicate_content_format}}}"),
        format!("{{\"plan\":{plan},\"content\":{content_with_duplicate_generation}}}"),
    ];

    for candidate in candidates {
        assert!(
            serde_json::from_str::<WorkloadNetworkIntent>(&candidate).is_err(),
            "duplicate carrier field must fail closed: {candidate}"
        );
    }
}

#[test]
fn saga_intent_rejects_activation_and_publication_crossings() {
    let tenant_id = tenant("tenant-network-correlation");
    let plan = compiled_plan(
        &tenant_id,
        "workload-network-correlation",
        11,
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
        6,
    );
    let admission = || {
        WorkloadAdmissionEvidence::new(
            TenantIsolationDecisionId::try_from(format!("tid_{}", digest_text(6))).unwrap(),
            TenantWorkloadUid::try_from(format!("twu_{}", digest_text(6))).unwrap(),
            NodeIdentity::new("node-correlation").unwrap(),
        )
    };
    let executable = WorkloadExecutableIntent::new(
        WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1,
        r#"{"fixture":"correlation"}"#,
    )
    .expect("fixture executable should validate");
    let source = WorkloadProvisionSourceEvidence::standalone_sandbox(
        WorkloadProvisionSourceIdentity::standalone_sandbox("workload-correlation", "profile")
            .expect("source identity should validate"),
        WorkloadProvisionSourceGeneration::new(11),
        WorkloadProvisionSourceResourceVersion::new("fixture-correlation")
            .expect("source version should validate"),
        executable.content_digest(),
        NetworkProviderId::for_registration_key("provider-correlation"),
        WorkloadExecutionProviderId::for_registration_key("execution-correlation"),
    )
    .expect("source evidence should validate");
    let build = |activation, publication| {
        WorkloadSagaIntent::new(
            DesiredWorkloadKind::Sandbox,
            DesiredWorkloadState::Running,
            WorkloadGeneration::new(11),
            executable.clone(),
            source.clone(),
            WorkloadNetworkIntent::new(plan.clone()),
            activation,
            publication,
            admission(),
        )
    };

    assert!(matches!(
        build(
            WorkloadActivationIntent::ActivateWhenAttached,
            WorkloadPublicationIntent::Withheld
        ),
        Err(WorkloadSagaError::InvalidIntent(
            "network activation must match workload activation"
        ))
    ));
    assert!(matches!(
        build(
            WorkloadActivationIntent::PrepareOnly,
            WorkloadPublicationIntent::PublishWhenReady
        ),
        Err(WorkloadSagaError::InvalidIntent(
            "network publication must match workload publication"
        ))
    ));
}

#[test]
fn phase_references_are_derived_tuples_without_compiled_payloads() {
    let intent = saga_intent(
        &tenant("tenant-network-reference"),
        "workload-network-reference",
        12,
        12,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
        7,
    )
    .expect("intent should validate");
    let network = WorkloadNetworkReference::for_intent(&intent);
    let publication = WorkloadPublicationReference::new([PublishedEndpointId::generate()], &intent)
        .expect("publication reference should validate");
    let network_wire = serde_json::to_value(&network).expect("reference should serialize");
    let publication_wire =
        serde_json::to_value(&publication).expect("publication should serialize");

    assert_eq!(network.plan_id(), intent.network().plan_id());
    assert_eq!(network.generation(), intent.network().generation());
    assert_eq!(network.digest(), intent.network().digest());
    assert_eq!(
        network_wire
            .as_object()
            .expect("reference should be an object")
            .len(),
        3
    );
    assert!(network_wire.get("content").is_none());
    assert_eq!(publication.network(), &network);
    assert!(publication_wire["network"].get("content").is_none());

    let mut unknown_reference = network_wire.clone();
    unknown_reference["unknown"] = json!(true);
    assert!(serde_json::from_value::<WorkloadNetworkReference>(unknown_reference).is_err());
    let mut numeric_reference = network_wire;
    numeric_reference["generation"] = json!(12);
    assert!(serde_json::from_value::<WorkloadNetworkReference>(numeric_reference).is_err());

    let other = saga_intent(
        &tenant("tenant-network-reference"),
        "workload-network-reference",
        12,
        12,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
        8,
    )
    .expect("other intent should validate");
    let crossed = WorkloadEffectReferences::new(
        Some(WorkloadNetworkReference::for_intent(&other)),
        None,
        None,
    );
    assert!(matches!(
        WorkloadPhaseDetail::provision(
            WorkloadSagaPhase::NetworkReserved,
            &intent,
            crossed,
            Vec::new(),
        ),
        Err(WorkloadSagaError::InvalidEvidence(
            "network reference is crossed or stale"
        ))
    ));
}

#[test]
fn record_tenant_and_transition_identity_bind_complete_compiled_content() {
    let key_tenant = tenant("tenant-network-record");
    let key = WorkloadSagaKey::new(
        key_tenant.clone(),
        WorkloadId::new("workload-network-record").unwrap(),
    );
    let first = saga_intent(
        &key_tenant,
        "workload-network-record",
        13,
        13,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
        9,
    )
    .expect("first intent should validate");
    let divergent = saga_intent(
        &key_tenant,
        "workload-network-record",
        13,
        13,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
        10,
    )
    .expect("divergent intent should validate");
    let first_record = WorkloadSagaRecord::new(key.clone(), first).unwrap();
    let divergent_record = WorkloadSagaRecord::new(key.clone(), divergent.clone()).unwrap();

    assert_ne!(
        first_record.last_transition().transition_id(),
        divergent_record.last_transition().transition_id()
    );
    assert!(matches!(
        first_record.apply_intent(divergent),
        Err(WorkloadSagaError::EqualGenerationConflict(generation))
            if generation == WorkloadGeneration::new(13)
    ));

    let successor_a = saga_intent(
        &key_tenant,
        "workload-network-record",
        14,
        14,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
        13,
    )
    .unwrap();
    let successor_b = saga_intent(
        &key_tenant,
        "workload-network-record",
        14,
        14,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
        14,
    )
    .unwrap();
    let WorkloadSagaIntentUpdate::Transition(successor_a) =
        first_record.apply_intent(successor_a).unwrap()
    else {
        panic!("successor should transition");
    };
    let WorkloadSagaIntentUpdate::Transition(successor_b) =
        first_record.apply_intent(successor_b).unwrap()
    else {
        panic!("successor should transition");
    };
    assert_ne!(
        successor_a.last_transition().transition_id(),
        successor_b.last_transition().transition_id()
    );

    let crossed_tenant = tenant("tenant-network-crossed");
    let crossed = saga_intent(
        &crossed_tenant,
        "workload-network-record",
        13,
        13,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
        11,
    )
    .expect("crossed intent should be intrinsically valid");
    assert!(matches!(
        WorkloadSagaRecord::new(key, crossed),
        Err(WorkloadSagaError::InvalidIntent(
            "active network plan tenant must match workload saga tenant"
        ))
    ));

    let successor_key = WorkloadSagaKey::new(
        key_tenant.clone(),
        WorkloadId::new("workload-network-record").unwrap(),
    );
    let active = saga_intent(
        &key_tenant,
        "workload-network-record",
        13,
        13,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
        9,
    )
    .unwrap();
    let successor_record = WorkloadSagaRecord::new(successor_key, active).unwrap();
    let crossed_successor = saga_intent(
        &crossed_tenant,
        "workload-network-record",
        14,
        14,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
        12,
    )
    .unwrap();
    assert!(matches!(
        successor_record.apply_intent(crossed_successor),
        Err(WorkloadSagaError::InvalidIntent(
            "successor network plan tenant must match workload saga tenant"
        ))
    ));
}

#[test]
fn saga_v3_rejects_older_and_future_record_versions() {
    let tenant_id = tenant("tenant-network-version");
    let intent = saga_intent(
        &tenant_id,
        "workload-network-version",
        14,
        14,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
        12,
    )
    .expect("intent should validate");
    let record = WorkloadSagaRecord::new(
        WorkloadSagaKey::new(
            tenant_id,
            WorkloadId::new("workload-network-version").unwrap(),
        ),
        intent,
    )
    .unwrap();
    assert_eq!(record.format_version(), 3);

    for version in [1, 2, 4] {
        let mut wire = serde_json::to_value(&record).unwrap();
        wire["formatVersion"] = json!(version);
        assert!(serde_json::from_value::<WorkloadSagaRecord>(wire).is_err());
    }
}
