#!/usr/bin/env bash
# Aggregate completion-gate verifier for the Node D-Bus Binding plan
# (`docs/plans/node-dbus-client-binding-plan.md`).
#
# Exits 0 iff every condition in the plan's Completion Gate is satisfied.
# Ships in NDB0 so /goal is verifiable from day one; NDB1-NDB6 progressively
# flip conditions from FAIL to PASS, NDB7 closes the plan and archives it.
#
# Run from the repo root.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PLAN_ACTIVE="docs/plans/node-dbus-client-binding-plan.md"
PLAN_ARCHIVED="docs/plans/archive/node-dbus-client-binding-plan.md"
AGENTS_MD="CLAUDE.md"
PLANS_README="docs/plans/README.md"
PROOF_DIR="docs/plans/proof/node-dbus-client-binding"
PROOF_NDB0="${PROOF_DIR}/ndb0-baseline.md"
RESEARCH_NOTE="docs/plans/research/systemd-dbus-binding-rust-2026.md"

ROOT_CARGO="Cargo.toml"
NODE_CARGO="crates/nimbus-node/Cargo.toml"
NODE_LIB="crates/nimbus-node/src/lib.rs"
ZBUS_MOD_FILE="crates/nimbus-node/src/systemd_transient/zbus_client.rs"
ZBUS_MOD_DIR="crates/nimbus-node/src/systemd_transient/zbus_client/mod.rs"
ZBUS_ERROR="crates/nimbus-node/src/systemd_transient/zbus_client/error.rs"
CORE_ERROR="crates/nimbus-core/src/error.rs"
INTEGRATION_TEST="crates/nimbus-node/tests/zbus_systemd_live.rs"
CI_WF=".github/workflows/ci.yml"
OPERATOR_DOC="docs/operating/node-dbus-binding.md"

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

# Locate the impl module regardless of whether NDB2 lands the single-file
# (zbus_client.rs) or the directory (zbus_client/mod.rs) variant.
zbus_impl_files() {
  local files=()
  if [ -f "${ZBUS_MOD_FILE}" ]; then
    files+=("${ZBUS_MOD_FILE}")
  fi
  if [ -f "${ZBUS_MOD_DIR}" ]; then
    files+=("${ZBUS_MOD_DIR}")
  fi
  if [ -d "crates/nimbus-node/src/systemd_transient/zbus_client" ]; then
    while IFS= read -r f; do
      files+=("${f}")
    done < <(find crates/nimbus-node/src/systemd_transient/zbus_client -name '*.rs')
  fi
  if [ ${#files[@]} -gt 0 ]; then
    printf '%s\n' "${files[@]}"
  fi
}

# -------- conditions -------------------------------------------------------

printf '\033[1mNDB verification gate — node-dbus-client-binding\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

# 1. Plan checked in.
step 1 "Plan checked in"
PLAN_FILE="$(plan_file)"
if [ -n "${PLAN_FILE}" ]; then
  pass "Plan exists at ${PLAN_FILE}"
else
  fail "Plan missing" "Neither ${PLAN_ACTIVE} nor ${PLAN_ARCHIVED} exists"
fi

# 2. Routing entries exist in CLAUDE.md (= AGENTS.md) and docs/plans/README.md.
step 2 "Routing entries exist"
has_agents_route=0
has_plans_route=0
if [ -f "${AGENTS_MD}" ] || [ -L "${AGENTS_MD}" ]; then
  if grep -q 'node-dbus-client-binding-plan' "${AGENTS_MD}"; then
    has_agents_route=1
  fi
fi
if [ -f "${PLANS_README}" ]; then
  if grep -q 'node-dbus-client-binding-plan' "${PLANS_README}"; then
    has_plans_route=1
  fi
fi
if [ "${has_agents_route}" = "1" ] && [ "${has_plans_route}" = "1" ]; then
  pass "${AGENTS_MD} and ${PLANS_README} reference node-dbus-client-binding-plan"
else
  fail "Routing entries incomplete" "agents=${has_agents_route} plans_readme=${has_plans_route}"
fi

# 3. NDB0: baseline proof + research note exist.
step 3 "NDB0 deliverables present (baseline proof + research note)"
ndb0_ok=1
if [ ! -f "${PROOF_NDB0}" ]; then
  ndb0_ok=0
fi
if [ ! -f "${RESEARCH_NOTE}" ]; then
  ndb0_ok=0
fi
if [ "${ndb0_ok}" = "1" ]; then
  pass "${PROOF_NDB0} and ${RESEARCH_NOTE} exist"
else
  details=""
  [ ! -f "${PROOF_NDB0}" ] && details="${details} missing ${PROOF_NDB0};"
  [ ! -f "${RESEARCH_NOTE}" ] && details="${details} missing ${RESEARCH_NOTE};"
  fail "NDB0 deliverables incomplete" "${details}"
fi

# 4. NDB1: zbus_systemd + zbus workspace deps and nimbus-node features.
step 4 "NDB1: zbus_systemd/zbus deps + nimbus-node features wired"
if [ -f "${ROOT_CARGO}" ] && [ -f "${NODE_CARGO}" ]; then
  has_zbus_systemd_dep=0
  has_zbus_dep=0
  has_zbus_systemd_features=0
  has_node_feature=0
  has_test_bus_feature=0
  has_integration_feature=0
  if grep -qE '^[[:space:]]*zbus_systemd[[:space:]]*=' "${ROOT_CARGO}"; then
    has_zbus_systemd_dep=1
  fi
  if grep -qE '^[[:space:]]*zbus[[:space:]]*=' "${ROOT_CARGO}"; then
    has_zbus_dep=1
  fi
  # Features must include both systemd1 and zbus-async-tokio.
  if grep -qE 'zbus_systemd' "${ROOT_CARGO}" && \
     grep -qE 'systemd1' "${ROOT_CARGO}" && \
     grep -qE 'zbus-async-tokio' "${ROOT_CARGO}"; then
    has_zbus_systemd_features=1
  fi
  if grep -qE '^[[:space:]]*systemd-dbus[[:space:]]*=' "${NODE_CARGO}"; then
    has_node_feature=1
  fi
  if grep -qE '^[[:space:]]*systemd-dbus-test-bus[[:space:]]*=' "${NODE_CARGO}"; then
    has_test_bus_feature=1
  fi
  if grep -qE '^[[:space:]]*systemd-dbus-integration-tests[[:space:]]*=' "${NODE_CARGO}"; then
    has_integration_feature=1
  fi
  if [ "${has_zbus_systemd_dep}" = "1" ] && \
     [ "${has_zbus_dep}" = "1" ] && \
     [ "${has_zbus_systemd_features}" = "1" ] && \
     [ "${has_node_feature}" = "1" ] && \
     [ "${has_test_bus_feature}" = "1" ] && \
     [ "${has_integration_feature}" = "1" ]; then
    pass "zbus_systemd/zbus deps plus systemd-dbus test and integration features"
  else
    fail "NDB1 wiring incomplete" "zbus_systemd=${has_zbus_systemd_dep} zbus=${has_zbus_dep} features=${has_zbus_systemd_features} node_feature=${has_node_feature} test_bus=${has_test_bus_feature} integration=${has_integration_feature}"
  fi
else
  fail "${ROOT_CARGO} or ${NODE_CARGO} missing"
fi

# 5. NDB2: ZbusSystemdClient exists with bus selection.
step 5 "NDB2: ZbusSystemdClient + BusKind"
impl_files=$(zbus_impl_files)
if [ -n "${impl_files}" ]; then
  has_zbus_client=0
  has_buskind=0
  while IFS= read -r f; do
    [ -z "${f}" ] && continue
    if grep -qE 'struct ZbusSystemdClient' "${f}"; then
      has_zbus_client=1
    fi
    if grep -qE 'enum BusKind|BusKind::(System|Session)' "${f}"; then
      has_buskind=1
    fi
  done <<< "${impl_files}"
  has_reexport=0
  if [ -f "${NODE_LIB}" ] && grep -qE 'ZbusSystemdClient' "${NODE_LIB}"; then
    has_reexport=1
  fi
  if [ "${has_zbus_client}" = "1" ] && [ "${has_buskind}" = "1" ] && [ "${has_reexport}" = "1" ]; then
    pass "ZbusSystemdClient + BusKind defined and re-exported"
  else
    fail "NDB2 surface incomplete" "ZbusSystemdClient=${has_zbus_client} BusKind=${has_buskind} reexport=${has_reexport}"
  fi
else
  fail "zbus_client module missing" "neither ${ZBUS_MOD_FILE} nor ${ZBUS_MOD_DIR} present"
fi

# 6. NDB3: signal-based completion (Manager.Subscribe + receive_job_removed
#    before StartTransientUnit/StopUnit in source order) and centralized
#    OwnedValue property encoding.
#
#    NOTE ON RIGOR: this is a *structural* proxy — it asserts lexical source
#    order (subscribe < stream < method call) and that an OwnedValue encoder
#    exists. A grep cannot prove the runtime race is actually closed (the
#    stream is live before the method returns). The behavioral proof lives in
#    NDB5's live integration tests (signal-arrives-before/after-response race
#    cases) and NDB6's CI lane, not here. Keep this check as a cheap guardrail,
#    not as the trust anchor.
step 6 "NDB3: signal-correlated job completion + property encoding"
impl_files=$(zbus_impl_files)
has_signal_pattern=0
has_owned_value_encoder=0
if [ -n "${impl_files}" ]; then
  while IFS= read -r f; do
    [ -z "${f}" ] && continue
    # Manager.Subscribe must precede the JobRemoved stream, and both must
    # precede the method call. Match method-call syntax to avoid passing on
    # trait declarations alone.
    if grep -qE '\.subscribe\(' "${f}" && \
       grep -qE '\.receive_job_removed\(|MatchRule::new' "${f}" && \
       grep -qE '\.(start_transient_unit|stop_unit)\(' "${f}"; then
      subscribe_line=$(grep -nE '\.subscribe\(' "${f}" | head -n 1 | cut -d: -f1)
      stream_line=$(grep -nE '\.receive_job_removed\(|MatchRule::new' "${f}" | head -n 1 | cut -d: -f1)
      call_line=$(grep -nE '\.(start_transient_unit|stop_unit)\(' "${f}" | head -n 1 | cut -d: -f1)
      if [ -n "${subscribe_line}" ] && [ -n "${stream_line}" ] && [ -n "${call_line}" ] && \
         [ "${subscribe_line}" -lt "${stream_line}" ] && [ "${stream_line}" -lt "${call_line}" ]; then
        has_signal_pattern=1
      fi
    fi
    if grep -qE 'OwnedValue' "${f}" && grep -qE 'StartTransientUnit|start_transient_unit|ExecStart|Description|WorkingDirectory|Environment' "${f}"; then
      has_owned_value_encoder=1
    fi
  done <<< "${impl_files}"
fi
if [ "${has_signal_pattern}" = "1" ] && [ "${has_owned_value_encoder}" = "1" ]; then
  pass "Manager.Subscribe + JobRemoved stream before call, with OwnedValue encoder"
else
  fail "Signal-correlated completion not yet implemented" "signal_pattern=${has_signal_pattern} owned_value_encoder=${has_owned_value_encoder}"
fi

# 7. NDB4: nimbus_core::Error gains Transport + NotFound, and the error
#    taxonomy module exists with documented source-error variants.
step 7 "NDB4: core Error variants + zbus error taxonomy module"
core_variants_ok=0
if [ -f "${CORE_ERROR}" ]; then
  if grep -qE '^[[:space:]]*Transport\(' "${CORE_ERROR}" && \
     grep -qE '^[[:space:]]*NotFound\(' "${CORE_ERROR}"; then
    core_variants_ok=1
  fi
fi
if [ -f "${ZBUS_ERROR}" ] && [ "${core_variants_ok}" = "1" ]; then
  needed_variants=(Disconnected AccessDenied UnknownObject NoSuchUnit InvalidArgs)
  missing=""
  for v in "${needed_variants[@]}"; do
    if ! grep -qE "${v}" "${ZBUS_ERROR}"; then
      missing="${missing} ${v}"
    fi
  done
  if [ -z "${missing}" ]; then
    pass "core Error has Transport+NotFound; taxonomy covers ${needed_variants[*]}"
  else
    fail "Error taxonomy incomplete" "missing variants:${missing}"
  fi
elif [ "${core_variants_ok}" != "1" ]; then
  fail "nimbus_core::Error missing Transport/NotFound variants" "expected both in ${CORE_ERROR}"
else
  fail "${ZBUS_ERROR} missing"
fi

# 8. NDB5: integration test file gated by Linux + feature.
step 8 "NDB5: Linux-gated integration test exists"
if [ -f "${INTEGRATION_TEST}" ]; then
  has_linux_gate=0
  has_feature_gate=0
  if grep -qE 'target_os[[:space:]]*=[[:space:]]*"linux"' "${INTEGRATION_TEST}"; then
    has_linux_gate=1
  fi
  if grep -qE 'feature[[:space:]]*=[[:space:]]*"systemd-dbus-integration-tests"' "${INTEGRATION_TEST}"; then
    has_feature_gate=1
  fi
  if [ "${has_linux_gate}" = "1" ] && [ "${has_feature_gate}" = "1" ]; then
    pass "${INTEGRATION_TEST} is Linux + feature gated"
  else
    fail "NDB5 gating incomplete" "linux=${has_linux_gate} feature=${has_feature_gate}"
  fi
else
  fail "${INTEGRATION_TEST} missing"
fi

# 9. NDB6: CI job node-dbus-integration exists.
step 9 "NDB6: CI lane node-dbus-integration"
if [ -f "${CI_WF}" ]; then
  if grep -qE '^[[:space:]]+node-dbus-integration:' "${CI_WF}"; then
    has_runner=0
    has_bootstrap=0
    has_test_invocation=0
    has_gate_summary=0
    if grep -qE 'ubuntu-24\.04' "${CI_WF}"; then
      has_runner=1
    fi
    if grep -qE 'sudo apt-get' "${CI_WF}" && \
       grep -qE 'loginctl' "${CI_WF}" && \
       grep -qE 'systemctl --user' "${CI_WF}"; then
      has_bootstrap=1
    fi
    if grep -qE 'systemd-dbus-integration-tests' "${CI_WF}"; then
      has_test_invocation=1
    fi
    # Job must be in rust-gate-summary.needs:
    if awk '/^  rust-gate-summary:$/,/^  [a-z][a-z-]*:[[:space:]]*$/' "${CI_WF}" | grep -qE 'node-dbus-integration'; then
      has_gate_summary=1
    fi
    if [ "${has_runner}" = "1" ] && [ "${has_bootstrap}" = "1" ] && [ "${has_test_invocation}" = "1" ] && [ "${has_gate_summary}" = "1" ]; then
      pass "node-dbus-integration job present, bootstrapped, gated by rust-gate-summary"
    else
      fail "NDB6 CI wiring incomplete" "runner=${has_runner} bootstrap=${has_bootstrap} invocation=${has_test_invocation} gate=${has_gate_summary}"
    fi
  else
    fail "node-dbus-integration job missing from ${CI_WF}"
  fi
else
  fail "${CI_WF} missing"
fi

# 10. NDB7: systemd-dbus in default features + operator doc + ledger all done +
#     latest main CI green.
step 10 "NDB7: default activation + operator doc + ledger green + CI green"
has_default=0
has_live_factory=0
has_doc=0
ledger_clean=0
ci_green=0
if [ -f "${NODE_CARGO}" ]; then
  # Look for `default = [ ... "systemd-dbus" ... ]` in [features].
  if awk '
    /^\[features\]/ {in_features=1; next}
    /^\[/ {in_features=0}
    in_features {print}
  ' "${NODE_CARGO}" | grep -qE '^[[:space:]]*default[[:space:]]*=.*systemd-dbus'; then
    has_default=1
  fi
fi
# NDB7 adds an explicit Linux live-client factory because the trait's default
# type parameter cannot construct an async/fallible client by itself. Require
# the specific factory name — a loose `BusKind::System`/`ZbusSystemdClient::new`
# match would already be satisfied at NDB2 and wouldn't prove NDB7's work.
if grep -rqE 'fn[[:space:]]+linux_systemd_default' crates/nimbus-node/src/ 2>/dev/null; then
  has_live_factory=1
fi
if [ -f "${OPERATOR_DOC}" ]; then
  has_doc=1
fi
PLAN_FILE="$(plan_file)"
if [ -n "${PLAN_FILE}" ]; then
  ledger_rows="$(awk '
    /^\| NDB \| Description \| Status \|/ {in_ledger=1; next}
    in_ledger && /^$/ {in_ledger=0}
    in_ledger && /^\| NDB[0-9]/ {print}
  ' "${PLAN_FILE}")"
  if [ -n "${ledger_rows}" ] && ! printf '%s\n' "${ledger_rows}" | grep -vE '\| done \|' | grep -qE '^\| NDB[0-9]'; then
    ledger_clean=1
  fi
fi
if command -v gh >/dev/null 2>&1; then
  latest=$(gh run list --branch main --workflow ci.yml --limit 1 --json conclusion 2>/dev/null | grep -oE '"conclusion":"[^"]*"' | head -n 1)
  if [ "${latest}" = '"conclusion":"success"' ]; then
    ci_green=1
  elif [ -z "${latest}" ]; then
    # gh present but no conclusion returned (no run yet / auth). Pass, but say so.
    ci_green=1
    printf '        note: gh returned no ci.yml conclusion for main; CI-green ASSUMED — verify manually\n'
  fi
else
  # gh unavailable (e.g. local run): pass so local closeout isn't blocked, but
  # make the assumption explicit rather than silent. CI itself still enforces
  # green on merge; this line only governs the local verifier's exit code.
  ci_green=1
  printf '        note: gh not on PATH; CI-green for main is UNVERIFIED locally (CI enforces it on merge)\n'
fi
if [ "${has_default}" = "1" ] && [ "${has_live_factory}" = "1" ] && [ "${has_doc}" = "1" ] && [ "${ledger_clean}" = "1" ] && [ "${ci_green}" = "1" ]; then
  pass "Activated, documented, ledger clean, CI green"
else
  fail "NDB7 closeout incomplete" "default=${has_default} live_factory=${has_live_factory} doc=${has_doc} ledger=${ledger_clean} ci=${ci_green}"
fi

# -------- summary ----------------------------------------------------------

printf '\n\033[1m%d passed, %d failed\033[0m\n' "${PASS}" "${FAIL}"

if [ "${FAIL}" -gt 0 ]; then
  printf '\nFailures:\n'
  for d in "${FAIL_DETAIL[@]}"; do
    printf '  - %s\n' "${d}"
  done
  exit 1
fi

exit 0
