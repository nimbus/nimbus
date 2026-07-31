# shellcheck shell=bash

verify_nnc54_attachment_crash_convergence() {
  if [ ! -f "${NNC54_ATTACHMENT_CRASH_HELPER}" ]; then
    fail "NNCV020" "attachment-crash-convergence" \
      "missing ${NNC54_ATTACHMENT_CRASH_HELPER}"
    return
  fi
  error="$(node "${NNC54_ATTACHMENT_CRASH_HELPER}" 2>&1)"
  status=$?
  if [ "${status}" -eq 0 ]; then
    pass "NNCV020" "attachment-crash-convergence"
  else
    fail "NNCV020" "attachment-crash-convergence" "${error}"
  fi
}

run_nnc54_attachment_crash_self_tests() {
  script="$1"
  temporary="$2"
  attachment_crash_fail=0
  for mutation in \
    missing-create-cut \
    missing-delete-cut \
    create-phase-swap \
    delete-phase-swap \
    publishing-never-bound \
    detached-namespace-unknown \
    duplicate-delete-unproven \
    unbounded-child \
    missing-pre-crash-witness; do
    output="${temporary}/attachment-crash-${mutation}.out"
    if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
      NIMBUS_NETWORK_VERIFY_TEST_ATTACHMENT_CRASH_MUTATION="${mutation}" \
      "${script}" >"${output}" 2>&1; then
      printf 'SELFTEST FAIL attachment-crash mutation %s unexpectedly exited zero\n' "${mutation}"
      attachment_crash_fail=$((attachment_crash_fail + 1))
    elif ! grep -q '^FAIL NNCV020 attachment-crash-convergence' "${output}" ||
      grep -q '^PASS NNCV020 attachment-crash-convergence' "${output}" ||
      [ "$(grep -c '^FAIL ' "${output}")" -ne 1 ]; then
      printf 'SELFTEST FAIL attachment-crash mutation %s did not fail exclusively as NNCV020\n' "${mutation}"
      attachment_crash_fail=$((attachment_crash_fail + 1))
    else
      printf 'SELFTEST PASS attachment-crash mutation %s fails closed as NNCV020\n' "${mutation}"
    fi
  done
  return "${attachment_crash_fail}"
}
