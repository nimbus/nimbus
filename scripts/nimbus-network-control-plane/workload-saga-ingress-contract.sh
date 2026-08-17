#!/usr/bin/env bash
# Static NNC6.1e1 contract for the bounded durable workload-saga ingress.

set -u

REPO_ROOT="${NIMBUS_NETWORK_NNC61E1_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
SCRIPT_PATH="${REPO_ROOT}/scripts/nimbus-network-control-plane/workload-saga-ingress-contract.sh"
AUDIT_CHECKPOINT="f7638e2fd73c4b3b5316ac74a8d1dc8ba2cb5675"
COMPLETION_CHECKPOINT="26df5075d7dab582a4c9602e248993eabd8eab49"
COMPUTE_ROOT="crates/nimbus-compute/src/workload_saga.rs"
INGRESS="crates/nimbus-compute/src/workload_saga/ingress.rs"
INGRESS_TESTS="crates/nimbus-compute/src/workload_saga/ingress/tests.rs"
SERVER_TEST_ROOT="crates/nimbus-server/src/workload_saga_store/tests/mod.rs"
SERVER_PROOF="crates/nimbus-server/src/workload_saga_store/tests/ingress.rs"
OWNER_CONTRACT="scripts/nimbus-network-control-plane/verification-contract.json"
HISTORICAL_PLAN_PATH="docs/private/plans/nimbus-network-control-plane-"'plan.md'

NNC61E1_ERRORS=()
NNC61E1_CHECKS=0

add_error() {
  NNC61E1_ERRORS+=("$1")
}

pass_check() {
  NNC61E1_CHECKS=$((NNC61E1_CHECKS + 1))
}

source_without_comments() {
  node - "$1" <<'NODE'
const fs = require("fs");
const path = process.argv[2];
const source = fs.existsSync(path) ? fs.readFileSync(path, "utf8") : "";
process.stdout.write(source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/.*$/gm, ""));
NODE
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

apply_test_mutation() {
  mutation="${NIMBUS_NETWORK_NNC61E1_TEST_MUTATION:-}"
  case "${mutation}" in
    missing-ingress) ingress_source="" ;;
    duplicate-submit) ingress_source="${ingress_source}
pub async fn submit_intent() {}" ;;
    public-raw-commit) compute_source="${compute_source}
pub async fn commit_loaded() {}" ;;
    effect-import) ingress_source="${ingress_source}
use nimbus_services::ServiceManager;" ;;
    missing-replay-test)
      ingress_tests_source="${ingress_tests_source/exact_replay_performs_zero_cas/removed_replay_case}"
      ;;
    missing-ambiguity-test)
      ingress_tests_source="${ingress_tests_source/ambiguous_exact_next_uses_one_fresh_read/removed_ambiguity_case}"
      ;;
    missing-crossed-key-test)
      ingress_tests_source="${ingress_tests_source/crossed_loaded_key_is_corrupt/removed_crossed_key_case}"
      ;;
    missing-contention-harness)
      server_source="${server_source/TwoProcessContentionHarness/RemovedContentionHarness}"
      ;;
    missing-crash-harness)
      server_source="${server_source/SubprocessCrashCutHarness/RemovedCrashHarness}"
      ;;
    duplicate-coordinator) compute_source="${compute_source}
pub struct WorkloadSagaCoordinator;" ;;
    unexpected-path)
      changed_paths="${changed_paths}
crates/nimbus-services/src/manager/activation.rs"
      ;;
    wrong-plan-route)
      contract_source="${contract_source/bounded compute-owned durable workload-saga submission seam/effectful runtime lifecycle registry}"
      ;;
  esac
}

verify_surface() {
  ingress_source="$(source_without_comments "${REPO_ROOT}/${INGRESS}")"
  compute_source="$(source_without_comments "${REPO_ROOT}/${COMPUTE_ROOT}")"
  ingress_tests_source="$(source_without_comments "${REPO_ROOT}/${INGRESS_TESTS}")"
  server_root_source="$(source_without_comments "${REPO_ROOT}/${SERVER_TEST_ROOT}")"
  server_source="$(source_without_comments "${REPO_ROOT}/${SERVER_PROOF}")"
  contract_source="$(source_without_comments "${REPO_ROOT}/${OWNER_CONTRACT}")"
  changed_paths="${NIMBUS_NETWORK_NNC61E1_TEST_CHANGED_PATHS:-}"

  if [ -z "${changed_paths}" ]; then
    if ! git -C "${REPO_ROOT}" cat-file -e "${AUDIT_CHECKPOINT}^{commit}" 2>/dev/null; then
      add_error "NNC6.1e1 audit checkpoint is missing: ${AUDIT_CHECKPOINT}"
      changed_paths=""
    else
      if ! git -C "${REPO_ROOT}" cat-file -e "${COMPLETION_CHECKPOINT}^{commit}" 2>/dev/null; then
        add_error "NNC6.1e1 completion checkpoint is missing: ${COMPLETION_CHECKPOINT}"
        committed_paths=""
      elif ! committed_paths="$(
        git -C "${REPO_ROOT}" diff --name-only "${AUDIT_CHECKPOINT}..${COMPLETION_CHECKPOINT}" 2>/dev/null
      )"; then
        add_error "NNC6.1e1 committed source range is unreadable"
        committed_paths=""
      fi
      changed_paths="$(printf '%s\n' "${committed_paths}" | sort -u)"
    fi
  fi

  apply_test_mutation

  if [ -z "${ingress_source}" ]; then
    add_error "missing concept-owned workload-saga ingress"
  else
    pass_check
  fi

  if ! printf '%s\n' "${compute_source}" |
    rg -q 'mod[[:space:]]+ingress[[:space:]]*;'; then
    add_error "compute coordinator does not own the ingress child module"
  elif ! printf '%s\n' "${compute_source}" |
    rg -U -q 'pub[[:space:]]+use[[:space:]]+ingress::[^;]*ConfirmedWorkloadSagaIntent'; then
    add_error "compute coordinator does not export the confirmed ingress result"
  else
    pass_check
  fi

  submit_count="$(
    printf '%s\n' "${ingress_source}" |
      rg -o 'pub[[:space:]]+async[[:space:]]+fn[[:space:]]+submit_intent[[:space:]]*\(' |
      awk 'END { print NR + 0 }'
  )"
  if [ "${submit_count}" -ne 1 ]; then
    add_error "expected one public submit_intent ingress, observed ${submit_count}"
  else
    pass_check
  fi

  if printf '%s\n' "${compute_source}" |
    rg -q 'pub([[:space:]]*\([^)]*\))?[[:space:]]+async[[:space:]]+fn[[:space:]]+commit_loaded[[:space:]]*\('; then
    add_error "raw commit_loaded remains externally callable"
  else
    pass_check
  fi

  forbidden_effects="$(
    printf '%s\n' "${ingress_source}" |
      rg -n 'ServiceManager|SandboxBackend|LocalNetworkManager|NetworkAttachmentProvider|IngressProvider|ForwardingProvider|start_service|stop_service|restart_service|apply_network_plan|TcpListener|UdpSocket|nimbus_(services|sandbox|node|server|system)' || true
  )"
  if [ -n "${forbidden_effects}" ]; then
    add_error "workload-saga ingress imports or calls an effect authority: ${forbidden_effects}"
  else
    pass_check
  fi

  coordinator_count="$(
    printf '%s\n' "${compute_source}" |
      rg -o 'pub[[:space:]]+struct[[:space:]]+WorkloadSagaCoordinator' |
      awk 'END { print NR + 0 }'
  )"
  if [ "${coordinator_count}" -ne 1 ]; then
    add_error "expected one canonical WorkloadSagaCoordinator, observed ${coordinator_count}"
  else
    pass_check
  fi

  test_error_count="${#NNC61E1_ERRORS[@]}"
  for required_test in \
    missing_intent_is_confirmed_before_decision \
    exact_replay_performs_zero_cas \
    successor_withdraws_before_reservation \
    conflict_is_not_retried \
    ambiguous_exact_next_uses_one_fresh_read \
    crossed_loaded_key_is_corrupt \
    cancellation_before_commit_exposes_no_decision; do
    if ! printf '%s\n' "${ingress_tests_source}" | rg -q -F "${required_test}"; then
      add_error "ingress behavioral matrix lacks ${required_test}"
    fi
  done
  if [ "${#NNC61E1_ERRORS[@]}" -eq "${test_error_count}" ]; then
    pass_check
  fi

  process_error_count="${#NNC61E1_ERRORS[@]}"
  for process_seam in \
    'mod ingress;' \
    TwoProcessContentionHarness \
    SubprocessCrashCutHarness \
    submit_intent \
    distinct_process_intent_contention_converges \
    crash_before_and_after_durability_reopens_exact_decision; do
    if ! printf '%s\n%s\n' "${server_root_source}" "${server_source}" |
      rg -q -F "${process_seam}"; then
      add_error "distinct-process ingress proof lacks ${process_seam}"
    fi
  done
  if [ "${#NNC61E1_ERRORS[@]}" -eq "${process_error_count}" ]; then
    pass_check
  fi

  if ! printf '%s\n' "${contract_source}" |
    rg -q '"NNC6\.1e1":[[:space:]]*"Implement the bounded compute-owned durable workload-saga submission seam after NNC6\.2a\.'; then
    add_error "stable contract does not route NNC6.1e1 to bounded durable submission"
  else
    pass_check
  fi

  unexpected="$(printf '%s\n' "${changed_paths}" | awk -v historical_plan_path="${HISTORICAL_PLAN_PATH}" '
    NF == 0 { next }
    $0 == "crates/nimbus-compute/src/workload_saga.rs" { next }
    $0 == "crates/nimbus-compute/src/workload_saga/ingress.rs" { next }
    $0 == "crates/nimbus-compute/src/workload_saga/ingress/tests.rs" { next }
    $0 == "crates/nimbus-server/src/workload_saga_store/tests/mod.rs" { next }
    $0 == "crates/nimbus-server/src/workload_saga_store/tests/ingress.rs" { next }
    $0 == "scripts/nimbus-network-control-plane/workload-saga-ingress-contract.sh" { next }
    $0 == "scripts/verify-nimbus-network-control-plane.sh" { next }
    $0 == historical_plan_path { next }
    $0 == "docs/private/plans/README.md" { next }
    $0 == "docs/private/plans/proof/nimbus-network-control-plane/nnc6.1e1-durable-workload-saga-ingress.md" { next }
    $0 ~ /^docs\/private\/plans\/proof\/nimbus-network-control-plane\/nnc[0-9][0-9A-Za-z._-]*\.md$/ { next }
    { print }
  ')"
  if [ -n "${unexpected}" ]; then
    add_error "NNC6.1e1 source diff escapes the frozen allowlist: ${unexpected}"
  else
    pass_check
  fi
}

run_contract() {
  cd "${REPO_ROOT}" || return 1
  NNC61E1_ERRORS=()
  NNC61E1_CHECKS=0
  for tool in git node rg awk; do
    if ! command -v "${tool}" >/dev/null 2>&1; then
      add_error "missing required verifier tool ${tool}"
    fi
  done
  verify_surface
  if [ "${#NNC61E1_ERRORS[@]}" -ne 0 ]; then
    for error in "${NNC61E1_ERRORS[@]}"; do
      printf 'NNC6.1e1 ingress contract failure: %s\n' "${error}" >&2
    done
    return 1
  fi
  printf 'NNC6.1e1 durable ingress contract: %d checks passed\n' "${NNC61E1_CHECKS}"
}

write_fixture() {
  fixture="$1"
  mkdir -p \
    "${fixture}/crates/nimbus-compute/src/workload_saga/ingress" \
    "${fixture}/crates/nimbus-server/src/workload_saga_store/tests" \
    "${fixture}/docs/private/plans" \
    "${fixture}/scripts/nimbus-network-control-plane"
  cp "${SCRIPT_PATH}" \
    "${fixture}/scripts/nimbus-network-control-plane/workload-saga-ingress-contract.sh"
  printf '%s\n' \
    'mod ingress;' \
    'pub use ingress::{ConfirmedWorkloadSagaIntent, WorkloadSagaIngressDisposition};' \
    'pub struct WorkloadSagaCoordinator;' \
    'impl WorkloadSagaCoordinator { async fn commit_loaded() {} }' \
    >"${fixture}/${COMPUTE_ROOT}"
  printf '%s\n' \
    'pub struct ConfirmedWorkloadSagaIntent;' \
    'pub enum WorkloadSagaIngressDisposition { Applied, ConfirmedReplay }' \
    'impl WorkloadSagaCoordinator {' \
    '  pub async fn submit_intent(&self) {' \
    '    let loaded = self.load();' \
    '    let committed = self.commit_loaded(loaded);' \
    '    let _decision = WorkloadSagaDecision::for_record(committed);' \
    '  }' \
    '}' \
    >"${fixture}/${INGRESS}"
  printf '%s\n' \
    'fn missing_intent_is_confirmed_before_decision() {}' \
    'fn exact_replay_performs_zero_cas() {}' \
    'fn successor_withdraws_before_reservation() {}' \
    'fn conflict_is_not_retried() {}' \
    'fn ambiguous_exact_next_uses_one_fresh_read() {}' \
    'fn crossed_loaded_key_is_corrupt() {}' \
    'fn cancellation_before_commit_exposes_no_decision() {}' \
    >"${fixture}/${INGRESS_TESTS}"
  printf '%s\n' 'mod ingress;' >"${fixture}/${SERVER_TEST_ROOT}"
  printf '%s\n' \
    'use nimbus_testing::{TwoProcessContentionHarness, SubprocessCrashCutHarness};' \
    'fn submit_intent() {}' \
    'fn distinct_process_intent_contention_converges() {}' \
    'fn crash_before_and_after_durability_reopens_exact_decision() {}' \
    >"${fixture}/${SERVER_PROOF}"
  printf '%s\n' \
    '{' \
    '  "routes": {' \
    '    "NNC6.1e1": "Implement the bounded compute-owned durable workload-saga submission seam after NNC6.2a."' \
    '  }' \
    '}' \
    >"${fixture}/${OWNER_CONTRACT}"
}

run_self_test() {
  self_test_root="$(mktemp -d "${TMPDIR:-/tmp}/nnc61e1-contract-self-test.XXXXXX")" || {
    printf 'NNC6.1e1 ingress contract self-test: unable to create temporary directory\n' >&2
    return 1
  }
  trap 'rm -rf "${self_test_root}"' EXIT
  fixture="${self_test_root}/fixture"
  write_fixture "${fixture}"
  failures=0
  proof_output="${self_test_root}/later-proof-path.out"
  if NIMBUS_NETWORK_NNC61E1_ROOT="${fixture}" \
    NIMBUS_NETWORK_NNC61E1_TEST_CHANGED_PATHS="docs/private/plans/proof/nimbus-network-control-plane/nnc6.3-later-item-proof.md" \
    bash "${fixture}/scripts/nimbus-network-control-plane/workload-saga-ingress-contract.sh" \
    >"${proof_output}" 2>&1; then
    printf 'SELFTEST PASS NNCV030 later item proof is not classified as product source\n'
  else
    printf 'SELFTEST FAIL NNCV030 later item proof was classified as product source\n'
    failures=$((failures + 1))
  fi
  for mutation in \
    missing-ingress \
    duplicate-submit \
    public-raw-commit \
    effect-import \
    missing-replay-test \
    missing-ambiguity-test \
    missing-crossed-key-test \
    missing-contention-harness \
    missing-crash-harness \
    duplicate-coordinator \
    unexpected-path \
    wrong-plan-route; do
    output="${self_test_root}/${mutation}.out"
    if NIMBUS_NETWORK_NNC61E1_ROOT="${fixture}" \
      NIMBUS_NETWORK_NNC61E1_TEST_MUTATION="${mutation}" \
      NIMBUS_NETWORK_NNC61E1_TEST_CHANGED_PATHS="scripts/nimbus-network-control-plane/workload-saga-ingress-contract.sh" \
      bash "${fixture}/scripts/nimbus-network-control-plane/workload-saga-ingress-contract.sh" \
      >"${output}" 2>&1; then
      printf 'SELFTEST FAIL NNCV030 %s unexpectedly passed\n' "${mutation}"
      failures=$((failures + 1))
      continue
    fi
    case "${mutation}" in
      missing-ingress) expected='missing concept-owned workload-saga ingress' ;;
      duplicate-submit) expected='expected one public submit_intent ingress' ;;
      public-raw-commit) expected='raw commit_loaded remains externally callable' ;;
      effect-import) expected='workload-saga ingress imports or calls an effect authority' ;;
      missing-replay-test) expected='ingress behavioral matrix lacks exact_replay_performs_zero_cas' ;;
      missing-ambiguity-test) expected='ingress behavioral matrix lacks ambiguous_exact_next_uses_one_fresh_read' ;;
      missing-crossed-key-test) expected='ingress behavioral matrix lacks crossed_loaded_key_is_corrupt' ;;
      missing-contention-harness) expected='distinct-process ingress proof lacks TwoProcessContentionHarness' ;;
      missing-crash-harness) expected='distinct-process ingress proof lacks SubprocessCrashCutHarness' ;;
      duplicate-coordinator) expected='expected one canonical WorkloadSagaCoordinator' ;;
      unexpected-path) expected='NNC6.1e1 source diff escapes the frozen allowlist' ;;
      wrong-plan-route) expected='stable contract does not route NNC6.1e1 to bounded durable submission' ;;
    esac
    if ! rg -q -F "${expected}" "${output}"; then
      printf 'SELFTEST FAIL NNCV030 %s missed diagnostic %s\n' "${mutation}" "${expected}"
      failures=$((failures + 1))
    else
      printf 'SELFTEST PASS NNCV030 %s fails closed\n' "${mutation}"
    fi
  done
  if [ "${failures}" -ne 0 ]; then
    printf 'NNC6.1e1 ingress contract self-test: %d failed\n' "${failures}"
    return 1
  fi
  printf 'NNC6.1e1 ingress contract self-test: 13 passed, 0 failed\n'
}

case "${1:-}" in
  '' | --check) run_contract ;;
  --self-test) run_self_test ;;
  *)
    printf 'usage: %s [--check|--self-test]\n' "$0" >&2
    exit 2
    ;;
esac
