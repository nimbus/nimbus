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
    'missing-compose-store|teardown-contract/compose: Compose down bypasses the canonical Engine-backed compute saga'
    'missing-compose-submit|teardown-contract/compose: Compose down bypasses the canonical Engine-backed compute saga'
    'missing-compose-wait|teardown-contract/compose: Compose down bypasses the canonical Engine-backed compute saga'
    'cli-local-saga-store|teardown-contract/compose: Compose down bypasses the canonical Engine-backed compute saga'
    'missing-machine-envelope|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'missing-machine-phase|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'missing-machine-fence|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'parent-release-before-absence|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'missing-machine-active-fence|teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences'
    'missing-forwarded-registry-registrations|teardown-contract/forwarded-machine-registry: forwarded teardown does not expose the exact five-phase compute registry substitution'
    'missing-forwarded-registry-capability|teardown-contract/forwarded-machine-registry: forwarded teardown does not expose the exact five-phase compute registry substitution'
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

  if [ "${failures}" -ne 0 ]; then
    printf 'NNC6.5 teardown contract self-test: %d passed, %d failed\n' \
      "${passed}" "${failures}"
    return 1
  fi
  if [ "${passed}" -ne 67 ]; then
    printf 'NNC6.5 teardown contract self-test: expected 67 mutations, observed %d\n' \
      "${passed}"
    return 1
  fi
  printf 'NNC6.5 teardown contract self-test: 67 passed, 0 failed\n'
}

case "${1:-}" in
  '' | --check) run_contract ;;
  --self-test) run_self_test ;;
  *)
    printf 'usage: %s [--check|--self-test]\n' "$0" >&2
    exit 2
    ;;
esac
