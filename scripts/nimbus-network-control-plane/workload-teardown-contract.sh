#!/usr/bin/env bash
# Static NNC6.5 contract for one fenced, compute-owned teardown lifecycle.

set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="${NIMBUS_NETWORK_NNC65_ROOT:-$(cd "${SCRIPT_DIR}/../.." && pwd)}"
SOURCE_CONTRACT="${REPO_ROOT}/scripts/verify-nimbus-network-source-contract.mjs"

run_contract() {
  cd "${REPO_ROOT}" || return 1
  output="$(node "${SOURCE_CONTRACT}" workload-teardown-contract 2>&1)"
  status=$?
  if [ "${status}" -eq 0 ]; then
    printf 'Summary: 1 passed, 0 failed\n'
    return 0
  fi
  printf '%s\n' "${output}"
  diagnostic_count="$(printf '%s\n' "${output}" | rg -c '^teardown-contract/' || true)"
  printf 'Summary: 0 passed, %d failed\n' "${diagnostic_count}"
  return 1
}

run_native_stage() {
  cd "${REPO_ROOT}" || return 1
  output="$(NIMBUS_NETWORK_VERIFY_TEARDOWN_STAGE=native \
    node "${SOURCE_CONTRACT}" workload-teardown-contract 2>&1)"
  status=$?
  if [ "${status}" -eq 0 ]; then
    printf 'Summary: 1 passed, 0 failed\n'
    return 0
  fi
  printf '%s\n' "${output}"
  diagnostic_count="$(printf '%s\n' "${output}" | rg -c '^teardown-contract/' || true)"
  printf 'Summary: 0 passed, %d failed\n' "${diagnostic_count}"
  return 1
}

run_physical_machine_stage() {
  cd "${REPO_ROOT}" || return 1
  output="$(NIMBUS_NETWORK_VERIFY_TEARDOWN_STAGE=physical-machine \
    node "${SOURCE_CONTRACT}" workload-teardown-contract 2>&1)"
  status=$?
  if [ "${status}" -eq 0 ]; then
    printf 'Summary: 1 passed, 0 failed\n'
    return 0
  fi
  printf '%s\n' "${output}"
  diagnostic_count="$(printf '%s\n' "${output}" | rg -c '^teardown-contract/' || true)"
  printf 'Summary: 0 passed, %d failed\n' "${diagnostic_count}"
  return 1
}

run_self_test() {
  cd "${REPO_ROOT}" || return 1
  failures=0
  passed=0
  cases=(
    'missing-phase|teardown-contract/vocabulary: portable teardown phase and reference vocabulary is incomplete or open'
    'open-phase-enum|teardown-contract/vocabulary: portable teardown phase and reference vocabulary is incomplete or open'
    'missing-reference-set|teardown-contract/vocabulary: portable teardown phase and reference vocabulary is incomplete or open'
    'missing-attempt-id|teardown-contract/reducer: compute is not the sole fenced teardown CAS authority'
    'missing-claim|teardown-contract/reducer: compute is not the sole fenced teardown CAS authority'
    'missing-reducer|teardown-contract/reducer: compute is not the sole fenced teardown CAS authority'
    'missing-revision-fence|teardown-contract/reducer: compute is not the sole fenced teardown CAS authority'
    'missing-commit-loaded|teardown-contract/reducer: compute is not the sole fenced teardown CAS authority'
    'missing-command|teardown-contract/command: confirmed teardown commands are forgeable or incompletely fenced'
    'missing-command-transition|teardown-contract/command: confirmed teardown commands are forgeable or incompletely fenced'
    'missing-command-attempt|teardown-contract/command: confirmed teardown commands are forgeable or incompletely fenced'
    'missing-command-epoch|teardown-contract/command: confirmed teardown commands are forgeable or incompletely fenced'
    'forgeable-command|teardown-contract/command: confirmed teardown commands are forgeable or incompletely fenced'
    'stop-before-withdraw|teardown-contract/order: durable teardown does not enforce withdrawal then drain then stop then detach then release then record'
    'detach-before-stop|teardown-contract/order: durable teardown does not enforce withdrawal then drain then stop then detach then release then record'
    'release-before-detach|teardown-contract/order: durable teardown does not enforce withdrawal then drain then stop then detach then release then record'
    'record-before-release|teardown-contract/order: durable teardown does not enforce withdrawal then drain then stop then detach then release then record'
    'missing-service-submit|teardown-contract/service: service or sandbox stop retains direct provider-effect authority'
    'missing-sandbox-submit|teardown-contract/service: service or sandbox stop retains direct provider-effect authority'
    'missing-service-projection|teardown-contract/service: service or sandbox stop retains direct provider-effect authority'
    'missing-sandbox-projection|teardown-contract/service: service or sandbox stop retains direct provider-effect authority'
    'missing-definition-claim|teardown-contract/definition-delete: definition removal can cross unresolved or late lifecycle work'
    'missing-provision-join|teardown-contract/definition-delete: definition removal can cross unresolved or late lifecycle work'
    'missing-late-result-drain|teardown-contract/definition-delete: definition removal can cross unresolved or late lifecycle work'
    'missing-definition-finalize|teardown-contract/definition-delete: definition removal can cross unresolved or late lifecycle work'
    'missing-compose-persistence|teardown-contract/compose: Compose down bypasses the canonical Engine-backed compute saga'
    'missing-compose-engine|teardown-contract/compose: Compose down bypasses the canonical Engine-backed compute saga'
    'missing-compose-store|teardown-contract/compose: Compose down bypasses the canonical Engine-backed compute saga'
    'compose-store-not-wired|teardown-contract/compose: Compose down bypasses the canonical Engine-backed compute saga'
    'compose-wired-activation-discarded|teardown-contract/compose: Compose down bypasses the canonical Engine-backed compute saga'
    'missing-compose-retirer|teardown-contract/compose: Compose down bypasses the canonical Engine-backed compute saga'
    'missing-compose-submit|teardown-contract/compose: Compose down bypasses the canonical Engine-backed compute saga'
    'missing-compose-recorded|teardown-contract/compose: Compose down bypasses the canonical Engine-backed compute saga'
    'compose-recorded-result-discarded|teardown-contract/compose: Compose down bypasses the canonical Engine-backed compute saga'
    'compose-terminal-reference-discarded|teardown-contract/compose: Compose down bypasses the canonical Engine-backed compute saga'
    'compose-recorded-omits-terminal-binding|teardown-contract/compose: Compose down bypasses the canonical Engine-backed compute saga'
    'cli-local-saga-store|teardown-contract/compose: Compose down bypasses the canonical Engine-backed compute saga'
    'compose-direct-stop|teardown-contract/compose: Compose down bypasses the canonical Engine-backed compute saga'
    'missing-machine-envelope|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'missing-machine-phase|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'missing-machine-fence|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'guest-dispatch-skips-validation|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'guest-dispatch-discards-validation|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'guest-remote-before-journal-claim|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'guest-aliased-remote-before-journal-claim|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'parent-release-before-absence|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'missing-machine-active-fence|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'missing-machine-rescan|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'untyped-machine-active-conflict|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'projection-address-machine-authority|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'unavailable-machine-authority-allows-stop|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'crossed-machine-authority-allows-stop|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'machine-barrier-after-publication|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'machine-standalone-bypass|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'machine-server-bypass|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'machine-restart-bypass|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'machine-bootc-restart-bypass|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'machine-os-apply-restart-bypass|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'machine-admission-after-empty-scan|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'missing-machine-active-barrier-clear|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'missing-initial-desire-admission-guard|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'initial-desire-guard-released-before-cas|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'missing-restart-desire-admission-guard|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'restart-desire-guard-released-before-cas|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'missing-machine-barrier-digest-machine-name|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'missing-machine-barrier-digest-authority|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'missing-machine-barrier-digest-epoch|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'missing-machine-barrier-digest-state|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'machine-barrier-digest-disconnected|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'missing-machine-admission-barrier-traversal|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'missing-machine-admission-provider-comparison|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'missing-machine-admission-generation-comparison|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'machine-admission-provider-fails-open|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'machine-admission-generation-fails-open|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'machine-admission-fence-fails-open|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'missing-machine-admission-authentication|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'machine-desire-guard-does-not-hold-lock|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'machine-barrier-claim-outside-provider-lock|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'machine-provider-auth-outside-provider-lock|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'machine-provision-admission-bypasses-barrier|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'machine-provision-effect-before-barrier-auth|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'machine-publication-admission-bypasses-barrier|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'machine-publication-effect-before-barrier-auth|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'machine-restart-admission-bypasses-barrier|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'machine-restart-effect-before-barrier-auth|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'physical-effect-authentication-outside-stop-body|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'machine-stop-policy-moved-to-server-adapter|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'machine-barrier-persistence-moved-to-backend|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'coarse-guest-route-survives|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'coarse-guest-wire-survives|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'coarse-guest-capability-survives|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'missing-forwarded-registry-registrations|teardown-contract/forwarded-machine-registry: forwarded teardown does not expose the exact five-phase compute registry substitution'
    'missing-forwarded-registry-capability|teardown-contract/forwarded-machine-registry: forwarded teardown does not expose the exact five-phase compute registry substitution'
    'missing-forwarded-canonical-registry|teardown-contract/forwarded-machine-registry: forwarded teardown does not expose the exact five-phase compute registry substitution'
    'missing-forwarded-server-registry|teardown-contract/forwarded-machine-registry: forwarded teardown does not expose the exact five-phase compute registry substitution'
    'forwarded-server-discards-canonical-result|teardown-contract/forwarded-machine-registry: forwarded teardown does not expose the exact five-phase compute registry substitution'
    'missing-forwarded-compose-registry|teardown-contract/forwarded-machine-registry: forwarded teardown does not expose the exact five-phase compute registry substitution'
    'forwarded-compose-discards-canonical-result|teardown-contract/forwarded-machine-registry: forwarded teardown does not expose the exact five-phase compute registry substitution'
    'missing-forwarded-registry-test|teardown-contract/forwarded-machine-registry: forwarded teardown does not expose the exact five-phase compute registry substitution'
    'missing-forwarded-registry-inspect-test|teardown-contract/forwarded-machine-registry: forwarded teardown does not expose the exact five-phase compute registry substitution'
    'missing-forwarded-lifecycle-prepared-start|teardown-contract/forwarded-machine-lifecycle: parent and guest lifecycle authority is incomplete or not batch fenced'
    'missing-forwarded-lifecycle-absence-retry|teardown-contract/forwarded-machine-lifecycle: parent and guest lifecycle authority is incomplete or not batch fenced'
    'missing-forwarded-lifecycle-batch-retain|teardown-contract/forwarded-machine-lifecycle: parent and guest lifecycle authority is incomplete or not batch fenced'
    'missing-forwarded-lifecycle-batch-release|teardown-contract/forwarded-machine-lifecycle: parent and guest lifecycle authority is incomplete or not batch fenced'
    'missing-forwarded-recovery-request-start|teardown-contract/forwarded-machine-recovery: request-loss and two-realm crash recovery proofs are incomplete'
    'missing-forwarded-recovery-inspect|teardown-contract/forwarded-machine-recovery: request-loss and two-realm crash recovery proofs are incomplete'
    'missing-forwarded-recovery-response-loss-test|teardown-contract/forwarded-machine-recovery: request-loss and two-realm crash recovery proofs are incomplete'
    'missing-forwarded-recovery-process-test|teardown-contract/forwarded-machine-recovery: request-loss and two-realm crash recovery proofs are incomplete'
    'missing-ingress-capability|teardown-contract/ingress: final ingress withdrawal cannot prove exact worker, route, and lease settlement'
    'missing-ingress-join|teardown-contract/ingress: final ingress withdrawal cannot prove exact worker, route, and lease settlement'
    'missing-ingress-settlement|teardown-contract/ingress: final ingress withdrawal cannot prove exact worker, route, and lease settlement'
    'swallowed-ingress-failure|teardown-contract/ingress: final ingress withdrawal cannot prove exact worker, route, and lease settlement'
    'missing-tenant-enumeration|teardown-contract/tenant: tenant deletion bypasses durable child-saga teardown'
    'missing-tenant-driver|teardown-contract/tenant: tenant deletion bypasses durable child-saga teardown'
    'finish-tenant-delete-early|teardown-contract/tenant: tenant deletion bypasses durable child-saga teardown'
    'missing-failed-provision-cause|teardown-contract/compensation: provision or restart handoff lacks exact durable settlement and ambiguity handling'
    'missing-failed-provision-compensation|teardown-contract/compensation: provision or restart handoff lacks exact durable settlement and ambiguity handling'
    'ambiguous-provision-stops|teardown-contract/compensation: provision or restart handoff lacks exact durable settlement and ambiguity handling'
    'missing-restart-handoff|teardown-contract/order: durable teardown does not enforce withdrawal then drain then stop then detach then release then record'
    'missing-attributed-tests|teardown-contract/behavior: required teardown behavior proofs are incomplete or non-assertive'
    'empty-test-body|teardown-contract/behavior: required teardown behavior proofs are incomplete or non-assertive'
    'helper-only-test-body|teardown-contract/behavior: required teardown behavior proofs are incomplete or non-assertive'
    'declaration-only-test-body|teardown-contract/behavior: required teardown behavior proofs are incomplete or non-assertive'
    'tautological-test-assertion|teardown-contract/behavior: required teardown behavior proofs are incomplete or non-assertive'
    'network-effect|teardown-contract/network: nimbus-network gained teardown effects or a god provider'
    'god-provider|teardown-contract/network: nimbus-network gained teardown effects or a god provider'
    'unexpected-path|teardown-contract/paths: NNC6.5 changed a path outside the frozen audit allowlist'
    'invalid-completed-item-checkpoint|teardown-contract/paths: NNC6.5 changed a path outside the frozen audit allowlist'
    'missing-ledger-token|teardown-contract/ledger: plan and proof do not retain the NNC6.5 expected-red acceptance tokens'
  )

  if ! NIMBUS_NETWORK_VERIFY_TEARDOWN_FIXTURE=1 \
    node "${SOURCE_CONTRACT}" workload-teardown-contract; then
    printf 'SELFTEST FAIL NNCV035 green fixture did not pass\n'
    failures=$((failures + 1))
  fi

  if ! NIMBUS_NETWORK_VERIFY_TEARDOWN_FIXTURE=1 \
    NIMBUS_NETWORK_VERIFY_TEARDOWN_STAGE=native \
    node "${SOURCE_CONTRACT}" workload-teardown-contract; then
    printf 'SELFTEST FAIL NNCV035 native green fixture did not pass\n'
    failures=$((failures + 1))
  fi

  if ! NIMBUS_NETWORK_VERIFY_TEARDOWN_FIXTURE=1 \
    NIMBUS_NETWORK_VERIFY_TEARDOWN_MUTATION=future-product-path \
    node "${SOURCE_CONTRACT}" workload-teardown-contract; then
    printf 'SELFTEST FAIL NNCV035 future product work changed the frozen audit path range\n'
    failures=$((failures + 1))
  else
    printf 'SELFTEST PASS NNCV035 completed audit path range ignores future product work\n'
  fi

  for entry in "${cases[@]}"; do
    mutation="${entry%%|*}"
    expected="${entry#*|}"
    output="$({
      NIMBUS_NETWORK_VERIFY_TEARDOWN_FIXTURE=1 \
        NIMBUS_NETWORK_VERIFY_TEARDOWN_MUTATION="${mutation}" \
        node "${SOURCE_CONTRACT}" workload-teardown-contract
    } 2>&1)"
    status=$?
    diagnostic_count="$(printf '%s\n' "${output}" | rg -c '^teardown-contract/' || true)"
    if [ "${status}" -eq 0 ]; then
      printf 'SELFTEST FAIL NNCV035 mutation %s unexpectedly passed\n' "${mutation}"
      failures=$((failures + 1))
    elif [ "${diagnostic_count}" -ne 1 ] || ! printf '%s\n' "${output}" | rg -q -F -x "${expected}"; then
      printf 'SELFTEST FAIL NNCV035 mutation %s did not fail with its sole named diagnostic\n' "${mutation}"
      printf '%s\n' "${output}"
      failures=$((failures + 1))
    else
      printf 'SELFTEST PASS NNCV035 mutation %s fails closed\n' "${mutation}"
      passed=$((passed + 1))
    fi
  done

  native_cases=(
    'missing-native-runtime'
    'missing-native-local-registry'
    'missing-provision-join'
    'missing-late-result-drain'
    'missing-native-restart-settlement'
    'missing-native-execution-reference'
    'source-execution-generation-conflated'
    'missing-native-test'
    'native-direct-effect'
    'native-direct-sandbox-retirement'
    'native-direct-backend-stop'
    'native-source-finalizer-direct-backend-stop'
    'native-source-finalizer-aliased-backend-stop'
    'native-source-finalizer-ufcs-backend-stop'
    'managed-teardown-raw-registry-field'
    'managed-teardown-unused-exact-realm'
    'service-source-claim-yield-poll'
    'sandbox-source-claim-yield-poll'
    'source-claim-helper-hidden-yield-poll'
    'sandbox-context-downgrade'
    'definition-context-downgrade'
  )
  native_diagnostic='teardown-contract/native-source-retirement: native stop or definition deletion bypasses the exact compute teardown, source fence, generation split, or attributed proof'
  for mutation in "${native_cases[@]}"; do
    output="$({
      NIMBUS_NETWORK_VERIFY_TEARDOWN_FIXTURE=1 \
        NIMBUS_NETWORK_VERIFY_TEARDOWN_STAGE=native \
        NIMBUS_NETWORK_VERIFY_TEARDOWN_MUTATION="${mutation}" \
        node "${SOURCE_CONTRACT}" workload-teardown-contract
    } 2>&1)"
    status=$?
    diagnostic_count="$(printf '%s\n' "${output}" | rg -c '^teardown-contract/' || true)"
    if [ "${status}" -eq 0 ]; then
      printf 'SELFTEST FAIL NNCV035 native mutation %s unexpectedly passed\n' "${mutation}"
      failures=$((failures + 1))
    elif [ "${diagnostic_count}" -ne 1 ] || ! printf '%s\n' "${output}" | rg -q -F -x "${native_diagnostic}"; then
      printf 'SELFTEST FAIL NNCV035 native mutation %s did not fail with its sole named diagnostic\n' "${mutation}"
      printf '%s\n' "${output}"
      failures=$((failures + 1))
    else
      printf 'SELFTEST PASS NNCV035 native mutation %s fails closed\n' "${mutation}"
      passed=$((passed + 1))
    fi
  done

  if [ "${failures}" -ne 0 ]; then
    printf 'NNC6.5 teardown contract self-test: %d passed, %d failed\n' \
      "${passed}" "${failures}"
    return 1
  fi
  if [ "${passed}" -ne 150 ]; then
    printf 'NNC6.5 teardown contract self-test: expected 150 mutations, observed %d\n' \
      "${passed}"
    return 1
  fi
  printf 'NNC6.5 teardown contract self-test: 150 passed, 0 failed\n'
}

case "${1:-}" in
  '' | --check) run_contract ;;
  --native-stage) run_native_stage ;;
  --physical-machine-stage) run_physical_machine_stage ;;
  --self-test) run_self_test ;;
  *)
    printf 'usage: %s [--check|--native-stage|--physical-machine-stage|--self-test]\n' "$0" >&2
    exit 2
    ;;
esac
