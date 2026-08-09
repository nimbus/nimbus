#!/usr/bin/env bash
# Static NNC6.2a contract for the durable complete workload network plan.

set -u

REPO_ROOT="${NIMBUS_NETWORK_NNC62A_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
SCRIPT_PATH="${REPO_ROOT}/scripts/nimbus-network-control-plane/workload-network-plan-durability-contract.sh"
STARTING_CHECKPOINT="${NIMBUS_NETWORK_NNC62A_STARTING_CHECKPOINT:-15544998c20410fec30d89eca187cdc8d6527609}"
COMPLETION_CHECKPOINT="${NIMBUS_NETWORK_NNC62A_COMPLETION_CHECKPOINT:-ba78303608a2a48f319e452fc585593c5140445e}"
if [ "${NIMBUS_NETWORK_NNC62A_TEST_MUTATION:-}" = "missing-completion-checkpoint" ]; then
  COMPLETION_CHECKPOINT="0000000000000000000000000000000000000000"
fi
if [ "${NIMBUS_NETWORK_NNC62A_TEST_MUTATION:-}" = "unreadable-source-diff" ]; then
  STARTING_CHECKPOINT="0000000000000000000000000000000000000000"
fi
SAGA="crates/nimbus-workloads/src/saga.rs"
SAGA_NETWORK="crates/nimbus-workloads/src/saga/network.rs"
SAGA_STATE="crates/nimbus-workloads/src/saga/state.rs"
COMPUTE_RECOVERY="crates/nimbus-compute/src/workload_saga/recovery.rs"
PROVISION_DECISION="crates/nimbus-compute/src/workload_saga/provision_decision.rs"
STORE_CODEC="crates/nimbus-server/src/workload_saga_store/codec.rs"
STORE_SCHEMA="crates/nimbus-server/src/workload_saga_store/schema.rs"
PROCESS_PROOF="crates/nimbus-server/src/workload_saga_store/tests/compiled_plan_durability.rs"
OWNER_PLAN="docs/private/plans/nimbus-network-control-plane-plan.md"
OWNER_PROOF="docs/private/plans/proof/nimbus-network-control-plane/nnc6.2a-durable-compiled-network-plan.md"

NNC62A_ERRORS=()
NNC62A_CHECKS=0

add_error() {
  NNC62A_ERRORS+=("$1")
}

pass_check() {
  NNC62A_CHECKS=$((NNC62A_CHECKS + 1))
}

require_nonempty_file() {
  target="$1"
  label="$2"
  if [ ! -s "${target}" ]; then
    add_error "missing or empty ${label}: ${target}"
    return 1
  fi
  pass_check
  return 0
}

source_without_comments() {
  node - "$1" <<'NODE'
const fs = require("fs");
const source = fs.readFileSync(process.argv[2], "utf8");
process.stdout.write(source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/.*$/gm, ""));
NODE
}

verify_complete_carrier() {
  if [ "${NIMBUS_NETWORK_NNC62A_TEST_MUTATION:-}" = "missing-carrier" ]; then
    SAGA_NETWORK="crates/nimbus-workloads/src/saga/missing_network.rs"
  fi
  if ! require_nonempty_file "${SAGA_NETWORK}" "workloads-owned compiled-plan carrier"; then
    if [ "${NIMBUS_NETWORK_NNC62A_TEST_MUTATION:-}" != "tuple-authority" ]; then
      return
    fi
    network_source=""
  else
    network_source="$(source_without_comments "${SAGA_NETWORK}")"
  fi
  compact_network="$(printf '%s\n' "${network_source}" | tr '\n' ' ')"
  if ! printf '%s\n' "${compact_network}" |
    rg -q 'WorkloadNetworkIntent([^{;]*\{[^}]*compiled_plan:[[:space:]]*CompiledWorkloadNetworkPlan|\([[:space:]]*CompiledWorkloadNetworkPlan[[:space:]]*\))'; then
    add_error "WorkloadNetworkIntent does not own exactly one complete CompiledWorkloadNetworkPlan"
  else
    pass_check
  fi

  intent_block="$(
    node - "${SAGA_NETWORK}" <<'NODE'
const fs = require("fs");
let source = fs.existsSync(process.argv[2]) ? fs.readFileSync(process.argv[2], "utf8") : "";
  const start = source.indexOf("pub struct WorkloadNetworkIntent");
  const end = source.indexOf("impl WorkloadNetworkIntent", start);
  process.stdout.write(start >= 0 && end > start ? source.slice(start, end) : source);
NODE
  )"
  if printf '%s\n' "${intent_block}${NIMBUS_NETWORK_NNC62A_TEST_MUTATION:-}" |
    rg -q 'plan_id:[[:space:]]*NetworkPlanId|generation:[[:space:]]*NetworkResourceGeneration|digest:[[:space:]]*NetworkPlanDigest|tuple-authority'; then
    add_error "compiled-plan carrier retains caller-supplied tuple authority"
  else
    pass_check
  fi

  for seam in 'compiled_plan(&self)' 'plan_id(&self)' 'generation(&self)' 'digest(&self)'; do
    if ! printf '%s\n' "${network_source}" | rg -q -F "${seam}"; then
      add_error "compiled-plan carrier lacks derived accessor ${seam}"
    fi
  done
  if printf '%s\n' "${network_source}" |
    rg -q 'WorkloadNetworkReference[^{]*\{[^}]*(intent|compiled_plan):'; then
    add_error "phase network reference duplicates complete desired plan instead of retaining a derived tuple"
  else
    pass_check
  fi
}

verify_current_format_and_correlations() {
  if ! require_nonempty_file "${SAGA}" "portable workload saga" ||
    ! require_nonempty_file "${SAGA_STATE}" "portable workload saga state"; then
    return
  fi
  saga_source="$(source_without_comments "${SAGA}")"
  state_source="$(source_without_comments "${SAGA_STATE}")"
  if [ "${NIMBUS_NETWORK_NNC62A_TEST_MUTATION:-}" = "wrong-version" ] ||
    ! printf '%s\n' "${saga_source}" |
      rg -q 'WORKLOAD_SAGA_FORMAT_VERSION:[[:space:]]*u32[[:space:]]*=[[:space:]]*5'; then
    add_error "workload saga format version is not the current strict v5"
  else
    pass_check
  fi
  if ! printf '%s\n' "${state_source}" |
    rg -q 'nimbus\.workloads\.saga\.transition\.v4'; then
    add_error "transition identity does not use the v4 complete-payload domain"
  elif printf '%s\n' "${state_source}" |
    rg -q 'nimbus\.workloads\.saga\.transition\.v[123]'; then
    add_error "production transition identity retains a pre-v4 domain"
  else
    pass_check
  fi

  correlation_error_count="${#NNC62A_ERRORS[@]}"
  for diagnostic in \
    'network generation must match workload generation' \
    'network activation must match workload activation' \
    'network publication must match workload publication' \
    'network plan tenant must match workload saga tenant'; do
    if ! printf '%s\n%s\n' "${saga_source}" "${state_source}" | rg -q -F "${diagnostic}"; then
      add_error "portable validation lacks exact correlation diagnostic: ${diagnostic}"
    fi
  done
  if [ "${#NNC62A_ERRORS[@]}" -eq "${correlation_error_count}" ]; then
    pass_check
  fi
}

verify_physical_codec() {
  if ! require_nonempty_file "${STORE_CODEC}" "durable compiled-plan codec" ||
    ! require_nonempty_file "${STORE_SCHEMA}" "durable compiled-plan schema"; then
    return
  fi
  codec_source="$(source_without_comments "${STORE_CODEC}")"
  schema_source="$(source_without_comments "${STORE_SCHEMA}")"
  physical_source="${codec_source}
${schema_source}"
  if ! printf '%s\n' "${physical_source}" | rg -q 'compiledNetworkPlan'; then
    add_error "physical saga codec/schema lacks one required compiledNetworkPlan object"
  else
    pass_check
  fi
  old_tuple="$(printf '%s\n' "${physical_source}" |
    rg -n 'networkPlanId|networkGeneration|networkPlanDigest' || true)"
  if [ "${NIMBUS_NETWORK_NNC62A_TEST_MUTATION:-}" = "physical-tuple" ]; then
    old_tuple="${old_tuple}\nnetworkPlanDigest"
  fi
  if [ -n "${old_tuple}" ]; then
    add_error "physical codec/schema retains flattened network tuple authority: ${old_tuple}"
  else
    pass_check
  fi
  if ! printf '%s\n' "${schema_source}" |
    rg -q 'field\("compiledNetworkPlan",[[:space:]]*FieldType::Object,[[:space:]]*true\)'; then
    add_error "compiledNetworkPlan is not one required physical object"
  else
    pass_check
  fi
}

verify_pure_action_and_process_proof() {
  if ! require_nonempty_file "${COMPUTE_RECOVERY}" "pure workload saga recovery decision" ||
    ! require_nonempty_file "${PROVISION_DECISION}" "pure workload provision decision"; then
    return
  fi
  recovery_source="$(source_without_comments "${COMPUTE_RECOVERY}")"
  provision_source="$(source_without_comments "${PROVISION_DECISION}")"
  compact_decisions="$(
    printf '%s\n%s\n' "${recovery_source}" "${provision_source}" | tr '\n' ' '
  )"
  if [ "${NIMBUS_NETWORK_NNC62A_TEST_MUTATION:-}" = "missing-action-plan" ] ||
    ! printf '%s\n' "${compact_decisions}" |
      rg -q 'WorkloadProvisionDecision::plan\(record\).*WorkloadProvisionStep::ReserveNetwork.*network_plan_digest:[[:space:]]*intent\.network\(\)\.digest\(\)'; then
    add_error "pure reserve attempt does not bind the exact durable compiled-plan digest"
  else
    pass_check
  fi

  if ! require_nonempty_file "${PROCESS_PROOF}" "distinct-process compiled-plan durability proof"; then
    case "${NIMBUS_NETWORK_NNC62A_TEST_MUTATION:-}" in
      snapshot-handoff | effect-import) process_source="" ;;
      *) return ;;
    esac
  else
    process_source="$(source_without_comments "${PROCESS_PROOF}")"
  fi
  for seam in \
    'SubprocessCrashCutHarness' \
    'workload-saga.compiled-plan-durable' \
    'CompiledWorkloadNetworkPlan' \
    'content().canonical_bytes()' \
    'IntentCommitted'; do
    if ! printf '%s\n' "${process_source}" | rg -q -F "${seam}"; then
      add_error "distinct-process proof lacks required seam ${seam}"
    fi
  done
  snapshot_handoff="$(printf '%s\n' "${process_source}" |
    rg -n '\.env\([^\n]*(PLAN_BYTES|PAYLOAD|RECORD_JSON|SNAPSHOT|DIGEST|EXPECTED)|\.arg\([^\n]*(plan-bytes|payload|record-json|snapshot|digest|expected)|stdin\(Stdio::piped|write\([^\n]*sidecar' || true)"
  if [ "${NIMBUS_NETWORK_NNC62A_TEST_MUTATION:-}" = "snapshot-handoff" ]; then
    snapshot_handoff="${snapshot_handoff}\n.env(\"PLAN_PAYLOAD\", bytes)"
  fi
  if [ -n "${snapshot_handoff}" ]; then
    add_error "distinct-process proof permits snapshot/payload handoff: ${snapshot_handoff}"
  else
    pass_check
  fi
  effect_import="$(printf '%s\n' "${process_source}" |
    rg -n 'LocalNetworkManager|LocalPortLeaseAuthority|NetworkAttachmentProvider|IngressProvider|ForwardingProvider|apply_network_plan|attach_network' || true)"
  if [ "${NIMBUS_NETWORK_NNC62A_TEST_MUTATION:-}" = "effect-import" ]; then
    effect_import="${effect_import}\nLocalNetworkManager"
  fi
  if [ -n "${effect_import}" ]; then
    add_error "distinct-process proof imports a network effect authority: ${effect_import}"
  else
    pass_check
  fi
}

verify_owner_and_allowlist() {
  if ! require_nonempty_file "${OWNER_PLAN}" "canonical network control-plane plan" ||
    ! require_nonempty_file "${OWNER_PROOF}" "NNC6.2a owner proof"; then
    return
  fi
  if ! rg -q 'NNC6\.2a.*durable complete compiled-plan carrier' "${OWNER_PLAN}"; then
    add_error "canonical plan does not route NNC6.2a to durable complete compiled-plan ownership"
  else
    pass_check
  fi

  if ! git cat-file -e "${COMPLETION_CHECKPOINT}^{commit}" 2>/dev/null; then
    add_error "NNC6.2a completion checkpoint is missing: ${COMPLETION_CHECKPOINT}"
    return
  fi
  if ! changed="$(
    git diff --name-only "${STARTING_CHECKPOINT}..${COMPLETION_CHECKPOINT}" 2>/dev/null
  )"; then
    add_error "NNC6.2a frozen source diff is unreadable"
    return
  fi
  changed="$(printf '%s\n' "${changed}" | sort -u)"
  if [ "${NIMBUS_NETWORK_NNC62A_TEST_MUTATION:-}" = "unexpected-path" ]; then
    changed="${changed}\ncrates/nimbus-network/src/provider_effect.rs"
  fi
  unexpected="$(printf '%s\n' "${changed}" | awk '
    NF == 0 { next }
    $0 == "crates/nimbus-workloads/src/saga.rs" { next }
    $0 == "crates/nimbus-workloads/src/network_plan.rs" { next }
    $0 == "crates/nimbus-workloads/src/saga/network.rs" { next }
    $0 == "crates/nimbus-workloads/src/saga/network/tests.rs" { next }
    $0 == "crates/nimbus-workloads/src/saga/state.rs" { next }
    $0 == "crates/nimbus-workloads/src/saga/tests.rs" { next }
    $0 == "crates/nimbus-workloads/src/store/tests.rs" { next }
    $0 == "crates/nimbus-compute/src/workload_saga/recovery.rs" { next }
    $0 == "crates/nimbus-compute/src/workload_saga/recovery/tests.rs" { next }
    $0 == "crates/nimbus-compute/src/workload_saga/tests.rs" { next }
    $0 == "crates/nimbus-server/src/workload_saga_store/codec.rs" { next }
    $0 == "crates/nimbus-server/src/workload_saga_store/schema.rs" { next }
    $0 == "crates/nimbus-server/src/workload_saga_store/tests/mod.rs" { next }
    $0 == "crates/nimbus-server/src/workload_saga_store/tests/codec.rs" { next }
    $0 == "crates/nimbus-server/src/workload_saga_store/tests/durability.rs" { next }
    $0 == "crates/nimbus-server/src/workload_saga_store/tests/recovery.rs" { next }
    $0 == "crates/nimbus-server/src/workload_saga_store/tests/tenant_enumeration.rs" { next }
    $0 == "crates/nimbus-server/src/workload_saga_store/tests/compiled_plan_durability.rs" { next }
    $0 == "crates/nimbus-server/src/workload_saga_store/tests/composition.rs" { next }
    $0 == "scripts/nimbus-network-control-plane/workload-network-plan-compiler-contract.sh" { next }
    $0 == "scripts/nimbus-network-control-plane/workload-network-plan-durability-contract.sh" { next }
    $0 == "scripts/verify-nimbus-network-control-plane.sh" { next }
    $0 == "docs/private/plans/proof/nimbus-network-control-plane/nnc6.2a-durable-compiled-network-plan.md" { next }
    $0 == "docs/private/plans/nimbus-network-control-plane-plan.md" { next }
    $0 == "docs/private/plans/README.md" { next }
    { print }
  ')"
  if [ -n "${unexpected}" ]; then
    add_error "source diff escapes the frozen NNC6.2a allowlist: ${unexpected}"
  else
    pass_check
  fi
}

run_contract() {
  cd "${REPO_ROOT}" || return 1
  NNC62A_ERRORS=()
  NNC62A_CHECKS=0
  for tool in git node rg awk; do
    if ! command -v "${tool}" >/dev/null 2>&1; then
      add_error "missing required verifier tool ${tool}"
    fi
  done
  verify_complete_carrier
  verify_current_format_and_correlations
  verify_physical_codec
  verify_pure_action_and_process_proof
  verify_owner_and_allowlist

  if [ "${#NNC62A_ERRORS[@]}" -ne 0 ]; then
    for error in "${NNC62A_ERRORS[@]}"; do
      printf 'NNC6.2a contract failure: %s\n' "${error}" >&2
    done
    return 1
  fi
  printf 'NNC6.2a durable compiled-plan contract: %d checks passed\n' "${NNC62A_CHECKS}"
}

run_self_test() {
  self_test_root="$(mktemp -d "${TMPDIR:-/tmp}/nnc62a-contract-self-test.XXXXXX")" || {
    printf 'NNC6.2a contract self-test: unable to create temporary directory\n' >&2
    return 1
  }
  trap 'rm -rf "${self_test_root}"' EXIT
  self_test_failures=0
  for mutation in \
    missing-carrier \
    tuple-authority \
    wrong-version \
    physical-tuple \
    missing-action-plan \
    snapshot-handoff \
    effect-import \
    missing-completion-checkpoint \
    unreadable-source-diff \
    unexpected-path; do
    output="${self_test_root}/${mutation}.out"
    if NIMBUS_NETWORK_NNC62A_TEST_MUTATION="${mutation}" bash "${SCRIPT_PATH}" >"${output}" 2>&1; then
      printf 'SELFTEST FAIL NNCV029 %s unexpectedly passed\n' "${mutation}"
      self_test_failures=$((self_test_failures + 1))
      continue
    fi
    case "${mutation}" in
      missing-carrier) expected='missing or empty workloads-owned compiled-plan carrier' ;;
      tuple-authority) expected='compiled-plan carrier retains caller-supplied tuple authority' ;;
      wrong-version) expected='workload saga format version is not the current strict v5' ;;
      physical-tuple) expected='physical codec/schema retains flattened network tuple authority' ;;
      missing-action-plan) expected='pure reserve attempt does not bind the exact durable compiled-plan digest' ;;
      snapshot-handoff) expected='distinct-process proof permits snapshot/payload handoff' ;;
      effect-import) expected='distinct-process proof imports a network effect authority' ;;
      missing-completion-checkpoint) expected='NNC6.2a completion checkpoint is missing' ;;
      unreadable-source-diff) expected='NNC6.2a frozen source diff is unreadable' ;;
      unexpected-path) expected='source diff escapes the frozen NNC6.2a allowlist' ;;
    esac
    if ! rg -q -F "${expected}" "${output}"; then
      printf 'SELFTEST FAIL NNCV029 %s missed diagnostic %s\n' "${mutation}" "${expected}"
      self_test_failures=$((self_test_failures + 1))
    else
      printf 'SELFTEST PASS NNCV029 %s fails closed\n' "${mutation}"
    fi
  done
  if [ "${self_test_failures}" -ne 0 ]; then
    printf 'NNC6.2a contract self-test: %d failed\n' "${self_test_failures}"
    return 1
  fi
  printf 'NNC6.2a contract self-test: 10 passed, 0 failed\n'
}

case "${1:-}" in
  "" | --check) run_contract ;;
  --self-test) run_self_test ;;
  *)
    printf 'usage: %s [--check|--self-test]\n' "$0" >&2
    exit 2
    ;;
esac
