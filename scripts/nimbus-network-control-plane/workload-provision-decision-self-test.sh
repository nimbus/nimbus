#!/usr/bin/env bash
# Concept-owned fixture and mutation runner for the NNC6.3b contract.

# workload-provision-decision-contract.sh sources this file.
# shellcheck shell=bash
# shellcheck disable=SC2154
fixture_payload() {
  printf '%s\n' \
    "${network_capability_source}" "${network_capability_tests_source}" \
    "${network_registry_source}" "${network_lib_source}" \
    "${network_tests_source}" "${network_manifest_source}" "${workload_network_source}" \
    "${workload_network_tests_source}" "${workload_network_child_tests_source}" \
    "${workload_saga_source}" "${workload_saga_tests_source}" \
    "${workload_provision_source}" "${workload_provision_tests_source}" \
    "${workload_state_source}" "${workload_test_support_source}" \
    "${workload_store_tests_source}" "${workloads_lib_source}" \
    "${workloads_manifest_source}" "${compute_network_source}" \
    "${compute_network_tests_source}" "${compute_composition_source}" \
    "${compute_composition_tests_source}" "${compute_saga_source}" \
    "${compute_saga_tests_source}" "${compute_ingress_source}" \
    "${compute_ingress_tests_source}" "${compute_decision_source}" \
    "${compute_decision_tests_source}" \
    "${compute_test_support_source}" \
    "${compute_recovery_source}" "${compute_recovery_tests_source}" \
    "${compute_lib_source}" "${server_capabilities_source}" \
    "${server_capabilities_tests_source}" "${server_codec_source}" \
    "${server_schema_source}" "${server_codec_tests_source}" \
    "${server_ingress_tests_source}" "${authority_census}" \
    "${caller_census}" "${changed_paths}" "${owner_plan_source}" "${owner_proof_source}"
}

apply_test_mutation() {
  mutation="${NIMBUS_NETWORK_NNC63B_TEST_MUTATION:-}"
  [ -z "${mutation}" ] && return
  before="$(fixture_payload)"
  case "${mutation}" in
    missing-result-vocabulary)
      workload_provision_source="${workload_provision_source/pub enum WorkloadProvisionEffectResult/pub enum RemovedProvisionEffectResult}"
      ;;
    unknown-result-variant)
      workload_provision_source="${workload_provision_source/Ambiguous { attempt_id: WorkloadProvisionAttemptId },/Ambiguous { attempt_id: WorkloadProvisionAttemptId },
    Unknown,}"
      ;;
    crossed-generation)
      compute_composition_tests_source="${compute_composition_tests_source/crossed_workload_generation_rejects_before_submission/removed_crossed_workload_generation_case}"
      ;;
    crossed-node)
      compute_composition_tests_source="${compute_composition_tests_source/crossed_local_node_rejects_before_submission/removed_crossed_local_node_case}"
      ;;
    crossed-selection)
      compute_composition_tests_source="${compute_composition_tests_source/crossed_provider_selection_rejects_before_submission/removed_crossed_provider_selection_case}"
      ;;
    crossed-source)
      compute_composition_tests_source="${compute_composition_tests_source/crossed_source_snapshot_rejects_before_submission/removed_crossed_source_snapshot_case}"
      ;;
    crossed-publication)
      compute_composition_tests_source="${compute_composition_tests_source/crossed_publication_rejects_before_submission/removed_crossed_publication_case}"
      ;;
    crossed-forwarding)
      compute_composition_tests_source="${compute_composition_tests_source/crossed_forwarding_semantics_rejects_before_submission/removed_crossed_forwarding_case}"
      ;;
    crossed-address)
      compute_composition_tests_source="${compute_composition_tests_source/crossed_address_semantics_rejects_before_submission/removed_crossed_address_case}"
      ;;
    crossed-sovereignty)
      compute_composition_tests_source="${compute_composition_tests_source/crossed_sovereignty_rejects_before_submission/removed_crossed_sovereignty_case}"
      ;;
    crossed-tls)
      compute_composition_tests_source="${compute_composition_tests_source/crossed_tls_semantics_rejects_before_submission/removed_crossed_tls_case}"
      ;;
    missing-source-generation)
      workload_provision_source="${workload_provision_source/pub struct WorkloadProvisionSourceGeneration/pub struct RemovedProvisionSourceGeneration}"
      ;;
    missing-resource-version)
      workload_provision_source="${workload_provision_source/pub struct WorkloadProvisionSourceResourceVersion/pub struct RemovedProvisionSourceResourceVersion}"
      ;;
    missing-provider-report-digest)
      network_registry_source="${network_registry_source/source_digest: NetworkCapabilitySourceDigest,/digest_removed: RemovedCapabilityDigest,}"
      ;;
    missing-attempt-fence)
      workload_provision_source="${workload_provision_source/issuing_revision: WorkloadSagaRevision,/revision_removed: RemovedSagaRevision,}"
      ;;
    crossed-attempt-id)
      compute_decision_tests_source="${compute_decision_tests_source/crossed_attempt_id_rejects_without_candidate_or_command/removed_crossed_attempt_id_case}"
      ;;
    wrong-success-evidence)
      compute_decision_tests_source="${compute_decision_tests_source/wrong_success_evidence_rejects_without_state_change/removed_wrong_success_evidence_case}"
      ;;
    missing-prerequisite-distinction)
      workload_provision_source="${workload_provision_source/ActivationPrerequisitesReady/WorkloadReady}"
      ;;
    missing-ready-publication-gate)
      compute_decision_tests_source="${compute_decision_tests_source/publication_is_unreachable_before_workload_readiness/removed_publication_gate_case}"
      ;;
    missing-strict-decode)
      workload_provision_tests_source="${workload_provision_tests_source/unknown_effect_result_variant_is_rejected/removed_unknown_result_case}"
      ;;
    definite-failure-advances)
      compute_decision_source="${compute_decision_source/definite_failure_retains_completed_phase/definite_failure_advances_completed_phase}"
      ;;
    definite-failure-emits-later)
      compute_decision_tests_source="${compute_decision_tests_source/definite_failure_reopen_emits_no_later_command/removed_definite_failure_reopen_case}"
      ;;
    ambiguous-non-inspection)
      compute_decision_tests_source="${compute_decision_tests_source/ambiguous_result_emits_exact_inspection_only/removed_ambiguous_inspection_case}"
      ;;
    ambiguous-loses-correlation)
      compute_decision_tests_source="${compute_decision_tests_source/ambiguous_reopen_retains_exact_attempt_correlation/removed_ambiguous_correlation_case}"
      ;;
    first-available-fallback)
      compute_network_source="${compute_network_source}
fn fallback(registry: &Registry) { let _ = registry.selections().next(); }"
      ;;
    provider-trait)
      compute_decision_source="${compute_decision_source}
pub trait WorkloadProvisionProvider { fn dispatch(&self); }"
      ;;
    product-effect)
      compute_decision_source="${compute_decision_source}
fn leak_effect() { let _ = std::net::TcpStream::connect(\"127.0.0.1:1\"); }"
      ;;
    caller-cutover)
      caller_census="crates/nimbus-services/src/manager.rs:44:compose_workload_provision(input)"
      ;;
    unexpected-path)
      changed_paths="${changed_paths}
crates/nimbus-services/src/manager.rs"
      ;;
    duplicate-coordinator)
      authority_census="${authority_census}
pub struct WorkloadSagaCoordinator;"
      ;;
    duplicate-store)
      authority_census="${authority_census}
pub trait WorkloadSagaStore {}"
      ;;
    forbidden-dependency)
      network_manifest_source="${network_manifest_source}
nimbus-server = { path = \"../nimbus-server\" }"
      ;;
    compatibility-shim)
      workload_provision_source="${workload_provision_source/deny_unknown_fields/deny_unknown_fields, alias = \"legacyProvisionResult\"}"
      ;;
    missing-behavior-proof)
      compute_decision_tests_source="${compute_decision_tests_source/every_provision_phase_and_result_is_exhaustive/removed_exhaustive_matrix_case}"
      ;;
    fixture-support-production-leak)
      workload_saga_source="$(printf '%s\n' "${workload_saga_source}" |
        awk '!removed && $0 == "#[cfg(test)]" { removed = 1; next } { print }')"
      ;;
    loose-attempt-revision-history)
      workload_state_source="${workload_state_source/validate_attempt_revision(record, disposition, attempt)?/removed_exact_attempt_revision_validation(record, disposition, attempt)?}"
      ;;
    *) add_error "unknown NNC6.3b self-test mutation ${mutation}" ;;
  esac
  after="$(fixture_payload)"
  if [ "${before}" = "${after}" ]; then
    add_error "NNC6.3b self-test mutation ${mutation} did not change its fixture"
  fi
}


write_fixture() {
  fixture="$1"
  mkdir -p \
    "${fixture}/crates/nimbus-network/src/capability" \
    "${fixture}/crates/nimbus-network/src/capability_registry" \
    "${fixture}/crates/nimbus-workloads/src/network_plan" \
    "${fixture}/crates/nimbus-workloads/src/saga/network" \
    "${fixture}/crates/nimbus-workloads/src/saga/provision" \
    "${fixture}/crates/nimbus-workloads/src/saga/state" \
    "${fixture}/crates/nimbus-workloads/src/store" \
    "${fixture}/crates/nimbus-compute/src/workload_network_plan" \
    "${fixture}/crates/nimbus-compute/src/workload_provision_composition" \
    "${fixture}/crates/nimbus-compute/src/workload_saga/ingress" \
    "${fixture}/crates/nimbus-compute/src/workload_saga/provision_decision" \
    "${fixture}/crates/nimbus-compute/src/workload_saga/recovery" \
    "${fixture}/crates/nimbus-server/src/network_capabilities" \
    "${fixture}/crates/nimbus-server/src/workload_saga_store/tests" \
    "${fixture}/docs/private/plans/proof/nimbus-network-control-plane" \
    "${fixture}/scripts/nimbus-network-control-plane"
  cp "${SCRIPT_PATH}" "${fixture}/${SCRIPT_PATH#"${REPO_ROOT}"/}"
  cp "${SELF_TEST_SCRIPT_PATH}" "${fixture}/${SELF_TEST_SCRIPT}"

  printf '%s\n' \
    'pub enum NetworkTlsBehavior {' '    Disabled,' '    Passthrough,' '    TerminateAtIngress,' '}' \
    'pub struct NetworkIngressCapabilitySet {' \
    '    tls_behaviors: BTreeSet<NetworkTlsBehavior>,' '}' \
    'impl NetworkIngressCapabilitySet {' \
    '  pub fn new() -> Self { Self { tls_behaviors: BTreeSet::new() } }' \
    '  pub fn tls_behaviors(&self) -> &BTreeSet<NetworkTlsBehavior> { &self.tls_behaviors }' '}' \
    >"${fixture}/${NETWORK_CAPABILITY}"
  printf '%s\n' \
    'pub struct NetworkCapabilitySourceDigest(String);' \
    'pub struct NetworkCapabilitySelectionEvidence {' \
    '  selection: NetworkCapabilitySelection,' \
    '  source_digest: NetworkCapabilitySourceDigest,' '}' \
    "struct SelectionEvidencePayload<'a> {" \
    "  attachment: &'a NetworkAttachmentProviderRegistration," \
    "  ingress: &'a NetworkIngressProviderRegistration," '}' \
    'const DOMAIN: &[u8] = b"nimbus.network.capability.selection.evidence.v1";' \
    'impl NetworkCapabilityBundle {' \
    '  pub fn selection_evidence(&self) -> NetworkCapabilitySelectionEvidence { NetworkCapabilitySelectionEvidence::fixture() }' '}' \
    'impl NetworkCapabilityRegistry {' \
    '  pub fn select_exact(&self, requested: &NetworkCapabilitySelection, requirements: &Requirements) { let safe_alternatives = vec![]; }' \
    '}' >"${fixture}/${NETWORK_REGISTRY}"
  printf '%s\n' 'pub use capability::*;' \
    'pub use capability_registry::{NetworkCapabilitySelectionEvidence, NetworkCapabilitySourceDigest};' \
    >"${fixture}/${NETWORK_LIB}"
  printf '%s\n' \
    'fn empty_ingress_capability_set_has_no_implicit_tls_behavior() {}' \
    'fn empty_tls_evidence_does_not_satisfy_disabled_requirement() {}' \
    >"${fixture}/${NETWORK_CAPABILITY_TESTS}"
  printf '%s\n' 'fn provider_report_digest_binds_complete_selected_reports() {}' \
    >"${fixture}/${NETWORK_TESTS}"
  printf '%s\n' '[dependencies]' 'nimbus-core = { path = "../nimbus-core" }' >"${fixture}/${NETWORK_MANIFEST}"

  printf '%s\n' \
    'pub enum WorkloadNetworkForwardingBehavior {' '    None,' '    PortForwarded,' '}' \
    'pub struct WorkloadNetworkEndpointSemantics {' \
    '  forwarding: WorkloadNetworkForwardingBehavior,' '  tls: NetworkTlsBehavior,' '}' \
    'pub struct WorkloadNetworkListenerBlueprint {' \
    '  endpoint_semantics: WorkloadNetworkEndpointSemantics,' '}' \
    'struct WorkloadNetworkListenerBlueprintWire {' \
    '  endpoint_semantics: WorkloadNetworkEndpointSemantics,' '}' \
    'impl WorkloadNetworkListenerBlueprint {' \
    '  pub const fn endpoint_semantics(&self) -> &WorkloadNetworkEndpointSemantics { &self.endpoint_semantics }' '}' \
    'pub struct WorkloadNetworkPlanContent {' \
    '  capability_selection_evidence: Option<NetworkCapabilitySelectionEvidence>,' '}' \
    'impl WorkloadNetworkPlanContent {' \
    '  pub fn capability_selection_evidence(&self) -> Option<&NetworkCapabilitySelectionEvidence> { self.capability_selection_evidence.as_ref() }' '}' \
    'fn from_wire(wire: Wire) { capability_selection_evidence: wire.capability_selection_evidence; }' \
    >"${fixture}/${WORKLOAD_NETWORK}"
  printf '%s\n' \
    'fn resource_free_plan_has_no_selection_evidence() {}' \
    'fn connected_plan_requires_selection_evidence() {}' \
    'fn endpoint_semantics_reject_missing_extra_duplicate_and_crossed_names() {}' \
    >"${fixture}/${WORKLOAD_NETWORK_TESTS}"
  printf '%s\n' 'fn network_fixture_retains_selection_evidence_shape() {}' \
    >"${fixture}/${WORKLOAD_NETWORK_CHILD_TESTS}"

  printf '%s\n' \
    '#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]' \
    'pub enum WorkloadProvisionSourceEvidence {' \
    '    StandaloneSandbox {' \
    '      source_identity: WorkloadProvisionSourceIdentity,' \
    '      source_generation: WorkloadProvisionSourceGeneration,' \
    '      resource_version: WorkloadProvisionSourceResourceVersion,' \
    '      source_digest: WorkloadProvisionSourceDigest,' \
    '      attachment_provider_id: NetworkProviderId,' '    },' \
    '    SandboxBackedService {' \
    '      source_identity: WorkloadProvisionSourceIdentity,' \
    '      source_generation: WorkloadProvisionSourceGeneration,' \
    '      resource_version: WorkloadProvisionSourceResourceVersion,' \
    '      source_digest: WorkloadProvisionSourceDigest,' \
    '      attachment_provider_id: NetworkProviderId,' '    },' '}' \
    'impl WorkloadProvisionSourceEvidence {' \
    '  pub fn required_workload_kind(&self) -> DesiredWorkloadKind { DesiredWorkloadKind::Sandbox }' '}' \
    'pub struct WorkloadProvisionSourceGeneration(u64);' \
    'pub struct WorkloadProvisionSourceResourceVersion(String);' \
    'pub struct WorkloadProvisionSourceDigest(String);' \
    'const SOURCE_DOMAIN: &[u8] = b"nimbus.workloads.provision.source.digest.v1";' \
    'pub enum WorkloadProvisionStep {' \
    '    ReserveNetwork,' '    PrepareWorkload,' '    AttachNetwork,' \
    '    InspectActivationPrerequisites,' '    ActivateWorkload,' \
    '    InspectWorkloadReadiness,' '    Publish,' '    ObservePublication,' '}' \
    'pub struct WorkloadProvisionAttempt {' \
    '  attempt_id: WorkloadProvisionAttemptId,' '  key: WorkloadSagaKey,' \
    '  saga_id: WorkloadSagaId,' \
    '  issuing_revision: WorkloadSagaRevision,' '  generation: WorkloadGeneration,' \
    '  desired_digest: WorkloadDesiredDigest,' '  required_node: NodeIdentity,' \
    '  source_digest: WorkloadProvisionSourceDigest,' \
    '  network_plan_digest: NetworkPlanDigest,' \
    '  selection_evidence: Option<NetworkCapabilitySelectionEvidence>,' \
    '  source_phase: WorkloadSagaPhase,' '  target_phase: WorkloadSagaPhase,' \
    '  step: WorkloadProvisionStep,' '  subjects: WorkloadProvisionSubjects,' \
    '  prerequisite: Option<WorkloadProvisionPrerequisiteEvidence>,' '}' \
    'impl WorkloadProvisionAttempt {' \
    '  pub fn key(&self) -> &WorkloadSagaKey { &self.key }' \
    '  pub fn prerequisite(&self) -> Option<&WorkloadProvisionPrerequisiteEvidence> { self.prerequisite.as_ref() }' '}' \
    'const ATTEMPT_DOMAIN: &[u8] = b"nimbus.workloads.provision.attempt.id.v1";' \
    'pub enum WorkloadProvisionSuccessEvidence {' \
    '    NetworkReserved,' '    WorkloadPrepared,' '    NetworkAttached,' \
    '    ActivationPrerequisitesReady,' '    WorkloadActivated,' '    WorkloadReady,' \
    '    Published,' '    PublicationObserved,' '}' \
    '#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]' \
    'pub enum WorkloadProvisionEffectResult {' \
    '    Succeeded { attempt_id: WorkloadProvisionAttemptId, evidence: WorkloadProvisionSuccessEvidence },' \
    '    DefiniteFailure { attempt_id: WorkloadProvisionAttemptId, failure: WorkloadProvisionFailure },' \
    '    Ambiguous { attempt_id: WorkloadProvisionAttemptId },' '}' \
    'pub enum WorkloadProvisionDisposition {' \
    '    Ready,' '    AttemptPending(WorkloadProvisionAttempt),' \
    '    InspectionRequired(WorkloadProvisionAttempt),' \
    '    DefiniteFailure { attempt: WorkloadProvisionAttempt, failure: WorkloadProvisionFailure },' '}' \
    >"${fixture}/${WORKLOAD_PROVISION}"
  printf '%s\n' \
    'fn unknown_effect_result_variant_is_rejected() {}' \
    'fn attempt_identity_binds_saga_key_and_prerequisite() {}' \
    'fn attempt_identity_binds_every_named_fence_and_rejects_forged_wire() {}' \
    'fn effect_result_round_trips_exactly_three_strict_variants() {}' \
    'fn resource_free_attempt_has_no_selection_evidence() {}' \
    'fn connected_attempt_requires_selection_evidence() {}' \
    >"${fixture}/${WORKLOAD_PROVISION_TESTS}"
  printf '%s\n' \
    'pub struct WorkloadAdmissionEvidence { assigned_node: NodeIdentity }' \
    'pub struct WorkloadSagaIntent { source: WorkloadProvisionSourceEvidence }' \
    "struct WorkloadDesiredDigestPayload<'a> { source: &'a WorkloadProvisionSourceEvidence }" \
    'impl WorkloadSagaIntent {' \
    '  pub fn source(&self) -> &WorkloadProvisionSourceEvidence { &self.source }' \
    '  pub fn validate(&self) { let _ = self.source.required_workload_kind(); let _ = "desired workload kind does not match provision source kind"; }' '}' \
    'mod provision;' \
    'pub use provision::{WorkloadProvisionEffectResult, WorkloadProvisionDisposition};' \
    '#[cfg(test)]' 'pub(crate) mod test_support;' \
    >"${fixture}/${WORKLOAD_SAGA}"
  printf '%s\n' \
    'fn saga_fixture_compiles_provision_exports() {}' \
    'fn non_provision_record_has_no_provision_disposition() {}' \
    'fn running_provision_record_requires_provision_disposition() {}' \
    'fn provision_disposition_requires_exact_attempt_revision_history() {}' \
    'fn activation_prerequisite_attempt_cannot_complete_activation() {}' \
    'fn activation_attempt_requires_retained_prerequisite_inspection() {}' \
    'fn activation_prerequisite_subjects_must_match_retained_inspection() {}' \
    'fn promoted_generation_requires_exact_initial_provision_disposition() {}' \
    'fn workload_kind_must_match_provision_source_variant() {}' \
    'fn observed_publish_when_ready_requires_publication_observation() {}' \
    >"${fixture}/${WORKLOAD_SAGA_TESTS}"
  printf '%s\n' \
    'pub struct WorkloadSagaRecord { provision_disposition: Option<WorkloadProvisionDisposition> }' \
    "struct TransitionIdentityPayload<'a> { provision_disposition: &'a Option<WorkloadProvisionDisposition> }" \
    'impl WorkloadSagaRecord {' \
    '  pub fn provision_disposition(&self) -> Option<&WorkloadProvisionDisposition> { self.provision_disposition.as_ref() }' \
    '  pub fn transition_provision_disposition(&self) {}' \
    '  pub fn requires_recovery(&self) { match self.provision_disposition { Some(DefiniteFailure) => false, _ => true }; }' '}' \
    'fn validate(record: &WorkloadSagaRecord, disposition: &WorkloadProvisionDisposition, attempt: &WorkloadProvisionAttempt) {' \
    '  validate_attempt_revision(record, disposition, attempt)?;' \
    '  let _ = attempt.step() != WorkloadProvisionStep::ActivateWorkload;' '}' \
    'fn validate_attempt_revision(record: &WorkloadSagaRecord, disposition: &WorkloadProvisionDisposition, attempt: &WorkloadProvisionAttempt) {' \
    '  let after_one = attempt.issuing_revision().checked_next();' \
    '  let after_two = after_one.and_then(WorkloadSagaRevision::checked_next);' \
    '  let after_three = after_two.and_then(WorkloadSagaRevision::checked_next);' '}' \
    >"${fixture}/${WORKLOAD_STATE}"
  printf '%s\n' \
    'fn exact_history(record: &Record) { record.transition_provision_disposition(); }' \
    >"${fixture}/${WORKLOAD_TEST_SUPPORT}"
  printf '%s\n' 'fn store_fixture_retains_optional_provision_disposition() {}' \
    >"${fixture}/${WORKLOAD_STORE_TESTS}"
  printf '%s\n' \
    'pub use saga::{WorkloadProvisionEffectResult, WorkloadProvisionDisposition};' \
    >"${fixture}/${WORKLOADS_LIB}"
  printf '%s\n' '[dependencies]' 'nimbus-core = { path = "../nimbus-core" }' >"${fixture}/${WORKLOADS_MANIFEST}"

  printf '%s\n' \
    'pub struct WorkloadNetworkEndpointSemanticsInput;' \
    'fn validate(input: Input, registry: Registry) {' \
    '  registry.select_exact(input.selection(), input.requirements());' \
    '  let _ = "duplicate endpoint semantics";' '  let _ = "missing endpoint semantics";' \
    '  let _ = "unexpected endpoint semantics";' \
    '  let _ = "listener name must match endpoint semantics";' \
    '  let _ = "forwarding behavior must match guest port shape";' \
    '  let _ = NetworkForwardingFeature::PortForwarding;' \
    '  let _ = WorkloadNetworkForwardingBehavior::PortForwarded;' \
    '  let _ = "TLS behavior must match listener protocol";' \
    '  let _ = NetworkTlsBehavior::Passthrough;' \
    '  let _ = NetworkTlsBehavior::TerminateAtIngress;' \
    '  let _ = input.ingress.tls_behaviors();' '}' \
    >"${fixture}/${COMPUTE_NETWORK}"
  printf '%s\n' 'fn compiler_retains_exact_endpoint_semantics() {}' >"${fixture}/${COMPUTE_NETWORK_TESTS}"
  printf '%s\n' \
    "pub enum WorkloadProvisionSourceSnapshot<'source> {" \
    "    StandaloneSandbox { source: &'source str }," \
    "    SandboxBackedService { source: &'source str }," '}' \
    'pub struct WorkloadProvisionCompositionInput;' 'pub struct ComposedWorkloadProvision {' \
    '  key: WorkloadSagaKey,' '  intent: WorkloadSagaIntent,' '}' \
    'pub fn compose_workload_provision(input: WorkloadProvisionCompositionInput)' \
    '  -> Result<ComposedWorkloadProvision, WorkloadProvisionCompositionError> {' \
    '  let _ = "local node does not match admitted assignment";' \
    '  let _ = encode_sandbox_spec();' \
    '  let _ = WorkloadNetworkPlanCompiler.compile();' \
    '  let _ = WorkloadProvisionSourceEvidence::new();' \
    '  let _ = WorkloadSagaIntent::new();' \
    '  let _ = "running empty source is unsupported";' \
    '  let _ = DesiredWorkloadState::Running;' '  let _ = DesiredWorkloadKind::Empty;' \
    '  Ok(ComposedWorkloadProvision { key: input.key(), intent: WorkloadSagaIntent::new() })' '}' \
    >"${fixture}/${COMPUTE_COMPOSITION}"
  printf '%s\n' \
    'fn crossed_workload_generation_rejects_before_submission() {}' \
    'fn crossed_local_node_rejects_before_submission() {}' \
    'fn crossed_provider_selection_rejects_before_submission() {}' \
    'fn crossed_source_snapshot_rejects_before_submission() {}' \
    'fn crossed_publication_rejects_before_submission() {}' \
    'fn crossed_forwarding_semantics_rejects_before_submission() {}' \
    'fn crossed_address_semantics_rejects_before_submission() {}' \
    'fn crossed_sovereignty_rejects_before_submission() {}' \
    'fn crossed_tls_semantics_rejects_before_submission() {}' \
    >"${fixture}/${COMPUTE_COMPOSITION_TESTS}"
  printf '%s\n' \
    'pub struct WorkloadSagaCoordinator;' \
    'mod provision_decision;' \
    'pub use provision_decision::WorkloadProvisionDecision;' \
    '#[cfg(test)]' 'pub(crate) mod test_support;' \
    >"${fixture}/${COMPUTE_SAGA}"
  printf '%s\n' 'fn saga_fixture_compiles_shared_provision_reducer() {}' \
    >"${fixture}/${COMPUTE_SAGA_TESTS}"
  printf '%s\n' \
    'fn submit(record: WorkloadSagaRecord) { let _ = WorkloadSagaDecision::for_record(&record); }' \
    >"${fixture}/${COMPUTE_INGRESS}"
  printf '%s\n' 'fn ingress_and_recovery_delegate_to_same_provision_reducer() {}' \
    >"${fixture}/${COMPUTE_INGRESS_TESTS}"
  printf '%s\n' \
    'pub enum WorkloadProvisionDecision { Proposed, InspectExact, DefiniteFailure, Wait }' \
    'impl WorkloadProvisionDecision {' \
    '  pub fn plan(record: &WorkloadSagaRecord) {' \
    '    let _ = WorkloadProvisionDisposition::Ready;' \
    '    match record.phase() { WorkloadSagaPhase::IntentCommitted => {} }' '  }' \
    '  pub fn reduce(record: &WorkloadSagaRecord, result: WorkloadProvisionEffectResult) {' \
    '    match result { Succeeded { .. } => {}, DefiniteFailure { .. } => {}, Ambiguous { .. } => {} }' '  }' '}' \
    'fn activation_prerequisite_success_prepares_activation_attempt() { let _ = InspectActivationPrerequisites; }' \
    'fn inspect_workload_readiness() { let _ = InspectWorkloadReadiness; }' \
    'fn definite_failure_retains_completed_phase() { let _ = "definite failure permits no later provision command"; }' \
    'fn effect_scan_ignores_diagnostic_text() { let _ = "std::net::TcpStream::connect"; }' \
    'fn ambiguous_result_requires_exact_inspection() {}' \
    'fn publication_requires_workload_readiness() {}' \
    >"${fixture}/${COMPUTE_DECISION}"
  printf '%s\n' \
    'fn crossed_attempt_id_rejects_without_candidate_or_command() {}' \
    'fn crossed_same_variant_subject_rejects_without_candidate_or_command() {}' \
    'fn wrong_success_evidence_rejects_without_state_change() {}' \
    'fn publication_is_unreachable_before_workload_readiness() {}' \
    'fn definite_failure_reopen_emits_no_later_command() {}' \
    'fn ambiguous_result_emits_exact_inspection_only() {}' \
    'fn ambiguous_reopen_retains_exact_attempt_correlation() {}' \
    'fn publication_observed_success_retains_exact_durable_observation_evidence() {}' \
    'fn every_provision_phase_and_result_is_exhaustive() {}' \
    >"${fixture}/${COMPUTE_DECISION_TESTS}"
  printf '%s\n' \
    'fn history(record: &Record, result: Result) {' \
    '  WorkloadProvisionDecision::plan(record);' \
    '  WorkloadProvisionDecision::reduce(record, result);' '}' \
    >"${fixture}/${COMPUTE_TEST_SUPPORT}"
  printf '%s\n' 'fn recover(record: &Record) { WorkloadProvisionDecision::plan(record); }' \
    >"${fixture}/${COMPUTE_RECOVERY}"
  printf '%s\n' \
    'fn recovery_delegates_provision() {}' \
    'fn ingress_and_recovery_delegate_to_same_provision_reducer() {}' \
    >"${fixture}/${COMPUTE_RECOVERY_TESTS}"
  printf '%s\n' \
    'mod workload_provision_composition;' \
    'pub use workload_provision_composition::compose_workload_provision;' \
    >"${fixture}/${COMPUTE_LIB}"
  printf '%s\n' \
    'fn registration() { let _ = NetworkTlsBehavior::Disabled; let _ = NetworkTlsBehavior::TerminateAtIngress; }' \
    >"${fixture}/${SERVER_CAPABILITIES}"
  printf '%s\n' 'fn production_ingress_capabilities_report_selected_tls_behavior() {}' \
    >"${fixture}/${SERVER_CAPABILITIES_TESTS}"
  printf '%s\n' \
    'fn encode(fields: &mut Fields, portable: &Portable) {' \
    '  copy(&mut fields, "source", active, "source");' \
    '  let _ = "provisionDisposition";' \
    '  copy_optional(&mut fields, "provisionDisposition", portable, "provisionDisposition");' '}' \
    'fn decode(fields: &Fields) {' \
    '  let _ = json!({ "source": required(fields, "source")? });' \
    '  let _ = fields.get("provisionDisposition").cloned();' '}' \
    >"${fixture}/${SERVER_CODEC}"
  printf '%s\n' \
    'fn schema() {' \
    '  let _ = field("source", FieldType::Object, true);' \
    '  let _ = field("provisionDisposition", FieldType::Object, false);' '}' \
    >"${fixture}/${SERVER_SCHEMA}"
  printf '%s\n' \
    'fn provision_source_round_trips_through_physical_codec() {}' \
    'fn provision_disposition_round_trips_through_physical_codec() {}' \
    >"${fixture}/${SERVER_CODEC_TESTS}"
  printf '%s\n' 'fn ingress_fixture_retains_provision_disposition() {}' \
    >"${fixture}/${SERVER_INGRESS_TESTS}"
  printf '%s\n' \
    '| NNC6.3b | After NNC6.3a, implement the pure provision decision protocol and exact admitted composition inputs without product effects or provider interfaces. | frozen |' \
    >"${fixture}/${OWNER_PLAN}"
  printf '%s\n' '# NNC6.3b' '## Acceptance Criteria' >"${fixture}/${OWNER_PROOF}"
  printf '%s\n' 'pub trait WorkloadSagaStore {}' >"${fixture}/crates/nimbus-workloads/src/store.rs"
}

run_self_test() {
  self_test_root="$(mktemp -d "${TMPDIR:-/tmp}/nnc63b-contract-self-test.XXXXXX")" || {
    printf 'NNC6.3b provision contract self-test: unable to create temporary directory\n' >&2
    return 1
  }
  trap 'rm -rf "${self_test_root}"' EXIT
  fixture="${self_test_root}/fixture"
  write_fixture "${fixture}"
  failures=0
  baseline_output="${self_test_root}/baseline.out"
  if ! NIMBUS_NETWORK_NNC63B_ROOT="${fixture}" \
    NIMBUS_NETWORK_NNC63B_TEST_CHANGED_PATHS="scripts/nimbus-network-control-plane/workload-provision-decision-contract.sh" \
    bash "${fixture}/scripts/nimbus-network-control-plane/workload-provision-decision-contract.sh" \
    >"${baseline_output}" 2>&1; then
    printf 'NNC6.3b provision contract self-test: known-good fixture is not green\n'
    sed -n '1,160p' "${baseline_output}"
    return 1
  fi
  mutations=(
    missing-result-vocabulary unknown-result-variant crossed-generation crossed-node
    crossed-selection crossed-source crossed-publication crossed-forwarding crossed-address
    crossed-sovereignty crossed-tls missing-source-generation missing-resource-version
    missing-provider-report-digest missing-attempt-fence crossed-attempt-id
    wrong-success-evidence missing-prerequisite-distinction missing-ready-publication-gate
    missing-strict-decode definite-failure-advances definite-failure-emits-later
    ambiguous-non-inspection ambiguous-loses-correlation first-available-fallback
    provider-trait product-effect caller-cutover unexpected-path duplicate-coordinator
    duplicate-store forbidden-dependency compatibility-shim missing-behavior-proof
    fixture-support-production-leak loose-attempt-revision-history
  )
  for mutation in "${mutations[@]}"; do
    output="${self_test_root}/${mutation}.out"
    case "${mutation}" in
      missing-result-vocabulary) expected='provision result vocabulary must contain exactly' ;;
      unknown-result-variant) expected='provision result vocabulary must contain exactly' ;;
      crossed-generation) expected='behavioral matrix lacks crossed_workload_generation_rejects_before_submission' ;;
      crossed-node) expected='behavioral matrix lacks crossed_local_node_rejects_before_submission' ;;
      crossed-selection) expected='behavioral matrix lacks crossed_provider_selection_rejects_before_submission' ;;
      crossed-source) expected='behavioral matrix lacks crossed_source_snapshot_rejects_before_submission' ;;
      crossed-publication) expected='behavioral matrix lacks crossed_publication_rejects_before_submission' ;;
      crossed-forwarding) expected='behavioral matrix lacks crossed_forwarding_semantics_rejects_before_submission' ;;
      crossed-address) expected='behavioral matrix lacks crossed_address_semantics_rejects_before_submission' ;;
      crossed-sovereignty) expected='behavioral matrix lacks crossed_sovereignty_rejects_before_submission' ;;
      crossed-tls) expected='behavioral matrix lacks crossed_tls_semantics_rejects_before_submission' ;;
      missing-source-generation) expected='independent source revision lacks WorkloadProvisionSourceGeneration' ;;
      missing-resource-version) expected='independent source revision lacks pub struct WorkloadProvisionSourceResourceVersion' ;;
      missing-provider-report-digest) expected='selection source evidence lacks source_digest: NetworkCapabilitySourceDigest' ;;
      missing-attempt-fence) expected='portable attempt fence lacks issuing_revision: WorkloadSagaRevision' ;;
      crossed-attempt-id) expected='behavioral matrix lacks crossed_attempt_id_rejects_without_candidate_or_command' ;;
      wrong-success-evidence) expected='behavioral matrix lacks wrong_success_evidence_rejects_without_state_change' ;;
      missing-prerequisite-distinction) expected='step-specific success evidence lacks ActivationPrerequisitesReady' ;;
      missing-ready-publication-gate) expected='behavioral matrix lacks publication_is_unreachable_before_workload_readiness' ;;
      missing-strict-decode) expected='behavioral matrix lacks unknown_effect_result_variant_is_rejected' ;;
      definite-failure-advances) expected='exhaustive provision decisions lacks definite_failure_retains_completed_phase' ;;
      definite-failure-emits-later) expected='behavioral matrix lacks definite_failure_reopen_emits_no_later_command' ;;
      ambiguous-non-inspection) expected='behavioral matrix lacks ambiguous_result_emits_exact_inspection_only' ;;
      ambiguous-loses-correlation) expected='behavioral matrix lacks ambiguous_reopen_retains_exact_attempt_correlation' ;;
      first-available-fallback) expected='exact selection adopts a first-available fallback' ;;
      provider-trait) expected='seam imports, defines, or calls provider effects' ;;
      product-effect) expected='seam imports, defines, or calls provider effects' ;;
      caller-cutover) expected='product caller cutover appears before NNC6.4' ;;
      unexpected-path) expected='source diff escapes the frozen allowlist' ;;
      duplicate-coordinator) expected='expected one WorkloadSagaCoordinator' ;;
      duplicate-store) expected='expected one WorkloadSagaStore authority' ;;
      forbidden-dependency) expected='nimbus-network gained a forbidden workspace dependency' ;;
      compatibility-shim) expected='provision protocol contains a compatibility shim' ;;
      missing-behavior-proof) expected='behavioral matrix lacks every_provision_phase_and_result_is_exhaustive' ;;
      fixture-support-production-leak) expected='workloads test support is not directly gated by cfg(test)' ;;
      loose-attempt-revision-history) expected='exact provision-attempt revision history lacks validate_attempt_revision(record, disposition, attempt)?' ;;
    esac
    if NIMBUS_NETWORK_NNC63B_ROOT="${fixture}" \
      NIMBUS_NETWORK_NNC63B_TEST_MUTATION="${mutation}" \
      NIMBUS_NETWORK_NNC63B_TEST_CHANGED_PATHS="scripts/nimbus-network-control-plane/workload-provision-decision-contract.sh" \
      bash "${fixture}/scripts/nimbus-network-control-plane/workload-provision-decision-contract.sh" \
      >"${output}" 2>&1; then
      printf 'SELFTEST FAIL NNCV032 %s unexpectedly passed\n' "${mutation}"
      failures=$((failures + 1))
    elif ! rg -q -F "${expected}" "${output}"; then
      printf 'SELFTEST FAIL NNCV032 %s missed expected diagnostic: %s\n' \
        "${mutation}" "${expected}"
      sed -n '1,100p' "${output}"
      failures=$((failures + 1))
    elif rg -q -F "mutation ${mutation} did not change its fixture" "${output}"; then
      printf 'SELFTEST FAIL NNCV032 %s used a no-op fixture substitution\n' "${mutation}"
      failures=$((failures + 1))
    else
      printf 'SELFTEST PASS NNCV032 %s fails closed with a changed fixture\n' "${mutation}"
    fi
    if [ "${mutation}" = "unexpected-path" ]; then
      census_output="${self_test_root}/changed-path-census-failure.out"
      real_git="$(command -v git)"
      "${real_git}" -C "${fixture}" init -q
      "${real_git}" -C "${fixture}" config user.email "nnc63b-self-test@nimbus.invalid"
      "${real_git}" -C "${fixture}" config user.name "NNC6.3b self-test"
      "${real_git}" -C "${fixture}" add .
      "${real_git}" -C "${fixture}" commit -qm "fixture"
      if (
        NIMBUS_NETWORK_NNC63B_REAL_GIT="${real_git}"
        export NIMBUS_NETWORK_NNC63B_REAL_GIT
        # Exported for the verifier subprocess; shellcheck cannot observe that call.
        # shellcheck disable=SC2329
        git() {
          case " $* " in
            *" ls-files --others --exclude-standard "*) return 73 ;;
          esac
          "${NIMBUS_NETWORK_NNC63B_REAL_GIT}" "$@"
        }
        export -f git
        NIMBUS_NETWORK_NNC63B_ROOT="${fixture}" \
          NIMBUS_NETWORK_NNC63B_STARTING_CHECKPOINT=HEAD \
          bash "${fixture}/scripts/nimbus-network-control-plane/workload-provision-decision-contract.sh"
      ) >"${census_output}" 2>&1; then
        printf 'SELFTEST FAIL NNCV032 changed-path census command failure unexpectedly passed\n'
        failures=$((failures + 1))
      elif ! rg -q -F 'untracked-path census failed' "${census_output}"; then
        printf 'SELFTEST FAIL NNCV032 changed-path census missed fail-closed diagnostic\n'
        sed -n '1,100p' "${census_output}"
        failures=$((failures + 1))
      else
        printf 'SELFTEST PASS NNCV032 changed-path census command failure fails closed\n'
      fi
    fi
  done
  if [ "${#mutations[@]}" -ne 36 ]; then
    printf 'NNC6.3b provision contract self-test: expected 36 mutations, observed %d\n' \
      "${#mutations[@]}" >&2
    return 1
  fi
  if [ "${failures}" -ne 0 ]; then
    printf 'NNC6.3b provision contract self-test: %d failed\n' "${failures}"
    return 1
  fi
  printf 'NNC6.3b provision contract self-test: 36 passed, 0 failed\n'
}
