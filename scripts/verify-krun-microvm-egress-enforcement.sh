#!/usr/bin/env bash
# Aggregate completion-gate verifier for the Krun MicroVM Egress Enforcement
# plan (`docs/private/plans/archive/krun-microvm-egress-enforcement-plan.md`).
#
# Exits 0 iff every condition in the plan's Completion Gate is satisfied.
# Ships in KME0 so /goal is verifiable from day one; KME1-KME5 progressively
# flip conditions from FAIL to PASS, KME6 closes the plan and archives it.
#
# The plan doc + README + proof live under untracked docs/private/; this script
# and the AGENTS.md routing pointer are the only tracked artifacts.
#
# Run from the repo root.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}" || exit 2

PLAN_ACTIVE="docs/private/plans/krun-microvm-egress-enforcement-plan.md"
PLAN_ARCHIVED="docs/private/plans/archive/krun-microvm-egress-enforcement-plan.md"
AGENTS_MD="AGENTS.md"
PLANS_README="docs/private/plans/README.md"
PROOF_DIR="docs/private/plans/proof/krun-microvm-egress"
PROOF_KME0="${PROOF_DIR}/kme0-baseline.md"
PROOF_KME1="${PROOF_DIR}/kme1-spike.md"
PROOF_KME5="${PROOF_DIR}/kme5-egress-proof.md"

KRUN_START="crates/nimbus-sandbox/src/backends/krun/vm/start.rs"
KRUN_DIR="crates/nimbus-sandbox/src/backends/krun"
TENANT_ISOLATION="docs/private/tenant-isolation.md"
MICROVM_BASELINE="docs/private/architecture/sandbox/microvm-service-baseline.md"
EGRESS_PROOF_TEST="crates/nimbus-sandbox/tests/krun_linux_egress.rs"

PASS=0
FAIL=0
FAIL_DETAIL=()

# -------- helpers ----------------------------------------------------------

pass() {
  PASS=$((PASS + 1))
  printf '  \033[32mPASS\033[0m  %s\n' "$1"
}

fail() {
  FAIL=$((FAIL + 1))
  printf '  \033[31mFAIL\033[0m  %s\n' "$1"
  if [ $# -ge 2 ]; then
    printf '        %s\n' "$2"
    FAIL_DETAIL+=("$1 — $2")
  else
    FAIL_DETAIL+=("$1")
  fi
}

step() {
  printf '\n\033[1m[%s]\033[0m %s\n' "$1" "$2"
}

plan_file() {
  if [ -f "${PLAN_ACTIVE}" ]; then
    printf '%s\n' "${PLAN_ACTIVE}"
  elif [ -f "${PLAN_ARCHIVED}" ]; then
    printf '%s\n' "${PLAN_ARCHIVED}"
  else
    printf ''
  fi
}

# -------- conditions -------------------------------------------------------

step 1 "Plan doc exists with the KME title"
PLAN="$(plan_file)"
if [ -n "${PLAN}" ] && grep -q "Krun MicroVM Egress Enforcement Plan (KME)" "${PLAN}"; then
  pass "plan doc present (${PLAN})"
else
  fail "plan doc missing or untitled" "expected ${PLAN_ACTIVE} or archived copy"
fi

step 2 "AGENTS.md routing pointer resolves"
if grep -q "krun-microvm-egress-enforcement-plan.md" "${AGENTS_MD}"; then
  pass "routing pointer present in ${AGENTS_MD}"
else
  fail "no routing pointer in ${AGENTS_MD}"
fi

step 3 "plans/README.md does not route completed KME as active work"
if [ -f "${PLANS_README}" ] && ! grep -q "krun-microvm-egress-enforcement-plan.md" "${PLANS_README}"; then
  pass "completed KME is absent from active README routing"
else
  fail "completed KME should not be routed in ${PLANS_README}"
fi

step 4 "KME0 baseline proof exists"
if [ -f "${PROOF_KME0}" ]; then
  pass "baseline proof present (${PROOF_KME0})"
else
  fail "baseline proof missing" "expected ${PROOF_KME0}"
fi

step 5 "KME1 TSI+netns spike proof exists"
if [ -f "${PROOF_KME1}" ]; then
  pass "TSI+netns spike proof present (${PROOF_KME1})"
else
  fail "KME1 spike proof missing" "expected ${PROOF_KME1}"
fi

step 6 "krun bundle wires a network namespace (TSI+netns design)"
if [ -d "${KRUN_DIR}" ] \
  && grep -rqE 'LinuxNamespaceType::Network|"type": ?"network"|create_persistent_network_namespace|setup_container_network' "${KRUN_DIR}" \
  && ! grep -rq 'bundle_config_omits_network_namespace' "${KRUN_DIR}"; then
  pass "krun bundle materializes a network namespace via the shared netns chain"
else
  fail "krun bundle does not yet wire a network namespace" "KME2 — reuse oci/network.rs netns chain; replace the omit-netns assertion"
fi

step 7 "krun backend forwards through the NEG PEP (EgressProxy, container shape)"
if [ -d "${KRUN_DIR}" ] && grep -rqE 'nimbus_proxy|EgressProxy|HTTP_PROXY' "${KRUN_DIR}"; then
  pass "egress PEP forwarding binding present in krun backend"
else
  fail "krun backend does not forward through the NEG PEP" "KME3 — compile spec.egress -> EgressProxy + HTTP_PROXY env (not the in-process EgressGateway trait)"
fi

step 8 "execute fail-close replaced by a real readiness gate (two-sided)"
KRUN_TESTS="${KRUN_DIR}/vm/tests.rs"
if [ ! -f "${KRUN_START}" ]; then
  fail "krun start.rs missing" "${KRUN_START}"
elif grep -q "packet-level egress enforcement path for libkrun TSI" "${KRUN_START}"; then
  fail "krun execute-mode still unconditionally fail-closed" "KME4 must replace the guard with a readiness gate"
elif ! grep -rqiE 'readiness|enforcement_ready|egress_ready' "${KRUN_DIR}"; then
  fail "fail-close removed but no readiness gate present (fail-open risk)" "KME4 — gate execute on netns+PEP+policy-generation ready"
elif [ ! -f "${KRUN_TESTS}" ] || ! grep -qiE 'not_ready|fail.?closed|denied|deny' "${KRUN_TESTS}"; then
  fail "readiness gate has no fail-closed-when-not-ready negative test" "KME4 — assert execute denies when the gate is unsatisfied"
else
  pass "readiness gate present with a fail-closed not-ready negative test"
fi

step 9 "krun_linux_egress proofs are non-vacuous (test fns + positive controls)"
# Anchor on the specific proof functions AND their positive-control / deny
# assertion strings, not just file existence: a gutted proof (deleted body,
# removed positive control, or a probe that only asserts `=denied` and would
# false-green on an offline guest) must fail here. (audit M15/M21.)
PROOF_OK=1
PROOF_MISS=""
if [ ! -f "${EGRESS_PROOF_TEST}" ]; then
  PROOF_OK=0
  PROOF_MISS="${EGRESS_PROOF_TEST} missing"
else
  for anchor in \
    'fn krun_guest_cannot_reach_a_sibling_tenants_pep' \
    'sibling_pep_reach=denied' \
    'own_pep=allowed' \
    'fn krun_execute_mode_denies_all_known_bypass_vectors' \
    'pep_allow=allowed' \
    'fn krun_and_container_pep_enforce_identical_allow_deny' \
    'allowed_internal=allowed'; do
    if ! grep -qF "${anchor}" "${EGRESS_PROOF_TEST}"; then
      PROOF_OK=0
      PROOF_MISS="missing anchor: ${anchor}"
    fi
  done
fi
if [ "${PROOF_OK}" -eq 1 ] && [ -f "${PROOF_KME5}" ]; then
  pass "krun_linux_egress proofs present with positive controls + sibling-PEP isolation"
else
  fail "krun egress proof incomplete or gutted" "${PROOF_MISS:-expected ${PROOF_KME5}}"
fi

step 10 "docs no longer claim the microVM egress gate"
GATE_HITS=0
for d in "${TENANT_ISOLATION}" "${MICROVM_BASELINE}"; do
  if [ -f "${d}" ] && grep -qi "packet-level libkrun TSI" "${d}"; then
    GATE_HITS=$((GATE_HITS + 1))
  fi
done
if [ "${GATE_HITS}" -eq 0 ]; then
  pass "fail-closed caveat lifted in tenant-isolation + microvm-service-baseline"
else
  fail "docs still cite the packet-level TSI PEP gate" "KME6 must lift the caveat"
fi

# -------- summary ----------------------------------------------------------

printf '\n\033[1m================ KME verifier ================\033[0m\n'
printf '  %d passed, %d failed\n' "${PASS}" "${FAIL}"
if [ "${FAIL}" -gt 0 ]; then
  printf '\n  Outstanding:\n'
  for d in "${FAIL_DETAIL[@]}"; do
    printf '   - %s\n' "${d}"
  done
  exit 1
fi
exit 0
