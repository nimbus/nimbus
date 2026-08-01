# shellcheck shell=bash

verify_nnc61_compute_network_manager_injection() {
  if [ ! -f "${SOURCE_CONTRACT_HELPER}" ]; then
    fail "NNCV025" "compute-network-manager-injection" "missing ${SOURCE_CONTRACT_HELPER}"
    return
  fi
  error="$(node "${SOURCE_CONTRACT_HELPER}" compute-network-manager-injection 2>&1)"
  nnc61_status=$?
  if [ "${nnc61_status}" -eq 0 ]; then
    pass "NNCV025" "compute-network-manager-injection"
  else
    fail "NNCV025" "compute-network-manager-injection" "${error}"
  fi
}

run_nnc61_compute_network_manager_self_tests() {
  script="$1"
  temporary="$2"
  nnc61_fail=0
  nnc61_mutations=(
    missing-compute-dependency
    missing-config-manager
    missing-state-manager
    missing-compute-accessor
    missing-compute-profile-fence
    copied-capability-registry
    hidden-prepared-manager
    authority-only-start
    authority-only-serve
    manager-less-router
    missing-router-build-handoff
    protocol-service-bypass
    protocol-machine-bypass
    parallel-compute-manager
    parallel-server-manager
  )

  for mutation in "${nnc61_mutations[@]}"; do
    output="${temporary}/compute-network-manager-${mutation}.out"
    if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
      NIMBUS_NETWORK_VERIFY_TEST_COMPUTE_MANAGER_MUTATION="${mutation}" \
      "${script}" >"${output}" 2>&1; then
      printf 'SELFTEST FAIL compute-manager mutation %s unexpectedly exited zero\n' "${mutation}"
      nnc61_fail=$((nnc61_fail + 1))
    elif ! grep -q '^FAIL NNCV025 compute-network-manager-injection' "${output}" ||
      grep -q '^PASS NNCV025 compute-network-manager-injection' "${output}" ||
      [ "$(grep -c '^FAIL ' "${output}")" -ne 1 ]; then
      printf 'SELFTEST FAIL compute-manager mutation %s did not fail exclusively as NNCV025\n' "${mutation}"
      nnc61_fail=$((nnc61_fail + 1))
    else
      printf 'SELFTEST PASS compute-manager mutation %s fails closed as NNCV025\n' "${mutation}"
    fi
  done

  return "${nnc61_fail}"
}
