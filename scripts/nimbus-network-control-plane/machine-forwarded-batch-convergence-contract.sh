# shellcheck shell=bash

verify_nnc54a_machine_forwarded_batch_convergence() {
  if [ ! -f "${NNC54A_MACHINE_BATCH_HELPER}" ]; then
    fail "NNCV021" "machine-forwarded-batch-convergence" \
      "missing ${NNC54A_MACHINE_BATCH_HELPER}"
    return
  fi
  error="$(node "${NNC54A_MACHINE_BATCH_HELPER}" 2>&1)"
  status=$?
  if [ "${status}" -eq 0 ]; then
    pass "NNCV021" "machine-forwarded-batch-convergence"
  else
    fail "NNCV021" "machine-forwarded-batch-convergence" "${error}"
  fi
}

run_nnc54a_machine_forwarded_batch_self_tests() {
  script="$1"
  temporary="$2"
  machine_batch_fail=0
  for mutation in \
    legacy-authority \
    restored-process-local-authority \
    missing-record-field \
    collapsed-ambiguity \
    effect-before-journal \
    missing-post-effect-inspection \
    weakened-store \
    unbounded-lock \
    broadened-provider \
    reordered-cuts \
    removed-contention; do
    output="${temporary}/machine-batch-${mutation}.out"
    if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
      NIMBUS_NETWORK_VERIFY_TEST_MACHINE_BATCH_CONVERGENCE_MUTATION="${mutation}" \
      "${script}" >"${output}" 2>&1; then
      printf 'SELFTEST FAIL machine-batch mutation %s unexpectedly exited zero\n' "${mutation}"
      machine_batch_fail=$((machine_batch_fail + 1))
    elif ! grep -q '^FAIL NNCV021 machine-forwarded-batch-convergence' "${output}" ||
      grep -q '^PASS NNCV021 machine-forwarded-batch-convergence' "${output}" ||
      [ "$(grep -c '^FAIL ' "${output}")" -ne 1 ]; then
      printf 'SELFTEST FAIL machine-batch mutation %s did not fail exclusively as NNCV021\n' "${mutation}"
      machine_batch_fail=$((machine_batch_fail + 1))
    else
      printf 'SELFTEST PASS machine-batch mutation %s fails closed as NNCV021\n' "${mutation}"
    fi
  done
  return "${machine_batch_fail}"
}
