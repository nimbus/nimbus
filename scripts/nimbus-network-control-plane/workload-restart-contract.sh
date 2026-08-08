#!/usr/bin/env bash
# Static NNC6.4a contract for one fenced, compute-owned restart lifecycle.

set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="${NIMBUS_NETWORK_NNC64A_ROOT:-$(cd "${SCRIPT_DIR}/../.." && pwd)}"
SOURCE_CONTRACT="${REPO_ROOT}/scripts/verify-nimbus-network-source-contract.mjs"

run_contract() {
  cd "${REPO_ROOT}" || return 1
  node "${SOURCE_CONTRACT}" workload-restart-contract
}

run_self_test() {
  cd "${REPO_ROOT}" || return 1
  failures=0
  passed=0

  if ! NIMBUS_NETWORK_VERIFY_RESTART_FIXTURE=1 \
    node "${SOURCE_CONTRACT}" workload-restart-contract; then
    printf 'SELFTEST FAIL NNCV034 green fixture did not pass\n'
    failures=$((failures + 1))
  fi

  cases=(
    "missing-saga-id|restart-contract/admission-identity: restart admission does not bind every identity and fence"
    "missing-source|restart-contract/admission-identity: restart admission does not bind every identity and fence"
    "missing-generation|restart-contract/admission-identity: restart admission does not bind every identity and fence"
    "missing-desired-digest|restart-contract/admission-identity: restart admission does not bind every identity and fence"
    "missing-revision|restart-contract/admission-identity: restart admission does not bind every identity and fence"
    "missing-trigger|restart-contract/admission-identity: restart admission does not bind every identity and fence"
    "missing-inspection-version|restart-contract/admission-identity: restart admission does not bind every identity and fence"
    "missing-provider-selection|restart-contract/admission-identity: restart admission does not bind every identity and fence"
    "missing-restart-epoch|restart-contract/admission-identity: restart admission does not bind every identity and fence"
    "missing-policy-count|restart-contract/admission-identity: restart admission does not bind every identity and fence"
    "missing-request-id|restart-contract/admission-identity: restart admission does not bind every identity and fence"
    "missing-attempt-id|restart-contract/admission-identity: restart admission does not bind every identity and fence"
    "crossed-attempt-id|restart-contract/admission-identity: restart admission does not bind every identity and fence"
    "synthetic-generation|restart-contract/nested-state: same-generation restart state or attempt identity is incomplete"
    "unknown-variant|restart-contract/vocabulary: portable restart vocabulary is missing or open"
    "forgeable-constructor|restart-contract/command: confirmed restart commands are forgeable or incompletely fenced"
    "bypass-admission-cas|restart-contract/reducer: compute is not the sole CAS restart admission authority"
    "direct-ambiguity-retry|restart-contract/ambiguity: ambiguous restart effects do not inspect before exact-absence retry"
    "reset-count|restart-contract/schedule: durable count, deadline, or deterministic-clock behavior is incomplete"
    "reset-deadline|restart-contract/schedule: durable count, deadline, or deterministic-clock behavior is incomplete"
    "withdrawal-loses|restart-contract/withdrawal: withdrawal or successor does not veto restart effects"
    "activate-before-readiness|restart-contract/readiness: activation or callback fencing can bypass attachment and PEP readiness"
    "old-attempt-callback|restart-contract/readiness: activation or callback fencing can bypass attachment and PEP readiness"
    "god-provider|restart-contract/capabilities: small Container and Krun restart substitutions are incomplete"
    "network-effect|restart-contract/network: nimbus-network gained restart effects or a god provider"
    "local-stop-start|restart-contract/service: service or SDK restart lacks fenced idempotent submission"
    "missing-api-idempotency|restart-contract/service: service or SDK restart lacks fenced idempotent submission"
    "node-restart|restart-contract/node: tenant workload node providers do not enforce Restart=No"
    "machine-fence-discard|restart-contract/machine: forwarded restart command drops a saga or inspection fence"
    "backend-local-scheduler|restart-contract/scheduler: provider-local restart scheduling or obsolete deadline state remains"
    "missing-behavior-proof|restart-contract/behavior: required restart behavior and recovery proofs are incomplete"
    "missing-ledger-token|restart-contract/ledger: plan and proof do not retain the NNC6.4a acceptance and review tokens"
    "unexpected-path|restart-contract/paths: NNC6.4a changed a path outside the frozen allowlist"
    "missing-restart-codec-field|restart-contract/vocabulary: portable restart vocabulary is missing or open"
    "accept-unknown-restart-codec-field|restart-contract/vocabulary: portable restart vocabulary is missing or open"
    "restart-transition-id-omits-state|restart-contract/nested-state: same-generation restart state or attempt identity is incomplete"
    "restart-phase-not-recoverable|restart-contract/nested-state: same-generation restart state or attempt identity is incomplete"
    "explicit-consumes-automatic-count|restart-contract/schedule: durable count, deadline, or deterministic-clock behavior is incomplete"
    "r1-scope-broadening|restart-contract/paths: NNC6.4a changed a path outside the frozen allowlist"
    "separate-explicit-reducer|restart-contract/reducer: compute is not the sole CAS restart admission authority"
    "stale-admission-revision|restart-contract/reducer: compute is not the sole CAS restart admission authority"
    "double-admission-winner|restart-contract/reducer: compute is not the sole CAS restart admission authority"
    "withdrawal-race-after-read|restart-contract/reducer: compute is not the sole CAS restart admission authority"
    "missing-command-transition-id|restart-contract/command: confirmed restart commands are forgeable or incompletely fenced"
    "missing-command-desired-digest|restart-contract/command: confirmed restart commands are forgeable or incompletely fenced"
    "missing-command-request-id|restart-contract/command: confirmed restart commands are forgeable or incompletely fenced"
    "missing-command-source-execution|restart-contract/command: confirmed restart commands are forgeable or incompletely fenced"
    "missing-command-target-execution|restart-contract/command: confirmed restart commands are forgeable or incompletely fenced"
    "crossed-command-result|restart-contract/command: confirmed restart commands are forgeable or incompletely fenced"
    "execute-on-confirmed-replay|restart-contract/command: confirmed restart commands are forgeable or incompletely fenced"
    "ambiguity-infers-absence|restart-contract/ambiguity: ambiguous restart effects do not inspect before exact-absence retry"
    "absence-retry-changes-attempt|restart-contract/ambiguity: ambiguous restart effects do not inspect before exact-absence retry"
    "absence-retry-reuses-dispatch-epoch|restart-contract/ambiguity: ambiguous restart effects do not inspect before exact-absence retry"
    "absence-retry-skips-dispatch-epoch|restart-contract/ambiguity: ambiguous restart effects do not inspect before exact-absence retry"
    "definite-failure-continues|restart-contract/ambiguity: ambiguous restart effects do not inspect before exact-absence retry"
    "quiesce-before-publication-withdrawal|restart-contract/readiness: activation or callback fencing can bypass attachment and PEP readiness"
    "restart-detach-releases-authority|restart-contract/readiness: activation or callback fencing can bypass attachment and PEP readiness"
    "attachment-drops-attempt-fence|restart-contract/readiness: activation or callback fencing can bypass attachment and PEP readiness"
    "pep-drops-attempt-fence|restart-contract/readiness: activation or callback fencing can bypass attachment and PEP readiness"
    "publish-before-new-attempt-ready|restart-contract/readiness: activation or callback fencing can bypass attachment and PEP readiness"
    "missing-container-restart-adapter|restart-contract/capabilities: small Container and Krun restart substitutions are incomplete"
    "missing-krun-restart-adapter|restart-contract/capabilities: small Container and Krun restart substitutions are incomplete"
    "restart-registry-first-available-fallback|restart-contract/capabilities: small Container and Krun restart substitutions are incomplete"
    "duplicate-restart-capability-registration|restart-contract/capabilities: small Container and Krun restart substitutions are incomplete"
    "unbounded-watch-page|restart-contract/watch: automatic restart is not a bounded compute-owned durable watch"
    "unbounded-watch-sweep|restart-contract/watch: automatic restart is not a bounded compute-owned durable watch"
    "watch-busy-spin|restart-contract/watch: automatic restart is not a bounded compute-owned durable watch"
    "watch-uses-system-clock|restart-contract/watch: automatic restart is not a bounded compute-owned durable watch"
    "watch-effects-from-read-only-hint|restart-contract/watch: automatic restart is not a bounded compute-owned durable watch"
    "get-starts-restart-watch|restart-contract/watch: automatic restart is not a bounded compute-owned durable watch"
    "watch-cancellation-drops-durable-work|restart-contract/watch: automatic restart is not a bounded compute-owned durable watch"
  )

  for entry in "${cases[@]}"; do
    mutation="${entry%%|*}"
    expected="${entry#*|}"
    output="$({
      NIMBUS_NETWORK_VERIFY_RESTART_FIXTURE=1 \
        NIMBUS_NETWORK_VERIFY_RESTART_MUTATION="${mutation}" \
        node "${SOURCE_CONTRACT}" workload-restart-contract
    } 2>&1)"
    status=$?
    diagnostic_count="$(printf '%s\n' "${output}" | rg -c '^restart-contract/' || true)"
    if [ "${status}" -eq 0 ]; then
      printf 'SELFTEST FAIL NNCV034 mutation %s unexpectedly passed\n' "${mutation}"
      failures=$((failures + 1))
    elif [ "${diagnostic_count}" -ne 1 ] || ! printf '%s\n' "${output}" | rg -q -F -x "${expected}"; then
      printf 'SELFTEST FAIL NNCV034 mutation %s did not fail with its sole named diagnostic\n' "${mutation}"
      printf '%s\n' "${output}"
      failures=$((failures + 1))
    else
      printf 'SELFTEST PASS NNCV034 mutation %s fails closed\n' "${mutation}"
      passed=$((passed + 1))
    fi
  done

  if [ "${failures}" -ne 0 ]; then
    printf 'NNC6.4a restart contract self-test: %d passed, %d failed\n' \
      "${passed}" "${failures}"
    return 1
  fi
  if [ "${passed}" -ne 71 ]; then
    printf 'NNC6.4a restart contract self-test: expected 71 mutations, observed %d\n' \
      "${passed}"
    return 1
  fi
  printf 'NNC6.4a restart contract self-test: 71 passed, 0 failed\n'
}

case "${1:-}" in
  '' | --check) run_contract ;;
  --self-test) run_self_test ;;
  *)
    printf 'usage: %s [--check|--self-test]\n' "$0" >&2
    exit 2
    ;;
esac
