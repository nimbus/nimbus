# shellcheck shell=bash

verify_nnc56_side_effect_free_sandbox_inspection() {
  if [ ! -f "${SOURCE_CONTRACT_HELPER}" ]; then
    fail "NNCV024" "side-effect-free-sandbox-inspection" "missing ${SOURCE_CONTRACT_HELPER}"
    return
  fi
  error="$(node "${SOURCE_CONTRACT_HELPER}" side-effect-free-sandbox-inspection 2>&1)"
  nnc56_status=$?
  if [ "${nnc56_status}" -eq 0 ]; then
    pass "NNCV024" "side-effect-free-sandbox-inspection"
  else
    fail "NNCV024" "side-effect-free-sandbox-inspection" "${error}"
  fi
}

run_nnc56_side_effect_free_inspection_self_tests() {
  script="$1"
  temporary="$2"
  nnc56_fail=0
  nnc56_mutations=(
    inspection-restart
    inspection-launch
    inspection-reset
    inspection-release
    inspection-cleanup
    inspection-finalize
    inspection-pep-start
    inspection-write
    inspection-effect-barrier
    creating-lock
    third-inspect-owner
    handle-only-trait
    handle-only-machine-dto
    missing-krun-classifier
    discarded-service-assessment
    cleanup-retained-eviction
    fabricated-forwarded-candidate
    implicit-launch-caller
    nimbus-network-effect
  )

  for mutation in "${nnc56_mutations[@]}"; do
    output="${temporary}/side-effect-free-inspection-${mutation}.out"
    if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
      NIMBUS_NETWORK_VERIFY_TEST_INSPECTION_MUTATION="${mutation}" \
      "${script}" >"${output}" 2>&1; then
      printf 'SELFTEST FAIL inspection mutation %s unexpectedly exited zero\n' "${mutation}"
      nnc56_fail=$((nnc56_fail + 1))
    elif ! grep -q '^FAIL NNCV024 side-effect-free-sandbox-inspection' "${output}" ||
      grep -q '^PASS NNCV024 side-effect-free-sandbox-inspection' "${output}" ||
      [ "$(grep -c '^FAIL ' "${output}")" -ne 1 ]; then
      printf 'SELFTEST FAIL inspection mutation %s did not fail exclusively as NNCV024\n' "${mutation}"
      nnc56_fail=$((nnc56_fail + 1))
    else
      printf 'SELFTEST PASS inspection mutation %s fails closed as NNCV024\n' "${mutation}"
    fi
  done

  return "${nnc56_fail}"
}
