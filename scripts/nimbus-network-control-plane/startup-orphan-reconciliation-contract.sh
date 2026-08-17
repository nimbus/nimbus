# shellcheck shell=bash

verify_nnc52d_startup_orphan_reconciliation() {
  if [ "${NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD:-}" = "1" ] &&
    [ -z "${NIMBUS_NETWORK_VERIFY_TEST_STARTUP_ORPHAN_MUTATION:-}" ]; then
    pass "NNCV018" "startup-orphan-reconciliation"
    return
  fi
  if [ ! -f "${NNC52D_STARTUP_ORPHAN_HELPER}" ]; then
    fail "NNCV018" "startup-orphan-reconciliation" \
      "missing ${NNC52D_STARTUP_ORPHAN_HELPER}"
    return
  fi
  error="$(node "${NNC52D_STARTUP_ORPHAN_HELPER}" 2>&1)"
  status=$?
  if [ "${status}" -eq 0 ]; then
    pass "NNCV018" "startup-orphan-reconciliation"
  else
    fail "NNCV018" "startup-orphan-reconciliation" "${error}"
  fi
}

run_nnc52d_startup_orphan_self_tests() {
  script="$1"
  temporary="$2"
  startup_self_fail=0
  for mutation in \
    legacy-live-set \
    missing-container-injection \
    missing-krun-injection \
    cleanup-capability \
    missing-exact-quarantine \
    cleanup-before-quarantine \
    missing-cleanup-subject \
    missing-container-context \
    missing-krun-context \
    generic-effect-capability \
    missing-deleting-resume \
    missing-terminal-resume \
    absent-only-effectful-artifacts \
    terminal-after-effectful \
    retire-before-publication \
    missing-publication-cut-proof \
    no-op-crash-child; do
    output="${temporary}/startup-orphan-${mutation}.out"
    if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
      NIMBUS_NETWORK_VERIFY_TEST_STARTUP_ORPHAN_MUTATION="${mutation}" \
      "${script}" >"${output}" 2>&1; then
      printf 'SELFTEST FAIL startup-orphan mutation %s unexpectedly exited zero\n' "${mutation}"
      startup_self_fail=$((startup_self_fail + 1))
    elif ! grep -q '^FAIL NNCV018 startup-orphan-reconciliation' "${output}" ||
      grep -q '^PASS NNCV018 startup-orphan-reconciliation' "${output}" ||
      [ "$(grep -c '^FAIL ' "${output}")" -ne 1 ]; then
      printf 'SELFTEST FAIL startup-orphan mutation %s did not fail exclusively as NNCV018\n' "${mutation}"
      startup_self_fail=$((startup_self_fail + 1))
    else
      printf 'SELFTEST PASS startup-orphan mutation %s fails closed as NNCV018\n' "${mutation}"
    fi
  done
  return "${startup_self_fail}"
}
