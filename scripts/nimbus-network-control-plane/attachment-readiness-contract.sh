# shellcheck shell=bash

verify_nnc53_attachment_readiness() {
  if [ "${NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD:-}" = "1" ] &&
    [ -z "${NIMBUS_NETWORK_VERIFY_TEST_ATTACHMENT_READINESS_MUTATION:-}" ]; then
    pass "NNCV019" "complete-attachment-readiness"
    return
  fi
  if [ ! -f "${NNC53_ATTACHMENT_READINESS_HELPER}" ]; then
    fail "NNCV019" "complete-attachment-readiness" \
      "missing ${NNC53_ATTACHMENT_READINESS_HELPER}"
    return
  fi
  error="$(node "${NNC53_ATTACHMENT_READINESS_HELPER}" 2>&1)"
  status=$?
  if [ "${status}" -eq 0 ]; then
    pass "NNCV019" "complete-attachment-readiness"
  else
    fail "NNCV019" "complete-attachment-readiness" "${error}"
  fi
}

run_nnc53_attachment_readiness_self_tests() {
  script="$1"
  temporary="$2"
  readiness_self_fail=0
  for mutation in \
    missing-common-module \
    missing-container-consumer \
    missing-krun-consumer \
    missing-pin-inspection \
    missing-active-reconciliation \
    readiness-effect-capability \
    missing-machine-current-inspection \
    machine-inspection-replays-expose \
    machine-inspection-uses-invented-endpoint \
    machine-inspection-multiplies-deadline \
    machine-native-route-leaks-authority \
    missing-explicit-machine-publication-mode \
    machine-mode-infers-from-forwarder-option \
    missing-machine-observation-type \
    missing-machine-durable-receipt \
    missing-machine-registry-composition \
    missing-machine-consumer \
    missing-machine-mode-completion \
    forgeable-machine-publication-proof \
    machine-readiness-effect-capability; do
    output="${temporary}/attachment-readiness-${mutation}.out"
    if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
      NIMBUS_NETWORK_VERIFY_TEST_ATTACHMENT_READINESS_MUTATION="${mutation}" \
      "${script}" >"${output}" 2>&1; then
      printf 'SELFTEST FAIL attachment-readiness mutation %s unexpectedly exited zero\n' "${mutation}"
      readiness_self_fail=$((readiness_self_fail + 1))
    elif ! grep -q '^FAIL NNCV019 complete-attachment-readiness' "${output}" ||
      grep -q '^PASS NNCV019 complete-attachment-readiness' "${output}" ||
      [ "$(grep -c '^FAIL ' "${output}")" -ne 1 ]; then
      printf 'SELFTEST FAIL attachment-readiness mutation %s did not fail exclusively as NNCV019\n' "${mutation}"
      readiness_self_fail=$((readiness_self_fail + 1))
    else
      printf 'SELFTEST PASS attachment-readiness mutation %s fails closed as NNCV019\n' "${mutation}"
    fi
  done
  return "${readiness_self_fail}"
}
