#!/usr/bin/env bash
# Aggregate completion-gate verifier for the Wasmtime backend plan
# (`docs/private/plans/wasmtime-backend-plan.md`, W0..W7).
#
# W0 creates this script before backend code lands. Conditions intentionally
# remain red until their owning phase supplies code, proof, and closeout state.
#
# Run from the repo root.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PLAN_ACTIVE="docs/private/plans/wasmtime-backend-plan.md"
PLAN_ARCHIVED="docs/private/plans/archive/wasmtime-backend-plan.md"
PLANS_README="docs/private/plans/README.md"
PROOF_DIR="docs/private/plans/proof/wasmtime-backend"
PROOF_W0="${PROOF_DIR}/w0-baseline.md"
PROOF_W1="${PROOF_DIR}/w1-backend-abstraction.md"
PROOF_W2="${PROOF_DIR}/w2-linker.md"
PROOF_W3="${PROOF_DIR}/w3-run-to-completion.md"
PROOF_W4="${PROOF_DIR}/w4-bundle-format.md"
PROOF_W5="${PROOF_DIR}/w5-cooperative-fuel.md"
PROOF_W6="${PROOF_DIR}/w6-retained-store-pool.md"
PROOF_W7="${PROOF_DIR}/w7-closeout.md"

RUNTIME_CARGO="crates/nimbus-runtime/Cargo.toml"
RUNTIME_SRC="crates/nimbus-runtime/src"
ARCHITECTURE_MD="ARCHITECTURE.md"

PASS=0
FAIL=0
FAIL_DETAIL=()

pass() {
  PASS=$((PASS + 1))
  printf '  \033[32mPASS\033[0m  %s\n' "$1"
}

fail() {
  FAIL=$((FAIL + 1))
  printf '  \033[31mFAIL\033[0m  %s\n' "$1"
  if [ $# -ge 2 ]; then
    printf '        %s\n' "$2"
    FAIL_DETAIL+=("$1 - $2")
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

grep_file() {
  [ -f "$2" ] || return 1
  grep -qE "$1" "$2" 2>/dev/null
}

grep_dir() {
  [ -d "$2" ] || return 1
  grep -RqsE --include='*.rs' --include='*.wit' --include='*.md' "$1" "$2" 2>/dev/null
}

ledger_done_count() {
  local file="$1"
  [ -f "${file}" ] || {
    printf '0\n'
    return
  }
  grep -Ec '^\| W[0-7] \| `done` \|' "${file}" 2>/dev/null || printf '0\n'
}

printf '\033[1mWasmtime backend verification gate\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

# 1. Plan file exists and routing points at it.
step 1 "Plan routing"
PLAN_FILE="$(plan_file)"
if [ -n "${PLAN_FILE}" ] \
  && grep_file 'wasmtime-backend-plan\.md' "${PLANS_README}" \
  && grep_file 'active as of 2026-06-27|WASM runtime' "${PLANS_README}"; then
  pass "Plan exists at ${PLAN_FILE} and ${PLANS_README} routes to it"
else
  fail "Plan routing incomplete" "plan=${PLAN_FILE:-missing} plans_readme=$(test -f "${PLANS_README}" && printf present || printf missing)"
fi

# 2. W0 baseline proof exists with the required control-plane anchors.
step 2 "W0 baseline proof"
if [ -f "${PROOF_W0}" ] \
  && grep_file 'Activation gate.*MET|activation gate.*MET' "${PROOF_W0}" \
  && grep_file 'no Wasmtime backend variant' "${PROOF_W0}" \
  && grep_file 'codex/wasmtime-backend' "${PROOF_W0}" \
  && grep_file 'NFS5.*NEG6.*WAC' "${PROOF_W0}" \
  && grep_file '3 passed, 7 failed' "${PROOF_W0}"; then
  pass "W0 proof records activation, baseline absence, branch workflow, joins, and expected verifier count"
else
  fail "W0 proof incomplete" "Expected ${PROOF_W0} with activation/no-backend/branch/NFS5-NEG6-WAC/verifier anchors"
fi

# 3. W0 runtime baseline confirms there is no Wasmtime backend yet.
step 3 "W0 runtime no-backend baseline"
no_wasmtime_dependency=0
no_wasmtime_backend=0
v8_baseline=0
no_workspace_deps=0
if ! grep_file '^[[:space:]]*wasmtime[[:space:].=]' "${RUNTIME_CARGO}"; then
  no_wasmtime_dependency=1
fi
if ! grep_dir 'RuntimeBackendKind::Wasmtime|WasmtimeBackend|mod wasmtime' "${RUNTIME_SRC}"; then
  no_wasmtime_backend=1
fi
if grep_dir 'RuntimeBackendKind::V8|V8RuntimeBackendFactory|BunJscRuntimeBackendFactory' "${RUNTIME_SRC}"; then
  v8_baseline=1
fi
workspace_deps="$(grep -En 'path = "../nimbus-' "${RUNTIME_CARGO}" 2>/dev/null || true)"
if [ -z "${workspace_deps}" ]; then
  no_workspace_deps=1
fi
if [ "${no_wasmtime_dependency}" = "1" ] \
  && [ "${no_wasmtime_backend}" = "1" ] \
  && [ "${v8_baseline}" = "1" ] \
  && [ "${no_workspace_deps}" = "1" ]; then
  pass "Runtime baseline has V8/Bun-JSC seams, no Wasmtime backend, and no workspace path deps"
else
  fail "W0 runtime baseline drifted" "no_dependency=${no_wasmtime_dependency} no_backend=${no_wasmtime_backend} v8_baseline=${v8_baseline} no_workspace_deps=${no_workspace_deps}"
fi

# 4. W1 backend abstraction exists without breaking V8 or the dependency boundary.
step 4 "W1 backend abstraction"
workspace_deps="$(grep -En 'path = "../nimbus-' "${RUNTIME_CARGO}" 2>/dev/null || true)"
w1_code=0
w1_proof=0
if grep_dir 'trait RuntimeBackendFactory|trait RuntimeBackend|RuntimeBackendInvocation' "${RUNTIME_SRC}/backends" \
  && grep_dir 'RunToCompletionWorkerLoopFactory|CooperativeWorkerLoopFactory' "${RUNTIME_SRC}/worker_loop"; then
  w1_code=1
fi
if [ -f "${PROOF_W1}" ] \
  && grep_file 'cargo test -p nimbus-runtime backend' "${PROOF_W1}" \
  && grep_file 'V8.*green|V8 regression' "${PROOF_W1}" \
  && grep_file 'zero-workspace-dep' "${PROOF_W1}"; then
  w1_proof=1
fi
if [ "${w1_code}" = "1" ] && [ "${w1_proof}" = "1" ] && [ -z "${workspace_deps}" ]; then
  pass "Backend abstraction and W1 proof exist; nimbus-runtime has no workspace path deps"
else
  fail "W1 backend abstraction incomplete" "code=${w1_code} proof=${w1_proof} workspace_path_deps=$(printf '%s' "${workspace_deps}" | wc -l | tr -d ' ')"
fi

# 5. W2 wasmtime engine, WIT, and host linker map imports through HostBridge.
step 5 "W2 wasmtime engine, WIT, and linker"
w2_dependency=0
w2_wit=0
w2_linker=0
w2_proof=0
grep_file '^[[:space:]]*wasmtime[[:space:].=]' "${RUNTIME_CARGO}" && w2_dependency=1
if find "${RUNTIME_SRC}" -path '*wit*' -type f 2>/dev/null | grep -q . \
  && grep_dir 'package nimbus:host|world nimbus-function' "${RUNTIME_SRC}"; then
  w2_wit=1
fi
grep_dir 'component::Linker<InvocationHostState>|HostBridge::call|call_async|HostCallRequest' "${RUNTIME_SRC}/backends" && w2_linker=1
if [ -f "${PROOF_W2}" ] \
  && grep_file 'cargo test -p nimbus-runtime wasmtime_linker' "${PROOF_W2}" \
  && grep_file 'HostBridge' "${PROOF_W2}"; then
  w2_proof=1
fi
if [ "${w2_dependency}" = "1" ] && [ "${w2_wit}" = "1" ] && [ "${w2_linker}" = "1" ] && [ "${w2_proof}" = "1" ]; then
  pass "Wasmtime dependency, WIT package, linker, and authority proof exist"
else
  fail "W2 wasmtime linker incomplete" "dependency=${w2_dependency} wit=${w2_wit} linker=${w2_linker} proof=${w2_proof}"
fi

# 6. W3 run-to-completion backend invokes nimbus-function components through the generic loop.
step 6 "W3 run-to-completion backend"
w3_variant=0
w3_backend=0
w3_cache=0
w3_proof=0
grep_dir 'RuntimeBackendKind::Wasmtime|Wasmtime' "${RUNTIME_SRC}" && w3_variant=1
grep_dir 'WasmtimeBackendFactory|WasmtimeBackend|RunToCompletion' "${RUNTIME_SRC}/backends" && w3_backend=1
grep_dir 'WasmtimeModuleCache|precompiled_module_cache|PrecompiledModuleCache' "${RUNTIME_SRC}/backends" && w3_cache=1
if [ -f "${PROOF_W3}" ] \
  && grep_file 'cargo test -p nimbus-runtime wasmtime_run_to_completion' "${PROOF_W3}" \
  && grep_file 'nimbus-function' "${PROOF_W3}" \
  && grep_file 'module cache' "${PROOF_W3}"; then
  w3_proof=1
fi
if [ "${w3_variant}" = "1" ] && [ "${w3_backend}" = "1" ] && [ "${w3_cache}" = "1" ] && [ "${w3_proof}" = "1" ]; then
  pass "Run-to-completion wasmtime backend and proof exist"
else
  fail "W3 run-to-completion backend incomplete" "variant=${w3_variant} backend=${w3_backend} cache=${w3_cache} proof=${w3_proof}"
fi

# 7. W4 bundle format carries WASM component provenance and integrity.
step 7 "W4 WASM bundle format"
w4_bundle=0
w4_world=0
w4_integrity=0
w4_proof=0
grep_dir 'WasmComponent' "${RUNTIME_SRC}" && w4_bundle=1
grep_dir 'ComponentWorld|NimbusFunction|NimbusAgent|target_world' "${RUNTIME_SRC}" && w4_world=1
grep_dir 'BundleIntegrityMismatch|verify_integrity|compute_sha256' "${RUNTIME_SRC}" && grep_dir 'WasmComponent|wasm component' "${RUNTIME_SRC}/runtime" && w4_integrity=1
if [ -f "${PROOF_W4}" ] \
  && grep_file 'cargo test -p nimbus-runtime wasm_bundle' "${PROOF_W4}" \
  && grep_file 'tampered WASM' "${PROOF_W4}"; then
  w4_proof=1
fi
if [ "${w4_bundle}" = "1" ] && [ "${w4_world}" = "1" ] && [ "${w4_integrity}" = "1" ] && [ "${w4_proof}" = "1" ]; then
  pass "WASM component bundle format, world metadata, integrity, and proof exist"
else
  fail "W4 WASM bundle format incomplete" "bundle=${w4_bundle} world=${w4_world} integrity=${w4_integrity} proof=${w4_proof}"
fi

# 8. W5 cooperative fuel scheduling parks/resumes async imports without starving V8.
step 8 "W5 cooperative fuel scheduling"
w5_model=0
w5_driver=0
w5_tests=0
w5_proof=0
grep_dir 'CooperativeFuel|fuel' "${RUNTIME_SRC}" && w5_model=1
grep_dir 'WasmtimeFuelDriver|WasmtimeFuelSlot|OutOfFuel|epoch|Parked' "${RUNTIME_SRC}/backends" && w5_driver=1
grep_dir 'mixed.*V8.*WASM|wasmtime_fuel|park.*resume|timeout' "${RUNTIME_SRC}/runtime/tests" && w5_tests=1
if [ -f "${PROOF_W5}" ] \
  && grep_file 'cargo test -p nimbus-runtime wasmtime_fuel' "${PROOF_W5}" \
  && grep_file 'mixed V8/WASM' "${PROOF_W5}"; then
  w5_proof=1
fi
if [ "${w5_model}" = "1" ] && [ "${w5_driver}" = "1" ] && [ "${w5_tests}" = "1" ] && [ "${w5_proof}" = "1" ]; then
  pass "Cooperative fuel scheduling and fairness proof exist"
else
  fail "W5 cooperative fuel scheduling incomplete" "model=${w5_model} driver=${w5_driver} tests=${w5_tests} proof=${w5_proof}"
fi

# 9. W6 retained Store pool proves reset, mismatch denial, eviction, retirement, and limits.
step 9 "W6 retained Store pool"
w6_pool=0
w6_authority=0
w6_limiter=0
w6_proof=0
grep_dir 'RetainedStorePool|ReusableStore|StorePool|retained store' "${RUNTIME_SRC}/backends" && w6_pool=1
grep_dir 'authority mismatch|InvocationHostState.*reset|reset.*InvocationHostState|mismatch denial' "${RUNTIME_SRC}" && w6_authority=1
grep_dir 'ResourceLimiter|memory limit|max_heap_mb|initial_heap_mb|evict|retire' "${RUNTIME_SRC}/backends" && w6_limiter=1
if [ -f "${PROOF_W6}" ] \
  && grep_file 'cargo test -p nimbus-runtime wasmtime_store_pool' "${PROOF_W6}" \
  && grep_file 'authority mismatch' "${PROOF_W6}" \
  && grep_file 'ResourceLimiter' "${PROOF_W6}"; then
  w6_proof=1
fi
if [ "${w6_pool}" = "1" ] && [ "${w6_authority}" = "1" ] && [ "${w6_limiter}" = "1" ] && [ "${w6_proof}" = "1" ]; then
  pass "Retained Store pool reset, authority, eviction, retirement, and limit proof exist"
else
  fail "W6 retained Store pool incomplete" "pool=${w6_pool} authority=${w6_authority} limiter=${w6_limiter} proof=${w6_proof}"
fi

# 10. W7 observability, benchmark comparison, docs, ledger, CI, and PR closeout.
step 10 "W7 observability, benchmarks, ledger, CI, and PR"
w7_metrics=0
w7_bench=0
w7_docs=0
w7_proof=0
ledger_done=0
archive_support=0
ci_pr=0
grep_dir 'wasmtime.*metric|fuel.*consumed|store pool|compilation time|module cache' "${RUNTIME_SRC}/metrics" "${RUNTIME_SRC}/backends" && w7_metrics=1
[ -f "crates/nimbus-runtime/benches/runtime_pool_modes.rs" ] && grep_file 'wasmtime|WASM|V8' "crates/nimbus-runtime/benches/runtime_pool_modes.rs" && w7_bench=1
if grep_file 'Wasmtime|WASM' "${ARCHITECTURE_MD}" \
  && { [ -f "docs/private/operating/wasmtime-backend.md" ] || grep_file 'wasmtime' "${PLAN_FILE:-/dev/null}"; }; then
  w7_docs=1
fi
if [ -f "${PROOF_W7}" ] \
  && grep_file 'V8 comparison' "${PROOF_W7}" \
  && grep_file 'residual risk' "${PROOF_W7}" \
  && grep_file 'cargo fmt --all --check' "${PROOF_W7}"; then
  w7_proof=1
fi
if [ -n "${PLAN_FILE}" ] && [ "$(ledger_done_count "${PLAN_FILE}")" = "8" ]; then
  ledger_done=1
fi
grep_file 'PLAN_ARCHIVED|docs/private/plans/archive/wasmtime-backend-plan.md' "$0" && archive_support=1
if [ -f "${PROOF_W7}" ] \
  && grep_file 'branch CI.*green' "${PROOF_W7}" \
  && grep_file 'PR.*codex/wasmtime-backend.*main|pull request' "${PROOF_W7}"; then
  ci_pr=1
fi
if [ "${w7_metrics}" = "1" ] \
  && [ "${w7_bench}" = "1" ] \
  && [ "${w7_docs}" = "1" ] \
  && [ "${w7_proof}" = "1" ] \
  && [ "${ledger_done}" = "1" ] \
  && [ "${archive_support}" = "1" ] \
  && [ "${ci_pr}" = "1" ]; then
  pass "Wasmtime metrics, benchmark comparison, docs, ledger, CI, and PR closeout exist"
else
  fail "W7 closeout incomplete" "metrics=${w7_metrics} bench=${w7_bench} docs=${w7_docs} proof=${w7_proof} ledger_done=${ledger_done} archive_support=${archive_support} ci_pr=${ci_pr}"
fi

printf '\nSummary: %d passed, %d failed\n' "${PASS}" "${FAIL}"
if [ "${FAIL}" -ne 0 ]; then
  printf '\nFailures:\n'
  for detail in "${FAIL_DETAIL[@]}"; do
    printf '  - %s\n' "${detail}"
  done
  exit 1
fi

exit 0
