# shellcheck shell=bash

verify_nnc52a_attachment_effect_ordering() {
  if [ ! -f "${NNC52A_ATTACHMENT_ORDERING_HELPER}" ]; then
    fail "NNCV017" "attachment-association-effect-ordering" \
      "missing ${NNC52A_ATTACHMENT_ORDERING_HELPER}"
    return
  fi
  error="$(node "${NNC52A_ATTACHMENT_ORDERING_HELPER}" 2>&1)"
  status=$?
  if [ "${status}" -eq 0 ]; then
    pass "NNCV017" "attachment-association-effect-ordering"
  else
    fail "NNCV017" "attachment-association-effect-ordering" "${error}"
  fi
}

run_nnc52a_attachment_ordering_self_tests() {
  script="$1"
  temporary="$2"
  attachment_self_fail=0
  for mutation in \
    missing-association \
    setup-fence \
    teardown-fence \
    machine-bypass \
    legacy-purge; do
    output="${temporary}/attachment-ordering-${mutation}.out"
    if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
      NIMBUS_NETWORK_VERIFY_TEST_ATTACHMENT_ORDERING_MUTATION="${mutation}" \
      "${script}" >"${output}" 2>&1; then
      printf 'SELFTEST FAIL attachment-ordering mutation %s unexpectedly exited zero\n' "${mutation}"
      attachment_self_fail=$((attachment_self_fail + 1))
    elif ! grep -q '^FAIL NNCV017 attachment-association-effect-ordering' "${output}" ||
      grep -q '^PASS NNCV017 attachment-association-effect-ordering' "${output}" ||
      [ "$(grep -c '^FAIL ' "${output}")" -ne 1 ]; then
      printf 'SELFTEST FAIL attachment-ordering mutation %s did not fail exclusively as NNCV017\n' "${mutation}"
      attachment_self_fail=$((attachment_self_fail + 1))
    else
      printf 'SELFTEST PASS attachment-ordering mutation %s fails closed as NNCV017\n' "${mutation}"
    fi
  done
  return "${attachment_self_fail}"
}
