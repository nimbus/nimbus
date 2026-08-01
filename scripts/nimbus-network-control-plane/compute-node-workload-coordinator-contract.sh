# shellcheck shell=bash

verify_nnc61a_compute_node_workload_coordinator() {
  if [ ! -f "${SOURCE_CONTRACT_HELPER}" ]; then
    fail "NNCV026" "compute-node-workload-coordinator" "missing ${SOURCE_CONTRACT_HELPER}"
    return
  fi
  error="$(node "${SOURCE_CONTRACT_HELPER}" compute-node-workload-coordinator 2>&1)"
  nnc61a_status=$?
  if [ "${nnc61a_status}" -eq 0 ]; then
    pass "NNCV026" "compute-node-workload-coordinator"
  else
    fail "NNCV026" "compute-node-workload-coordinator" "${error}"
  fi
}

run_nnc61a_compute_node_workload_coordinator_self_tests() {
  script="$1"
  temporary="$2"
  nnc61a_fail=0
  nnc61a_mutations=(
    missing-node-capability
    missing-compute-coordinator
    missing-state-coordinator
    missing-profile-fence
    direct-cli-reconcile
    direct-guest-reconcile
    direct-guest-inspect
    runner-provider-restart
    missing-restart-fence
    duplicate-restart-accepted
    coordinator-desired-store
    coordinator-network-authority
    second-coordinator
    duplicate-saga-coordinator
    duplicate-saga-coordinator-enum
  )

  for mutation in "${nnc61a_mutations[@]}"; do
    output="${temporary}/compute-node-coordinator-${mutation}.out"
    if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
      NIMBUS_NETWORK_VERIFY_TEST_COMPUTE_COORDINATOR_MUTATION="${mutation}" \
      "${script}" >"${output}" 2>&1; then
      printf 'SELFTEST FAIL compute-node-coordinator mutation %s unexpectedly exited zero\n' "${mutation}"
      nnc61a_fail=$((nnc61a_fail + 1))
    elif ! grep -q '^FAIL NNCV026 compute-node-workload-coordinator' "${output}" ||
      grep -q '^PASS NNCV026 compute-node-workload-coordinator' "${output}" ||
      [ "$(grep -c '^FAIL ' "${output}")" -ne 1 ]; then
      printf 'SELFTEST FAIL compute-node-coordinator mutation %s did not fail exclusively as NNCV026\n' "${mutation}"
      nnc61a_fail=$((nnc61a_fail + 1))
    else
      printf 'SELFTEST PASS compute-node-coordinator mutation %s fails closed as NNCV026\n' "${mutation}"
    fi
  done

  return "${nnc61a_fail}"
}
