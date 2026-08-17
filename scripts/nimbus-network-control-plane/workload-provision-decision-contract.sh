#!/usr/bin/env bash
# Static NNC6.3b contract for pure workload-provision composition and decisions.

set -u

REPO_ROOT="${NIMBUS_NETWORK_NNC63B_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
SCRIPT_PATH="${REPO_ROOT}/scripts/nimbus-network-control-plane/workload-provision-decision-contract.sh"
SELF_TEST_SCRIPT="scripts/nimbus-network-control-plane/workload-provision-decision-self-test.sh"
SELF_TEST_SCRIPT_PATH="${REPO_ROOT}/${SELF_TEST_SCRIPT}"
STARTING_CHECKPOINT="${NIMBUS_NETWORK_NNC63B_STARTING_CHECKPOINT:-ed0560b4e45f7ec571934624962de72d021a71a8}"
COMPLETION_CHECKPOINT="${NIMBUS_NETWORK_NNC63B_COMPLETION_CHECKPOINT:-c42c61fb2d97d037069f3b27b9055d6e58f11d1d}"
HISTORICAL_PLAN_PATH="docs/private/plans/nimbus-network-control-plane-"'plan.md'

NETWORK_CAPABILITY="crates/nimbus-network/src/capability.rs"
NETWORK_CAPABILITY_TESTS="crates/nimbus-network/src/capability/tests.rs"
NETWORK_REGISTRY="crates/nimbus-network/src/capability_registry.rs"
NETWORK_LIB="crates/nimbus-network/src/lib.rs"
NETWORK_TESTS="crates/nimbus-network/src/capability_registry/tests.rs"
NETWORK_PLAN_DIGEST_TESTS="crates/nimbus-network/src/plan.rs"
NETWORK_READINESS_DIGEST_TESTS="crates/nimbus-network/tests/readiness_dependency.rs"
NETWORK_MANIFEST="crates/nimbus-network/Cargo.toml"
WORKLOAD_NETWORK="crates/nimbus-workloads/src/network_plan.rs"
WORKLOAD_NETWORK_TESTS="crates/nimbus-workloads/src/network_plan/tests.rs"
WORKLOAD_NETWORK_CHILD_TESTS="crates/nimbus-workloads/src/saga/network/tests.rs"
WORKLOAD_SAGA="crates/nimbus-workloads/src/saga.rs"
WORKLOAD_SAGA_TESTS="crates/nimbus-workloads/src/saga/tests.rs"
WORKLOAD_PROVISION="crates/nimbus-workloads/src/saga/provision.rs"
WORKLOAD_PROVISION_TESTS="crates/nimbus-workloads/src/saga/provision/tests.rs"
WORKLOAD_STATE="crates/nimbus-workloads/src/saga/state.rs"
WORKLOAD_TEST_SUPPORT="crates/nimbus-workloads/src/saga/test_support.rs"
WORKLOAD_STORE_TESTS="crates/nimbus-workloads/src/store/tests.rs"
WORKLOADS_LIB="crates/nimbus-workloads/src/lib.rs"
WORKLOADS_MANIFEST="crates/nimbus-workloads/Cargo.toml"
COMPUTE_NETWORK="crates/nimbus-compute/src/workload_network_plan.rs"
COMPUTE_NETWORK_TESTS="crates/nimbus-compute/src/workload_network_plan/tests.rs"
COMPUTE_COMPOSITION="crates/nimbus-compute/src/workload_provision_composition.rs"
COMPUTE_COMPOSITION_TESTS="crates/nimbus-compute/src/workload_provision_composition/tests.rs"
COMPUTE_SAGA="crates/nimbus-compute/src/workload_saga.rs"
COMPUTE_SAGA_TESTS="crates/nimbus-compute/src/workload_saga/tests.rs"
COMPUTE_INGRESS="crates/nimbus-compute/src/workload_saga/ingress.rs"
COMPUTE_INGRESS_TESTS="crates/nimbus-compute/src/workload_saga/ingress/tests.rs"
COMPUTE_DECISION="crates/nimbus-compute/src/workload_saga/provision_decision.rs"
COMPUTE_DECISION_TESTS="crates/nimbus-compute/src/workload_saga/provision_decision/tests.rs"
COMPUTE_TEST_SUPPORT="crates/nimbus-compute/src/workload_saga/test_support.rs"
COMPUTE_RECOVERY="crates/nimbus-compute/src/workload_saga/recovery.rs"
COMPUTE_RECOVERY_TESTS="crates/nimbus-compute/src/workload_saga/recovery/tests.rs"
COMPUTE_LIB="crates/nimbus-compute/src/lib.rs"
SERVER_CAPABILITIES="crates/nimbus-server/src/network_capabilities.rs"
SERVER_CAPABILITIES_TESTS="crates/nimbus-server/src/network_capabilities/tests.rs"
SERVER_CODEC="crates/nimbus-server/src/workload_saga_store/codec.rs"
SERVER_SCHEMA="crates/nimbus-server/src/workload_saga_store/schema.rs"
SERVER_CODEC_TESTS="crates/nimbus-server/src/workload_saga_store/tests/codec.rs"
SERVER_INGRESS_TESTS="crates/nimbus-server/src/workload_saga_store/tests/ingress.rs"
OWNER_CONTRACT="scripts/nimbus-network-control-plane/verification-contract.json"
OWNER_PROOF="docs/private/plans/proof/nimbus-network-control-plane/nnc6.3b-pure-provision-decision.md"

NNC63B_ERRORS=()
NNC63B_CHECKS=0

add_error() {
  NNC63B_ERRORS+=("$1")
}

pass_check() {
  NNC63B_CHECKS=$((NNC63B_CHECKS + 1))
}

source_without_comments() {
  node - "$1" <<'NODE'
const fs = require("fs");
const path = process.argv[2];
const source = fs.existsSync(path) ? fs.readFileSync(path, "utf8") : "";
process.stdout.write(source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/.*$/gm, ""));
NODE
}

source_at_completion() {
  path="$1"
  if [ "${COMPLETION_CHECKPOINT}" = "WORKTREE" ]; then
    source_without_comments "${REPO_ROOT}/${path}"
    return
  fi
  if ! git -C "${REPO_ROOT}" cat-file -e "${COMPLETION_CHECKPOINT}^{commit}" 2>/dev/null; then
    add_error "NNC6.3b completion checkpoint is missing: ${COMPLETION_CHECKPOINT}"
    return
  fi
  if ! git -C "${REPO_ROOT}" cat-file -e "${COMPLETION_CHECKPOINT}:${path}" 2>/dev/null; then
    add_error "NNC6.3b completion checkpoint lacks ${path}"
    return
  fi
  git -C "${REPO_ROOT}" show "${COMPLETION_CHECKPOINT}:${path}" 2>/dev/null |
    node -e '
let source = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", chunk => { source += chunk; });
process.stdin.on("end", () => {
  process.stdout.write(source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/.*$/gm, ""));
});
'
}

source_without_comments_or_strings() {
  # shellcheck disable=SC2016
  node -e '
let source = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", chunk => { source += chunk; });
process.stdin.on("end", () => {
  let output = "";
  let index = 0;
  const blank = character => character === "\n" ? "\n" : " ";
  while (index < source.length) {
    if (source.startsWith("//", index)) {
      while (index < source.length && source[index] !== "\n") {
        output += " ";
        index += 1;
      }
      continue;
    }
    if (source.startsWith("/*", index)) {
      let depth = 0;
      do {
        if (source.startsWith("/*", index)) {
          depth += 1;
          output += "  ";
          index += 2;
        } else if (source.startsWith("*/", index)) {
          depth -= 1;
          output += "  ";
          index += 2;
        } else {
          output += blank(source[index]);
          index += 1;
        }
      } while (index < source.length && depth > 0);
      continue;
    }
    const raw = source.slice(index).match(/^(?:b?r)(#+)?"/);
    if (raw) {
      const hashes = raw[1] || "";
      const end = `"${hashes}`;
      for (let count = 0; count < raw[0].length; count += 1) output += " ";
      index += raw[0].length;
      while (index < source.length && !source.startsWith(end, index)) {
        output += blank(source[index]);
        index += 1;
      }
      for (let count = 0; count < end.length && index < source.length; count += 1) {
        output += " ";
        index += 1;
      }
      continue;
    }
    const stringPrefix = source.startsWith("b\"", index) ? 2 : source[index] === "\"" ? 1 : 0;
    if (stringPrefix > 0) {
      output += " ".repeat(stringPrefix);
      index += stringPrefix;
      while (index < source.length) {
        const character = source[index];
        output += blank(character);
        index += 1;
        if (character === "\\" && index < source.length) {
          output += blank(source[index]);
          index += 1;
        } else if (character === "\"") {
          break;
        }
      }
      continue;
    }
    output += source[index];
    index += 1;
  }
  process.stdout.write(output);
});
'
}

extract_rust_item() {
  marker="$1"
  node -e '
    let source = "";
    process.stdin.setEncoding("utf8");
    process.stdin.on("data", chunk => { source += chunk; });
    process.stdin.on("end", () => {
      const marker = process.argv[1];
      const start = source.indexOf(marker);
      const open = source.indexOf("{", start);
      if (start < 0 || open < 0) return;
      let depth = 0;
      for (let index = open; index < source.length; index += 1) {
        if (source[index] === "{") depth += 1;
        if (source[index] === "}") depth -= 1;
        if (depth === 0) {
          process.stdout.write(source.slice(start, index + 1));
          return;
        }
      }
    });
  ' "${marker}"
}

require_loaded_source() {
  value="$1"
  path="$2"
  label="$3"
  if [ -z "${value}" ]; then
    add_error "missing or empty ${label}: ${path}"
  fi
}

require_loaded_sources() {
  require_loaded_source "${network_capability_source}" "${NETWORK_CAPABILITY}" \
    "network capability vocabulary"
  require_loaded_source "${network_registry_source}" "${NETWORK_REGISTRY}" \
    "network capability registry"
  require_loaded_source "${workload_network_source}" "${WORKLOAD_NETWORK}" \
    "workload network plan"
  require_loaded_source "${workload_saga_source}" "${WORKLOAD_SAGA}" \
    "workload saga intent"
  require_loaded_source "${workload_provision_source}" "${WORKLOAD_PROVISION}" \
    "portable provision protocol"
  require_loaded_source "${workload_state_source}" "${WORKLOAD_STATE}" \
    "portable saga state"
  require_loaded_source "${workload_test_support_source}" "${WORKLOAD_TEST_SUPPORT}" \
    "workloads exact-history test support"
  require_loaded_source "${compute_network_source}" "${COMPUTE_NETWORK}" \
    "compute network compiler"
  require_loaded_source "${compute_composition_source}" "${COMPUTE_COMPOSITION}" \
    "pure provision composition"
  require_loaded_source "${compute_decision_source}" "${COMPUTE_DECISION}" \
    "pure provision reducer"
  require_loaded_source "${compute_test_support_source}" "${COMPUTE_TEST_SUPPORT}" \
    "compute reducer-driven test support"
  require_loaded_source "${compute_recovery_source}" "${COMPUTE_RECOVERY}" \
    "recovery delegation"
  require_loaded_source "${compute_ingress_source}" "${COMPUTE_INGRESS}" \
    "ingress delegation"
  require_loaded_source "${server_capabilities_source}" "${SERVER_CAPABILITIES}" \
    "server ingress capability report"
  require_loaded_source "${server_capabilities_tests_source}" "${SERVER_CAPABILITIES_TESTS}" \
    "server ingress capability report tests"
  require_loaded_source "${server_codec_source}" "${SERVER_CODEC}" \
    "server workload-saga codec"
  require_loaded_source "${server_schema_source}" "${SERVER_SCHEMA}" \
    "server workload-saga schema"
  require_loaded_source "${owner_contract_source}" "${OWNER_CONTRACT}" \
    "stable verification contract"
  require_loaded_source "${owner_proof_source}" "${OWNER_PROOF}" "NNC6.3b proof"
}

check_literals() {
  label="$1"
  source="$2"
  shift 2
  starting_errors="${#NNC63B_ERRORS[@]}"
  for literal in "$@"; do
    if ! printf '%s\n' "${source}" | rg -q -F "${literal}"; then
      add_error "${label} lacks ${literal}"
    fi
  done
  if [ "${#NNC63B_ERRORS[@]}" -eq "${starting_errors}" ]; then
    pass_check
  fi
}

check_exact_variants() {
  label="$1"
  source="$2"
  marker="$3"
  expected="$4"
  block="$(printf '%s' "${source}" | extract_rust_item "${marker}")"
  variants="$(printf '%s\n' "${block}" |
    sed -nE 's/^[[:space:]]{4}([A-Z][A-Za-z0-9_]*)[[:space:]]*([{(,]).*/\1/p' |
    sort | tr '\n' ' ')"
  if [ "${variants}" != "${expected}" ]; then
    add_error "${label} must contain exactly ${expected}(observed ${variants:-none})"
  else
    pass_check
  fi
}

load_sources() {
  network_capability_source="$(source_at_completion "${NETWORK_CAPABILITY}")"
  network_capability_tests_source="$(source_at_completion "${NETWORK_CAPABILITY_TESTS}")"
  network_registry_source="$(source_at_completion "${NETWORK_REGISTRY}")"
  network_lib_source="$(source_at_completion "${NETWORK_LIB}")"
  network_tests_source="$(source_at_completion "${NETWORK_TESTS}")"
  network_manifest_source="$(source_at_completion "${NETWORK_MANIFEST}")"
  workload_network_source="$(source_at_completion "${WORKLOAD_NETWORK}")"
  workload_network_tests_source="$(source_at_completion "${WORKLOAD_NETWORK_TESTS}")"
  workload_network_child_tests_source="$(source_at_completion "${WORKLOAD_NETWORK_CHILD_TESTS}")"
  workload_saga_source="$(source_at_completion "${WORKLOAD_SAGA}")"
  workload_saga_tests_source="$(source_at_completion "${WORKLOAD_SAGA_TESTS}")"
  workload_provision_source="$(source_at_completion "${WORKLOAD_PROVISION}")"
  workload_provision_tests_source="$(source_at_completion "${WORKLOAD_PROVISION_TESTS}")"
  workload_state_source="$(source_at_completion "${WORKLOAD_STATE}")"
  workload_test_support_source="$(source_at_completion "${WORKLOAD_TEST_SUPPORT}")"
  workload_store_tests_source="$(source_at_completion "${WORKLOAD_STORE_TESTS}")"
  workloads_lib_source="$(source_at_completion "${WORKLOADS_LIB}")"
  workloads_manifest_source="$(source_at_completion "${WORKLOADS_MANIFEST}")"
  compute_network_source="$(source_at_completion "${COMPUTE_NETWORK}")"
  compute_network_tests_source="$(source_at_completion "${COMPUTE_NETWORK_TESTS}")"
  compute_composition_source="$(source_at_completion "${COMPUTE_COMPOSITION}")"
  compute_composition_tests_source="$(source_at_completion "${COMPUTE_COMPOSITION_TESTS}")"
  compute_saga_source="$(source_at_completion "${COMPUTE_SAGA}")"
  compute_saga_tests_source="$(source_at_completion "${COMPUTE_SAGA_TESTS}")"
  compute_ingress_source="$(source_at_completion "${COMPUTE_INGRESS}")"
  compute_ingress_tests_source="$(source_at_completion "${COMPUTE_INGRESS_TESTS}")"
  compute_decision_source="$(source_at_completion "${COMPUTE_DECISION}")"
  compute_decision_tests_source="$(source_at_completion "${COMPUTE_DECISION_TESTS}")"
  compute_test_support_source="$(source_at_completion "${COMPUTE_TEST_SUPPORT}")"
  compute_recovery_source="$(source_at_completion "${COMPUTE_RECOVERY}")"
  compute_recovery_tests_source="$(source_at_completion "${COMPUTE_RECOVERY_TESTS}")"
  compute_lib_source="$(source_at_completion "${COMPUTE_LIB}")"
  server_capabilities_source="$(source_at_completion "${SERVER_CAPABILITIES}")"
  server_capabilities_tests_source="$(source_at_completion "${SERVER_CAPABILITIES_TESTS}")"
  server_codec_source="$(source_at_completion "${SERVER_CODEC}")"
  server_schema_source="$(source_at_completion "${SERVER_SCHEMA}")"
  server_codec_tests_source="$(source_at_completion "${SERVER_CODEC_TESTS}")"
  server_ingress_tests_source="$(source_at_completion "${SERVER_INGRESS_TESTS}")"
  owner_contract_source="$(source_without_comments "${REPO_ROOT}/${OWNER_CONTRACT}")"
  owner_proof_source="$(source_at_completion "${OWNER_PROOF}")"

  if [ "${COMPLETION_CHECKPOINT}" = "WORKTREE" ]; then
    authority_census="$(rg -n \
      'pub[[:space:]]+(struct[[:space:]]+WorkloadSagaCoordinator|trait[[:space:]]+WorkloadSagaStore)' \
      "${REPO_ROOT}/crates" -g '*.rs' 2>/dev/null || true)"
    caller_census="$(rg -n 'compose_workload_provision[[:space:]]*\(' \
      "${REPO_ROOT}/crates" -g '*.rs' 2>/dev/null |
      rg -v '/tests([/.]|:)|workload_provision_composition\.rs:' || true)"
  else
    authority_census="$(git -C "${REPO_ROOT}" grep -n -E \
      'pub[[:space:]]+(struct[[:space:]]+WorkloadSagaCoordinator|trait[[:space:]]+WorkloadSagaStore)' \
      "${COMPLETION_CHECKPOINT}" -- 'crates/**/*.rs' 2>/dev/null || true)"
    caller_census="$(git -C "${REPO_ROOT}" grep -n -E \
      'compose_workload_provision[[:space:]]*\(' "${COMPLETION_CHECKPOINT}" -- \
      'crates/**/*.rs' 2>/dev/null |
      rg -v '/tests([/.]|:)|workload_provision_composition\.rs:' || true)"
  fi

  if [ -n "${NIMBUS_NETWORK_NNC63B_TEST_CHANGED_PATHS:-}" ]; then
    changed_paths="${NIMBUS_NETWORK_NNC63B_TEST_CHANGED_PATHS}"
  elif ! git -C "${REPO_ROOT}" cat-file -e "${STARTING_CHECKPOINT}^{commit}" 2>/dev/null; then
    add_error "NNC6.3b starting checkpoint is missing: ${STARTING_CHECKPOINT}"
    changed_paths=""
  elif [ "${COMPLETION_CHECKPOINT}" != "WORKTREE" ]; then
    if ! git -C "${REPO_ROOT}" cat-file -e "${COMPLETION_CHECKPOINT}^{commit}" 2>/dev/null; then
      add_error "NNC6.3b completion checkpoint is missing: ${COMPLETION_CHECKPOINT}"
      changed_paths=""
    elif ! changed_paths="$(git -C "${REPO_ROOT}" diff --name-only \
      "${STARTING_CHECKPOINT}..${COMPLETION_CHECKPOINT}" 2>/dev/null)"; then
      add_error "NNC6.3b completion-path census failed"
      changed_paths=""
    fi
  else
    census_failed=0
    if ! committed="$(git -C "${REPO_ROOT}" diff --name-only "${STARTING_CHECKPOINT}..HEAD" 2>/dev/null)"; then
      add_error "NNC6.3b committed-path census failed"
      committed=""
      census_failed=1
    fi
    if ! working="$(git -C "${REPO_ROOT}" diff --name-only 2>/dev/null)"; then
      add_error "NNC6.3b working-path census failed"
      working=""
      census_failed=1
    fi
    if ! staged="$(git -C "${REPO_ROOT}" diff --cached --name-only 2>/dev/null)"; then
      add_error "NNC6.3b staged-path census failed"
      staged=""
      census_failed=1
    fi
    if ! untracked="$(git -C "${REPO_ROOT}" ls-files --others --exclude-standard 2>/dev/null)"; then
      add_error "NNC6.3b untracked-path census failed"
      untracked=""
      census_failed=1
    fi
    if [ "${census_failed}" -ne 0 ]; then
      changed_paths=""
    else
      changed_paths="$(printf '%s\n%s\n%s\n%s\n' \
        "${committed}" "${working}" "${staged}" "${untracked}" | sort -u)"
    fi
  fi
}

verify_contract() {
  load_sources
  require_loaded_sources
  apply_test_mutation

  check_literals "selection source evidence" "${network_registry_source}" \
    'pub struct NetworkCapabilitySourceDigest' \
    'pub struct NetworkCapabilitySelectionEvidence' \
    'selection: NetworkCapabilitySelection' \
    'source_digest: NetworkCapabilitySourceDigest'

  check_literals "selection evidence digest" "${network_registry_source}" \
    'nimbus.network.capability.selection.evidence.v1' \
    "attachment: &'a NetworkAttachmentProviderRegistration" \
    "ingress: &'a NetworkIngressProviderRegistration" \
    'pub fn selection_evidence(&self)'

  selection_source="${network_registry_source}
${compute_network_source}"
  selection_errors="${#NNC63B_ERRORS[@]}"
  for seam in 'pub fn select_exact(' 'safe_alternatives'; do
    if ! printf '%s\n' "${selection_source}" | rg -q -F "${seam}"; then
      add_error "exact selection authority lacks ${seam}"
    fi
  done
  if printf '%s\n' "${compute_network_source}" |
    rg -q 'selections\(\)(\.(first|next)|\.iter\(\)\.next\(\)|\.values\(\)\.next\(\))|safe_alternatives[^;]*(first|next)\('; then
    add_error "exact selection adopts a first-available fallback"
  fi
  if [ "${#NNC63B_ERRORS[@]}" -eq "${selection_errors}" ]; then pass_check; fi

  check_literals "portable TLS behavior" "${network_capability_source}" \
    'pub enum NetworkTlsBehavior' 'Disabled,' 'Passthrough,' 'TerminateAtIngress,' \
    'tls_behaviors: BTreeSet<NetworkTlsBehavior>' 'tls_behaviors: BTreeSet::new()' \
    'pub fn tls_behaviors(&self)'

  check_literals "endpoint semantics vocabulary" "${workload_network_source}" \
    'pub enum WorkloadNetworkForwardingBehavior' 'None,' 'PortForwarded,' \
    'pub struct WorkloadNetworkEndpointSemantics' \
    'forwarding: WorkloadNetworkForwardingBehavior' 'tls: NetworkTlsBehavior'

  check_literals "listener semantics durability" "${workload_network_source}" \
    'endpoint_semantics: WorkloadNetworkEndpointSemantics' \
    'pub const fn endpoint_semantics(&self)' \
    'endpoint_semantics: WorkloadNetworkEndpointSemantics,'

  plan_errors="${#NNC63B_ERRORS[@]}"
  for seam in \
    'capability_selection_evidence: Option<NetworkCapabilitySelectionEvidence>' \
    'pub fn capability_selection_evidence(&self) -> Option<&NetworkCapabilitySelectionEvidence>' \
    'wire.capability_selection_evidence'; do
    if ! printf '%s\n' "${workload_network_source}" | rg -q -F "${seam}"; then
      add_error "compiled plan evidence lacks ${seam}"
    fi
  done
  if ! printf '%s\n%s\n' "${workload_network_source}" "${workload_network_tests_source}" |
    rg -q -F 'resource_free_plan_has_no_selection_evidence'; then
    add_error "resource-free plan does not prove absent capability selection evidence"
  fi
  if ! printf '%s\n%s\n' "${workload_network_source}" "${workload_network_tests_source}" |
    rg -q -F 'connected_plan_requires_selection_evidence'; then
    add_error "connected plan does not prove required capability selection evidence"
  fi
  if [ "${#NNC63B_ERRORS[@]}" -eq "${plan_errors}" ]; then pass_check; fi

  check_literals "listener-name semantic validation" "${compute_network_source}" \
    'WorkloadNetworkEndpointSemanticsInput' 'duplicate endpoint semantics' \
    'missing endpoint semantics' 'unexpected endpoint semantics'

  check_literals "forwarding validation" "${compute_network_source}" \
    'forwarding behavior must match guest port shape' \
    'NetworkForwardingFeature::PortForwarding' \
    'WorkloadNetworkForwardingBehavior::PortForwarded'

  check_literals "TLS validation" "${compute_network_source}" \
    'TLS behavior must match listener protocol' 'NetworkTlsBehavior::Passthrough' \
    'NetworkTlsBehavior::TerminateAtIngress' 'tls_behaviors()'

  for seam in \
    'NetworkTlsBehavior::Disabled' \
    'NetworkTlsBehavior::TerminateAtIngress' \
    'production_ingress_capabilities_report_selected_tls_behavior'; do
    if ! printf '%s\n%s\n' \
      "${server_capabilities_source}" "${server_capabilities_tests_source}" |
      rg -q -F "${seam}"; then
      add_error "server ingress TLS report lacks ${seam}"
    fi
  done

  check_literals "provision source evidence and workload-kind correlation" "${workload_provision_source}
${workload_saga_source}" \
    'pub enum WorkloadProvisionSourceEvidence' 'StandaloneSandbox' 'SandboxBackedService' \
    'source_identity: WorkloadProvisionSourceIdentity' \
    'source_generation: WorkloadProvisionSourceGeneration' \
    'resource_version: WorkloadProvisionSourceResourceVersion' \
    'source_digest: WorkloadProvisionSourceDigest' \
    'attachment_provider_id: NetworkProviderId' 'required_workload_kind' \
    'self.source.required_workload_kind()' \
    'desired workload kind does not match provision source kind'

  source_revision_errors="${#NNC63B_ERRORS[@]}"
  if ! printf '%s\n' "${workload_provision_source}" |
    rg -U -q 'pub struct WorkloadProvisionSourceGeneration|define_decimal_counter!\(\s*WorkloadProvisionSourceGeneration'; then
    add_error "independent source revision lacks WorkloadProvisionSourceGeneration"
  fi
  for seam in \
    'pub struct WorkloadProvisionSourceResourceVersion' \
    'nimbus.workloads.provision.source.digest.v1'; do
    if ! printf '%s\n' "${workload_provision_source}" | rg -q -F "${seam}"; then
      add_error "independent source revision lacks ${seam}"
    fi
  done
  if [ "${#NNC63B_ERRORS[@]}" -eq "${source_revision_errors}" ]; then pass_check; fi

  check_literals "source-bound desired intent" "${workload_saga_source}" \
    'source: WorkloadProvisionSourceEvidence' \
    "source: &'a WorkloadProvisionSourceEvidence" \
    'pub fn source(&self) -> &WorkloadProvisionSourceEvidence'

  admission_errors="${#NNC63B_ERRORS[@]}"
  if ! printf '%s\n' "${workload_saga_source}" |
    rg -q 'assigned_node:[[:space:]]*NodeIdentity'; then
    add_error "workload admission does not require assigned node"
  fi
  if printf '%s\n' "${workload_saga_source}" |
    rg -q 'assigned_node:[[:space:]]*Option<NodeIdentity>'; then
    add_error "workload admission still permits an absent assigned node"
  fi
  if [ "${#NNC63B_ERRORS[@]}" -eq "${admission_errors}" ]; then pass_check; fi

  if ! printf '%s\n' "${network_lib_source}" |
    rg -q -F 'NetworkCapabilitySelectionEvidence'; then
    add_error "network public exports lack NetworkCapabilitySelectionEvidence"
  fi
  if ! printf '%s\n' "${workload_saga_source}" | rg -q -F 'mod provision;' ||
    ! printf '%s\n' "${workload_saga_source}" |
      rg -q -F 'WorkloadProvisionEffectResult'; then
    add_error "workload saga does not wire and re-export the provision module"
  fi
  if ! printf '%s\n' "${workloads_lib_source}" |
    rg -q -F 'WorkloadProvisionEffectResult'; then
    add_error "workloads public exports lack WorkloadProvisionEffectResult"
  fi
  if ! printf '%s\n' "${compute_saga_source}" | rg -q -F 'mod provision_decision;' ||
    ! printf '%s\n' "${compute_saga_source}" | rg -q -F 'WorkloadProvisionDecision'; then
    add_error "compute saga does not wire and re-export the provision reducer"
  fi
  if ! printf '%s\n' "${compute_lib_source}" |
    rg -q -F 'workload_provision_composition' ||
    ! printf '%s\n' "${compute_lib_source}" | rg -q -F 'compose_workload_provision'; then
    add_error "compute public exports do not wire the provision composer"
  fi

  fixture_support_errors="${#NNC63B_ERRORS[@]}"
  if ! printf '%s\n' "${workload_saga_source}" |
    rg -U -q '#\[cfg\(test\)\][[:space:]]*pub\(crate\)[[:space:]]+mod[[:space:]]+test_support[[:space:]]*;'; then
    add_error "workloads test support is not directly gated by cfg(test)"
  fi
  if ! printf '%s\n' "${compute_saga_source}" |
    rg -U -q '#\[cfg\(test\)\][[:space:]]*pub\(crate\)[[:space:]]+mod[[:space:]]+test_support[[:space:]]*;'; then
    add_error "compute test support is not directly gated by cfg(test)"
  fi
  if ! printf '%s\n' "${workload_test_support_source}" |
    rg -q -F 'transition_provision_disposition'; then
    add_error "workloads test support does not drive transition_provision_disposition"
  fi
  if ! printf '%s\n' "${compute_test_support_source}" |
    rg -q -F 'WorkloadProvisionDecision::plan'; then
    add_error "compute test support does not drive WorkloadProvisionDecision::plan"
  fi
  if ! printf '%s\n' "${compute_test_support_source}" |
    rg -q -F 'WorkloadProvisionDecision::reduce'; then
    add_error "compute test support does not drive WorkloadProvisionDecision::reduce"
  fi
  if [ "${#NNC63B_ERRORS[@]}" -eq "${fixture_support_errors}" ]; then pass_check; fi

  required_body_source="${network_registry_source}
${workload_network_source}
${workload_provision_source}
${workload_state_source}
${compute_network_source}
${compute_composition_source}
${compute_decision_source}"
  if printf '%s\n' "${required_body_source}" | rg -q '(todo|unimplemented)!\('; then
    add_error "required NNC6.3b product body contains todo! or unimplemented!"
  fi

  check_literals "pure provision composition API" "${compute_composition_source}" \
    'pub struct WorkloadProvisionCompositionInput' \
    'pub fn compose_workload_provision(' \
    'Result<ComposedWorkloadProvision, WorkloadProvisionCompositionError>' \
    'pub struct ComposedWorkloadProvision' 'key: WorkloadSagaKey' 'intent: WorkloadSagaIntent'

  check_literals "exact provision composition" "${compute_composition_source}" \
    'does not match admitted assignment' 'encode_sandbox_spec' \
    'WorkloadNetworkPlanCompiler' '.compile(' \
    'WorkloadProvisionSourceEvidence' 'WorkloadSagaIntent::new'

  check_exact_variants "closed executable provision sources" "${compute_composition_source}" \
    "pub enum WorkloadProvisionSourceSnapshot<'source>" \
    'SandboxBackedService StandaloneSandbox '

  check_exact_variants "provision step vocabulary" "${workload_provision_source}" \
    'pub enum WorkloadProvisionStep' \
    'ActivateWorkload AttachNetwork InspectActivationPrerequisites InspectWorkloadReadiness ObservePublication PrepareWorkload Publish ReserveNetwork '

  check_literals "portable attempt fence" "${workload_provision_source}" \
    'pub struct WorkloadProvisionAttempt' 'attempt_id: WorkloadProvisionAttemptId' \
    'key: WorkloadSagaKey' \
    'saga_id: WorkloadSagaId' 'issuing_revision: WorkloadSagaRevision' \
    'generation: WorkloadGeneration' 'desired_digest: WorkloadDesiredDigest' \
    'required_node: NodeIdentity' 'source_digest: WorkloadProvisionSourceDigest' \
    'network_plan_digest: NetworkPlanDigest' \
    'selection_evidence: Option<NetworkCapabilitySelectionEvidence>' \
    'source_phase: WorkloadSagaPhase' 'target_phase: WorkloadSagaPhase' \
    'step: WorkloadProvisionStep' 'subjects: WorkloadProvisionSubjects' \
    'prerequisite: Option<WorkloadProvisionPrerequisiteEvidence>' \
    'pub fn key(&self) -> &WorkloadSagaKey' \
    'pub fn prerequisite(&self) -> Option<&WorkloadProvisionPrerequisiteEvidence>' \
    'nimbus.workloads.provision.attempt.id.v1'

  check_exact_variants "provision result vocabulary" "${workload_provision_source}" \
    'pub enum WorkloadProvisionEffectResult' 'Ambiguous DefiniteFailure Succeeded '

  check_literals "step-specific success evidence" "${workload_provision_source}" \
    'pub enum WorkloadProvisionSuccessEvidence' 'NetworkReserved' 'WorkloadPrepared' \
    'NetworkAttached' 'ActivationPrerequisitesReady' 'WorkloadActivated' \
    'WorkloadReady' 'Published' 'PublicationObserved'

  check_exact_variants "provision disposition vocabulary" "${workload_provision_source}" \
    'pub enum WorkloadProvisionDisposition' \
    'AttemptPending DefiniteFailure InspectionRequired Ready '

  disposition_errors="${#NNC63B_ERRORS[@]}"
  for seam in \
    'provision_disposition: Option<WorkloadProvisionDisposition>' \
    "provision_disposition: &'a Option<WorkloadProvisionDisposition>" \
    'pub fn provision_disposition(&self) -> Option<&WorkloadProvisionDisposition>' \
    'transition_provision_disposition' \
    'non_provision_record_has_no_provision_disposition' \
    'running_provision_record_requires_provision_disposition'; do
    if ! printf '%s\n%s\n' "${workload_state_source}" "${workload_saga_tests_source}" |
      rg -q -F "${seam}"; then
      add_error "durable provision disposition lacks ${seam}"
    fi
  done
  for seam in \
    'copy(&mut fields, "source", active, "source")' \
    '"source": required(fields, "source")?' \
    'field("source", FieldType::Object, true)' \
    'provision_source_round_trips_through_physical_codec' \
    '"provisionDisposition"' \
    'fields.get("provisionDisposition").cloned()' \
    'field("provisionDisposition", FieldType::Object, false)' \
    'provision_disposition_round_trips_through_physical_codec'; do
    if ! printf '%s\n%s\n%s\n%s\n' \
      "${server_codec_source}" "${server_schema_source}" \
      "${server_codec_tests_source}" "${server_ingress_tests_source}" |
      rg -q -F "${seam}"; then
      add_error "physical workload saga durability lacks ${seam}"
    fi
  done
  if ! printf '%s\n' "${server_codec_source}" |
    rg -U -q 'copy_optional\(\s*&mut fields,\s*"provisionDisposition"'; then
    add_error "physical workload saga durability lacks optional provisionDisposition encoding"
  fi
  if [ "${#NNC63B_ERRORS[@]}" -eq "${disposition_errors}" ]; then pass_check; fi

  attempt_revision_errors="${#NNC63B_ERRORS[@]}"
  for seam in \
    'validate_attempt_revision(record, disposition, attempt)?' \
    'let after_one = attempt.issuing_revision().checked_next()' \
    'let after_two = after_one.and_then(WorkloadSagaRevision::checked_next)' \
    'let after_three = after_two.and_then(WorkloadSagaRevision::checked_next)' \
    'attempt.step() != WorkloadProvisionStep::ActivateWorkload' \
    'provision_disposition_requires_exact_attempt_revision_history'; do
    if ! printf '%s\n%s\n' "${workload_state_source}" "${workload_saga_tests_source}" |
      rg -q -F "${seam}"; then
      add_error "exact provision-attempt revision history lacks ${seam}"
    fi
  done
  if [ "${#NNC63B_ERRORS[@]}" -eq "${attempt_revision_errors}" ]; then pass_check; fi

  check_literals "terminal provision failure" "${workload_state_source}
${compute_decision_source}
${compute_decision_tests_source}" \
    'DefiniteFailure' 'requires_recovery' 'definite_failure_retains_completed_phase' \
    'definite failure permits no later provision command'

  check_literals "pure provision reducer" "${compute_decision_source}" \
    'pub enum WorkloadProvisionDecision' 'pub fn plan(' 'pub fn reduce(' \
    'WorkloadProvisionEffectResult' 'WorkloadProvisionDisposition'

  check_literals "exhaustive provision decisions" "${compute_decision_source}
${compute_decision_tests_source}" \
    'InspectActivationPrerequisites' 'InspectWorkloadReadiness' \
    'activation_prerequisite_success_prepares_activation_attempt' \
    'definite_failure_retains_completed_phase' 'ambiguous_result_requires_exact_inspection' \
    'publication_requires_workload_readiness'

  recovery_errors="${#NNC63B_ERRORS[@]}"
  if ! printf '%s\n' "${compute_recovery_source}" |
    rg -q -F 'WorkloadProvisionDecision::plan(record)'; then
    add_error "general recovery does not delegate provision phases to the provision reducer"
  fi
  if ! printf '%s\n' "${compute_ingress_source}" |
    rg -q -F 'WorkloadSagaDecision::for_record(&record)'; then
    add_error "confirmed ingress does not delegate through the shared saga decision selector"
  fi
  if ! printf '%s\n%s\n%s\n%s\n' \
    "${compute_ingress_tests_source}" "${compute_recovery_tests_source}" \
    "${compute_saga_tests_source}" "${compute_decision_tests_source}" |
    rg -q -F 'ingress_and_recovery_delegate_to_same_provision_reducer'; then
    add_error "behavioral matrix lacks shared ingress and recovery provision delegation"
  fi
  provision_switches="$(printf '%s\n%s\n' "${compute_recovery_source}" "${compute_decision_source}" |
    rg -o 'WorkloadSagaPhase::IntentCommitted[[:space:]]*=>' | awk 'END { print NR + 0 }')"
  if [ "${provision_switches}" -ne 1 ]; then
    add_error "expected one provision phase switch, observed ${provision_switches}"
  fi
  if [ "${#NNC63B_ERRORS[@]}" -eq "${recovery_errors}" ]; then pass_check; fi

  behavior_source="${network_capability_tests_source}
${network_tests_source}
${workload_network_tests_source}
${workload_network_child_tests_source}
${workload_saga_tests_source}
${workload_provision_tests_source}
${workload_test_support_source}
${workload_store_tests_source}
${compute_network_tests_source}
${compute_composition_tests_source}
${compute_saga_tests_source}
${compute_ingress_tests_source}
${compute_decision_tests_source}
${compute_test_support_source}
${compute_recovery_tests_source}
${server_capabilities_tests_source}
${server_codec_tests_source}
${server_ingress_tests_source}"
  check_literals "NNC6.3b behavioral matrix" "${behavior_source}" \
    'provider_report_digest_binds_complete_selected_reports' \
    'resource_free_plan_has_no_selection_evidence' \
    'connected_plan_requires_selection_evidence' \
    'endpoint_semantics_reject_missing_extra_duplicate_and_crossed_names' \
    'crossed_workload_generation_rejects_before_submission' \
    'crossed_local_node_rejects_before_submission' \
    'crossed_provider_selection_rejects_before_submission' \
    'crossed_source_snapshot_rejects_before_submission' \
    'crossed_publication_rejects_before_submission' \
    'crossed_forwarding_semantics_rejects_before_submission' \
    'crossed_address_semantics_rejects_before_submission' \
    'crossed_sovereignty_rejects_before_submission' \
    'crossed_tls_semantics_rejects_before_submission' \
    'unknown_effect_result_variant_is_rejected' \
    'attempt_identity_binds_saga_key_and_prerequisite' \
    'attempt_identity_binds_every_named_fence_and_rejects_forged_wire' \
    'effect_result_round_trips_exactly_three_strict_variants' \
    'resource_free_attempt_has_no_selection_evidence' \
    'connected_attempt_requires_selection_evidence' \
    'non_provision_record_has_no_provision_disposition' \
    'running_provision_record_requires_provision_disposition' \
    'crossed_attempt_id_rejects_without_candidate_or_command' \
    'crossed_same_variant_subject_rejects_without_candidate_or_command' \
    'wrong_success_evidence_rejects_without_state_change' \
    'publication_is_unreachable_before_workload_readiness' \
    'definite_failure_reopen_emits_no_later_command' \
    'ambiguous_result_emits_exact_inspection_only' \
    'ambiguous_reopen_retains_exact_attempt_correlation' \
    'empty_ingress_capability_set_has_no_implicit_tls_behavior' \
    'empty_tls_evidence_does_not_satisfy_disabled_requirement' \
    'activation_prerequisite_attempt_cannot_complete_activation' \
    'activation_attempt_requires_retained_prerequisite_inspection' \
    'activation_prerequisite_subjects_must_match_retained_inspection' \
    'promoted_generation_requires_exact_initial_provision_disposition' \
    'workload_kind_must_match_provision_source_variant' \
    'observed_publish_when_ready_requires_publication_observation' \
    'publication_observed_success_retains_exact_durable_observation_evidence' \
    'every_provision_phase_and_result_is_exhaustive' \
    'provision_disposition_requires_exact_attempt_revision_history' \
    'ingress_and_recovery_delegate_to_same_provision_reducer' \
    'production_ingress_capabilities_report_selected_tls_behavior' \
    'provision_disposition_round_trips_through_physical_codec'

  authority_errors="${#NNC63B_ERRORS[@]}"
  new_seam_source="${network_registry_source}
${workload_provision_source}
${workload_state_source}
${compute_composition_source}
${compute_decision_source}"
  pure_seam_code="$(printf '%s\n' "${new_seam_source}" | source_without_comments_or_strings)"
  forbidden_effects="$(printf '%s\n' "${pure_seam_code}" |
    rg -n 'trait[[:space:]].*Provider|SandboxBackend|ServiceManager|LocalNetworkManager|std::(net|fs|process)|Tcp(Listener|Stream)|UdpSocket|Unix(Listener|Stream)|tokio::|async[[:space:]]+fn|\.await\b|\.(bind|connect|listen|accept)[[:space:]]*\(|start_service|apply_network_plan' || true)"
  if [ -n "${forbidden_effects}" ]; then
    add_error "NNC6.3b seam imports, defines, or calls provider effects: ${forbidden_effects}"
  fi
  if [ -n "${caller_census}" ]; then
    add_error "product caller cutover appears before NNC6.4: ${caller_census}"
  fi
  coordinator_count="$(printf '%s\n' "${authority_census}" |
    rg -o 'pub[[:space:]]+struct[[:space:]]+WorkloadSagaCoordinator' | awk 'END { print NR + 0 }')"
  store_count="$(printf '%s\n' "${authority_census}" |
    rg -o 'pub[[:space:]]+trait[[:space:]]+WorkloadSagaStore' | awk 'END { print NR + 0 }')"
  if [ "${coordinator_count}" -ne 1 ]; then
    add_error "expected one WorkloadSagaCoordinator, observed ${coordinator_count}"
  fi
  if [ "${store_count}" -ne 1 ]; then
    add_error "expected one WorkloadSagaStore authority, observed ${store_count}"
  fi
  if printf '%s\n' "${network_manifest_source}" |
    rg -q '^nimbus-(?!core)[A-Za-z0-9_-]*[[:space:]]*=' --pcre2; then
    add_error "nimbus-network gained a forbidden workspace dependency"
  fi
  if printf '%s\n' "${workload_provision_source}" |
    rg -q 'serde\([^)]*alias|legacyProvision|compatibility'; then
    add_error "provision protocol contains a compatibility shim"
  fi
  if [ "${#NNC63B_ERRORS[@]}" -eq "${authority_errors}" ]; then pass_check; fi

  routing_errors="${#NNC63B_ERRORS[@]}"
  if ! printf '%s\n' "${owner_contract_source}" |
    rg -q '"NNC6\.3b":[[:space:]]*"After NNC6\.3a, implement the pure provision decision protocol'; then
    add_error "stable contract does not route NNC6.3b to the pure provision decision protocol"
  fi
  if ! printf '%s\n' "${owner_proof_source}" | rg -q '^## Acceptance Criteria$'; then
    add_error "NNC6.3b proof lacks its acceptance ledger"
  fi
  unexpected="$(printf '%s\n' "${changed_paths}" | awk -v historical_plan_path="${HISTORICAL_PLAN_PATH}" '
    NF == 0 { next }
    $0 == "crates/nimbus-network/src/capability.rs" { next }
    $0 == "crates/nimbus-network/src/capability/tests.rs" { next }
    $0 == "crates/nimbus-network/src/capability_registry.rs" { next }
    $0 == "crates/nimbus-network/src/capability_registry/tests.rs" { next }
    $0 == "crates/nimbus-network/src/lib.rs" { next }
    $0 == "crates/nimbus-network/src/plan.rs" { next }
    $0 == "crates/nimbus-network/tests/readiness_dependency.rs" { next }
    $0 == "crates/nimbus-workloads/src/network_plan.rs" { next }
    $0 == "crates/nimbus-workloads/src/network_plan/tests.rs" { next }
    $0 == "crates/nimbus-workloads/src/saga/network/tests.rs" { next }
    $0 == "crates/nimbus-workloads/src/saga.rs" { next }
    $0 == "crates/nimbus-workloads/src/saga/provision.rs" { next }
    $0 == "crates/nimbus-workloads/src/saga/provision/tests.rs" { next }
    $0 == "crates/nimbus-workloads/src/saga/state.rs" { next }
    $0 == "crates/nimbus-workloads/src/saga/test_support.rs" { next }
    $0 == "crates/nimbus-workloads/src/saga/tests.rs" { next }
    $0 == "crates/nimbus-workloads/src/store/tests.rs" { next }
    $0 == "crates/nimbus-workloads/src/lib.rs" { next }
    $0 == "crates/nimbus-compute/src/workload_network_plan.rs" { next }
    $0 == "crates/nimbus-compute/src/workload_network_plan/tests.rs" { next }
    $0 ~ /^crates\/nimbus-compute\/src\/workload_network_plan\/tests\/[0-9A-Za-z._-]+\.rs$/ { next }
    $0 == "crates/nimbus-compute/src/workload_provision_composition.rs" { next }
    $0 == "crates/nimbus-compute/src/workload_provision_composition/tests.rs" { next }
    $0 == "crates/nimbus-compute/src/workload_saga.rs" { next }
    $0 == "crates/nimbus-compute/src/workload_saga/tests.rs" { next }
    $0 == "crates/nimbus-compute/src/workload_saga/ingress.rs" { next }
    $0 == "crates/nimbus-compute/src/workload_saga/ingress/tests.rs" { next }
    $0 == "crates/nimbus-compute/src/workload_saga/provision_decision.rs" { next }
    $0 == "crates/nimbus-compute/src/workload_saga/provision_decision/tests.rs" { next }
    $0 == "crates/nimbus-compute/src/workload_saga/test_support.rs" { next }
    $0 == "crates/nimbus-compute/src/workload_saga/recovery.rs" { next }
    $0 == "crates/nimbus-compute/src/workload_saga/recovery/tests.rs" { next }
    $0 == "crates/nimbus-compute/src/lib.rs" { next }
    $0 == "crates/nimbus-server/src/network_capabilities.rs" { next }
    $0 == "crates/nimbus-server/src/network_capabilities/tests.rs" { next }
    $0 == "crates/nimbus-server/src/workload_saga_store/codec.rs" { next }
    $0 == "crates/nimbus-server/src/workload_saga_store/schema.rs" { next }
    $0 == "crates/nimbus-server/src/workload_saga_store/tests/mod.rs" { next }
    $0 ~ /^crates\/nimbus-server\/src\/workload_saga_store\/tests\/[0-9A-Za-z._-]+\.rs$/ { next }
    $0 == "crates/nimbus-server/tests/network_capability_registration.rs" { next }
    $0 == "scripts/nimbus-network-control-plane/workload-provision-decision-contract.sh" { next }
    $0 == "scripts/nimbus-network-control-plane/workload-provision-decision-self-test.sh" { next }
    $0 == "scripts/nimbus-network-control-plane/workload-executable-carrier-contract.sh" { next }
    $0 == "scripts/nimbus-network-control-plane/workload-network-plan-durability-contract.sh" { next }
    $0 == "scripts/nimbus-network-control-plane/workload-saga-authority-contract.sh" { next }
    $0 == "scripts/verify-nimbus-network-control-plane.sh" { next }
    $0 == historical_plan_path { next }
    $0 == "docs/private/plans/README.md" { next }
    $0 == "docs/private/plans/proof/nimbus-network-control-plane/nnc4.6f-production-network-authority-census.json" { next }
    $0 == "docs/private/plans/proof/nimbus-network-control-plane/nnc6.3b-pure-provision-decision.md" { next }
    { print }
  ')"
  if [ -n "${unexpected}" ]; then
    add_error "NNC6.3b source diff escapes the frozen allowlist: ${unexpected}"
  fi
  for digest_path in "${NETWORK_PLAN_DIGEST_TESTS}" "${NETWORK_READINESS_DIGEST_TESTS}"; do
    if printf '%s\n' "${changed_paths}" | rg -q -x -F "${digest_path}"; then
      if [ "${COMPLETION_CHECKPOINT}" = "WORKTREE" ]; then
        digest_range="${STARTING_CHECKPOINT}"
      else
        digest_range="${STARTING_CHECKPOINT}..${COMPLETION_CHECKPOINT}"
      fi
      if ! digest_diff="$(git -C "${REPO_ROOT}" diff "${digest_range}" -- "${digest_path}" 2>/dev/null)"; then
        add_error "NNC6.3b digest-ripple census failed: ${digest_path}"
        continue
      fi
      digest_lines="$(printf '%s\n' "${digest_diff}" |
        awk 'substr($0,1,1) ~ /[+-]/ && substr($0,2,1) != substr($0,1,1) { print }')"
      if [ -z "${digest_lines}" ] || printf '%s\n' "${digest_lines}" |
        rg -v -q '^[+-][[:space:]]*"[0-9a-f]{64}",?[[:space:]]*$'; then
        add_error "NNC6.3b digest-ripple path contains more than a pinned 64-hex expectation: ${digest_path}"
      fi
    fi
  done
  if [ "${#NNC63B_ERRORS[@]}" -eq "${routing_errors}" ]; then pass_check; fi
}

run_contract() {
  cd "${REPO_ROOT}" || return 1
  NNC63B_ERRORS=()
  NNC63B_CHECKS=0
  for tool in git node rg awk sed; do
    command -v "${tool}" >/dev/null 2>&1 || add_error "missing required verifier tool ${tool}"
  done
  verify_contract
  if [ "${#NNC63B_ERRORS[@]}" -ne 0 ]; then
    for error in "${NNC63B_ERRORS[@]}"; do
      printf 'NNC6.3b provision contract failure: %s\n' "${error}" >&2
    done
    return 1
  fi
  if [ "${NNC63B_CHECKS}" -ne 32 ]; then
    printf 'NNC6.3b provision contract failure: expected 32 checks, observed %d\n' \
      "${NNC63B_CHECKS}" >&2
    return 1
  fi
  printf 'NNC6.3b provision contract: 32 checks passed\n'
}

# shellcheck source=scripts/nimbus-network-control-plane/workload-provision-decision-self-test.sh
. "${SELF_TEST_SCRIPT_PATH}"

case "${1:-}" in
  '' | --check) run_contract ;;
  --self-test) run_self_test ;;
  *)
    printf 'usage: %s [--check|--self-test]\n' "$0" >&2
    exit 2
    ;;
esac
