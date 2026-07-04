#!/usr/bin/env bash
# Verifies the Nimbus egress engine (EE) control plane.

set -u
set -o pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck disable=SC2164
cd "${ROOT}"

PLAN="docs/private/plans/nimbus-egress-engine-plan.md"
PROOF_DIR="docs/private/plans/proof/nimbus-egress-engine"
PLANS_README="docs/private/plans/README.md"
K11P_VERIFIER="scripts/verify-nimbus-proxy-pingora.sh"
EE0_PROOF="${PROOF_DIR}/ee0-scaffold.md"

passed=0
failed=0
failures=()

pass() {
  printf 'PASS %s: %s\n' "$1" "$2"
  passed=$((passed + 1))
}

fail() {
  printf 'FAIL %s: %s\n' "$1" "$2"
  failures+=("$1: $2")
  failed=$((failed + 1))
}

contains() {
  local path="$1"
  local pattern="$2"
  test -e "${path}" && grep -Eq "${pattern}" "${path}"
}

rejects() {
  local path="$1"
  local pattern="$2"
  ! grep -Eq "${pattern}" "${path}" 2>/dev/null
}

row_status() {
  local row="$1"
  awk -F'|' -v row="${row}" '
    $2 ~ (" " row " ") {
      gsub(/^[ \t]+|[ \t]+$/, "", $3);
      print $3;
      exit;
    }
  ' "${PLAN}" 2>/dev/null
}

require_file() {
  local row="$1"
  local path="$2"
  local message="$3"
  if [ -f "${path}" ]; then
    pass "${row}" "${message}"
  else
    fail "${row}" "missing ${path}: ${message}"
  fi
}

require_dir() {
  local row="$1"
  local path="$2"
  local message="$3"
  if [ -d "${path}" ]; then
    pass "${row}" "${message}"
  else
    fail "${row}" "missing ${path}: ${message}"
  fi
}

require_done() {
  local row="$1"
  local status
  status="$(row_status "${row}")"
  if [ "${status}" = "done" ]; then
    pass "${row}" "${row} ledger row is done"
  else
    fail "${row}" "${row} ledger row is ${status:-missing}, expected done"
    return 1
  fi
}

require_proposed() {
  local row="$1"
  local status
  status="$(row_status "${row}")"
  if [ "${status}" = "proposed" ]; then
    pass "${row}" "${row} is PENDING (ledger status proposed)"
  else
    fail "${row}" "${row} ledger row is ${status:-missing}, expected proposed"
    return 1
  fi
}

require_grep() {
  local row="$1"
  local pattern="$2"
  local path="$3"
  local message="$4"
  if contains "${path}" "${pattern}"; then
    pass "${row}" "${message}"
  else
    fail "${row}" "${message} (${path} lacks pattern: ${pattern})"
  fi
}

require_reject() {
  local row="$1"
  local pattern="$2"
  local path="$3"
  local message="$4"
  if rejects "${path}" "${pattern}"; then
    pass "${row}" "${message}"
  else
    fail "${row}" "${message} (${path} matches forbidden pattern: ${pattern})"
  fi
}

# 1. Plan routing
check_plan_routing() {
  require_file EE0 "${PLAN}" "EE plan exists"
  require_file EE0 "${PLANS_README}" "plans README exists"
  require_grep EE0 'nimbus-egress-engine' "${PLANS_README}" "plans README routes to EE plan"
}

# 2. Proof scaffold
check_proof_scaffold() {
  require_dir EE0 "${PROOF_DIR}" "EE proof directory exists"
  require_file EE0 "${EE0_PROOF}" "EE0 scaffold proof exists"
}

# 3. Activation gate: K11P merged to main — GIT-VERIFIED, not text-only.
# The plan gates EE0 on K11P being *merged to main* (not just present on a
# branch or claimed in a doc), so this asserts git state: the K11P squash
# commit must be reachable from the mainline AND its substrate committed there.
# The proof greps only corroborate; they no longer stand alone.
check_activation_gate() {
  local k11p_sha='5558fe9f7'
  local k11p_marker='crates/nimbus-proxy/src/pingora_app.rs'
  # Prefer the remote-tracking mainline (canonical "merged to main"); fall back
  # to local main if origin/main is unavailable (e.g. no fetch).
  local mainref='origin/main'
  git rev-parse --verify --quiet "${mainref}^{commit}" >/dev/null 2>&1 || mainref='main'

  if git merge-base --is-ancestor "${k11p_sha}" "${mainref}" 2>/dev/null; then
    pass EE0 "K11P ${k11p_sha} is merged into ${mainref} (git-verified)"
  else
    fail EE0 "K11P ${k11p_sha} is not an ancestor of ${mainref} (not merged to main)"
  fi

  if git cat-file -e "${mainref}:${k11p_marker}" 2>/dev/null; then
    pass EE0 "K11P substrate committed on ${mainref} (${k11p_marker})"
  else
    fail EE0 "K11P substrate ${k11p_marker} absent on ${mainref} (not merged)"
  fi

  require_grep EE0 '5558fe9f7' "${EE0_PROOF}" "EE0 proof corroborates K11P merge commit"
  require_grep EE0 '#94' "${EE0_PROOF}" "EE0 proof corroborates K11P PR number"
  require_file EE0 "${K11P_VERIFIER}" "K11P verifier still present (EE does not regress K11P)"
}

# 4. Ceded-boundary cross-refs present in EE0 proof
check_proof_cross_refs() {
  require_grep EE0 'PPH' "${EE0_PROOF}" "EE0 proof cross-refs PPH"
  require_grep EE0 'PDD' "${EE0_PROOF}" "EE0 proof cross-refs PDD"
  require_grep EE0 'connection-broker' "${EE0_PROOF}" "EE0 proof cross-refs connection broker"
  require_grep EE0 'tenant-admission-audit|TAA' "${EE0_PROOF}" "EE0 proof cross-refs tenant admission audit"
  require_grep EE0 'horizontal-scaling|HS' "${EE0_PROOF}" "EE0 proof cross-refs horizontal scaling"
}

# 5. Architecture decision recorded
check_architecture_decision() {
  require_grep EE0 'WorkloadPep' "${EE0_PROOF}" "EE0 proof records WorkloadPep decision"
  require_grep EE0 'EgressEngine' "${EE0_PROOF}" "EE0 proof records EgressEngine decision"
  require_grep EE0 'per-PEP' "${EE0_PROOF}" "EE0 proof records per-PEP isolation term"
}

# 6. Boundary cross-refs in plan non-goals
check_plan_non_goals() {
  require_grep EE0 'policy-hardening' "${PLAN}" "EE plan non-goals cross-ref policy hardening"
  require_grep EE0 'density' "${PLAN}" "EE plan non-goals cross-ref density"
  require_grep EE0 'connection-broker' "${PLAN}" "EE plan non-goals cross-ref connection broker"
  require_grep EE0 'tenant-admission-audit' "${PLAN}" "EE plan non-goals cross-ref tenant admission audit"
  require_grep EE0 'horizontal-scaling' "${PLAN}" "EE plan non-goals cross-ref horizontal scaling"
}

# 7. EE1: engine promotion, WorkloadPep rename, WorkloadId re-key,
#    no-cycle, and the reachability lint (the isolation compensating control).
check_ee1_engine() {
  local status
  status="$(row_status EE1)"
  if [ "${status}" = "done" ]; then
    pass EE1 "EE1 ledger row is done"
  else
    fail EE1 "EE1 ledger row is ${status:-missing}, expected done"
  fi

  require_file EE1 "crates/nimbus-proxy/src/engine.rs" "EgressEngine module exists"
  require_grep EE1 'pub struct EgressEngine' "crates/nimbus-proxy/src/engine.rs" \
    "EgressEngine defined in engine.rs"
  require_grep EE1 'WorkloadId' "crates/nimbus-proxy/src/engine.rs" \
    "engine map keyed by nimbus-core WorkloadId"
  require_grep EE1 'pub struct WorkloadId' "crates/nimbus-core/src/types.rs" \
    "opaque WorkloadId lives in nimbus-core"
  require_grep EE1 '\bWorkloadPep\b' "crates/nimbus-proxy/src/worker.rs" \
    "per-sandbox PEP renamed to WorkloadPep"

  # No-cycle gate: nimbus-proxy must never depend on nimbus-sandbox.
  require_reject EE1 'nimbus-sandbox' "crates/nimbus-proxy/Cargo.toml" \
    "nimbus-proxy has no nimbus-sandbox dependency (no crate cycle)"

  # Reachability lint, mirrored from outside the crate: within nimbus-proxy,
  # only engine.rs (definition), lib.rs (export), and tests.rs (the in-crate
  # lint itself) may name EgressEngine or WorkloadId. A plain
  # "no Map<SandboxId,...>" grep is vacuous (nimbus-proxy has no SandboxId);
  # scanning for the engine's own key/type names is the non-vacuous form.
  local reach_violations
  reach_violations="$(grep -rlE 'EgressEngine|WorkloadId' crates/nimbus-proxy/src \
    --include='*.rs' 2>/dev/null \
    | grep -vE '/(engine|lib|tests)\.rs$' || true)"
  if [ -z "${reach_violations}" ]; then
    pass EE1 "workload map unreachable from request-path modules (reachability lint)"
  else
    fail EE1 "request-path modules reference the workload map: ${reach_violations}"
  fi

  # The in-crate Rust lint must exist (it runs in plain cargo test).
  require_grep EE1 'ee1_reachability_lint_workload_map_unreachable_from_request_path' \
    "crates/nimbus-proxy/src/tests.rs" "in-crate EE1 reachability lint test exists"
}

# 8. EE2: node-global allow-ceiling combinator (pure PDP intersection).
check_ee2_ceiling() {
  require_done EE2 || true
  require_file EE2 "crates/nimbus-egress/src/layered.rs" "LayeredEgressPolicy module exists"
  require_grep EE2 'pub struct LayeredEgressPolicy' "crates/nimbus-egress/src/layered.rs" \
    "allow-ceiling combinator defined"
  require_grep EE2 'with_global_ceiling' "crates/nimbus-egress/src/layered.rs" \
    "ceiling constructor present (combinator, not a vec merge)"
  require_grep EE2 'LayeredEgressPolicy' "crates/nimbus-egress/src/lib.rs" \
    "combinator exported from nimbus-egress"
  # The load-bearing negative test: intersection, not union.
  require_grep EE2 'anti_union' "crates/nimbus-egress/src/layered.rs" \
    "anti-union composition test present"
  # Empty-global parity: absence of a ceiling is byte-identical passthrough.
  require_grep EE2 'byte_identical' "crates/nimbus-egress/src/layered.rs" \
    "sandbox-only byte-identical parity test present"
  # PDP purity: the combinator imports no I/O or transport.
  require_reject EE2 'tokio|pingora|std::net|std::fs|std::io' \
    "crates/nimbus-egress/src/layered.rs" "combinator stays pure (no I/O/transport imports)"
}

# 9. EE3: per-tenant fairness mechanism (DNS budget, byte metering, CPU
#    accounting). Mechanism/seam only — budget VALUES ride TAA.
check_ee3_fairness() {
  require_done EE3 || true
  require_file EE3 "crates/nimbus-proxy/src/fairness.rs" "fairness module exists"
  require_grep EE3 'pub struct FairnessRegistry' "crates/nimbus-proxy/src/fairness.rs" \
    "node-wide fairness registry defined"
  require_grep EE3 'acquire_dns' "crates/nimbus-proxy/src/fairness.rs" \
    "per-tenant DNS acquire budget present (distinct from the node guard)"
  require_grep EE3 'TenantCpuAccounting' "crates/nimbus-proxy/src/fairness.rs" \
    "named CPU accounting primitive present"
  require_grep EE3 'NOT' "crates/nimbus-proxy/src/fairness.rs" \
    "CPU primitive scoped as accounting, not claimed isolation"
  require_grep EE3 'record_bytes_to_upstream' "crates/nimbus-proxy/src/worker.rs" \
    "relay copy-loop byte metering wired (splice tunnel)"
  require_grep EE3 'record_bytes_to_workload' "crates/nimbus-proxy/src/https_intercept.rs" \
    "relay copy-loop byte metering wired (intercept relay)"
  require_grep EE3 'tenant_fairness' "crates/nimbus-proxy/src/worker.rs" \
    "fairness handle captured into the PEP context"
  require_grep EE3 'with_tenant_fairness' "crates/nimbus-sandbox/src/backends/oci/egress.rs" \
    "sandbox captures the tenant handle at registration (no per-request lookup)"
  require_grep EE3 'cannot_starve_another_tenant' "crates/nimbus-proxy/src/fairness.rs" \
    "cross-tenant starvation-independence test present"
}

# 10. EE4: decision-event fan-out seam + per-tenant counters.
check_ee4_fanout() {
  require_done EE4 || true
  require_file EE4 "crates/nimbus-proxy/src/fanout.rs" "fan-out module exists"
  require_grep EE4 'pub fn fan_out_decision_loggers' "crates/nimbus-proxy/src/fanout.rs" \
    "composable multi-sink fan-out defined"
  require_grep EE4 'tenant_decision_counter_sink' "crates/nimbus-proxy/src/fanout.rs" \
    "per-tenant decision counter sink defined"
  require_grep EE4 'decisions_allowed' "crates/nimbus-proxy/src/fairness.rs" \
    "per-tenant counters live on TenantFairness (registration-keyed)"
  # SELH baseline preserved: the sandbox composes the append-only sink FIRST.
  require_grep EE4 'fan_out_decision_loggers' \
    "crates/nimbus-sandbox/src/backends/oci/egress.rs" \
    "sandbox composes the fan-out (append-only baseline first)"
  require_grep EE4 'append-only sink stays' "crates/nimbus-proxy/src/fanout.rs" \
    "append-only baseline ordering documented in the seam"
  require_grep EE4 'every_event_to_every_sink' "crates/nimbus-proxy/src/fanout.rs" \
    "fan-out delivery test present"
}

printf 'Nimbus egress engine verifier\n'
printf 'Repo: %s\n\n' "${ROOT}"

check_plan_routing
check_proof_scaffold
check_activation_gate
check_proof_cross_refs
check_architecture_decision
check_plan_non_goals
check_ee1_engine
check_ee2_ceiling
check_ee3_fairness
check_ee4_fanout

printf '\nSummary: %d passed, %d failed\n' "${passed}" "${failed}"

if [ "${failed}" -ne 0 ]; then
  printf '\nFailed conditions:\n'
  for failure in "${failures[@]}"; do
    printf -- '- %s\n' "${failure}"
  done
  exit 1
fi
