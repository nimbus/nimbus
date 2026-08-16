#!/usr/bin/env bash
# Static NNC8.2 contract for exact provider-command live-claim authority.

set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="${NIMBUS_NETWORK_NNC82_ROOT:-$(cd "${SCRIPT_DIR}/../.." && pwd)}"
CONTRACT="${REPO_ROOT}/scripts/nimbus-network-control-plane/provider-command-current-claim-contract.mjs"

run_contract() {
  cd "${REPO_ROOT}" || return 1
  node "${CONTRACT}"
}

run_self_test() {
  cd "${REPO_ROOT}" || return 1
  failures=0
  passed=0
  temporary="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-network-nnc82-self-test.XXXXXX")" || {
    printf 'SELFTEST FAIL NNCV037 cannot create the aggregate mutation directory\n'
    return 1
  }
  trap 'rm -rf "${temporary}"' EXIT

  if run_contract >/dev/null; then
    printf 'SELFTEST PASS NNCV037 green current-claim source passes\n'
    passed=$((passed + 1))
  else
    printf 'SELFTEST FAIL NNCV037 green current-claim source failed\n'
    failures=$((failures + 1))
  fi

  cases=(
    "discard-compute-token|provider-command-current-claim/producer-token:"
    "discard-guest-token|provider-command-current-claim/producer-token:"
    "missing-sync-execution|provider-command-current-claim/producer-interval:"
    "missing-async-inspection|provider-command-current-claim/producer-interval:"
    "skip-claimed-recovery|provider-command-current-claim/decision-matrix:"
    "missing-protected-teardown|provider-command-current-claim/protected-teardown:"
    "private-protocol|provider-command-current-claim/protocol-surface:"
  )

  for entry in "${cases[@]}"; do
    mutation="${entry%%|*}"
    expected="${entry#*|}"
    output="$(NIMBUS_NETWORK_VERIFY_NNC82_MUTATION="${mutation}" node "${CONTRACT}" 2>&1)"
    status=$?
    diagnostic_count="$(printf '%s\n' "${output}" | rg -c '^provider-command-current-claim/' || true)"
    if [ "${status}" -eq 0 ]; then
      printf 'SELFTEST FAIL NNCV037 mutation %s unexpectedly passed\n' "${mutation}"
      failures=$((failures + 1))
    elif [ "${diagnostic_count}" -ne 1 ] || ! printf '%s\n' "${output}" | rg -q -F "${expected}"; then
      printf 'SELFTEST FAIL NNCV037 mutation %s lacked its sole diagnostic\n' "${mutation}"
      printf '%s\n' "${output}"
      failures=$((failures + 1))
    else
      printf 'SELFTEST PASS NNCV037 mutation %s fails closed\n' "${mutation}"
      passed=$((passed + 1))
    fi
  done

  aggregate="${REPO_ROOT}/scripts/verify-nimbus-network-control-plane.sh"
  aggregate_output="${temporary}/aggregate-mutation.out"
  if NIMBUS_NETWORK_VERIFY_NNC82_MUTATION=discard-compute-token \
    NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    "${aggregate}" >"${aggregate_output}" 2>&1; then
    printf 'SELFTEST FAIL NNCV037 aggregate mutation unexpectedly passed\n'
    failures=$((failures + 1))
  elif ! grep -q '^FAIL NNCV037 provider-command-current-claim-authority' \
    "${aggregate_output}" ||
    grep -q '^PASS NNCV037 provider-command-current-claim-authority' \
      "${aggregate_output}" ||
    [ "$(grep -c '^FAIL NNCV' "${aggregate_output}")" -ne 1 ] ||
    ! grep -q '^Targeted summary: 0 passed, 1 failed, 38 skipped$' \
      "${aggregate_output}"; then
    printf 'SELFTEST FAIL NNCV037 aggregate mutation did not fail exclusively\n'
    failures=$((failures + 1))
  else
    printf 'SELFTEST PASS NNCV037 aggregate mutation fails closed exclusively\n'
    passed=$((passed + 1))
  fi

  if [ "${failures}" -ne 0 ]; then
    printf 'NNC8.2 current-claim contract self-test: %d passed, %d failed\n' \
      "${passed}" "${failures}"
    return 1
  fi
  if [ "${passed}" -ne 9 ]; then
    printf 'NNC8.2 current-claim contract self-test: expected 9 cases, observed %d\n' \
      "${passed}"
    return 1
  fi
  printf 'NNC8.2 current-claim contract self-test: 9 passed, 0 failed\n'
}

case "${1:-}" in
  '' | --check) run_contract ;;
  --self-test) run_self_test ;;
  *)
    printf 'usage: %s [--check|--self-test]\n' "$0" >&2
    exit 2
    ;;
esac
