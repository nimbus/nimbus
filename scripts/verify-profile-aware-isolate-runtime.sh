#!/usr/bin/env bash
# Aggregate verification gate for
# docs/private/plans/profile-aware-isolate-runtime-plan.md.
#
# PIR0 shipped this scaffold as an honest control gate. Conditions 1-10 prove the
# measurement harness, proof surface, benchmark evidence, blocked lanes, and ROI
# selection. Conditions 11-15 prove the full-plan audit/control-plane contract.
# Conditions 16-20 prove PIR1 classification landed without user-facing
# efficiency knobs. Conditions 21-27 prove PIR6 snapshot/registry/code-cache
# closeout. Conditions 28-42 prove PIR5 density/GC slices and
# pointer-compression closeout. Conditions 43-46 prove PIR3 side-channel posture
# closeout. Conditions 47-65 record PIR2 cooperative scheduler /
# context-recycling safety, impact, and phase-attribution evidence. Condition 66
# records PIR2 closeout by measured rejection and PIR4 reroute. Conditions
# 67-70 record the PIR4 host-call session binding and mutation-exclusion slice.
# Condition 71 records the PIR4 user-timeout accounting slice.
# Condition 72 records the PIR4 system-wall timeout slice. Condition 73 records
# the PIR4 waitUntil runtime-substrate slice. Condition 74 records the PIR4
# response-ready executor/server boundary. Condition 75 records PIR4 closeout
# and PIR7 activation. Condition 76 records the first PIR7 host-resource budget
# policy slice. Condition 77 records `nimbus start` host-budget lowering.
# Condition 78 records `nimbus dev` inheritance of the same host-budget policy.
# Condition 79 records native `nimbus node` service rendering of that start
# policy. Condition 80 records server construction carrying the typed host
# budget into AppState without conflating it with per-invocation RuntimeLimits.
# Condition 81 records runtime-owned injected host-pressure admission gating.
# Condition 82 records server registry injection of the explicit host governor.
# Condition 83 records the node cgroup v2 host-pressure source and server
# default source selection.
# Condition 84 records low-cardinality host-pressure telemetry.
# Condition 85 records the scheduled/manual CI crossover guard for PIR0/PIR2
# runtime-pool benchmarks.
# Condition 86 records safe process-backed service workload cgroup controls.
# Condition 87 records fixed-bucket runtime-profile telemetry.
# Condition 88 records PIR7 controller replay closeout with live adaptivity off.
# Condition 89 records the final architecture decision for target-bounded
# pointer-compression defaults, the Nimbus-owned source-build expansion lane,
# exemplar import rules, and the explicit WebStandard versus Deno/Node isolate
# lifecycle split for setup, teardown, pooling, and reuse.
# Condition 90 records the target-specific pointer-compression release-default
# stabilizer that wires supported Nimbus release targets through the explicit
# crate feature while leaving unsupported targets non-ptrcomp.
# Condition 91 records the post-PIR optimization benchmark backlog scaffold and
# its guardrails before benchmark implementation starts.
# Condition 92 records the opt-in post-PIR optimization benchmark harness.
# Condition 93 records the post-PIR benchmark smoke trace.
# Condition 94 records the first-wave post-PIR optimization matrix.
# Condition 95 records post-PIR warm-hit attribution.
# Condition 96 records the service-extension startup snapshot partition and
# host-bridge session drift closeout.
# Condition 97 records the fanout retained-density curve.
# Condition 98 records the fixed-window hot-tail prewarm policy curve.
# Condition 99 records the fixed-window pool sizing and pressure-eviction curve.
# Condition 100 records the fixed-window cooperative mixed I/O/CPU scheduler
# curve.
# Condition 101 records the fixed-window exact-key fragmentation curve.
# Condition 102 records the WebStandard code-cache variant curve.
# Condition 103 records the NodeFull lazy-init closeout curve.
# Condition 104 records the replay-based adaptive controller curve.
# Condition 105 records the PIR7L live adaptive autoscaling plan-readiness audit
# and keeps live actuation gated behind captured-trace/shadow/canary proof.
# Condition 106 records the PIR7L live adaptive controller implementation,
# operator controls, adapter seams, low-cardinality metrics, and fixed-window
# benchmark proof while keeping live defaults off.
# Condition 107 records the PIR7M function-scaling UX plan contract: baked
# defaults, `functions.scaling` tenant intent, operator quota envelopes,
# preset/classes DX, effective-plan diagnostics, top-level plural-resource CLI
# grammar, and no public isolate/runtime tuning vocabulary.
# Condition 108 records the PIR7M implementation: typed effective scaling plan,
# operator quota admission, baked config lowering, root-verb CLI grammar,
# server/registry propagation, focused tests, and proof artifact.
#
# Ownership-size exception: this verifier intentionally stays monolithic while
# PIR0-PIR7 and the post-PIR benchmark proof surface are being reconciled through
# one resumable control gate. Before PIR7L implementation or CI promotion, split
# it by concept-owned condition groups rather than into utils/misc files.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PLAN_ACTIVE="docs/private/plans/profile-aware-isolate-runtime-plan.md"
PLAN_ARCHIVED="docs/private/plans/archive/profile-aware-isolate-runtime-plan.md"
if [ -f "${PLAN_ACTIVE}" ]; then
  PLAN="${PLAN_ACTIVE}"
else
  PLAN="${PLAN_ARCHIVED}"
fi
FINAL_ARCH_PLAN="docs/private/plans/profile-aware-isolate-runtime-final-architecture-plan.md"
PLANS_INDEX="docs/private/plans/README.md"
LAYERED_PLAN="docs/private/plans/layered-admission-control-plan.md"
FINDINGS="docs/private/architecture/runtime/profile-aware-isolate-runtime-findings.md"
PROOF="docs/private/plans/proof/profile-aware-isolate-runtime/pir0-baseline.md"
PIR1_PROOF="docs/private/plans/proof/profile-aware-isolate-runtime/pir1-classification.md"
PIR2_SYNTHETIC_AWAIT_PROOF="docs/private/plans/proof/profile-aware-isolate-runtime/pir2-synthetic-await-warm-pool.md"
PIR2_AUTHORITY_PARTITION_PROOF="docs/private/plans/proof/profile-aware-isolate-runtime/pir2-authority-partition.md"
PIR2_RUNTIME_LIFECYCLE_PROOF="docs/private/plans/proof/profile-aware-isolate-runtime/pir2-runtime-lifecycle.md"
PIR2_CLEANLINESS_PROOF="docs/private/plans/proof/profile-aware-isolate-runtime/pir2-cleanliness-gate.md"
PIR2_DENO_REALM_SEAM_PROOF="docs/private/plans/proof/profile-aware-isolate-runtime/pir2-deno-realm-seam.md"
PIR2_MIXED_PROFILE_PROOF="docs/private/plans/proof/profile-aware-isolate-runtime/pir2-mixed-profile-state.md"
PIR2_NODE_REALM_PROOF="docs/private/plans/proof/profile-aware-isolate-runtime/pir2-node-realm-boundary.md"
PIR2_CONTEXT_RECYCLE_IMPACT_PROOF="docs/private/plans/proof/profile-aware-isolate-runtime/pir2-context-recycle-impact.md"
PIR2_CLOSEOUT_REROUTE_PROOF="docs/private/plans/proof/profile-aware-isolate-runtime/pir2-closeout-reroute.md"
NFR6_NODE_FULL_PROOF="docs/private/plans/proof/node-full-substrate-realm/nfr6-benchmark-adoption.md"
PIR2_CONTEXT_RECYCLE_IMPACT_TRACE="docs/private/plans/proof/profile-aware-isolate-runtime/artifacts/pir2-context-recycle-impact-trace.jsonl"
PIR2_CONTEXT_RECYCLE_PHASE_TRACE="docs/private/plans/proof/profile-aware-isolate-runtime/artifacts/pir2-context-recycle-phase-trace.jsonl"
PIR5_PROOF="docs/private/plans/proof/profile-aware-isolate-runtime/pir5-density-gc.md"
PIR5_ISOLATE_GROUP_VALIDATION="docs/private/plans/proof/profile-aware-isolate-runtime/pir5-isolate-group-validation.md"
PIR5_POINTER_COMPRESSION_PROOF="docs/private/plans/proof/profile-aware-isolate-runtime/pir5-pointer-compression.md"
PIR5_POINTER_COMPRESSION_PATCH="docs/private/plans/proof/profile-aware-isolate-runtime/artifacts/rusty-v8-ptrcomp-simdutf-release-assets.patch"
PIR5_POINTER_COMPRESSION_WINDOWS_HOTFIX_PATCH="docs/private/plans/proof/profile-aware-isolate-runtime/artifacts/0001-Skip-Windows-ptrcomp-simdutf-artifacts.patch"
PIR5_POINTER_COMPRESSION_UPSTREAM_JOB_PATCH="docs/private/plans/proof/profile-aware-isolate-runtime/artifacts/0001-Split-ptrcomp-simdutf-release-job.patch"
PIR5_POINTER_COMPRESSION_MATRIX_FIX_PATCH="docs/private/plans/proof/profile-aware-isolate-runtime/artifacts/0001-Fix-ptrcomp-release-matrix-target-selection.patch"
PIR5_POINTER_COMPRESSION_LINUX_ARM_RELEASE_PATCH="docs/private/plans/proof/profile-aware-isolate-runtime/artifacts/0001-Add-Linux-ARM64-ptrcomp-release-artifact.patch"
PIR5_RETAINED_DENSITY_TRACE="docs/private/plans/proof/profile-aware-isolate-runtime/artifacts/pir5-retained-density-current-rss.jsonl"
PIR5_RETAINED_DENSITY_PTRCOMP_TRACE="docs/private/plans/proof/profile-aware-isolate-runtime/artifacts/pir5-retained-density-current-rss-ptrcomp.jsonl"
PIR2_SYNTHETIC_AWAIT_TRACE="docs/private/plans/proof/profile-aware-isolate-runtime/artifacts/pir2-synthetic-await-warm-pool-trace.jsonl"
PIR6_PROOF="docs/private/plans/proof/profile-aware-isolate-runtime/pir6-snapshots-code-cache.md"
PIR3_PROOF="docs/private/plans/proof/profile-aware-isolate-runtime/pir3-side-channel.md"
PIR4_PROOF="docs/private/plans/proof/profile-aware-isolate-runtime/pir4-host-call-session-binding.md"
PIR7_HOST_BUDGET_PROOF="docs/private/plans/proof/profile-aware-isolate-runtime/pir7-host-resource-budget.md"
PIR7M_FUNCTION_SCALING_PROOF="docs/private/plans/proof/profile-aware-isolate-runtime/pir7m-function-scaling-ux.md"
TFA_PLAN_ACTIVE="docs/private/plans/tenant-function-autoscaling-plan.md"
TFA_PLAN_ARCHIVED="docs/private/plans/archive/tenant-function-autoscaling-plan.md"
if [[ -f "${TFA_PLAN_ACTIVE}" ]]; then
  TFA_PLAN="${TFA_PLAN_ACTIVE}"
else
  TFA_PLAN="${TFA_PLAN_ARCHIVED}"
fi
TFA_PROOF="docs/private/plans/proof/tenant-function-autoscaling/README.md"
POST_PIR_OPTIMIZATION_PROOF="docs/private/plans/proof/profile-aware-isolate-runtime/post-pir-optimization-benchmarks.md"
POST_PIR_OPTIMIZATION_SMOKE_TRACE="docs/private/plans/proof/profile-aware-isolate-runtime/artifacts/post-pir-optimization-smoke.jsonl"
POST_PIR_OPTIMIZATION_FIRST_WAVE_TRACE="docs/private/plans/proof/profile-aware-isolate-runtime/artifacts/post-pir-optimization-first-wave.jsonl"
POST_PIR_OPTIMIZATION_ATTRIBUTION_TRACE="docs/private/plans/proof/profile-aware-isolate-runtime/artifacts/post-pir-warm-hit-attribution.jsonl"
POST_PIR_OPTIMIZATION_FANOUT_TRACE="docs/private/plans/proof/profile-aware-isolate-runtime/artifacts/post-pir-fanout-retained-density-current-rss.jsonl"
POST_PIR_OPTIMIZATION_HOT_TAIL_TRACE="docs/private/plans/proof/profile-aware-isolate-runtime/artifacts/post-pir-hot-tail-prewarm-fixed.jsonl"
POST_PIR_OPTIMIZATION_POOL_SIZING_TRACE="docs/private/plans/proof/profile-aware-isolate-runtime/artifacts/post-pir-pool-sizing-curve-fixed.jsonl"
POST_PIR_OPTIMIZATION_COOPERATIVE_MIXED_TRACE="docs/private/plans/proof/profile-aware-isolate-runtime/artifacts/post-pir-cooperative-mixed-fixed.jsonl"
POST_PIR_OPTIMIZATION_FRAGMENTATION_TRACE="docs/private/plans/proof/profile-aware-isolate-runtime/artifacts/post-pir-exact-key-fragmentation-fixed.jsonl"
POST_PIR_OPTIMIZATION_CODE_CACHE_TRACE="docs/private/plans/proof/profile-aware-isolate-runtime/artifacts/post-pir-webstandard-code-cache-fixed.jsonl"
POST_PIR_OPTIMIZATION_NODE_LAZY_INIT_TRACE="docs/private/plans/proof/profile-aware-isolate-runtime/artifacts/post-pir-nodefull-lazy-init-fixed-window.jsonl"
POST_PIR_OPTIMIZATION_CONTROLLER_REPLAY_TRACE="docs/private/plans/proof/profile-aware-isolate-runtime/artifacts/post-pir-controller-replay-fixed.jsonl"
POST_PIR_OPTIMIZATION_LIVE_ADAPTIVE_TRACE="docs/private/plans/proof/profile-aware-isolate-runtime/artifacts/post-pir-live-adaptive-controller-fixed.jsonl"
BENCH="crates/nimbus-runtime/benches/runtime_pool_modes.rs"
POST_PIR_BENCH_MODULE="crates/nimbus-runtime/benches/runtime_pool_modes/post_pir.rs"
RUNTIME_PROFILE="crates/nimbus-runtime/src/limits/profile.rs"
RUNTIME_CONTROLLER_REPLAY="crates/nimbus-runtime/src/limits/controller_replay.rs"
RUNTIME_ADAPTIVE_CONTROLLER="crates/nimbus-runtime/src/limits/adaptive_controller.rs"
RUNTIME_DENSITY="crates/nimbus-runtime/src/limits/density.rs"
RUNTIME_PRESSURE="crates/nimbus-runtime/src/limits/pressure.rs"
CGROUP_PRESSURE_SOURCE="crates/nimbus-node/src/memory_pressure.rs"
TENANT_PROFILE="crates/nimbus-tenant/src/runtime_profile.rs"
START_CLI_TEST="crates/nimbus-bin/src/start/tests/cli_surface.rs"
START_COMMAND="crates/nimbus-bin/src/start/mod.rs"
START_BOOT="crates/nimbus-bin/src/start/boot.rs"
START_RUNTIME_LIMITS="crates/nimbus-bin/src/start/runtime_limits.rs"
DEV_PLAN_TEST="crates/nimbus-bin/src/dev/tests/plan.rs"
NODE_SERVICE="crates/nimbus-bin/src/node_service.rs"
SERVER_CONSTRUCTION="crates/nimbus-server/src/construction.rs"
SERVER_ROUTER="crates/nimbus-server/src/router.rs"
SERVER_STATE="crates/nimbus-server/src/state.rs"
CONVEX_LIB="crates/nimbus-convex/src/lib.rs"
CONVEX_REGISTRY_LOADING="crates/nimbus-convex/src/registry/loading.rs"
CONVEX_RUNTIME_ACCESS="crates/nimbus-convex/src/registry/resolution/runtime_access.rs"
CLOUD_FUNCTIONS_REGISTRY="crates/nimbus-cloud-functions/src/registry.rs"
BOOTSTRAP_EXTENSIONS="crates/nimbus-runtime/src/runtime/bootstrap/extensions.rs"
WARM_POOL_TEST="crates/nimbus-runtime/src/runtime/tests/warm_pool.rs"
POOL_REUSE_TEST="crates/nimbus-runtime/src/runtime/tests/pool_reuse.rs"
SNAPSHOT_LIFECYCLE_TEST="crates/nimbus-runtime/src/runtime/tests/snapshot_lifecycle.rs"
WARM_POOL="crates/nimbus-runtime/src/backends/v8/warm_pool.rs"
V8_LIFECYCLE="crates/nimbus-runtime/src/backends/v8/lifecycle.rs"
V8_STARTUP="crates/nimbus-runtime/src/backends/v8/startup.rs"
V8_STARTUP_KEY="crates/nimbus-runtime/src/backends/v8/startup_key.rs"
RUNTIME_METRICS="crates/nimbus-runtime/src/metrics.rs"
RUNTIME_METRICS_GLOBAL="crates/nimbus-runtime/src/metrics/global.rs"
RUNTIME_METRICS_PROFILES="crates/nimbus-runtime/src/metrics/profiles.rs"
RUNTIME_INVOCATION_DRIVER="crates/nimbus-runtime/src/runtime/driver/invocation.rs"
RUNTIME_HELPERS="crates/nimbus-runtime/src/runtime/helpers.rs"
RUNTIME_ERROR="crates/nimbus-runtime/src/error.rs"
RUNTIME_LOADING="crates/nimbus-runtime/src/runtime/driver/loading.rs"
RUNTIME_COOPERATIVE="crates/nimbus-runtime/src/runtime/cooperative.rs"
RUNTIME_INVOCATION_KIND="crates/nimbus-runtime/src/runtime/invocation.rs"
REC_EXECUTION_PLAN="crates/nimbus-runtime/src/execution_plan.rs"
RUNTIME_REALM_LIFECYCLE="crates/nimbus-runtime/src/runtime/realm_lifecycle.rs"
RUNTIME_BOOTSTRAP_OPS="crates/nimbus-runtime/src/runtime/bootstrap/ops.rs"
RUNTIME_BOOTSTRAP_OPS_SHARED="crates/nimbus-runtime/src/runtime/bootstrap/ops/shared.rs"
RUNTIME_COOPERATIVE_TEST="crates/nimbus-runtime/src/runtime/tests/cooperative.rs"
COOPERATIVE_RETENTION="crates/nimbus-runtime/src/worker_loop/cooperative/retention.rs"
RUNTIME_BUNDLE="crates/nimbus-runtime/src/runtime/bundle.rs"
BUNDLE_INTEGRITY_TEST="crates/nimbus-runtime/src/runtime/tests/bundle_integrity.rs"
RUNTIME_CONSTRUCTION="crates/nimbus-runtime/src/runtime/driver/construction.rs"
RUNTIME_BOOTSTRAP_SOURCE="crates/nimbus-runtime/src/runtime/bootstrap/source.rs"
RUNTIME_HOST_BRIDGE_TEST="crates/nimbus-runtime/src/runtime/tests/host_bridge.rs"
RUNTIME_V8_EMBEDDER="crates/nimbus-runtime/src/backends/v8/embedder.rs"
PIR3_SIDE_CHANNEL_TEST="crates/nimbus-runtime/src/runtime/tests/basic_invocation/side_channel.rs"
RUNTIME_TEST_SUPPORT="crates/nimbus-runtime/src/runtime/tests/support.rs"
RUNTIME_TIMEOUT_TEST="crates/nimbus-runtime/src/runtime/tests/timeout_cancellation.rs"
RUNTIME_LIB="crates/nimbus-runtime/src/lib.rs"
BUN_JSC_BACKEND="crates/nimbus-runtime/src/backends/bun_jsc/mod.rs"
EXECUTOR_LIFECYCLE="crates/nimbus-runtime/src/executor/lifecycle.rs"
EXECUTOR_INVOKE="crates/nimbus-runtime/src/executor/invoke.rs"
EXECUTOR_QUEUE_JOB="crates/nimbus-runtime/src/executor/queue/job.rs"
RUNTIME_LIMITS_RESOURCES="crates/nimbus-runtime/src/limits/resources.rs"
RUNTIME_LIMITS_POLICY="crates/nimbus-runtime/src/limits/policy.rs"
RUNTIME_LIMITS_SCALING="crates/nimbus-runtime/src/limits/scaling.rs"
RUNTIME_LIMITS_TEST="crates/nimbus-runtime/src/limits/tests.rs"
COOPERATIVE_WORKER_LOOP="crates/nimbus-runtime/src/worker_loop/cooperative.rs"
COOPERATIVE_WORKER_RUN="crates/nimbus-runtime/src/worker_loop/cooperative/run.rs"
COOPERATIVE_WORKER_EXECUTION="crates/nimbus-runtime/src/worker_loop/cooperative/execution.rs"
EXECUTOR_ADMISSION="crates/nimbus-runtime/src/executor/admission.rs"
EXECUTOR_TENANT_FAIRNESS="crates/nimbus-runtime/src/executor/admission/tenant_fairness.rs"
EXECUTOR_ADMISSION_PERMIT="crates/nimbus-runtime/src/executor/admission/permit.rs"
EXECUTOR_ADMISSION_DISPATCH="crates/nimbus-runtime/src/executor/admission/dispatch.rs"
COOPERATIVE_EXECUTOR_TEST="crates/nimbus-runtime/src/executor/tests/cooperative.rs"
QUEUE_FAIRNESS_EXECUTOR_TEST="crates/nimbus-runtime/src/executor/tests/queue_fairness.rs"
EXECUTOR_TEST_SUPPORT="crates/nimbus-runtime/src/executor/tests/support.rs"
SERVER_INVOCATION_WORKER="crates/nimbus-server/src/execution/invocations/worker.rs"
HOST_LIFECYCLE="crates/nimbus-node/src/host_lifecycle.rs"
SYSTEMD_TRANSIENT="crates/nimbus-node/src/systemd_transient.rs"
MACHINE_SERVICE_WORKLOADS="crates/nimbus-bin/src/machine/api/service_workloads.rs"
MAKEFILE_PATH="Makefile"
CI_WORKFLOW=".github/workflows/ci.yml"
RELEASE_WORKFLOW=".github/workflows/release.yml"
RELEASE_FEATURE_SCRIPT="scripts/nimbus-release-rust-features.sh"
NIMBUS_CARGO="crates/nimbus/Cargo.toml"
NIMBUS_BIN_CARGO="crates/nimbus-bin/Cargo.toml"
CROSSOVER_SCRIPT="scripts/verify-profile-aware-isolate-runtime-crossover.sh"
TENANT_OPERATOR_POLICY="crates/nimbus-tenant/src/operator_policy.rs"
TENANT_OPERATOR_VALIDATION="crates/nimbus-tenant/src/operator_policy/validation.rs"
TENANT_RUNTIME_SCALING="crates/nimbus-tenant/src/operator_policy/runtime_scaling.rs"
NIMBUS_FACADE_LIB="crates/nimbus/src/lib.rs"
NIMBUS_BIN_MAIN="crates/nimbus-bin/src/main.rs"
NIMBUS_BIN_FUNCTION_SCALING="crates/nimbus-bin/src/function_scaling.rs"
NIMBUS_BIN_EXPLAIN="crates/nimbus-bin/src/explain.rs"
NIMBUS_BIN_VALIDATE="crates/nimbus-bin/src/validate.rs"
NIMBUS_BIN_LIST="crates/nimbus-bin/src/list.rs"
NIMBUS_BIN_RUN="crates/nimbus-bin/src/run.rs"
START_CONFIG="crates/nimbus-bin/src/start/config.rs"

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
    FAIL_DETAIL+=("$1 -- $2")
  else
    FAIL_DETAIL+=("$1")
  fi
}

step() {
  printf '\n\033[1m[%s]\033[0m %s\n' "$1" "$2"
}

contains() {
  grep -qE -- "$1" "$2" 2>/dev/null
}

contains_all() {
  local file="$1"
  shift
  local missing=()
  local pattern
  for pattern in "$@"; do
    if ! contains "${pattern}" "${file}"; then
      missing+=("${pattern}")
    fi
  done
  if [ "${#missing[@]}" -eq 0 ]; then
    return 0
  fi
  printf '%s' "${missing[*]}"
  return 1
}

printf '\033[1mPIR verification gate -- profile-aware isolate runtime\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

step 1 "Control plan has PIR0 through PIR7 complete with PIR8 deferred"
if [ -f "${PLAN}" ] &&
  contains '\| PIR0 \| `done`' "${PLAN}" &&
  contains '\| PIR1 \| `done`' "${PLAN}" &&
  contains '\| PIR2 \| `done`' "${PLAN}" &&
  contains '\| PIR3 \| `done`' "${PLAN}" &&
  contains '\| PIR4 \| `done`' "${PLAN}" &&
  contains '\| PIR5 \| `done`' "${PLAN}" &&
  contains '\| PIR6 \| `done`' "${PLAN}" &&
  contains '\| PIR7 \| `done`' "${PLAN}" &&
  contains '\| PIR8 \| `deferred`' "${PLAN}"; then
  pass "PIR0 through PIR7 are done; PIR8 remains deferred"
else
  fail "PIR ledger is not in the expected PIR7-complete state" \
    "expected PIR0/PIR1/PIR2/PIR3/PIR4/PIR5/PIR6/PIR7 done, PIR8 deferred"
fi

step 2 "Host-resource governance companion plan exists"
if [ -f "${LAYERED_PLAN}" ] &&
  contains 'Host Resource Governance Addendum' "${LAYERED_PLAN}" &&
  contains 'Runtime host resource budget' "${LAYERED_PLAN}" &&
  contains 'memory PSI/RSS/cgroup headroom' "${LAYERED_PLAN}"; then
  pass "layered admission plan records host resource budget follow-up"
else
  fail "layered admission host-resource addendum incomplete"
fi

step 3 "PIR0 benchmark matrix scaffold exists"
if [ -f "${BENCH}" ] &&
  contains_all "${BENCH}" \
    'runtime_pool_modes_pir0_profile_matrix' \
    'runtime_pool_modes_pir0_synthetic_await_matrix' \
    'BenchmarkProfile::WebStandard' \
    'BenchmarkProfile::Node20' \
    'BenchmarkProfile::Node22' \
    'BenchmarkProfile::Node24' \
    'BenchmarkProfile::Node26' \
    'PureJsWorkloadKind::HostlessTrivial' \
    'PureJsWorkloadKind::ComputeBound' \
    'PureJsWorkloadKind::SetupHeavy' \
    'Duration::from_millis\(0\)' \
    'Duration::from_millis\(1\)' \
    'Duration::from_millis\(5\)' \
    'Duration::from_millis\(50\)' >/tmp/pir0-bench-missing.txt; then
  pass "runtime_pool_modes bench covers PIR0 profiles, workloads, and await delays"
else
  fail "PIR0 benchmark matrix scaffold incomplete" "$(cat /tmp/pir0-bench-missing.txt 2>/dev/null)"
fi

step 4 "PIR0 trace emission is machine-readable"
if [ -f "${BENCH}" ] &&
  contains 'NIMBUS_PIR0_TRACE_PATH' "${BENCH}" &&
  contains 'NIMBUS_PIR0_INCLUDE_BLOCKED_AWAIT_ROWS' "${BENCH}" &&
  contains 'nimbus.profile_aware_isolate_runtime.pir0.trace.v1' "${BENCH}" &&
  contains 'serde_json::to_writer' "${BENCH}" &&
  contains 'current_rss_bytes' "${BENCH}"; then
  pass "benchmark emits JSONL trace records with RSS support"
else
  fail "PIR0 trace emission missing"
fi

step 5 "Benchmark matrix avoids real backend dependencies"
if [ -f "${BENCH}" ] &&
  ! grep -qE 'Firestore|Postgres|MySQL|DynamoDB|MongoDB|libsql|firebase|convex' "${BENCH}"; then
  pass "runtime benchmark uses synthetic host work only"
else
  fail "runtime benchmark appears to reference a real backend"
fi

step 6 "Findings doc scaffold exists"
if [ -f "${FINDINGS}" ] &&
  contains_all "${FINDINGS}" \
    '^# Profile-Aware Isolate Runtime Findings' \
    '^## Status' \
    '^## Live Default Inventory' \
    '^## Exemplar Cross-Check' \
    '^## Findings' \
    '^## ROI Ranking' \
    '^## Recommended Next Band' \
    'Measurement status: complete with blocked synthetic-await lane' >/tmp/pir0-findings-missing.txt; then
  pass "findings doc has required PIR0 sections"
else
  fail "findings doc scaffold incomplete" "$(cat /tmp/pir0-findings-missing.txt 2>/dev/null)"
fi

step 7 "Proof artifact scaffold exists"
if [ -f "${PROOF}" ] &&
  contains_all "${PROOF}" \
    '^# PIR0 Baseline' \
    '^## Measurement Status' \
    '^## Benchmark Commands' \
    '^## Git And Host Metadata' \
    '^## Environment Variables' \
    '^## RSS Collection Method' \
    '^## Trace Output Contract' \
    '^## Pool-Sizing Equation Inputs' \
    'NIMBUS_PIR0_TRACE_PATH' >/tmp/pir0-proof-missing.txt; then
  pass "proof artifact has required PIR0 metadata sections"
else
  fail "proof artifact scaffold incomplete" "$(cat /tmp/pir0-proof-missing.txt 2>/dev/null)"
fi

step 8 "Verifier contract is referenced by plan and proof"
if contains 'bash scripts/verify-profile-aware-isolate-runtime.sh' "${PLAN}" &&
  contains 'bash scripts/verify-profile-aware-isolate-runtime.sh' "${PROOF}"; then
  pass "verifier command is wired into control-plane docs"
else
  fail "verifier command missing from plan or proof"
fi

step 9 "PIR0 raw benchmark results recorded"
if [ -f "${PROOF}" ] &&
  contains 'Measurement status: complete' "${PROOF}" &&
  contains 'Raw Criterion Summary' "${PROOF}" &&
  contains 'Trace Record Count' "${PROOF}" &&
  contains 'Blocked Synthetic-Await Lane' "${PROOF}" &&
  contains 'NIMBUS_PIR0_INCLUDE_BLOCKED_AWAIT_ROWS' "${PROOF}" &&
  ! contains 'Measurement status: scaffolded' "${PROOF}"; then
  pass "proof artifact records benchmark results and blocked-lane evidence"
else
  fail "PIR0 benchmark results pending" \
    "run the matrix and replace scaffold status with raw Criterion summaries, trace counts, and blocked-lane evidence"
fi

step 10 "ROI ranking selects the next band"
if [ -f "${FINDINGS}" ] &&
  contains 'Recommended next band: PIR[1-8]' "${FINDINGS}" &&
  contains 'ROI ranking:' "${FINDINGS}" &&
  ! contains 'Recommended next band: pending' "${FINDINGS}" &&
  ! contains 'ROI ranking: pending' "${FINDINGS}"; then
  pass "findings doc selects the next eligible band"
else
  fail "PIR0 ROI ranking pending" \
    "findings doc must rank PIR1-PIR8 and name the next band after benchmarks"
fi

step 11 "Architecture audit closes PIR7 and routes live adaptivity to PIR7L"
if [ -f "${PLAN}" ] &&
  contains 'PIR0, PIR1, PIR2, PIR3, PIR4, PIR5, PIR6, and PIR7 are complete' "${PLAN}" &&
  contains 'PIR7 closed the static host-resource governance lane' "${PLAN}" &&
  contains 'named PIR7L follow-on' "${PLAN}" &&
  contains 'PIR3 closed the in-process side-channel posture' "${PLAN}" &&
  contains 'PIR2 therefore remains closed by deliberate' "${PLAN}" &&
  contains 'rejection/reroute' "${PLAN}" &&
  contains 'cooperative synthetic-await warm/reuse lane' "${PLAN}" &&
  contains 'mixed-profile startup-snapshot/external-reference state' "${PLAN}" &&
  contains 'PIR4 then closed that throughput gate' "${PLAN}" &&
  contains 'PIR7 closed host-safety' "${PLAN}" &&
  contains 'PIR7L owns any future live adaptive autoscaling promotion' "${PLAN}"; then
  pass "plan reflects PIR7 closeout and PIR7L live-adaptive sequencing truth"
else
  fail "PIR7/PIR7L sequencing audit incomplete" \
    "plan must say PIR0-PIR7 are complete, PIR7L owns future live adaptive autoscaling, and PIR4 closed the throughput gate"
fi

step 12 "Universal promotion checklist exists"
if [ -f "${PLAN}" ] &&
  contains_all "${PLAN}" \
    '^### Universal band promotion checklist' \
    '\| Cohesion \|' \
    '\| Maintainability \|' \
    '\| Testability \|' \
    '\| Security \|' \
    '\| Resilience \|' \
    '\| Canonicality \|' \
    '\| Rust idiom \|' \
    '\| Verifiability \|' \
    '\| Autonomous state \|' >/tmp/pir-promotion-checklist-missing.txt; then
  pass "plan has per-band architecture and proof checklist"
else
  fail "universal promotion checklist incomplete" "$(cat /tmp/pir-promotion-checklist-missing.txt 2>/dev/null)"
fi

step 13 "PIR1 implementation contract is concrete"
if [ -f "${PLAN}" ] &&
  contains_all "${PLAN}" \
    'small internal module' \
    'already-normalized `RuntimeLimits`' \
    'admitted tenant decision' \
    'internal `RuntimeEfficiencyPlan`' \
    'Keep profile selection out of `StartCommand`, SDK request shapes, bundle' \
    'Verifier growth: add PIR1 checks' \
    'user-configurable `runtime-profile`/`profile`/pool-kind flag' >/tmp/pir1-contract-missing.txt; then
  pass "PIR1 names code placement, input order, and verifier growth"
else
  fail "PIR1 implementation contract incomplete" "$(cat /tmp/pir1-contract-missing.txt 2>/dev/null)"
fi

step 14 "PIR7 host governance is PIR-owned"
if [ -f "${PLAN}" ] &&
  contains 'PIR7 owns the concrete host-budget policy shape' "${PLAN}" &&
  contains 'layered admission records rationale, not a blocking dependency' "${PLAN}" &&
  contains '\| PIR7 \| `done` .* \| PIR0, PIR4 \|' "${PLAN}" &&
  ! grep -E '\| PIR7 \| `in_progress` .*EO8 host-budget decision' "${PLAN}" >/dev/null 2>&1; then
  pass "PIR7 host safety has no external EO8 blocker"
else
  fail "PIR7 host-governance ownership is ambiguous" \
    "PIR7 should depend on PIR0/PIR4 and treat layered admission as rationale, not an external implementation owner"
fi

step 15 "Autonomous goal spans the full non-deferred plan"
if [ -f "${PLAN}" ] &&
	contains_all "${PLAN}" \
	  '/goal Autonomously execute docs/private/plans/profile-aware-isolate-runtime-plan.md' \
	  'through all eligible non-deferred bands' \
	  'PIR0, PIR1, PIR2, PIR3, PIR4, PIR5, PIR6, PIR7, and PIR7L are done' \
	  'named PIR follow-on' \
	  'If live adaptive autoscaling is the requested work, PIR7L is already the required controller follow-on' \
	  'If tenant/developer scaling UX, baked defaults, `nimbus.yaml`, or operator quota admission is the requested work, execute PIR7M' \
	  'continue to the next eligible band by dependency order and recorded ROI' \
	  'Do not promote PIR8 unless the plan and nimbus-sandbox S-band explicitly mark it active' \
	  'PIR2 closed by rejecting context recycling as a default substrate for this release' \
	  'PIR4 preserved PIR2'\''s safety gates while implementing isolate-scoped multiplexing only' \
	  'PIR7 closed with static measured defaults, host resource budget guardrails' \
	  'PIR7L is the named follow-on for live adaptive autoscaling and is done as an operator-gated controller' \
	  'PIR7M is the named follow-on for product-grade function scaling UX' \
	  'or adaptive scaling as speed tuning' \
	  'Universal band promotion checklist' \
    'The verifier condition count must grow before a band is marked done' >/tmp/pir-goal-missing.txt; then
  pass "goal prompt can drive autonomous band-by-band execution"
else
  fail "autonomous goal prompt incomplete" "$(cat /tmp/pir-goal-missing.txt 2>/dev/null)"
fi

step 16 "RuntimeProfile type is present and bounded"
if [ -f "${RUNTIME_PROFILE}" ] &&
  contains_all "${RUNTIME_PROFILE}" \
    'pub enum RuntimeProfile' \
    'WebLean' \
    'NodeFull' \
    'serde\(rename_all = "snake_case"\)' \
    'pub fn for_limits' \
    'RuntimeCompatibilityTarget::BunJsc => None' >/tmp/pir1-runtime-profile-missing.txt &&
  contains 'RuntimeProfile' "crates/nimbus-runtime/src/lib.rs" &&
  contains 'RuntimeProfile' "crates/nimbus/src/lib.rs"; then
  pass "RuntimeProfile is a closed Rust runtime axis"
else
  fail "RuntimeProfile type/export incomplete" "$(cat /tmp/pir1-runtime-profile-missing.txt 2>/dev/null)"
fi

step 17 "RuntimeEfficiencyPlan classifier consumes admitted decision"
if [ -f "${TENANT_PROFILE}" ] &&
  contains_all "${TENANT_PROFILE}" \
    'pub struct RuntimeEfficiencyPlan' \
    'pub enum RuntimeEfficiencyPlanState' \
    'FlagOffCurrentBehavior' \
    'EscalatedOrRouted' \
    'UnsupportedSurface' \
    'normalized_limits: &RuntimeLimits' \
    'admitted_decision: &TenantRuntimePolicyDecision' \
    'effective_pool_kind: normalized_limits.runtime_pool_kind' \
    'effective_execution_model: normalized_limits.execution_model' \
    'TenantRuntimePolicyAdmission::AdmitInProcess' >/tmp/pir1-classifier-missing.txt; then
  pass "classifier is admission-after-policy and behavior-neutral"
else
  fail "RuntimeEfficiencyPlan classifier incomplete" "$(cat /tmp/pir1-classifier-missing.txt 2>/dev/null)"
fi

step 18 "PIR1 focused tests are present and recorded"
if contains 'runtime_profile_is_derived_from_v8_javascript_surface_only' "crates/nimbus-runtime/src/limits/tests.rs" &&
  contains 'runtime_efficiency_plan_classifies_web_and_node_after_admission_without_changing_axes' "crates/nimbus-tenant/src/tests.rs" &&
  contains 'runtime_efficiency_plan_never_downgrades_escalated_or_unsupported_surfaces' "crates/nimbus-tenant/src/tests.rs" &&
  contains 'start_does_not_accept_runtime_efficiency_profile_knobs' "${START_CLI_TEST}" &&
  [ -f "${PIR1_PROOF}" ] &&
  contains 'cargo test -p nimbus-runtime runtime_profile_is_derived_from_v8_javascript_surface_only' "${PIR1_PROOF}" &&
  contains 'cargo test -p nimbus-tenant runtime_efficiency_plan' "${PIR1_PROOF}" &&
  contains 'cargo test -p nimbus-bin --bin nimbus start_does_not_accept_runtime_efficiency_profile_knobs' "${PIR1_PROOF}"; then
  pass "PIR1 tests and proof artifact are wired"
else
  fail "PIR1 tests/proof artifact incomplete"
fi

step 19 "No user-facing runtime efficiency knob was added"
if contains 'start_does_not_accept_runtime_efficiency_profile_knobs' "${START_CLI_TEST}" &&
  ! grep -E '#\[arg\([^]]*(runtime-profile|runtime-pool-kind|runtime-execution-model|runtime-reset-strategy)' "crates/nimbus-bin/src/start/mod.rs" >/dev/null 2>&1 &&
  ! grep -R -E 'runtimeProfile|runtime_profile|runtime-profile' packages/nimbus/src crates/nimbus-server/src/protocol.rs >/dev/null 2>&1; then
  pass "runtime profile remains derived evidence, not request/config input"
else
  fail "runtime efficiency profile appears user-configurable" \
    "check StartCommand, JS SDK request shapes, and runtime protocol payloads"
fi

step 20 "PIR1 proof and next-band state are recorded"
if [ -f "${PIR1_PROOF}" ] &&
  contains 'PIR1 status: complete' "${PIR1_PROOF}" &&
  contains 'PIR6 is `done`; PIR5 is now' "${PLAN}" &&
  contains 'PIR1 classification band completed' "${PLAN}" &&
  contains 'PIR1 implementation and proof' "${PLAN}"; then
  pass "PIR1 is documented as complete and later bands advanced to PIR5"
else
  fail "PIR1 closeout state incomplete"
fi

step 21 "PIR6 bootstrap extension registry is the shared seam"
if [ -f "${BOOTSTRAP_EXTENSIONS}" ] &&
  contains_all "${BOOTSTRAP_EXTENSIONS}" \
    'struct RuntimeBootstrapExtensionRegistry' \
    'enum NodeBootstrapExtensionSlot' \
    'NODE_BOOTSTRAP_EXTENSION_SLOTS' \
    'fn snapshot_extension' \
    'fn execution_extension' \
    'node_extension_registry_is_single_ordered_source' >/tmp/pir6-registry-missing.txt; then
  pass "PIR6 has a single ordered registry for node snapshot/execution extensions"
else
  fail "PIR6 bootstrap registry seam incomplete" "$(cat /tmp/pir6-registry-missing.txt 2>/dev/null)"
fi

step 22 "PIR6 node snapshot consumption path is wired and tested"
if [ -f "${WARM_POOL}" ] &&
  [ -f "${V8_STARTUP}" ] &&
  [ -f "${WARM_POOL_TEST}" ] &&
  [ -f "${PIR6_PROOF}" ] &&
  contains 'for_compatibility_target' "${V8_STARTUP}" &&
  contains 'V8RuntimeConstructionMode::for_compatibility_target' "${WARM_POOL}" &&
  ! grep -qE 'use_startup_snapshot = !.*is_node|Proper Node22 snapshotting requires' "${WARM_POOL}" &&
  contains 'warm_pool_uses_startup_snapshot_for_node_targets' "${WARM_POOL_TEST}" &&
  contains 'node22_target_exposes_minimal_node_globals' "${PIR6_PROOF}"; then
  pass "node-compatible V8 cold misses now consume startup snapshots with focused proof"
else
  fail "PIR6 node snapshot consumption path incomplete" \
    "expected construction-mode selector, no node unsnapshotted exception, focused warm-pool test, and PIR6 proof"
fi

step 23 "PIR6 module code-cache key includes profile and authority-affecting runtime config"
if [ -f "${RUNTIME_BUNDLE}" ] &&
  [ -f "${BUNDLE_INTEGRITY_TEST}" ] &&
  [ -f "${PIR6_PROOF}" ] &&
  contains_all "${RUNTIME_BUNDLE}" \
    'struct RuntimeBundleEngineCacheKey' \
    'runtime_profile: Option<RuntimeProfile>' \
    'node_conditions: Vec<String>' \
    'service_extension_enabled: bool' \
    'exact_service_grants: Vec<String>' \
    'RuntimeProfile::for_limits' \
    'limits.grants.sorted_service_grants' >/tmp/pir6-code-cache-key-missing.txt &&
  contains 'runtime_bundle_module_code_cache_is_partitioned_by_engine_config' "${BUNDLE_INTEGRITY_TEST}" &&
  contains 'node_custom_condition_limits' "${BUNDLE_INTEGRITY_TEST}" &&
  contains 'node_service_limits' "${BUNDLE_INTEGRITY_TEST}" &&
  contains 'runtime_bundle_module_code_cache_is_partitioned_by_engine_config' "${PIR6_PROOF}" &&
  contains 'startup_snapshot_runtime_populates_and_reuses_bundle_module_code_cache' "${PIR6_PROOF}"; then
  pass "module code-cache partitions include PIR6 profile/config safety dimensions"
else
  fail "PIR6 module code-cache key safety incomplete" "$(cat /tmp/pir6-code-cache-key-missing.txt 2>/dev/null)"
fi

step 24 "PIR6 snapshot measurement has completed focused Criterion rows"
if [ -f "${PIR6_PROOF}" ] &&
  [ -f "${FINDINGS}" ] &&
  contains '/opt/homebrew/bin/timeout 600 cargo bench -p nimbus-runtime --bench runtime_pool_modes --no-run' "${PIR6_PROOF}" &&
  contains 'optimized bench target finished in 4m03s' "${PIR6_PROOF}" &&
  contains 'runtime_pool_modes_pir0_profile_matrix/node22/hostless_trivial/cooperative_locker/startup_snapshot_cache' "${PIR6_PROOF}" &&
  contains 'runtime_pool_modes_pir0_profile_matrix/web_standard/hostless_trivial/cooperative_locker/startup_snapshot_cache' "${PIR6_PROOF}" &&
  contains 'runtime_pool_modes_pir0_profile_matrix/node22/setup_heavy_large_module/cooperative_locker/startup_snapshot_cache' "${PIR6_PROOF}" &&
  contains 'runtime_pool_modes_pir0_profile_matrix/web_standard/setup_heavy_large_module/cooperative_locker/startup_snapshot_cache' "${PIR6_PROOF}" &&
  contains '9\.9210 ms 10\.005 ms 10\.125 ms' "${PIR6_PROOF}" &&
  contains '1\.7878 ms 1\.7965 ms 1\.8060 ms' "${PIR6_PROOF}" &&
  contains '10\.239 ms 10\.362 ms 10\.462 ms' "${PIR6_PROOF}" &&
  contains '2\.2121 ms 2\.3096 ms 2\.4493 ms' "${PIR6_PROOF}" &&
  contains '95\.189%' "${PIR6_PROOF}" &&
  contains 'wired node startup-snapshot consumption' "${FINDINGS}" &&
  contains 'reran the full profile matrix' "${FINDINGS}" &&
  contains 'Node20/22/24/26 startup-snapshot-cache medians now cluster around 10-11 ms' "${FINDINGS}" &&
  contains 'WebStandard cold medians remain around 1\.8-2\.2 ms' "${FINDINGS}"; then
  pass "PIR6 records focused WebStandard and Node22 startup-snapshot measurements"
else
  fail "PIR6 snapshot Criterion measurement incomplete" \
    "proof and findings must include the compile/run split and focused WebStandard/Node22 startup-snapshot-cache rows"
fi

step 25 "PIR6 dedicated code-cache impact rows are recorded"
if contains 'struct CodeCacheImpactScenario' "${BENCH}" &&
  contains 'runtime_pool_modes_pir6_code_cache_impact' "${BENCH}" &&
  contains 'FreshBundleEachInvocation' "${BENCH}" &&
  contains 'PrimedBundleCodeCache' "${BENCH}" &&
  contains 'runtime_pool_modes_pir6_code_cache_impact/web_standard/setup_heavy_large_module/fresh_bundle_each_invocation' "${PIR6_PROOF}" &&
  contains 'runtime_pool_modes_pir6_code_cache_impact/web_standard/setup_heavy_large_module/primed_bundle_code_cache' "${PIR6_PROOF}" &&
  contains 'runtime_pool_modes_pir6_code_cache_impact/node22/setup_heavy_large_module/fresh_bundle_each_invocation' "${PIR6_PROOF}" &&
  contains 'runtime_pool_modes_pir6_code_cache_impact/node22/setup_heavy_large_module/primed_bundle_code_cache' "${PIR6_PROOF}" &&
  contains '2\.2724 ms 2\.2957 ms 2\.3220 ms' "${PIR6_PROOF}" &&
  contains '2\.0905 ms 2\.1007 ms 2\.1186 ms' "${PIR6_PROOF}" &&
  contains '10\.586 ms 10\.624 ms 10\.668 ms' "${PIR6_PROOF}" &&
  contains '10\.364 ms 10\.415 ms 10\.520 ms' "${PIR6_PROOF}" &&
  contains '2\.2724 ms 2\.2957 ms 2\.3220 ms' "${FINDINGS}" &&
  contains '2\.0905 ms 2\.1007 ms 2\.1186 ms' "${FINDINGS}" &&
  contains '10\.586 ms 10\.624 ms 10\.668 ms' "${FINDINGS}" &&
  contains '10\.364 ms 10\.415 ms 10\.520 ms' "${FINDINGS}"; then
  pass "PIR6 records focused WebStandard and Node22 code-cache impact rows"
else
  fail "PIR6 code-cache impact measurement incomplete" \
    "bench, proof, and findings must include fresh-bundle versus primed-bundle rows for WebStandard and Node22"
fi

step 26 "PIR6 web-lean snapshot registry shape is explicit and tested"
if [ -f "${BOOTSTRAP_EXTENSIONS}" ] &&
  [ -f "${PIR6_PROOF}" ] &&
  contains 'fn snapshot_extension_labels' "${BOOTSTRAP_EXTENSIONS}" &&
  contains 'web_standard_snapshot_registry_carries_web_extensions_not_node_internals' "${BOOTSTRAP_EXTENSIONS}" &&
  contains 'node_snapshot_registry_extends_ordered_node_slots' "${BOOTSTRAP_EXTENSIONS}" &&
  contains 'RuntimeCompatibilityTarget::WebStandardIsolate' "${BOOTSTRAP_EXTENSIONS}" &&
  contains 'labels.push\("nimbus_runtime"\)' "${BOOTSTRAP_EXTENSIONS}" &&
  contains 'labels.push\("nimbus_runtime_test"\)' "${BOOTSTRAP_EXTENSIONS}" &&
  contains 'cargo test -p nimbus-runtime snapshot_registry --lib -- --nocapture' "${PIR6_PROOF}" &&
  contains '2 passed, 0 failed, 1231 filtered out' "${PIR6_PROOF}" &&
  contains 'web-lean snapshot shape proof' "${PLAN}"; then
  pass "PIR6 records a tested web-lean snapshot registry shape"
else
  fail "PIR6 web-lean snapshot proof incomplete" \
    "extensions.rs, PIR6 proof, and plan ledger must prove WebStandard snapshots stay runtime-only"
fi

step 27 "PIR6 full matrix rerun and code-cache v1 decisions are recorded"
if [ -f "${PIR6_PROOF}" ] &&
  [ -f "${FINDINGS}" ] &&
  contains 'PIR6 status: complete' "${PIR6_PROOF}" &&
  contains '/opt/homebrew/bin/timeout 900 cargo bench -p nimbus-runtime --bench runtime_pool_modes -- runtime_pool_modes_pir0_profile_matrix --sample-size 10 --measurement-time 1 --warm-up-time 1' "${PIR6_PROOF}" &&
  contains 'Criterion saved 45 `new/estimates\.json` rows' "${PIR6_PROOF}" &&
  contains 'node20_hostless_trivial_cooperative_locker/startup_snapshot_cache` | 10\.077 ms' "${PIR6_PROOF}" &&
  contains 'node22_hostless_trivial_cooperative_locker/startup_snapshot_cache` | 10\.019 ms' "${PIR6_PROOF}" &&
  contains 'node24_hostless_trivial_cooperative_locker/startup_snapshot_cache` | 10\.675 ms' "${PIR6_PROOF}" &&
  contains 'node26_hostless_trivial_cooperative_locker/startup_snapshot_cache` | 10\.480 ms' "${PIR6_PROOF}" &&
  contains 'web_standard_hostless_trivial_cooperative_locker/startup_snapshot_cache` | 1\.835 ms' "${PIR6_PROOF}" &&
  contains 'Decision for PIR6 v1: keep the current in-memory per-bundle module bytecode' "${PIR6_PROOF}" &&
  contains 'cache and defer a disk-persistent bytecode cache until a restart/redeploy' "${PIR6_PROOF}" &&
  contains 'Decision for PIR6 v1: do not implement import-set tree-shaken Node extension' "${PIR6_PROOF}" &&
  contains 'PIR6 is complete' "${FINDINGS}" &&
  contains 'Recommended next band: PIR5' "${FINDINGS}" &&
  contains 'PIR6 is `done`; PIR5 is now' "${PLAN}"; then
  pass "PIR6 records full matrix closeout, v1 code-cache decisions, and PIR5 activation"
else
  fail "PIR6 closeout evidence incomplete" \
    "proof, findings, and plan must record the full matrix rerun, code-cache v1 decision, and PIR5 activation"
fi

step 28 "PIR5 density planner is measured-RSS driven and host-budget bounded"
if [ -f "${RUNTIME_DENSITY}" ] &&
  contains_all "${RUNTIME_DENSITY}" \
    'pub struct RuntimeDensityMeasurement' \
    'retained_runtime_count: NonZeroUsize' \
    'total_rss_delta_bytes: u64' \
    'pub struct RuntimeDensityBudget' \
    'operator_reserved_headroom_bytes' \
    'pub struct RuntimeDensityPlan' \
    'measured_per_runtime_rss_bytes' \
    'heap_cap_bytes_per_runtime' \
    'planning_bytes_per_runtime' \
    'active_runtime_reservation_bytes' \
    'max_retained_runtimes_per_worker_by_memory' \
    'effective_max_warm_pool_entries_per_worker' \
    'RuntimeIsolateGroupFfiStatus::DeferredPendingValidation' >/tmp/pir5-density-missing.txt &&
  contains 'RuntimeDensityPlan' "crates/nimbus-runtime/src/lib.rs" &&
  contains 'RuntimeDensityPlan' "crates/nimbus/src/lib.rs"; then
  pass "PIR5 density planner derives retained pool bounds from measured RSS and host budget"
else
  fail "PIR5 density planner incomplete" "$(cat /tmp/pir5-density-missing.txt 2>/dev/null)"
fi

step 29 "PIR5 density tests prove measured RSS, node sizing, and FFI deferral"
if contains 'runtime_density_plan_uses_measured_profile_rss_and_reserves_active_slots' "crates/nimbus-runtime/src/limits/tests.rs" &&
  contains 'runtime_density_plan_bounds_node_pool_lower_than_web_under_same_budget' "crates/nimbus-runtime/src/limits/tests.rs" &&
  contains 'runtime_density_plan_keeps_isolate_group_ffi_deferred' "crates/nimbus-runtime/src/limits/tests.rs" &&
  [ -f "${PIR5_PROOF}" ] &&
  contains 'cargo test -p nimbus-runtime runtime_density_plan --lib -- --nocapture' "${PIR5_PROOF}" &&
  contains '3 passed; 0 failed; 0 ignored; 0 measured; 984 filtered out' "${PIR5_PROOF}"; then
  pass "PIR5 focused density tests and proof are recorded"
else
  fail "PIR5 density tests/proof incomplete"
fi

step 30 "PIR5 proof records RSS methodology and remaining gates honestly"
if [ -f "${PIR5_PROOF}" ] &&
  contains_all "${PIR5_PROOF}" \
    'PIR5 status: in_progress' \
    'Module: `crates/nimbus-runtime/src/limits/density.rs`' \
    'WebStandard | 83\.6 MiB' \
    'Node26 | 189\.2 MiB' \
    'process-level trace maxima' \
    'Historical PIR0 rows are not used as the final PIR5 retained-RSS proof' \
    'Pointer compression: not measured' \
    'Promotion-grade retained-RSS methodology: accepted' \
    'fresh-process per-profile current-RSS delta' \
    'External-memory accounting: V8-reported external memory is now included' \
    'Host/cgroup memory-pressure source: runtime-side injected decision policy is' \
    'node-owned cgroup v2 reader exists' \
    'Near-heap-limit checked-out runtime condemnation: wired' \
    'warm-pool response API consumes the normalized' >/tmp/pir5-proof-missing.txt; then
  pass "PIR5 proof distinguishes completed density policy work from remaining GC/measurement gates"
else
  fail "PIR5 proof artifact is not honest about remaining gates" "$(cat /tmp/pir5-proof-missing.txt 2>/dev/null)"
fi

step 31 "PIR5 IsolateGroup/shared-RO FFI remains gated"
if [ -f "${PIR5_PROOF}" ] &&
  [ -f "${PIR5_ISOLATE_GROUP_VALIDATION}" ] &&
  contains 'IsolateGroup FFI status: deferred after validation' "${PIR5_PROOF}" &&
  contains 'pir5-isolate-group-validation.md' "${PIR5_PROOF}" &&
  contains 'one-group-per-authority fallback does not erase the density win' "${PIR5_PROOF}" &&
  ! grep -R -E 'v8_enable_shared_ro_heap|set_isolate_group|CreateParams.*IsolateGroup|IsolateGroup<' \
    crates/nimbus-runtime/src crates/nimbus-runtime/build.rs Cargo.toml >/dev/null 2>&1; then
  pass "no shared-read-only-heap FFI branch exists without a validation artifact"
else
  fail "IsolateGroup/shared-RO FFI gate is not enforced" \
    "either remove the FFI branch or add the PIR5 validation artifact before allowing it"
fi

step 32 "PIR5 V8 lifecycle seam runs boundary memory maintenance before retention"
if [ -f "${V8_LIFECYCLE}" ] &&
  [ -f "${WARM_POOL}" ] &&
  contains_all "${V8_LIFECYCLE}" \
    'pub\(crate\) fn prepare_warm_runtime_for_retention' \
    'WarmRuntimeRetentionDecision' \
    'WarmRuntimeBoundaryMaintenance' \
    'memory_pressure_notification\(v8::MemoryPressureLevel::Moderate\)' \
    'low_memory_notification\(\)' \
    'HeapCarryoverExceeded' \
    'MaxWarmReusesExceeded' \
    'heap_carryover_limit_bytes' >/tmp/pir5-lifecycle-missing.txt &&
  contains 'prepare_warm_runtime_for_retention' "${WARM_POOL}" &&
  contains 'last_boundary_maintenance_for_test' "${WARM_POOL}" &&
  contains 'record_retained_runtime_pool_retirement' "${WARM_POOL}"; then
  pass "PIR5 lifecycle seam performs boundary V8 pressure/low-memory maintenance and typed condemnation"
else
  fail "PIR5 V8 lifecycle seam incomplete" "$(cat /tmp/pir5-lifecycle-missing.txt 2>/dev/null)"
fi

step 33 "PIR5 warm-pool lifecycle tests and proof are recorded"
if [ -f "${WARM_POOL_TEST}" ] &&
  [ -f "${PIR5_PROOF}" ] &&
  contains 'warm_pool_runs_boundary_memory_maintenance' "${WARM_POOL_TEST}" &&
  contains 'warm_pool_condemns_after_max_warm_reuses' "${WARM_POOL_TEST}" &&
  contains 'warm_pool_heap_carryover_limit_is_internal_fraction_of_heap_cap' "${WARM_POOL_TEST}" &&
  contains 'cargo test -p nimbus-runtime runtime::tests::warm_pool --lib -- --nocapture' "${PIR5_PROOF}" &&
  contains '12 passed; 0 failed; 8 ignored; 0 measured; 967 filtered out' "${PIR5_PROOF}"; then
  pass "PIR5 lifecycle tests prove boundary maintenance, max-reuse condemnation, and heap-carryover threshold"
else
  fail "PIR5 warm-pool lifecycle tests/proof incomplete"
fi

step 34 "PIR5 warm-pool memory pressure evicts idle retained entries"
if [ -f "${V8_LIFECYCLE}" ] &&
  [ -f "${RUNTIME_PRESSURE}" ] &&
  [ -f "${WARM_POOL}" ] &&
  [ -f "${WARM_POOL_TEST}" ] &&
  [ -f "${PIR5_PROOF}" ] &&
  contains_all "${V8_LIFECYCLE}" \
    'WarmPoolMemoryPressureEviction' \
    'retained_entry_eviction_count_for_pressure' \
    'RuntimeMemoryPressureDecision::for_level' \
    'retained_runtime_eviction_target' >/tmp/pir5-pressure-missing.txt &&
  contains_all "${RUNTIME_PRESSURE}" \
    'RuntimeMemoryPressureLevel::Nominal' \
    'RuntimeMemoryPressureLevel::High' \
    'RuntimeMemoryPressureLevel::Critical' \
    'retained_runtime_eviction_target' >/tmp/pir5-pressure-policy-missing.txt &&
  contains 'RuntimeMemoryPressureLevel' "${V8_LIFECYCLE}" &&
  contains 'apply_memory_pressure' "${WARM_POOL}" &&
  contains 'evict_lru_entry' "${WARM_POOL}" &&
  contains 'warm_pool_memory_pressure_evicts_idle_entries' "${WARM_POOL_TEST}" &&
  contains 'warm_pool_pressure_eviction_count_is_conservative' "${WARM_POOL_TEST}" &&
  contains 'High pressure evicts the oldest half' "${PIR5_PROOF}" &&
  contains 'critical pressure evicts all idle retained runtimes' "${PIR5_PROOF}" &&
  contains 'cargo check -p nimbus-runtime --lib' "${PIR5_PROOF}"; then
  pass "PIR5 pressure response can evict idle warm entries before host OOM pressure"
else
  fail "PIR5 warm-pool memory-pressure eviction incomplete" "$(cat /tmp/pir5-pressure-missing.txt 2>/dev/null) $(cat /tmp/pir5-pressure-policy-missing.txt 2>/dev/null)"
fi

step 35 "PIR5 near-heap-limit callbacks condemn checked-out warm-pool runtimes"
if [ -f "${WARM_POOL_TEST}" ] &&
  [ -f "${PIR5_PROOF}" ] &&
  [ -f "crates/nimbus-runtime/src/runtime/driver/invocation.rs" ] &&
  contains 'add_near_heap_limit_callback' "crates/nimbus-runtime/src/runtime/driver/invocation.rs" &&
  contains 'record_warm_pool_retirement' "crates/nimbus-runtime/src/runtime/driver/invocation.rs" &&
  contains 'RuntimePoolKind::WarmPool' "crates/nimbus-runtime/src/runtime/driver/invocation.rs" &&
  contains 'warm_pool_condemns_checked_out_runtime_after_near_heap_limit' "${WARM_POOL_TEST}" &&
  contains 'Near-heap-limit checked-out runtime condemnation' "${PIR5_PROOF}" &&
  contains 'runtime_pool_replacements' "${PIR5_PROOF}" &&
  contains 'warm_pool_retirement' "${PIR5_PROOF}"; then
  pass "PIR5 near-heap-limit path retires checked-out warm-pool runtimes"
else
  fail "PIR5 near-heap-limit checked-out runtime condemnation incomplete"
fi

step 36 "PIR5 retained-density current-RSS trace records fresh-process profile rows"
retained_density_rows=0
if [ -f "${PIR5_RETAINED_DENSITY_TRACE}" ]; then
  retained_density_rows="$(grep -c '"schema":"nimbus.profile_aware_isolate_runtime.pir5.retained_density.v1"' "${PIR5_RETAINED_DENSITY_TRACE}" || true)"
fi
if [ -f "${BENCH}" ] &&
  [ -f "${PIR5_PROOF}" ] &&
  [ -f "${PIR5_RETAINED_DENSITY_TRACE}" ] &&
  [ "${retained_density_rows}" -ge 5 ] &&
  contains_all "${BENCH}" \
    'PIR5_RETAINED_DENSITY_TRACE_SCHEMA' \
    'NIMBUS_PIR5_RETAINED_DENSITY_TRACE_PATH' \
    'NIMBUS_PIR5_RETAINED_DENSITY_PROFILE' \
    'MACH_TASK_BASIC_INFO' \
    'resident_size' \
    'include_pir5_retained_density_profile' \
    'struct RetainedDensityScenario' \
    'RuntimeBundle::for_tenant' \
    'retained_runtime_count' \
    'measured_iterations' \
    'rss_source' \
    'fresh_process_profile_filter_enabled' \
    'measured_per_runtime_rss_bytes' \
    'runtime_pool_modes_pir5_retained_density' >/tmp/pir5-retained-density-bench-missing.txt &&
  contains_all "${PIR5_RETAINED_DENSITY_TRACE}" \
    '"profile":"web_standard"' \
    '"profile":"node20"' \
    '"profile":"node22"' \
    '"profile":"node24"' \
    '"profile":"node26"' \
    '"rss_source":"macos_mach_task_basic_info_resident_size"' \
    '"fresh_process_profile_filter_enabled":true' \
    '"retained_runtime_count":4' \
    '"measured_iterations":1' \
    '"total_retained_invocations":4' \
    '"retained_runtime_pool_entries":4' \
    '"retained_runtime_pool_evictions":0' \
    '"retained_runtime_pool_retirements":0' >/tmp/pir5-retained-density-trace-missing.txt &&
  contains 'runtime_pool_modes_pir5_retained_density' "${PIR5_PROOF}" &&
  contains 'wrote 5 retained-density current-RSS rows' "${PIR5_PROOF}" &&
  contains 'fresh-process per-profile current-RSS delta' "${PIR5_PROOF}" &&
  contains 'Promotion-grade retained RSS methodology' "${PLAN}" &&
  contains 'retained RSS methodology accepted' "${PLAN}"; then
  pass "PIR5 retained-density trace records current RSS deltas from fresh per-profile benchmark processes"
else
  fail "PIR5 retained-density current-RSS trace incomplete" \
    "rows=${retained_density_rows}; bench missing: $(cat /tmp/pir5-retained-density-bench-missing.txt 2>/dev/null); trace missing: $(cat /tmp/pir5-retained-density-trace-missing.txt 2>/dev/null)"
fi

step 37 "PIR5 host memory-pressure decision module is injected and conservative"
if [ -f "${RUNTIME_PRESSURE}" ] &&
  [ -f "${PIR5_PROOF}" ] &&
  contains_all "${RUNTIME_PRESSURE}" \
    'RuntimeMemoryPressureLevel' \
    'RuntimeMemoryPressureSample' \
    'RuntimeMemoryPressureDecision' \
    'RuntimeMemoryPressureSourceStatus' \
    'pause_prewarming' \
    'run_idle_low_memory_maintenance' \
    'evict_idle_retained_runtimes' \
    'conservative_degraded' >/tmp/pir5-pressure-policy-missing.txt &&
  contains 'RuntimeMemoryPressureSample' "crates/nimbus-runtime/src/limits.rs" &&
  contains 'RuntimeMemoryPressureLevel' "crates/nimbus-runtime/src/lib.rs" &&
  contains 'RuntimeMemoryPressureSample' "crates/nimbus/src/lib.rs" &&
  contains 'runtime_memory_pressure_sample_classifies_observed_watermarks' "crates/nimbus-runtime/src/limits/tests.rs" &&
  contains 'runtime_memory_pressure_sample_degrades_conservatively_without_source' "crates/nimbus-runtime/src/limits/tests.rs" &&
  contains 'cargo test -p nimbus-runtime runtime_memory_pressure --lib -- --nocapture' "${PIR5_PROOF}" &&
  contains '3 passed; 0 failed; 0 ignored; 0 measured; 985 filtered out' "${PIR5_PROOF}" &&
  contains 'host/cgroup memory-pressure decision Module' "${PIR5_PROOF}" &&
  ! grep -R --include='*.rs' -E 'memory\.current|memory\.high|memory\.max|/proc/pressure|/sys/fs/cgroup' \
    crates/nimbus-runtime/src >/dev/null 2>&1; then
  pass "PIR5 pressure policy accepts injected host samples and degrades by pausing prewarm plus shrinking idle retained runtimes"
else
  fail "PIR5 host memory-pressure decision module incomplete" "$(cat /tmp/pir5-pressure-policy-missing.txt 2>/dev/null)"
fi

step 38 "PIR5 node cgroup v2 memory-pressure reader feeds runtime pressure samples"
if [ -f "${CGROUP_PRESSURE_SOURCE}" ] &&
  [ -f "${PIR5_PROOF}" ] &&
  contains 'nimbus-runtime = \{ path = "\.\./nimbus-runtime" \}' "crates/nimbus-node/Cargo.toml" &&
  contains 'CgroupV2MemoryPressureSource' "crates/nimbus-node/src/lib.rs" &&
  contains 'HostMemoryPressureObservation' "crates/nimbus-node/src/lib.rs" &&
  contains_all "${CGROUP_PRESSURE_SOURCE}" \
    'CgroupV2MemoryPressureSource' \
    'HostMemoryPressureObservation' \
    'memory.current' \
    'memory.high' \
    'memory.max' \
    'RuntimeMemoryPressureSample::observed' \
    'RuntimeMemoryPressureSample::unavailable' \
    'must be absolute' \
    'cgroup_v2_memory_pressure_observes_high_pressure' \
    'cgroup_v2_memory_pressure_observes_critical_pressure' \
    'cgroup_v2_memory_pressure_degrades_when_watermark_is_unavailable' \
    'cgroup_v2_memory_pressure_source_requires_absolute_path' >/tmp/pir5-cgroup-pressure-missing.txt &&
  contains 'cargo test -p nimbus-node cgroup_v2_memory_pressure --lib -- --nocapture' "${PIR5_PROOF}" &&
  contains '4 passed; 0 failed; 0 ignored; 0 measured; 37 filtered out' "${PIR5_PROOF}" &&
  contains 'node cgroup v2 pressure reader' "${PLAN}"; then
  pass "PIR5 has a node-owned cgroup v2 memory reader that feeds runtime pressure samples"
else
  fail "PIR5 node cgroup v2 memory-pressure reader incomplete" "$(cat /tmp/pir5-cgroup-pressure-missing.txt 2>/dev/null)"
fi

step 39 "PIR5 pressure-aware prewarm scheduler admission is explicit"
if [ -f "${RUNTIME_PRESSURE}" ] &&
  [ -f "${PIR5_PROOF}" ] &&
  contains_all "${RUNTIME_PRESSURE}" \
    'RuntimePrewarmScheduleDecision' \
    'schedule_prewarm_entries' \
    'requested_entries' \
    'admitted_entries' \
    'paused_by_memory_pressure' >/tmp/pir5-prewarm-scheduler-missing.txt &&
  contains 'RuntimePrewarmScheduleDecision' "crates/nimbus-runtime/src/limits.rs" &&
  contains 'RuntimePrewarmScheduleDecision' "crates/nimbus-runtime/src/lib.rs" &&
  contains 'RuntimePrewarmScheduleDecision' "crates/nimbus/src/lib.rs" &&
  contains 'runtime_memory_pressure_decision_pauses_prewarm_scheduler' "crates/nimbus-runtime/src/limits/tests.rs" &&
  contains 'admitted_entries, 0' "crates/nimbus-runtime/src/limits/tests.rs" &&
  contains 'prewarm scheduler admission' "${PIR5_PROOF}" &&
  contains 'PIR5 prewarm scheduler admission slice' "${PLAN}" &&
  contains 'actual adaptive/background prewarm loop remains PIR7-owned' "${PLAN}"; then
  pass "PIR5 prewarm admission admits nominal requests and pauses speculative prewarm under high, critical, or unavailable pressure"
else
  fail "PIR5 pressure-aware prewarm scheduler admission incomplete" "$(cat /tmp/pir5-prewarm-scheduler-missing.txt 2>/dev/null)"
fi

step 40 "PIR5 V8-reported external memory participates in retained-memory accounting"
if [ -f "${V8_LIFECYCLE}" ] &&
  [ -f "${PIR5_PROOF}" ] &&
  contains_all "${V8_LIFECYCLE}" \
    'external_memory_bytes' \
    'retained_memory_bytes' \
    'saturating_add\(self\.external_memory_bytes\)' \
    'HeapCarryoverExceeded' \
    'retained_memory_bytes_includes_v8_reported_external_memory' >/tmp/pir5-external-memory-missing.txt &&
  contains 'retained runtime accounting should include V8-reported external memory' "${WARM_POOL_TEST}" &&
  contains 'cargo test -p nimbus-runtime retained_memory_bytes --lib -- --nocapture' "${PIR5_PROOF}" &&
  contains '1 passed; 0 failed; 0 ignored; 0 measured; 988 filtered out' "${PIR5_PROOF}" &&
  contains 'cargo test -p nimbus-runtime warm_pool_runs_boundary_memory_maintenance --lib -- --nocapture' "${PIR5_PROOF}" &&
  contains '1 passed; 0 failed; 1 ignored; 0 measured; 987 filtered out' "${PIR5_PROOF}" &&
  contains 'PIR5 external-memory accounting slice' "${PLAN}" &&
  contains 'custom host-owned V8 backing stores' "${PIR5_PROOF}"; then
  pass "PIR5 boundary retention accounts for V8-reported external memory before retaining warm runtimes"
else
  fail "PIR5 external-memory retained accounting incomplete" "$(cat /tmp/pir5-external-memory-missing.txt 2>/dev/null)"
fi

step 41 "PIR5 IsolateGroup validation artifact keeps FFI deferred"
if [ -f "${PIR5_ISOLATE_GROUP_VALIDATION}" ] &&
  contains_all "${PIR5_ISOLATE_GROUP_VALIDATION}" \
    'Do not bind or use V8 `IsolateGroup`' \
    'pointer-compressed total heap usage' \
    'cannot exceed 4 GB' \
    'shared JS objects cannot cross' \
    'shared read-only heap' \
    'multi-cage mode creates one cage per isolate' \
    'no group field on `CreateParams`' \
    'binds only `v8::Isolate::New\(params\)`' \
    'RuntimeIsolateGroupFfiStatus` remains `DeferredPendingValidation`' \
    'measured density win' >/tmp/pir5-isolate-group-validation-missing.txt &&
  contains 'IsolateGroup validation' "${PLAN}" &&
  contains 'pointer-compression impact only' "${PLAN}" &&
  contains '42 passed, 0 failed' "${PIR5_PROOF}"; then
  pass "PIR5 records IsolateGroup constraints and keeps shared-RO FFI deferred"
else
  fail "PIR5 IsolateGroup validation artifact incomplete" "$(cat /tmp/pir5-isolate-group-validation-missing.txt 2>/dev/null)"
fi

step 42 "PIR5 pointer-compression impact is measured and non-default"
retained_density_ptrcomp_rows=0
if [ -f "${PIR5_RETAINED_DENSITY_PTRCOMP_TRACE}" ]; then
  retained_density_ptrcomp_rows="$(grep -c '"schema":"nimbus.profile_aware_isolate_runtime.pir5.retained_density.v1"' "${PIR5_RETAINED_DENSITY_PTRCOMP_TRACE}" || true)"
fi
if [ -f "${PIR5_POINTER_COMPRESSION_PROOF}" ] &&
  [ -f "${PIR5_POINTER_COMPRESSION_PATCH}" ] &&
  [ -f "${PIR5_POINTER_COMPRESSION_WINDOWS_HOTFIX_PATCH}" ] &&
  [ -f "${PIR5_POINTER_COMPRESSION_UPSTREAM_JOB_PATCH}" ] &&
  [ -f "${PIR5_POINTER_COMPRESSION_MATRIX_FIX_PATCH}" ] &&
  [ -f "${PIR5_POINTER_COMPRESSION_LINUX_ARM_RELEASE_PATCH}" ] &&
  [ -f "${PIR5_RETAINED_DENSITY_PTRCOMP_TRACE}" ] &&
  [ "${retained_density_ptrcomp_rows}" -ge 5 ] &&
  contains 'v8-pointer-compression = \["deno_core/v8_enable_pointer_compression"\]' "crates/nimbus-runtime/Cargo.toml" &&
  ! grep -E 'deno_core.*v8_enable_pointer_compression' Cargo.toml >/dev/null 2>&1 &&
	  contains 'v2.8.3-nimbus.80' Cargo.toml &&
	  contains 'v149.4.0-nimbus.10' Cargo.toml &&
	  contains 'RUSTY_V8_VERSION = "149.4.0-nimbus.10"' ".cargo/config.toml" &&
	  contains 'v2.8.3-nimbus.80#5414432bfe59346f442e81d8c50d04e39d4f1611' Cargo.lock &&
	  contains 'v149.4.0-nimbus.10#f9457373150679d9db9eb577dcd3a687a3ec25ef' Cargo.lock &&
  contains_all "${PIR5_RETAINED_DENSITY_PTRCOMP_TRACE}" \
    '"profile":"web_standard"' \
    '"profile":"node20"' \
    '"profile":"node22"' \
    '"profile":"node24"' \
    '"profile":"node26"' \
    '"fresh_process_profile_filter_enabled":true' \
    '"retained_runtime_count":4' \
    '"measured_iterations":1' \
    '"retained_runtime_pool_evictions":0' \
    '"retained_runtime_pool_retirements":0' \
	    '"measured_per_runtime_rss_bytes":1007616' \
	    '"measured_per_runtime_rss_bytes":3092480' \
	    '"measured_per_runtime_rss_bytes":3256320' \
	    '"measured_per_runtime_rss_bytes":3600384' \
	    '"measured_per_runtime_rss_bytes":2772992' >/tmp/pir5-pointer-compression-trace-missing.txt &&
  contains_all "${PIR5_POINTER_COMPRESSION_PROOF}" \
    'release-artifact blocker unblocked by `v149.4.0-nimbus.10`' \
    'downstream compile and retained-RSS impact measured' \
    'cargo check -p nimbus-runtime --lib --features v8-pointer-compression' \
    'env -u V8_FROM_SOURCE cargo check -p nimbus-runtime --lib --features v8-pointer-compression' \
    'src_binding_ptrcomp_simdutf_release_aarch64-apple-darwin.rs' \
    'HTTP Error 404: Not Found' \
    '149.4.0-nimbus.2' \
	    '3aefa0a2730db325cb66d387fad0fdcc01182594' \
	    'v149.4.0-nimbus.8' \
	    '4bc0152e583c501335380826ee8983f486961609' \
	    'v2.8.3-nimbus.72' \
	    'f62f006545b64db23e80ff6add7432af1f26d3e7' \
	    'v2.8.3-nimbus.73' \
	    '9e1a1d96d8cef3d269fa9b1eb521364839bfc8ed' \
	    'v2.8.3-nimbus.76' \
	    '828cd062096fc765d672f8678b8b39f9cca148c6' \
	    'v2.8.3-nimbus.79' \
	    '5414432bfe59346f442e81d8c50d04e39d4f1611' \
	    'v2.8.3-nimbus.80' \
	    'f9457373150679d9db9eb577dcd3a687a3ec25ef' \
	    'v149.4.0-nimbus.10' \
	    'RUSTY_V8_VERSION = "149.4.0-nimbus.10"' \
    'without falling back to `V8_FROM_SOURCE`' \
    'normal and simdutf assets only' \
    'no `ptrcomp_simdutf` assets' \
    'v149.4.0-nimbus.4' \
    '83ccf0205eb22ec4b15135db16000a53375839bf' \
    '27875775139' \
    'v149.4.0-nimbus.5' \
    '5b376a0b6738ac63c17c52f654fc9c3844705917' \
    '27876168204' \
    '82495913025' \
    'qemu: uncaught target signal 11' \
    'QEMU Linux ARM64 ptrcomp `nextest` may be execution stability evidence only' \
    'native Nimbus Linux ARM64 release build consuming it' \
    'Linux ARM64 ptrcomp release artifact' \
    'skips only the full' \
    'Clippy ptrcomp simdutf' \
    'Build ptrcomp simdutf release artifacts' \
    'v149.4.0-nimbus.10' \
    '27930407697' \
    'release aarch64-unknown-linux-gnu ptrcomp simdutf' \
    'published release has 22 assets' \
    'librusty_v8_ptrcomp_simdutf_release_aarch64-unknown-linux-gnu.a.gz' \
    'src_binding_ptrcomp_simdutf_release_aarch64-unknown-linux-gnu.rs' \
    '0001-Add-Linux-ARM64-ptrcomp-release-artifact.patch' \
    '2f329e173b918672e330451ad0a1a33054cc27638bd25e1b81cf91d1bf8a68fa' \
    'v149.4.0-nimbus.6' \
    '27877679911' \
    'v149.4.0-nimbus.7' \
    'efe67cd1a7a8d6c02a33beaf33dbe8767b57b506' \
    '27877737361' \
    'completed successfully at' \
    'reported 20 published assets' \
    'librusty_v8_ptrcomp_simdutf_release_aarch64-apple-darwin.a.gz' \
    'src_binding_ptrcomp_simdutf_release_aarch64-apple-darwin.rs' \
    'librusty_v8_ptrcomp_simdutf_release_x86_64-unknown-linux-gnu.a.gz' \
    'src_binding_ptrcomp_simdutf_release_x86_64-unknown-linux-gnu.rs' \
    'no Linux ARM or Windows pointer-compression assets' \
    'refs/tags/v149.4.0-nimbus.7' \
    'no `librusty_v8_ptrcomp_[*]_aarch64-unknown-linux-gnu`' \
    'Linux ARM in the base release job for default[+]simdutf assets' \
    'removes Linux ARM from the combined ptrcomp[+]simdutf job' \
    'no `release aarch64-unknown-linux-gnu ptrcomp simdutf` job' \
    'IsManagedByPartitionAlloc\(object_addr\)' \
    'unsupported feature/target combinations are absent from the' \
    'build-ptrcomp-simdutf' \
    'The local `~/src/github.com/nimbus/rusty_v8` worktree first patched' \
    'actionlint .github/workflows/ci.yml' \
    '0006f84ba030eceb19327bb4795bc6c45aafb4c5' \
    'rusty-v8-ptrcomp-simdutf-release-assets.patch' \
    '690eff81ae335ac35f42fb15a0500a7bd9e115880597e8a32100ca4eccaf254f' \
    '0001-Skip-Windows-ptrcomp-simdutf-artifacts.patch' \
    'ba7df1e76e845624a61517fe42ecaac51c799b12aca40896bba7e39a0a1bceb6' \
    '0001-Split-ptrcomp-simdutf-release-job.patch' \
    '8e061987c00d69e7e43c4edd27e07fe4eac383384689480ceabc8bd7244cd761' \
    '0001-Fix-ptrcomp-release-matrix-target-selection.patch' \
    '4821cdc512d204638aec51f2df0ef6dc641571f9dc2b8de0ccab0cfcf3ce8e4c' \
    'not `workflow` scope' \
    'temporary remote branch was deleted' \
    'https://github.com/nimbus/rusty_v8/issues/1' \
    'https://github.com/nimbus/rusty_v8/pull/2' \
    'codex/ptrcomp-simdutf-release-assets' \
    'sccache: error: Server startup failed: cache storage failed to read' \
    'pir5-retained-density-current-rss-ptrcomp.jsonl' \
    'wrote 5 pointer-compression retained-density current-RSS rows' \
	    '`web_standard` | 1814528 | 1007616 | -806912 | -44.47%' \
	    '`node20` | 4444160 | 3092480 | -1351680 | -30.41%' \
	    '`node22` | 4591616 | 3256320 | -1335296 | -29.08%' \
	    '`node24` | 5632000 | 3600384 | -2031616 | -36.07%' \
	    '`node26` | 4558848 | 2772992 | -1785856 | -39.17%' \
    'Pointer-compression impact is measured and no longer blocks PIR5 promotion' \
    'Pointer compression remains a non-default Nimbus runtime feature' \
    'not sufficient to make pointer compression the default Nimbus runtime build' >/tmp/pir5-pointer-compression-missing.txt &&
  contains_all "${PIR5_POINTER_COMPRESSION_PATCH}" \
    'From 0006f84ba030eceb19327bb4795bc6c45aafb4c5' \
    'Subject: \[PATCH\] Add ptrcomp simdutf release assets' \
    'Test ptrcomp simdutf' \
    'Build ptrcomp simdutf release artifacts' \
    'librusty_v8_ptrcomp_simdutf_release_aarch64-apple-darwin.a.gz' \
    'src_binding_ptrcomp_simdutf_release_x86_64-pc-windows-msvc.rs' >/tmp/pir5-pointer-compression-patch-missing.txt &&
  contains_all "${PIR5_POINTER_COMPRESSION_WINDOWS_HOTFIX_PATCH}" \
    'From 83ccf0205eb22ec4b15135db16000a53375839bf' \
    'Subject: \[PATCH\] Skip Windows ptrcomp simdutf artifacts' \
    "matrix.config.target != 'x86_64-pc-windows-msvc'" \
    'rusty-v8-ptrcomp-simdutf-release-' \
    'pattern: rusty-v8-\*' >/tmp/pir5-pointer-compression-hotfix-patch-missing.txt &&
  contains_all "${PIR5_POINTER_COMPRESSION_UPSTREAM_JOB_PATCH}" \
    'From 5b376a0b6738ac63c17c52f654fc9c3844705917' \
    'Subject: \[PATCH\] Split ptrcomp simdutf release job' \
    'build-ptrcomp-simdutf' \
    'release \$\{\{ matrix.config.target \}\} ptrcomp simdutf' \
    'cargo1-\$\{\{ matrix.config.target \}\}-release-ptrcomp-simdutf-' \
    'needs: \[build, build-ptrcomp-simdutf\]' >/tmp/pir5-pointer-compression-job-patch-missing.txt &&
  contains_all "${PIR5_POINTER_COMPRESSION_MATRIX_FIX_PATCH}" \
    'From efe67cd1a7a8d6c02a33beaf33dbe8767b57b506' \
    'Subject: \[PATCH\] Fix ptrcomp release matrix target selection' \
    '[+]          - os: ubuntu-22.04' \
    '[+]            target: aarch64-unknown-linux-gnu' \
    '[-]          - os: ubuntu-22.04' \
    '[-]            target: aarch64-unknown-linux-gnu' >/tmp/pir5-pointer-compression-matrix-fix-patch-missing.txt &&
  contains_all "${PIR5_POINTER_COMPRESSION_LINUX_ARM_RELEASE_PATCH}" \
    'From f9457373150679d9db9eb577dcd3a687a3ec25ef' \
    'Subject: \[PATCH\] Add Linux ARM64 ptrcomp release artifact' \
    'build-ptrcomp-simdutf' \
    'target: aarch64-unknown-linux-gnu' \
    "matrix.config.target != 'aarch64-unknown-linux-gnu'" \
    'Clippy ptrcomp simdutf' \
    'rusty-v8-ptrcomp-simdutf-release-\$\{\{ matrix.config.target \}\}' \
    'librusty_v8_ptrcomp_simdutf_release_aarch64-unknown-linux-gnu.a.gz' \
    'src_binding_ptrcomp_simdutf_release_aarch64-unknown-linux-gnu.rs' >/tmp/pir5-pointer-compression-linux-arm-release-patch-missing.txt &&
  [ "$(shasum -a 256 "${PIR5_POINTER_COMPRESSION_PATCH}" | awk '{print $1}')" = "690eff81ae335ac35f42fb15a0500a7bd9e115880597e8a32100ca4eccaf254f" ] &&
  [ "$(shasum -a 256 "${PIR5_POINTER_COMPRESSION_WINDOWS_HOTFIX_PATCH}" | awk '{print $1}')" = "ba7df1e76e845624a61517fe42ecaac51c799b12aca40896bba7e39a0a1bceb6" ] &&
  [ "$(shasum -a 256 "${PIR5_POINTER_COMPRESSION_UPSTREAM_JOB_PATCH}" | awk '{print $1}')" = "8e061987c00d69e7e43c4edd27e07fe4eac383384689480ceabc8bd7244cd761" ] &&
  [ "$(shasum -a 256 "${PIR5_POINTER_COMPRESSION_MATRIX_FIX_PATCH}" | awk '{print $1}')" = "4821cdc512d204638aec51f2df0ef6dc641571f9dc2b8de0ccab0cfcf3ce8e4c" ] &&
  [ "$(shasum -a 256 "${PIR5_POINTER_COMPRESSION_LINUX_ARM_RELEASE_PATCH}" | awk '{print $1}')" = "2f329e173b918672e330451ad0a1a33054cc27638bd25e1b81cf91d1bf8a68fa" ] &&
  contains 'pir5-pointer-compression.md' "${PLAN}" &&
  contains 'pointer-compression artifact blocker' "${PLAN}" &&
  contains 'rusty-v8-ptrcomp-simdutf-release-assets.patch' "${PLAN}" &&
  contains 'v149.4.0-nimbus.7' "${PLAN}" &&
  contains '27877737361' "${PLAN}" &&
  contains 'completed successfully and published upstream-supported' "${PLAN}" &&
  contains 'published release has 22' "${PLAN}" &&
  contains 'release aarch64-unknown-linux-gnu ptrcomp simdutf' "${PLAN}" &&
  contains 'v149.4.0-nimbus.10' "${PLAN}" &&
	  contains 'v2.8.3-nimbus.80' "${PLAN}" &&
  contains 'pir5-retained-density-current-rss-ptrcomp.jsonl' "${PLAN}" &&
  contains 'pointer compression remains opt-in, not default' "${PLAN}" &&
	  contains 'PIR5 Linux ARM64 ptrcomp release artifact' "${PLAN}" &&
	  contains 'skip only the QEMU `nextest` step' "${PLAN}" &&
	  contains 'librusty_v8_ptrcomp_simdutf_release_aarch64-unknown-linux-gnu.a.gz' "${PLAN}" &&
  contains 'PIR3 is now `in_progress`' "${PLAN}" &&
  contains 'https://github.com/nimbus/rusty_v8/issues/1' "${PLAN}" &&
  contains 'workflow-file write restriction' "${PLAN}"; then
  pass "PIR5 keeps pointer compression opt-in, records release support boundaries, and measures retained-RSS impact"
else
  fail "PIR5 pointer-compression closeout is not explicit" \
    "ptrcomp_rows=${retained_density_ptrcomp_rows}; $(cat /tmp/pir5-pointer-compression-missing.txt 2>/dev/null) $(cat /tmp/pir5-pointer-compression-trace-missing.txt 2>/dev/null) $(cat /tmp/pir5-pointer-compression-patch-missing.txt 2>/dev/null) $(cat /tmp/pir5-pointer-compression-hotfix-patch-missing.txt 2>/dev/null) $(cat /tmp/pir5-pointer-compression-job-patch-missing.txt 2>/dev/null) $(cat /tmp/pir5-pointer-compression-matrix-fix-patch-missing.txt 2>/dev/null) $(cat /tmp/pir5-pointer-compression-linux-arm-release-patch-missing.txt 2>/dev/null)"
fi

step 43 "PIR3 native and bootstrap side-channel hardening is wired"
if [ -f "${RUNTIME_CONSTRUCTION}" ] &&
  [ -f "${RUNTIME_BOOTSTRAP_SOURCE}" ] &&
  [ -f "${RUNTIME_V8_EMBEDDER}" ] &&
  [ -f "${PIR3_PROOF}" ] &&
  contains 'shared_array_buffer_store: Some\(SharedArrayBufferStore::default\(\)\)' "${RUNTIME_CONSTRUCTION}" &&
  contains 'allow_atomics_wait\(false\)' "${RUNTIME_CONSTRUCTION}" &&
  contains_all "${RUNTIME_BOOTSTRAP_SOURCE}" \
    'NIMBUS_SIDE_CHANNEL_HARDENING_SOURCE' \
    '__nimbusTimerResolutionMs = 10' \
    '__nimbusCoarsenTimerValue' \
    '__nimbusInstallDateNowCoarsening' \
    '__nimbusInstallPerformanceNowCoarsening' \
    '__nimbusDisableBlockingAtomicsWait' \
    '__nimbusHideSharedArrayBuffer' \
    '__nimbusDisableSharedWebAssemblyMemory' \
    '__nimbusInstallSideChannelHardening' \
    'Atomics.waitAsync' \
    'SharedArrayBuffer' \
    'WebAssembly.Memory' \
    '<nimbus-runtime:bootstrap:side-channel-hardening>' \
    'pir3_side_channel_hardening_source_coarsens_timers_and_removes_shared_memory' >/tmp/pir3-bootstrap-missing.txt &&
  contains 'SharedArrayBufferStore' "${RUNTIME_V8_EMBEDDER}" &&
  contains 'embedder-owned transfer' "${PIR3_PROOF}" &&
  contains 'backing store' "${PIR3_PROOF}" &&
  contains 'SharedArrayBuffer remains hidden from user code' "${PIR3_PROOF}" &&
  contains '1226 filtered out' "${PIR3_PROOF}"; then
  pass "runtime construction disables Atomics.wait, hides SAB from user code, and keeps the embedder backing store policy-scoped"
else
  fail "PIR3 runtime hardening is incomplete" "$(cat /tmp/pir3-bootstrap-missing.txt 2>/dev/null)"
fi

step 44 "PIR3 Web, Node, and worker side-channel probes are present"
if [ -f "${PIR3_SIDE_CHANNEL_TEST}" ] &&
  contains 'mod side_channel;' "crates/nimbus-runtime/src/runtime/tests/basic_invocation.rs" &&
  contains_all "${PIR3_SIDE_CHANNEL_TEST}" \
    'pir3_web_standard_side_channel_surface_is_hardened' \
    'pir3_node_targets_side_channel_surface_is_hardened' \
    'pir3_node_worker_thread_side_channel_surface_is_hardened' \
    'pir3_web_standard_side_channel_surface_is_hardened_subprocess' \
    'pir3_node_targets_side_channel_surface_is_hardened_subprocess' \
    'pir3_node_worker_thread_side_channel_surface_is_hardened_subprocess' \
    'IsolatedRuntimeTestCase::new' \
    'run_v8_sensitive_runtime_test_in_subprocess' \
    'mixed-profile V8 snapshot external-reference state' \
    'RuntimeLimits::application_web_standard\(\)' \
    'RuntimeLimits::application_node20\(\)' \
    'RuntimeLimits::application_node22\(\)' \
    'RuntimeLimits::application_node24\(\)' \
    'RuntimeLimits::application_node26\(\)' \
    'RuntimeLimits::application_node22_local_development\(\)' \
    'SharedArrayBuffer' \
    'wasmPlainMemory' \
    'wasmSharedMemory' \
    '\[object ArrayBuffer\]' \
    'Nimbus disables shared WebAssembly memory' \
    'Atomics.wait' \
    'Atomics.waitAsync' \
    'Date.now' \
    'performance.now' \
    'Nimbus disables Atomics.wait' \
    'Nimbus disables Atomics.waitAsync' \
    'should be coarsened to 10ms buckets' >/tmp/pir3-test-missing.txt; then
  pass "PIR3 focused tests cover WebStandard, Node20/22/24/26, and the Node worker-thread surface"
else
  fail "PIR3 focused side-channel tests are incomplete" "$(cat /tmp/pir3-test-missing.txt 2>/dev/null)"
fi

step 45 "PIR3 proof records exemplar comparison, threat model, and focused verification"
if [ -f "${PIR3_PROOF}" ] &&
  contains_all "${PIR3_PROOF}" \
    'PIR3 status: complete' \
    'set_allow_atomics_wait\(false\)' \
    'Cloudflare workerd' \
    'Convex' \
    'Dynamic Process Isolation' \
    'process-bulkhead' \
    'PIR8/microVM' \
    'same-customer' \
    'operator-approved trust domains' \
    'mutually untrusted cross-customer' \
    'Timer/SAB/Atomics mitigations are not' \
    'run_v8_sensitive_runtime_test_in_subprocess' \
    'WebStandard/Node startup-snapshot probes' \
    'cargo test -p nimbus-runtime pir3_ --lib -- --nocapture' \
    '4 passed; 0 failed; 3 ignored; 0 measured; 989 filtered out' \
    'pir3_side_channel_hardening_source_coarsens_timers_and_removes_shared_memory' \
    'pir3_web_standard_side_channel_surface_is_hardened' \
    'pir3_node_targets_side_channel_surface_is_hardened' \
    'pir3_node_worker_thread_side_channel_surface_is_hardened' \
    'before PIR4 can claim untrusted multiplexing readiness' >/tmp/pir3-proof-missing.txt; then
  pass "PIR3 proof scopes in-process mitigation to approved trust domains and records exact test evidence"
else
  fail "PIR3 proof artifact is incomplete" "$(cat /tmp/pir3-proof-missing.txt 2>/dev/null)"
fi

step 46 "PIR3 plan closeout preserves history and current PIR4 reroute"
if [ -f "${PLAN}" ] &&
  contains 'PIR3 side-channel posture closeout' "${PLAN}" &&
  contains 'docs/private/plans/proof/profile-aware-isolate-runtime/pir3-side-channel.md' "${PLAN}" &&
  contains 'PIR3 is `done`' "${PLAN}" &&
  contains 'PIR2 is now `in_progress`' "${PLAN}" &&
  contains 'PIR2 is now the single' "${PLAN}" &&
  contains 'timer/SAB/Atomics mitigations are not sufficient alone' "${PLAN}" &&
  contains 'process-bulkhead/Dynamic Process Isolation control or PIR8/microVM placement' "${PLAN}" &&
  contains 'subprocess-isolated test pattern' "${PLAN}" &&
  contains 'mixed-profile snapshot/external-reference state' "${PLAN}" &&
  contains '4 passed, 0 failed, 3 ignored, 0 measured, 989 filtered out' "${PLAN}" &&
  contains 'PIR2 closeout/reroute slice' "${PLAN}" &&
  contains 'PIR4 is now the single active band' "${PLAN}"; then
  pass "plan preserves PIR3-to-PIR2 history and records the current PIR4-active reroute"
else
  fail "PIR3/PIR2 plan closeout state is incomplete"
fi

step 47 "PIR2 cooperative scheduler defers admissions behind parked slots"
if [ -f "${COOPERATIVE_WORKER_LOOP}" ] &&
  [ -f "${COOPERATIVE_WORKER_RUN}" ] &&
  [ -f "${COOPERATIVE_WORKER_EXECUTION}" ] &&
  [ -f "${EXECUTOR_ADMISSION_PERMIT}" ] &&
  [ -f "${EXECUTOR_ADMISSION_DISPATCH}" ] &&
  contains_all "${COOPERATIVE_WORKER_LOOP}" \
    'pending_admissions: VecDeque<RuntimeWorkerJob>' \
    'pending_admissions: VecDeque::new' >/tmp/pir2-loop-missing.txt &&
  contains_all "${COOPERATIVE_WORKER_RUN}" \
    'drain_pending_admissions' \
    'next_admission_job' \
    'self.scheduler.has_parked\(\)' \
    'self.try_admit_job' \
    'pending_admissions.push_front' \
    'queue.complete_job\(job, Err\(NimbusRuntimeError::Cancelled\), Vec::new\(\)\)' >/tmp/pir2-run-missing.txt &&
  contains_all "${COOPERATIVE_WORKER_EXECUTION}" \
    'try_admit_job' \
    'admit_job_inner' \
    'allow_blocking_acquire: bool' \
    'SharedInvocationPermitAcquire::WouldBlock' \
    'runtime worker deferred admission behind parked cooperative slots' \
    'debug_assert!' >/tmp/pir2-execution-missing.txt &&
  contains_all "${EXECUTOR_ADMISSION_PERMIT}" \
    'pub\(crate\) enum SharedInvocationPermitAcquire' \
    'Acquired' \
    'WouldBlock' \
    'try_acquire_initial' \
    'try_acquire_owned' \
    'record_invocation_started_for_tenant' >/tmp/pir2-permit-missing.txt &&
  contains_all "${EXECUTOR_ADMISSION_DISPATCH}" \
    'try_acquire_active_permit' \
    'try_acquire_owned' >/tmp/pir2-dispatch-missing.txt; then
  pass "PIR2 cooperative worker avoids blocking fresh admissions while parked slots can hold capacity"
else
  fail "PIR2 cooperative scheduler unblock implementation is incomplete" \
    "$(cat /tmp/pir2-loop-missing.txt 2>/dev/null) $(cat /tmp/pir2-run-missing.txt 2>/dev/null) $(cat /tmp/pir2-execution-missing.txt 2>/dev/null) $(cat /tmp/pir2-permit-missing.txt 2>/dev/null) $(cat /tmp/pir2-dispatch-missing.txt 2>/dev/null)"
fi

step 48 "PIR2 regression tests cover synthetic await warm-pool reuse and nonblocking permits"
if [ -f "${COOPERATIVE_EXECUTOR_TEST}" ] &&
  [ -f "${EXECUTOR_TEST_SUPPORT}" ] &&
  contains_all "${COOPERATIVE_EXECUTOR_TEST}" \
    'cooperative_warm_pool_handles_synthetic_await_four_tenants' \
    'const BATCHES: usize = 16' \
    'max_concurrent_runtime_instances = 1' \
    'worker_threads = 1' \
    'max_warm_reuses = 1_000_000' \
    'SyntheticAwaitHost::new\(Duration::ZERO\)' \
    'worker_dispatched_invocations' \
    'warm_pool_hits' \
    'retained_runtime_pool_entries' >/tmp/pir2-cooperative-test-missing.txt &&
  contains_all "${EXECUTOR_TEST_SUPPORT}" \
    'struct SyntheticAwaitHost' \
    'tokio::time::sleep\(delay\).await' \
    'synthetic-await host expects async db.get path' >/tmp/pir2-support-missing.txt &&
  contains_all "${EXECUTOR_ADMISSION_PERMIT}" \
    'try_acquire_initial_would_block_without_starting_invocation' \
    'try_acquire_initial_would_block_on_tenant_active_limit_without_metrics_leak' \
    'active_runtime_instances, 0' \
    'started_invocations, 0' \
    'completed_invocations, 0' >/tmp/pir2-permit-test-missing.txt; then
  pass "PIR2 tests cover the deadlock regression and metrics-clean WouldBlock paths"
else
  fail "PIR2 cooperative regression tests are incomplete" \
    "$(cat /tmp/pir2-cooperative-test-missing.txt 2>/dev/null) $(cat /tmp/pir2-support-missing.txt 2>/dev/null) $(cat /tmp/pir2-permit-test-missing.txt 2>/dev/null)"
fi

step 49 "PIR2 synthetic-await trace records warm-pool reuse under cooperative locker"
pir2_synthetic_await_rows=0
if [ -f "${PIR2_SYNTHETIC_AWAIT_TRACE}" ]; then
  pir2_synthetic_await_rows="$(grep -c '"benchmark_id":"web_standard/await_0ms/cooperative_locker_four_tenants/warm_pool"' "${PIR2_SYNTHETIC_AWAIT_TRACE}" || true)"
fi
if [ "${pir2_synthetic_await_rows}" -ge 8 ] &&
  contains_all "${PIR2_SYNTHETIC_AWAIT_TRACE}" \
    '"schema":"nimbus.profile_aware_isolate_runtime.pir0.trace.v1"' \
    '"benchmark_group":"runtime_pool_modes_pir0_synthetic_await_matrix"' \
    '"benchmark_id":"web_standard/await_0ms/cooperative_locker_four_tenants/warm_pool"' \
    '"tenant_count":4' \
    '"measured_iterations":128' \
    '"total_invocations":516' \
    '"bundle_loads":4' \
    '"runtime_pool_hits":512' \
    '"runtime_pool_misses":4' \
    '"warm_pool_hits":512' \
    '"warm_pool_misses":4' \
    '"retained_runtime_pool_entries":4' \
    '"retained_runtime_pool_evictions":0' \
    '"retained_runtime_pool_retirements":0' >/tmp/pir2-trace-missing.txt; then
  pass "PIR2 synthetic-await trace proves retained warm-pool reuse without eviction or retirement"
else
  fail "PIR2 synthetic-await warm-pool trace is incomplete" \
    "rows=${pir2_synthetic_await_rows}; missing: $(cat /tmp/pir2-trace-missing.txt 2>/dev/null)"
fi

step 50 "PIR2 proof records root cause, release-mode guard, benchmark evidence, and remaining work"
if [ -f "${PIR2_SYNTHETIC_AWAIT_PROOF}" ] &&
  contains_all "${PIR2_SYNTHETIC_AWAIT_PROOF}" \
    'PIR2 status: in progress' \
    'cooperative synthetic-await warm/reuse lane is unblocked' \
    'parked invocation can' \
    'reacquire the runtime semaphore' \
    'SharedInvocationPermitAcquire::\{Acquired, WouldBlock\}' \
    'pending_admissions' \
    'debug_assert!' \
    'cargo test -p nimbus-runtime executor::tests::cooperative --lib -- --nocapture' \
    '5 passed; 0 failed; 0 ignored; 0 measured; 994 filtered out' \
    'cargo test -p nimbus-runtime try_acquire_initial --lib -- --nocapture' \
    '2 passed; 0 failed; 0 ignored; 0 measured; 997 filtered out' \
    'cargo test -p nimbus-runtime executor::tests::queue_fairness --lib -- --nocapture' \
    '6 passed; 0 failed; 0 ignored; 0 measured; 993 filtered out' \
    'cooperative_warm_pool_handles_synthetic_await_four_tenants --lib --release' \
    'cooperative_execution_model_processes_worker_invocations --lib --release' \
    'NIMBUS_PIR0_INCLUDE_BLOCKED_AWAIT_ROWS=1' \
    'NIMBUS_PIR0_TRACE_PATH=docs/private/plans/proof/profile-aware-isolate-runtime/artifacts/pir2-synthetic-await-warm-pool-trace.jsonl' \
    'Collecting 10 samples' \
    '220 iterations' \
    '5\.3164 ms 5\.3863 ms 5\.4935 ms' \
    'measured_iterations=128' \
    'total_invocations=516' \
    'warm_pool_hits=512' \
    'warm_pool_misses=4' \
    'retained_runtime_pool_entries=4' \
    'PIR2 remains in progress' \
    'context-recycling substrate' \
    'authority partition safety' >/tmp/pir2-proof-missing.txt &&
  contains_all "${PLAN}" \
    'pir2-synthetic-await-warm-pool.md' \
    'cooperative synthetic-await warm/reuse lane unblocked' \
    'PIR2 remains `in_progress`' \
    'do not promote PIR4' >/tmp/pir2-plan-missing.txt; then
  pass "PIR2 proof and plan record the unblock without claiming full context-recycling completion"
else
  fail "PIR2 cooperative warm-pool proof is incomplete" \
    "$(cat /tmp/pir2-proof-missing.txt 2>/dev/null) $(cat /tmp/pir2-plan-missing.txt 2>/dev/null)"
fi

step 51 "PIR2 warm-pool reuse preserves exact affinity in the authority key"
if [ -f "${WARM_POOL}" ] &&
  contains_all "${WARM_POOL}" \
    'fn matches_reusable_entry' \
    'self.matches_exact\(other\)' \
    'retained runtime must not cross tenant/function/script affinity' \
    'reusable_partition_key_preserves_tenant_affinity_for_unscoped_bundle' \
    'reusable_partition_key_preserves_function_affinity_for_unscoped_bundle' \
    'same unscoped bundle and limits must not cross tenant affinity' \
    'same unscoped bundle and limits must not cross function affinity' >/tmp/pir2-authority-code-missing.txt &&
  ! contains 'matches_bundle_and_runtime_shape' "${WARM_POOL}" &&
  ! contains 'same bundle identity and runtime shape with any affinity' "${WARM_POOL}"; then
  pass "PIR2 warm-pool reuse no longer falls back across tenant/function/script affinity"
else
  fail "PIR2 warm-pool authority key still permits cross-affinity reuse" \
    "$(cat /tmp/pir2-authority-code-missing.txt 2>/dev/null)"
fi

step 52 "PIR2 authority partition proof records the fallback removal and focused tests"
if [ -f "${PIR2_AUTHORITY_PARTITION_PROOF}" ] &&
  contains_all "${PIR2_AUTHORITY_PARTITION_PROOF}" \
    'PIR2 status: in progress' \
    'same bundle' \
    'runtime shape with any affinity' \
    'not acceptable for PIR2' \
    'tenant/function/script affinity' \
    'RuntimePoolPartitionKey::matches_reusable_entry' \
    'delegates to exact-key' \
    'equality only' \
    'V8WorkerRuntimePool::take_warm_pool_entry' \
    'unscoped bundles still partition' \
    'by function affinity' \
    'cargo test -p nimbus-runtime reusable_partition_key --lib -- --nocapture' \
    '2 passed; 0 failed; 0 ignored; 0 measured; 1001 filtered out' \
    'cargo test -p nimbus-runtime runtime::tests::warm_pool --lib -- --nocapture' \
    '12 passed; 0 failed; 8 ignored; 0 measured; 983 filtered out' \
    'WarmContextRecycle Authority Parity Slice' \
    'script/permission-profile' \
    'Dirty,' \
    'failed-cleanliness' >/tmp/pir2-authority-proof-missing.txt &&
  contains_all "${PLAN}" \
    'pir2-authority-partition.md' \
    'warm-pool affinity fallback removed' \
    'matches_reusable_entry' \
    'future context-recycling cache' \
    'same exact authority boundary' >/tmp/pir2-authority-plan-missing.txt; then
  pass "PIR2 authority partition proof and plan record exact-affinity reuse as a prerequisite for context recycling"
else
  fail "PIR2 authority partition proof is incomplete" \
    "$(cat /tmp/pir2-authority-proof-missing.txt 2>/dev/null) $(cat /tmp/pir2-authority-plan-missing.txt 2>/dev/null)"
fi

step 53 "PIR2 runtime reuse lifecycle is explicit and wired through return/discard paths"
if [ -f "${V8_LIFECYCLE}" ] &&
  [ -f "${WARM_POOL}" ] &&
  [ -f "${RUNTIME_INVOCATION_DRIVER}" ] &&
  [ -f "${COOPERATIVE_RETENTION}" ] &&
  contains_all "${V8_LIFECYCLE}" \
    'enum RuntimeReuseLifecycleState' \
    'Cold' \
    'Bootstrapping' \
    'Ready' \
    'Leased' \
    'Draining' \
    'CleanReturn' \
    'DirtyDiscard' \
    'Condemned' \
    'struct RuntimeReuseLifecycle' \
    'bootstrapped_and_leased' \
    'mark_clean_return' \
    'mark_dirty_discard' \
    'mark_condemned' \
    'runtime_reuse_lifecycle_tracks_clean_return_to_ready' \
    'runtime_reuse_lifecycle_records_terminal_discard_states' >/tmp/pir2-lifecycle-module-missing.txt &&
  contains_all "${WARM_POOL}" \
    'lifecycle: RuntimeReuseLifecycle' \
    'RuntimeReuseLifecycle::bootstrapped_and_leased' \
    'lifecycle.mark_leased' \
    'runtime.lifecycle.mark_draining' \
    'runtime.lifecycle.mark_clean_return' \
    'runtime.lifecycle.mark_dirty_discard' \
    'runtime.lifecycle.mark_condemned' \
    'warm_runtime_condemnation_is_dirty_discard' \
    'last_lifecycle_state_for_test' \
    'last_lifecycle_history_for_test' >/tmp/pir2-lifecycle-warm-pool-missing.txt &&
  contains_all "${RUNTIME_INVOCATION_DRIVER}" \
    'lifecycle: crate::backends::v8::RuntimeReuseLifecycle' \
    'self.lifecycle.mark_condemned' >/tmp/pir2-lifecycle-driver-missing.txt &&
  contains_all "${COOPERATIVE_RETENTION}" \
    'return_runtime_for_invocation' >/tmp/pir2-lifecycle-retention-missing.txt &&
  ! contains 'runtime.mark_dirty_discard' "${RUNTIME_INVOCATION_DRIVER}" &&
  ! contains 'runtime.mark_dirty_discard' "${COOPERATIVE_RETENTION}" &&
  contains_all "${WARM_POOL_TEST}" \
    'RuntimeReuseLifecycleState' \
    'last_lifecycle_state_for_test' \
    'last_lifecycle_history_for_test' \
    'Cold' \
    'Bootstrapping' \
    'CleanReturn' >/tmp/pir2-lifecycle-test-missing.txt; then
  pass "PIR2 lifecycle state is runtime-owned and distinguishes clean return, dirty discard, and condemnation"
else
  fail "PIR2 runtime reuse lifecycle wiring is incomplete" \
    "$(cat /tmp/pir2-lifecycle-module-missing.txt 2>/dev/null) $(cat /tmp/pir2-lifecycle-warm-pool-missing.txt 2>/dev/null) $(cat /tmp/pir2-lifecycle-driver-missing.txt 2>/dev/null) $(cat /tmp/pir2-lifecycle-retention-missing.txt 2>/dev/null) $(cat /tmp/pir2-lifecycle-test-missing.txt 2>/dev/null)"
fi

step 54 "PIR2 lifecycle proof records exact commands and remaining context-recycling gates"
if [ -f "${PIR2_RUNTIME_LIFECYCLE_PROOF}" ] &&
  contains_all "${PIR2_RUNTIME_LIFECYCLE_PROOF}" \
    'PIR2 status: in progress' \
    'RuntimeReuseLifecycleState' \
    'Cold' \
    'Bootstrapping' \
    'Ready' \
    'Leased' \
    'Draining' \
    'CleanReturn' \
    'DirtyDiscard' \
    'Condemned' \
    'ReusableV8Runtime' \
    'Cleanliness failures mark `DirtyDiscard`' \
    'replacement-required' \
    'mark_condemned' \
    'cargo test -p nimbus-runtime runtime_reuse_lifecycle --lib -- --nocapture' \
    '2 passed; 0 failed; 0 ignored; 0 measured; 1001 filtered out' \
    'cargo test -p nimbus-runtime runtime::tests::warm_pool --lib -- --nocapture' \
    '12 passed; 0 failed; 8 ignored; 0 measured; 983 filtered out' \
    'Cold -> Bootstrapping -> Ready -> Leased -> Draining -> CleanReturn -> Ready' \
    'fresh' \
    'per-request/per-module context cache' \
    'failed cleanliness check' >/tmp/pir2-lifecycle-proof-missing.txt &&
  contains_all "${PLAN}" \
    'pir2-runtime-lifecycle.md' \
    'explicit runtime reuse lifecycle landed' \
    'context-recycling cache must consume this lifecycle' \
    'PIR2 remains `in_progress`' >/tmp/pir2-lifecycle-plan-missing.txt; then
  pass "PIR2 lifecycle proof and plan record runtime-owned state without claiming full context recycling"
else
  fail "PIR2 lifecycle proof is incomplete" \
    "$(cat /tmp/pir2-lifecycle-proof-missing.txt 2>/dev/null) $(cat /tmp/pir2-lifecycle-plan-missing.txt 2>/dev/null)"
fi

step 55 "PIR2 warm-runtime cleanliness gate is centralized before retention"
if [ -f "${V8_LIFECYCLE}" ] &&
  [ -f "${WARM_POOL}" ] &&
  [ -f "${RUNTIME_INVOCATION_DRIVER}" ] &&
  [ -f "${COOPERATIVE_RETENTION}" ] &&
  [ -f "${WARM_POOL_TEST}" ] &&
  contains_all "${V8_LIFECYCLE}" \
    'WarmRuntimeCleanlinessReport' \
    'warm_reuse_safe_before_reset' \
    'request_state_reset_succeeded' \
    'heap_after_maintenance' \
    'is_warm_reuse_safe' \
    'reset_request_state' \
    'number_of_native_contexts' \
    'number_of_detached_contexts' \
    'EventLoopNotQuiescent' \
    'RequestStateResetFailed' \
    'DetachedContextsPresent' \
    'heap_carryover_limit_bytes\(limits\)' \
    'runtime_cleanliness_report_records_context_counts_and_configured_limit' >/tmp/pir2-cleanliness-lifecycle-missing.txt &&
  contains_all "${WARM_POOL}" \
    'warm_runtime_condemnation_is_dirty_discard' \
    'record_warm_pool_discard_unquiesced' \
    'WarmRuntimeCondemnationReason::EventLoopNotQuiescent' \
    'WarmRuntimeCondemnationReason::RequestStateResetFailed' \
    'WarmRuntimeCondemnationReason::MaxWarmReusesExceeded' \
    'runtime.lifecycle.mark_dirty_discard' \
    'runtime.lifecycle.mark_condemned' >/tmp/pir2-cleanliness-warm-pool-missing.txt &&
  contains_all "${WARM_POOL_TEST}" \
    'warm_reuse_safe_before_reset' \
    'request_state_reset_succeeded' \
    'heap_after_maintenance' \
    'retained_memory_bytes' \
    'carryover_limit_bytes' \
    'detached_context_count' >/tmp/pir2-cleanliness-test-missing.txt &&
  ! contains 'reset_request_state\(\)' "${RUNTIME_INVOCATION_DRIVER}" &&
  ! contains 'reset_request_state\(\)' "${COOPERATIVE_RETENTION}"; then
  pass "PIR2 retention cleanup has one Deno/V8-backed cleanliness gate"
else
  fail "PIR2 warm-runtime cleanliness gate is incomplete" \
    "$(cat /tmp/pir2-cleanliness-lifecycle-missing.txt 2>/dev/null) $(cat /tmp/pir2-cleanliness-warm-pool-missing.txt 2>/dev/null) $(cat /tmp/pir2-cleanliness-test-missing.txt 2>/dev/null)"
fi

step 56 "PIR2 cleanliness proof records exact commands and remaining context-recycling gates"
if [ -f "${PIR2_CLEANLINESS_PROOF}" ] &&
  contains_all "${PIR2_CLEANLINESS_PROOF}" \
    'PIR2 status: in progress' \
    'prepare_warm_runtime_for_retention' \
    'Deno-backed quiescence check' \
    'is_warm_reuse_safe' \
    'reset_request_state' \
    'V8 context counters' \
    'number_of_detached_contexts' \
    'WarmRuntimeCleanlinessReport' \
    'EventLoopNotQuiescent' \
    'RequestStateResetFailed' \
    'DetachedContextsPresent' \
    'RuntimeLimits' \
    'cargo test -p nimbus-runtime runtime_cleanliness --lib -- --nocapture' \
    '1 passed; 0 failed; 0 ignored; 0 measured; 1003 filtered out' \
    'cargo test -p nimbus-runtime runtime::tests::warm_pool --lib -- --nocapture' \
    '12 passed; 0 failed; 8 ignored; 0 measured; 984 filtered out' \
    'PIR2 remains in progress' \
    'fresh-context recycling substrate' \
    'deno_core realm/context seam' >/tmp/pir2-cleanliness-proof-missing.txt &&
  contains_all "${PLAN}" \
    'pir2-cleanliness-gate.md' \
    'PIR2 cleanliness gate slice' \
    'Deno-backed quiescence' \
    'detached V8 contexts' \
    'PIR2 remains `in_progress`' >/tmp/pir2-cleanliness-plan-missing.txt; then
  pass "PIR2 cleanliness proof and plan record the gate without claiming context recycling complete"
else
  fail "PIR2 cleanliness proof is incomplete" \
    "$(cat /tmp/pir2-cleanliness-proof-missing.txt 2>/dev/null) $(cat /tmp/pir2-cleanliness-plan-missing.txt 2>/dev/null)"
fi

step 57 "PIR2 negative control proves current warm-module reuse is not context recycling"
if [ -f "${SNAPSHOT_LIFECYCLE_TEST}" ] &&
  contains_all "${SNAPSHOT_LIFECYCLE_TEST}" \
    'reused_runtime_still_leaks_user_module_state_after_current_resets' \
    '__userCounter' \
    'reset_runtime_invocation_state' \
    'reset_bootstrap_invocation_state' \
    'serde_json::json!\(\{ "counter": 2 \}\)' \
    'user-module/global state still persists on a reused loaded runtime' >/tmp/pir2-negative-control-missing.txt; then
  pass "PIR2 has a negative-control test preventing current warm-module reuse from being mistaken for fresh-context recycling"
else
  fail "PIR2 negative-control global-state leak test is missing" \
    "$(cat /tmp/pir2-negative-control-missing.txt 2>/dev/null)"
fi

step 58 "PIR2 deno_core realm seam proof records the published fork API"
if [ -f "${PIR2_DENO_REALM_SEAM_PROOF}" ] &&
  contains_all "${PIR2_DENO_REALM_SEAM_PROOF}" \
    'PIR2 status: in progress' \
    'v2.8.3-nimbus.76' \
    '9e1a1d96d8cef3d269fa9b1eb521364839bfc8ed' \
    'v149.4.0-nimbus.8' \
    'is_warm_reuse_safe' \
    'reset_request_state' \
    'pub fn create_realm' \
    'pub struct JsRealm' \
    'pub use crate::runtime::JsRealm' \
    'clone_for_realm' \
    'global_template_middlewares' \
    'load_main_es_module_in_realm' \
    'load_side_es_module_in_realm' \
    'mod_evaluate_in_realm' \
    'resolve_in_realm' \
    'poll_event_loop_in_realm' \
    'run_event_loop_in_realm' \
    'with_event_loop_promise_in_realm' \
    'create_realm_loads_modules_in_realm_module_map' \
    '1 passed; 0 failed; 0 ignored; 0 measured; 426 filtered out' \
    '6 passed; 0 failed; 0 ignored; 0 measured; 421 filtered out' \
    'current warm module reuse path' \
	    'reused_runtime_still_leaks_user_module_state_after_current_resets' \
	    '1 passed; 0 failed; 0 ignored; 0 measured; 1003 filtered out' \
	    'PIR2 remains in progress' \
	    'production WebStandard context recycling' \
	    'module graph semantics' \
	    'extension JavaScript' \
	    'explicit destroy' >/tmp/pir2-deno-seam-proof-missing.txt &&
  contains_all "${PLAN}" \
    'pir2-deno-realm-seam.md' \
    'Deno realm-seam API published' \
    'deno_core realm-seam API' \
    'v2.8.3-nimbus.76' \
    'fresh-context recycling substrate' \
    'PIR2 remains `in_progress`' \
    'production WebStandard context-recycling path' >/tmp/pir2-deno-seam-plan-missing.txt; then
  pass "PIR2 fork-seam proof records the published deno_core fresh-realm module/event-loop API"
else
  fail "PIR2 deno_core realm seam proof is incomplete" \
    "$(cat /tmp/pir2-deno-seam-proof-missing.txt 2>/dev/null) $(cat /tmp/pir2-deno-seam-plan-missing.txt 2>/dev/null)"
fi

step 59 "PIR2 WebStandard context recycling is wired behind WarmContextRecycle"
if [ -f "${RUNTIME_BOOTSTRAP_SOURCE}" ] &&
  [ -f "${POOL_REUSE_TEST}" ] &&
  [ -f "${PIR2_DENO_REALM_SEAM_PROOF}" ] &&
  contains 'JsRealm' "${RUNTIME_V8_EMBEDDER}" &&
  contains_all "${RUNTIME_BOOTSTRAP_SOURCE}" \
    'install_bootstrap_in_realm' \
    'finalize_bootstrap_in_realm' \
    'reset_bootstrap_invocation_state_in_realm' \
    'execute_realm_script' >/tmp/pir2-nimbus-realm-bootstrap-source-missing.txt &&
  contains_all "crates/nimbus-runtime/src/runtime/bootstrap/mod.rs" \
    'install_bootstrap_in_realm' \
    'finalize_bootstrap_in_realm' \
    'reset_bootstrap_invocation_state_in_realm' >/tmp/pir2-nimbus-realm-bootstrap-mod-missing.txt &&
  contains_all "crates/nimbus-runtime/src/runtime/driver/loading.rs" \
    'start_fresh_realm_bundle_invocation_with_trace' \
    'load_main_es_module_in_realm' \
    'load_side_es_module_in_realm' \
    'mod_evaluate_in_realm' \
    'run_event_loop_in_realm' \
    'resolve_in_realm' \
    'with_event_loop_promise_in_realm' >/tmp/pir2-nimbus-context-recycle-loading-missing.txt &&
  contains_all "crates/nimbus-runtime/src/runtime/cooperative.rs" \
    'poll_event_loop_in_realm' \
    'resolve_in_realm' \
    'destroy_fresh_realm' >/tmp/pir2-nimbus-context-recycle-cooperative-missing.txt &&
  contains_all "crates/nimbus-runtime/src/limits/axes.rs" \
    'WarmContextRecycle' \
    'RuntimeNodeFullRealmReusePolicy::SameOwnerExactAuthority' \
    'same-owner exact-authority realm reuse proof' >/tmp/pir2-nimbus-context-recycle-policy-missing.txt &&
  contains_all "${POOL_REUSE_TEST}" \
    'fresh_realm_installs_bootstrap_and_uses_bound_host_bridge' \
    'warm_context_recycle_reuses_runtime_with_fresh_realm_module_state' \
    'create_realm\(Default::default\(\)\)' \
    'install_bootstrap_in_realm' \
    'finalize_bootstrap_in_realm' \
    'reset_bootstrap_invocation_state_in_realm' \
    '__freshRealmMarker' \
    'query:messages:get' \
    'entryLoadCount' \
    'dependencyLoadCount' \
    'query:messages:first' \
    'query:messages:second' \
    'runtime_pool_misses' \
    'runtime_pool_hits' >/tmp/pir2-nimbus-realm-bootstrap-test-missing.txt &&
  contains_all "${PIR2_DENO_REALM_SEAM_PROOF}" \
    'Nimbus Bootstrap Seam' \
    'Production WebStandard Context Recycling' \
    'fresh_realm_installs_bootstrap_and_uses_bound_host_bridge' \
    'warm_context_recycle_reuses_runtime_with_fresh_realm_module_state' \
    '1 passed; 0 failed; 0 ignored; 0 measured; 1007 filtered out' \
    '7 passed; 0 failed; 1 ignored; 0 measured; 1008 filtered out' \
    'host_call_session_id = "query:messages:get"' \
    'query:messages:first' \
    'query:messages:second' \
    'realm-local forged prefix' \
    'dispatch' \
    'retained runtime main context' >/tmp/pir2-nimbus-realm-bootstrap-proof-missing.txt &&
  contains_all "${PLAN}" \
    'PIR2 fresh-realm bootstrap seam' \
    'PIR2 WebStandard context-recycling production slice' \
    'install_bootstrap_in_realm' \
    'fresh_realm_installs_bootstrap_and_uses_bound_host_bridge' \
    'warm_context_recycle_reuses_runtime_with_fresh_realm_module_state' \
    '1007 filtered out' \
    'production WebStandard context-recycling path' >/tmp/pir2-nimbus-realm-bootstrap-plan-missing.txt; then
  pass "PIR2 has a tested production WebStandard fresh-realm context-recycling path"
else
  fail "PIR2 Nimbus fresh-realm bootstrap seam is incomplete" \
    "$(cat /tmp/pir2-nimbus-realm-bootstrap-source-missing.txt 2>/dev/null) $(cat /tmp/pir2-nimbus-realm-bootstrap-mod-missing.txt 2>/dev/null) $(cat /tmp/pir2-nimbus-context-recycle-loading-missing.txt 2>/dev/null) $(cat /tmp/pir2-nimbus-context-recycle-cooperative-missing.txt 2>/dev/null) $(cat /tmp/pir2-nimbus-context-recycle-policy-missing.txt 2>/dev/null) $(cat /tmp/pir2-nimbus-realm-bootstrap-test-missing.txt 2>/dev/null) $(cat /tmp/pir2-nimbus-realm-bootstrap-proof-missing.txt 2>/dev/null) $(cat /tmp/pir2-nimbus-realm-bootstrap-plan-missing.txt 2>/dev/null)"
fi

step 60 "PIR2 fresh-realm teardown is proved on success, error, and early finish"
if [ -f "${RUNTIME_REALM_LIFECYCLE}" ] &&
  [ -f "${POOL_REUSE_TEST}" ] &&
  [ -f "crates/nimbus-runtime/src/runtime/tests/cooperative.rs" ] &&
  [ -f "${PIR2_DENO_REALM_SEAM_PROOF}" ] &&
  contains_all "${RUNTIME_REALM_LIFECYCLE}" \
    'pub\(crate\) fn destroy_fresh_realm' \
    'realm.destroy\(runtime.v8_isolate\(\)\)' \
    'FreshRealmDestroyProbe' \
    'start_fresh_realm_destroy_probe' \
    'record_fresh_realm_destroy' \
    'ThreadId' >/tmp/pir2-realm-lifecycle-helper-missing.txt &&
  contains_all "crates/nimbus-runtime/src/runtime/driver/loading.rs" \
    'destroy_fresh_realm\(runtime, realm\)' \
    'with_event_loop_promise_in_realm' >/tmp/pir2-realm-lifecycle-loading-missing.txt &&
  contains_all "crates/nimbus-runtime/src/runtime/cooperative.rs" \
    'destroy_fresh_realm\(&mut driver.runtime, realm\)' \
    'finish_with_result_and_runtime' \
    'poll_event_loop_in_realm' >/tmp/pir2-realm-lifecycle-cooperative-missing.txt &&
  contains_all "${POOL_REUSE_TEST}" \
    'fresh_realm_driver_destroys_realm_after_success_and_error' \
    'start_fresh_realm_destroy_probe' \
    'messages:fail' \
    'fresh realm failure' \
    'destroy_probe.count\(\)' >/tmp/pir2-realm-lifecycle-driver-test-missing.txt &&
  contains_all "crates/nimbus-runtime/src/runtime/tests/cooperative.rs" \
    'FRESH_REALM_EARLY_FINISH_CASE' \
    'warm_context_recycle_cooperative_slot_destroys_fresh_realm_on_early_finish' \
    'new Promise\(\(\) => \{\}\)' \
    'finish_with_result_and_runtime' \
    'NimbusRuntimeError::Cancelled' \
    'destroy_probe.count\(\)' >/tmp/pir2-realm-lifecycle-cooperative-test-missing.txt &&
  contains_all "${PIR2_DENO_REALM_SEAM_PROOF}" \
    'Nimbus Fresh Realm Teardown/Release Proof' \
    'runtime::realm_lifecycle::destroy_fresh_realm' \
    'FreshRealmDestroyProbe' \
    'fresh_realm_driver_destroys_realm_after_success_and_error' \
    '1 passed; 0 failed; 0 ignored; 0 measured; 1010 filtered out' \
    'warm_context_recycle_cooperative_slot_destroys_fresh_realm_on_early_finish' \
    '1 passed; 0 failed; 1 ignored; 0 measured; 1009 filtered out' \
    'probe count remains `0`' \
    'becomes `1`' \
    'serialized lifecycle proof' >/tmp/pir2-realm-lifecycle-proof-missing.txt &&
  contains_all "${PLAN}" \
    'PIR2 fresh-realm teardown/release proof slice' \
    'runtime::realm_lifecycle::destroy_fresh_realm' \
    'fresh_realm_driver_destroys_realm_after_success_and_error' \
    'warm_context_recycle_cooperative_slot_destroys_fresh_realm_on_early_finish' \
    '1010 filtered out' \
    '1009 filtered out' \
    'remaining gates are authority parity/fuzz coverage' >/tmp/pir2-realm-lifecycle-plan-missing.txt; then
  pass "PIR2 fresh realms are destroyed across direct success/error and cooperative early-finish paths"
else
  fail "PIR2 fresh-realm teardown proof is incomplete" \
    "$(cat /tmp/pir2-realm-lifecycle-helper-missing.txt 2>/dev/null) $(cat /tmp/pir2-realm-lifecycle-loading-missing.txt 2>/dev/null) $(cat /tmp/pir2-realm-lifecycle-cooperative-missing.txt 2>/dev/null) $(cat /tmp/pir2-realm-lifecycle-driver-test-missing.txt 2>/dev/null) $(cat /tmp/pir2-realm-lifecycle-cooperative-test-missing.txt 2>/dev/null) $(cat /tmp/pir2-realm-lifecycle-proof-missing.txt 2>/dev/null) $(cat /tmp/pir2-realm-lifecycle-plan-missing.txt 2>/dev/null)"
fi

step 61 "PIR2 WarmContextRecycle authority fuzz covers affinity, grants, profile, and construction mode"
if [ -f "${POOL_REUSE_TEST}" ] &&
  [ -f "${WARM_POOL}" ] &&
  [ -f "${PIR2_AUTHORITY_PARTITION_PROOF}" ] &&
  contains_all "${POOL_REUSE_TEST}" \
    'warm_context_recycle_preserves_tenant_affinity_for_unscoped_bundle' \
    'warm_context_recycle_preserves_function_affinity_for_unscoped_bundle' \
    'warm_context_recycle_preserves_script_affinity_for_distinct_bundle_entries' \
    'RuntimeRoutingAffinity::Function' \
    'RuntimeRoutingAffinity::Script' \
    'RuntimeInvocationContext::top_level_for_tenant' \
    'metrics.runtime_pool_misses, 2' \
    'metrics.runtime_pool_hits, 1' \
    'entryLoadCount' >/tmp/pir2-context-recycle-authority-tests-missing.txt &&
  contains_all "${WARM_POOL}" \
    'context_recycle_partition_key_preserves_exact_service_grants' \
    'context_recycle_partition_key_rejects_authority_dimension_fuzz_cases' \
    'RuntimePoolKind::WarmContextRecycle' \
    'context_recycle_restricted_limits' \
    'db_limits.grants.service = vec!\["db"' \
    'cache_limits.grants.service = vec!\["cache"' \
    'construction_mode' \
    'WarmContextRecycle must not reuse across authority dimensions' >/tmp/pir2-context-recycle-authority-key-missing.txt &&
  contains_all "${PIR2_AUTHORITY_PARTITION_PROOF}" \
    'WarmContextRecycle Authority Parity Slice' \
    'warm_context_recycle_preserves_tenant_affinity_for_unscoped_bundle' \
    'warm_context_recycle_preserves_function_affinity_for_unscoped_bundle' \
    'warm_context_recycle_preserves_script_affinity_for_distinct_bundle_entries' \
    'context_recycle_partition_key_preserves_exact_service_grants' \
    'context_recycle_partition_key_rejects_authority_dimension_fuzz_cases' \
    '3 passed; 0 failed; 0 ignored; 0 measured; 1013 filtered out' \
    '2 passed; 0 failed; 0 ignored; 0 measured; 1014 filtered out' \
    '7 passed; 0 failed; 1 ignored; 0 measured; 1008 filtered out' \
    'Mixed-profile state is recorded separately' >/tmp/pir2-context-recycle-authority-proof-missing.txt &&
  contains_all "${PLAN}" \
    'PIR2 WarmContextRecycle authority parity slice' \
    'warm_context_recycle_preserves_tenant_affinity_for_unscoped_bundle' \
    'warm_context_recycle_preserves_function_affinity_for_unscoped_bundle' \
    'warm_context_recycle_preserves_script_affinity_for_distinct_bundle_entries' \
    'context_recycle_partition_key_rejects_authority_dimension_fuzz_cases' \
    '1013 filtered out' \
    '1014 filtered out' \
    '1008 filtered out' \
    '`in_progress` for mixed-profile state and Node loader/extension replay proof' >/tmp/pir2-context-recycle-authority-plan-missing.txt; then
  pass "PIR2 production context recycling preserves affinity, grant, permission-profile, and construction-mode partitions"
else
  fail "PIR2 WarmContextRecycle authority parity proof is incomplete" \
    "$(cat /tmp/pir2-context-recycle-authority-tests-missing.txt 2>/dev/null) $(cat /tmp/pir2-context-recycle-authority-key-missing.txt 2>/dev/null) $(cat /tmp/pir2-context-recycle-authority-proof-missing.txt 2>/dev/null) $(cat /tmp/pir2-context-recycle-authority-plan-missing.txt 2>/dev/null)"
fi

step 62 "PIR2 mixed-profile startup-snapshot state is partitioned and Node realm reuse is exact-authority gated"
if [ -f "${PIR2_MIXED_PROFILE_PROOF}" ] &&
  [ -f "${RUNTIME_CONSTRUCTION}" ] &&
  [ -f "${V8_STARTUP_KEY}" ] &&
  [ -f "crates/nimbus-runtime/src/limits/axes.rs" ] &&
  [ -f "crates/nimbus-runtime/src/limits/tests.rs" ] &&
  [ -f "${PIR3_SIDE_CHANNEL_TEST}" ] &&
  contains_all "${RUNTIME_CONSTRUCTION}" \
    'static WEB_STANDARD_BOOTSTRAP_SNAPSHOT' \
    'static NODE_FULL_BOOTSTRAP_SNAPSHOT' \
    'RuntimeStartupSnapshotKey::for_limits' \
    'RuntimeStartupSnapshotKey::WebLean => &WEB_STANDARD_BOOTSTRAP_SNAPSHOT' \
    'RuntimeStartupSnapshotKey::NodeFull => &NODE_FULL_BOOTSTRAP_SNAPSHOT' \
    'snapshot_key.snapshot_build_target' \
    'Bun/JSC compatibility target cannot use the V8 bootstrap snapshot path' >/tmp/pir2-mixed-profile-construction-missing.txt &&
  contains_all "${V8_STARTUP_KEY}" \
    'pub\(crate\) enum RuntimeStartupSnapshotKey' \
    'WebLean' \
    'NodeFull' \
    'RuntimeProfile::for_compatibility_target' \
    'RuntimeCompatibilityTarget::Node22' \
    'startup_snapshot_key_collapses_node_majors_to_node_full' \
    'startup_snapshot_key_keeps_web_and_unsupported_targets_separate' >/tmp/pir2-mixed-profile-startup-key-missing.txt &&
  contains_all "crates/nimbus-runtime/src/limits/axes.rs" \
    'WarmContextRecycle' \
    'RuntimeNodeFullRealmReusePolicy::SameOwnerExactAuthority' \
    'same-owner exact-authority realm reuse proof' >/tmp/pir2-mixed-profile-policy-missing.txt &&
  contains_all "crates/nimbus-runtime/src/limits/tests.rs" \
    'runtime_policy_accepts_current_v8_javascript_axis_combinations' \
    'warm_context_recycle_rejects_node_without_same_owner_exact_authority_realm_reuse_proof' \
    'warm_context_recycle_accepts_node_with_same_owner_exact_authority_realm_reuse_proof' \
    'warm_context_recycle_rejects_run_to_completion' \
    'RuntimeModuleStateSemantics::FreshPerInvocation' >/tmp/pir2-mixed-profile-tests-missing.txt &&
  contains_all "${PIR3_SIDE_CHANNEL_TEST}" \
    'pir3_web_standard_side_channel_surface_is_hardened' \
    'pir3_node_targets_side_channel_surface_is_hardened' \
    'pir3_node_worker_thread_side_channel_surface_is_hardened' \
    'runs in a subprocess to isolate mixed-profile V8 snapshot external-reference state' >/tmp/pir2-mixed-profile-side-channel-missing.txt &&
  contains_all "${PIR2_MIXED_PROFILE_PROOF}" \
    'PIR2 mixed-profile startup snapshot state remains fail-closed' \
    'WEB_STANDARD_BOOTSTRAP_SNAPSHOT' \
    'NODE_FULL_BOOTSTRAP_SNAPSHOT' \
    'RuntimeStartupSnapshotKey' \
    'Node20/22/24/26 collapse to NodeFull only for immutable startup snapshots' \
    'Node targets require' \
    'RuntimeNodeFullRealmReusePolicy::SameOwnerExactAuthority' \
    'default' \
    'Unproven' \
    'same-owner exact-authority proof reason' \
    'NFR6 measured Node20/22/24/26' \
    'runs in a subprocess to isolate mixed-profile V8 snapshot external-reference state' \
    'runtime_policy_accepts_current_v8_javascript_axis_combinations' \
    'warm_context_recycle_rejects_node_without_same_owner_exact_authority_realm_reuse_proof' \
    'warm_context_recycle_accepts_node_with_same_owner_exact_authority_realm_reuse_proof' \
    'warm_context_recycle_rejects_run_to_completion' \
    '1 passed; 0 failed; 0 ignored; 0 measured; 1015 filtered out' \
    'V8 Node warm context recycling requires same-owner exact-authority realm reuse proof' \
    'The measured NodeFull outcome rejects adoption for this plan' >/tmp/pir2-mixed-profile-proof-missing.txt &&
  contains_all "${PLAN}" \
    'PIR2 mixed-profile startup-snapshot/external-reference' \
    'exact-authority proof slice' \
    'snapshot caches isolated' \
    'RuntimeNodeFullRealmReusePolicy::SameOwnerExactAuthority' \
    'runtime_policy_accepts_current_v8_javascript_axis_combinations' \
    'warm_context_recycle_rejects_node_without_same_owner_exact_authority_realm_reuse_proof' \
    'warm_context_recycle_rejects_run_to_completion' \
    'NFR6 measured Node20/22/24/26' >/tmp/pir2-mixed-profile-plan-missing.txt; then
  pass "PIR2 keeps mixed Web/Node state partitioned and Node realm reuse exact-authority gated"
else
  fail "PIR2 mixed-profile startup-snapshot proof is incomplete" \
    "$(cat /tmp/pir2-mixed-profile-construction-missing.txt 2>/dev/null) $(cat /tmp/pir2-mixed-profile-startup-key-missing.txt 2>/dev/null) $(cat /tmp/pir2-mixed-profile-policy-missing.txt 2>/dev/null) $(cat /tmp/pir2-mixed-profile-tests-missing.txt 2>/dev/null) $(cat /tmp/pir2-mixed-profile-side-channel-missing.txt 2>/dev/null) $(cat /tmp/pir2-mixed-profile-proof-missing.txt 2>/dev/null) $(cat /tmp/pir2-mixed-profile-plan-missing.txt 2>/dev/null)"
fi

step 63 "PIR2 Node context recycling is exact-authority gated and benchmark-rejected for adoption"
if [ -f "${PIR2_NODE_REALM_PROOF}" ] &&
  [ -f "${NFR6_NODE_FULL_PROOF}" ] &&
  [ -f "${RUNTIME_CONSTRUCTION}" ] &&
  [ -f "${BOOTSTRAP_EXTENSIONS}" ] &&
  [ -f "crates/nimbus-runtime/src/module_loader.rs" ] &&
  [ -f "crates/nimbus-runtime/src/limits/axes.rs" ] &&
  [ -f "crates/nimbus-runtime/src/runtime/tests/node/cases/nds3_cycle96_esm_async_loader_hooks.rs" ] &&
  contains_all "${RUNTIME_CONSTRUCTION}" \
    'LoaderHookRegistry::default' \
    'loader_hook_registry.clone' \
    'RestrictedModuleLoader::new' >/tmp/pir2-node-realm-construction-missing.txt &&
  contains_all "${BOOTSTRAP_EXTENSIONS}" \
    'struct NodeExecutionExtensionContext' \
    'build_node_init_services' \
    'loader_hook_registry: Option<LoaderHookRegistry>' \
    'loader_hook_registry_extension\(Some\(registry\)\)' \
    'fn snapshot_extension' \
    'fn execution_extension' >/tmp/pir2-node-realm-extensions-missing.txt &&
  contains_all "crates/nimbus-runtime/src/module_loader.rs" \
    'loader_hook_registry: Option<LoaderHookRegistry>' \
    'set_default_resolve' \
    'registry.resolve' \
    'push_load' \
    'take_resolved_attributes' \
    'module_type_from_hook_format' >/tmp/pir2-node-realm-module-loader-missing.txt &&
  contains_all "crates/nimbus-runtime/src/limits/axes.rs" \
    'WarmContextRecycle' \
    'RuntimeNodeFullRealmReusePolicy::SameOwnerExactAuthority' \
    'same-owner exact-authority realm reuse proof' >/tmp/pir2-node-realm-policy-missing.txt &&
  contains_all "crates/nimbus-runtime/src/runtime/tests/node/cases/nds3_cycle96_esm_async_loader_hooks.rs" \
    'node22_default_lane_executes_cycle96_esm_async_loader_hooks' \
    'node24_default_lane_executes_cycle96_esm_async_loader_hooks' \
    'test/es-module/test-esm-loader-mock.mjs' \
    'test/es-module/test-esm-virtual-json.mjs' >/tmp/pir2-node-realm-watchpoints-missing.txt &&
  contains_all "${PIR2_NODE_REALM_PROOF}" \
    'Node context recycling remains intentionally non-default in PIR2' \
    'RuntimeNodeFullRealmReusePolicy::Unproven' \
    'RuntimeNodeFullRealmReusePolicy::SameOwnerExactAuthority' \
    'NFR6 measured Node20/22/24/26' \
    'LoaderHookRegistry' \
    'node22_default_lane_executes_cycle96_esm_async_loader_hooks' \
    'node24_default_lane_executes_cycle96_esm_async_loader_hooks' \
    'selected=2, passed=2, skipped=0, failed=0' \
    'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1015 filtered out' \
    'warm_context_recycle_rejects_node_without_same_owner_exact_authority_realm_reuse_proof' \
    'V8 Node warm context recycling requires same-owner exact-authority realm reuse proof, got Unproven' \
    'NodeFull realm pooling for default/operator-facing adoption' >/tmp/pir2-node-realm-proof-missing.txt &&
  contains_all "${NFR6_NODE_FULL_PROOF}" \
    'NFR6 Benchmark And Adoption Decision' \
    'Status: `done`' \
    '5\.38x\.\.10\.01x' \
    '13\.35x\.\.16\.13x' \
    'do not make `WarmContextRecycle` the NodeFull default' >/tmp/pir2-node-nfr6-proof-missing.txt &&
  contains_all "${PLAN}" \
    'PIR2 Node realm boundary exact-authority proof slice' \
    'pir2-node-realm-boundary.md' \
    'selected=2, passed=2' \
    'RuntimeNodeFullRealmReusePolicy::SameOwnerExactAuthority' \
    'NFR6 measured Node20/22/24/26' \
    'rejected default/operator-facing adoption' >/tmp/pir2-node-realm-plan-missing.txt; then
  pass "PIR2 keeps Node context recycling exact-authority gated and rejected for default/operator adoption"
else
  fail "PIR2 Node realm boundary proof is incomplete" \
    "$(cat /tmp/pir2-node-realm-construction-missing.txt 2>/dev/null) $(cat /tmp/pir2-node-realm-extensions-missing.txt 2>/dev/null) $(cat /tmp/pir2-node-realm-module-loader-missing.txt 2>/dev/null) $(cat /tmp/pir2-node-realm-policy-missing.txt 2>/dev/null) $(cat /tmp/pir2-node-realm-watchpoints-missing.txt 2>/dev/null) $(cat /tmp/pir2-node-realm-proof-missing.txt 2>/dev/null) $(cat /tmp/pir2-node-nfr6-proof-missing.txt 2>/dev/null) $(cat /tmp/pir2-node-realm-plan-missing.txt 2>/dev/null)"
fi

step 64 "PIR2 context-recycle impact benchmark blocks promotion"
pir2_context_recycle_rows=0
if [ -f "${PIR2_CONTEXT_RECYCLE_IMPACT_TRACE}" ]; then
  pir2_context_recycle_rows="$(grep -c '"benchmark_group":"runtime_pool_modes_pir2_context_recycle_impact"' "${PIR2_CONTEXT_RECYCLE_IMPACT_TRACE}" || true)"
fi
if [ -f "${PIR2_CONTEXT_RECYCLE_IMPACT_PROOF}" ] &&
  [ -f "${PIR2_CONTEXT_RECYCLE_IMPACT_TRACE}" ] &&
  [ "${pir2_context_recycle_rows}" -ge 20 ] &&
  contains_all "${BENCH}" \
    'WarmContextRecycle' \
    'PoolMode::WarmContextRecycle' \
    'runtime_pool_modes_pir2_context_recycle_impact' \
    'PureJsWorkloadKind::HostlessTrivial' \
    'PureJsWorkloadKind::SetupHeavy' >/tmp/pir2-context-impact-bench-missing.txt &&
  contains_all "${PIR2_CONTEXT_RECYCLE_IMPACT_TRACE}" \
    '"benchmark_id":"web_standard/setup_heavy_large_module/cooperative_locker/startup_snapshot_cache"' \
    '"benchmark_id":"web_standard/setup_heavy_large_module/cooperative_locker/warm_pool"' \
    '"benchmark_id":"web_standard/setup_heavy_large_module/cooperative_locker/warm_context_recycle"' \
    '"benchmark_id":"web_standard/hostless_trivial/cooperative_locker/startup_snapshot_cache"' \
    '"benchmark_id":"web_standard/hostless_trivial/cooperative_locker/warm_pool"' \
    '"benchmark_id":"web_standard/hostless_trivial/cooperative_locker/warm_context_recycle"' \
    '"pool_kind":"warm_context_recycle"' \
    '"retained_runtime_pool_entries":1' \
    '"bundle_loads":129' >/tmp/pir2-context-impact-trace-missing.txt &&
  contains_all "${PIR2_CONTEXT_RECYCLE_IMPACT_PROOF}" \
    'PIR2 status: in progress; promotion is blocked by the current benchmark result' \
    'runtime_pool_modes_pir2_context_recycle_impact' \
    'Finished `bench` profile \[optimized\] target\(s\) in 5m 02s' \
    'setup-heavy | startup snapshot cache | 2\.3718 ms' \
    'setup-heavy | warm context recycle | 5\.2941 ms' \
    'hostless | startup snapshot cache | 1\.9633 ms' \
    'hostless | warm context recycle | 4\.9123 ms' \
    'phase-metrics cooperative_locker/warm_context_recycle: module_load=0\.153ms evaluation=0\.016ms bundle_total=0\.509ms' \
    'fresh-realm phase artifact' \
    'WarmContextRecycle` is 2\.23x the startup-snapshot median' \
    'WarmContextRecycle` is 2\.50x the startup-snapshot median' \
    'Do not mark PIR2 done and do not promote PIR4' \
    'fails the promotion criterion' >/tmp/pir2-context-impact-proof-missing.txt &&
  contains_all "${PLAN}" \
    'PIR2 context-recycle impact benchmark slice' \
    'WarmContextRecycle` slower than `startup_snapshot_cache`' \
    '2\.3718 ms' \
    '5\.2941 ms' \
    '1\.9633 ms' \
    '4\.9123 ms' \
    'fails the promotion criterion' \
    'do not promote PIR4' >/tmp/pir2-context-impact-plan-missing.txt; then
  pass "PIR2 records a measured negative context-recycle result and blocks promotion"
else
  fail "PIR2 context-recycle impact proof is incomplete" \
    "rows=${pir2_context_recycle_rows}; $(cat /tmp/pir2-context-impact-bench-missing.txt 2>/dev/null) $(cat /tmp/pir2-context-impact-trace-missing.txt 2>/dev/null) $(cat /tmp/pir2-context-impact-proof-missing.txt 2>/dev/null) $(cat /tmp/pir2-context-impact-plan-missing.txt 2>/dev/null)"
fi

step 65 "PIR2 fresh-realm phase attribution names create_realm as the blocker"
pir2_context_recycle_phase_rows=0
if [ -f "${PIR2_CONTEXT_RECYCLE_PHASE_TRACE}" ]; then
  pir2_context_recycle_phase_rows="$(grep -c '"benchmark_group":"runtime_pool_modes_pir2_context_recycle_impact"' "${PIR2_CONTEXT_RECYCLE_PHASE_TRACE}" || true)"
fi
if [ -f "${PIR2_CONTEXT_RECYCLE_IMPACT_PROOF}" ] &&
  [ -f "${PIR2_CONTEXT_RECYCLE_PHASE_TRACE}" ] &&
  [ "${pir2_context_recycle_phase_rows}" -ge 20 ] &&
  contains_all "${RUNTIME_METRICS}" \
    'fresh_realm_creates' \
    'record_fresh_realm_create' \
    'record_fresh_realm_bootstrap_install' \
    'record_fresh_realm_destroy' >/tmp/pir2-context-phase-metrics-missing.txt &&
  contains_all "${RUNTIME_METRICS_GLOBAL}" \
    'fresh_realm_create_nanos_total' \
    'fresh_realm_bootstrap_install_nanos_total' \
    'fresh_realm_invocation_script_nanos_total' \
    'fresh_realm_destroy_nanos_total' >/tmp/pir2-context-phase-global-missing.txt &&
  contains_all "${RUNTIME_LOADING}" \
    'record_fresh_realm_create' \
    'record_fresh_realm_bootstrap_install' \
    'record_fresh_realm_bootstrap_finalize' \
    'record_fresh_realm_bootstrap_reset' \
    'record_fresh_realm_invocation_script' \
    'record_fresh_realm_destroy' >/tmp/pir2-context-phase-loading-missing.txt &&
  contains_all "${RUNTIME_COOPERATIVE}" \
    'resolve_started_at' \
    'record_fresh_realm_promise_resolve' \
    'record_fresh_realm_deserialization' \
    'record_fresh_realm_destroy' >/tmp/pir2-context-phase-cooperative-missing.txt &&
  contains_all "${BENCH}" \
    'fresh_realm_create_nanos_total' \
    'realm_create=' \
    'fresh_realm_bootstrap_install_nanos_total' \
    'fresh_realm_destroy_nanos_total' >/tmp/pir2-context-phase-bench-missing.txt &&
  contains_all "${PIR2_CONTEXT_RECYCLE_PHASE_TRACE}" \
    '"benchmark_id":"web_standard/setup_heavy_large_module/cooperative_locker/warm_context_recycle"' \
    '"benchmark_id":"web_standard/hostless_trivial/cooperative_locker/warm_context_recycle"' \
    '"fresh_realm_create_nanos_total":' \
    '"fresh_realm_bootstrap_install_nanos_total":' \
    '"fresh_realm_invocation_script_nanos_total":' \
    '"fresh_realm_destroy_nanos_total":' >/tmp/pir2-context-phase-trace-missing.txt &&
  contains_all "${PIR2_CONTEXT_RECYCLE_IMPACT_PROOF}" \
    'PIR2 fresh-realm phase attribution slice conclusion: `create_realm` dominates' \
    'setup-heavy | 3\.054 ms | 0\.374 ms | 0\.160 ms' \
    'hostless | 3\.123 ms | 0\.372 ms | 0\.163 ms' \
    'Reducing bootstrap script replay' \
    'candidate must reduce realm creation itself' \
    'Deno Realm-Creation Source Audit' \
    'create_realm` passes `false`' \
    'Context::from_snapshot' \
    'SnapshotCreator::add_context' >/tmp/pir2-context-phase-proof-missing.txt &&
  contains_all "${PLAN}" \
    'fresh-realm phase attribution slice' \
    '`create_realm` dominates the gap' \
    '3\.054 ms for setup-heavy' \
    '3\.123 ms for hostless' \
    'Deno/rusty_v8 source audit confirms this is a Deno/V8 context-template' \
    'creation itself, introduce a measured per-context snapshot/realm template' >/tmp/pir2-context-phase-plan-missing.txt; then
  pass "PIR2 records diagnostic phase metrics and attributes the promotion blocker to create_realm"
else
  fail "PIR2 fresh-realm phase attribution proof is incomplete" \
    "rows=${pir2_context_recycle_phase_rows}; $(cat /tmp/pir2-context-phase-metrics-missing.txt 2>/dev/null) $(cat /tmp/pir2-context-phase-global-missing.txt 2>/dev/null) $(cat /tmp/pir2-context-phase-loading-missing.txt 2>/dev/null) $(cat /tmp/pir2-context-phase-cooperative-missing.txt 2>/dev/null) $(cat /tmp/pir2-context-phase-bench-missing.txt 2>/dev/null) $(cat /tmp/pir2-context-phase-trace-missing.txt 2>/dev/null) $(cat /tmp/pir2-context-phase-proof-missing.txt 2>/dev/null) $(cat /tmp/pir2-context-phase-plan-missing.txt 2>/dev/null)"
fi

step 66 "PIR2 closes by measured rejection and reroutes PIR4 to isolate-scoped multiplexing"
if [ -f "${PIR2_CLOSEOUT_REROUTE_PROOF}" ] &&
  contains_all "${PIR2_CLOSEOUT_REROUTE_PROOF}" \
    'PIR2 is closed for this release by deliberate architecture rejection' \
    '`WarmContextRecycle` may remain as an explicit internal/diagnostic pool kind' \
    'not a developer-facing tuning knob' \
    'PIR4 is unblocked only for the safe baseline' \
    'unit of multiplexing is the isolate' \
    'one request per context' \
    'mutations remain single-entry/run-to-completion' \
    'tenant principal, host-call session, and runtime authority are task/future or' \
    'PIR7 remains blocked on PIR4' \
    'context-from-snapshot proof' >/tmp/pir2-closeout-proof-missing.txt &&
  contains_all "${PLAN}" \
    '\| PIR2 \| `done`' \
    '\| PIR4 \| `done`' \
    'PIR2 closeout/reroute slice' \
    'PIR2 is `done`; PIR4 is now the single active band' \
    'PIR2 closed by rejecting context recycling as a default substrate for this release' \
    'PIR4 preserved PIR2'\''s safety gates while implementing isolate-scoped multiplexing only' \
    'context sharing is not part of the reroute' \
    'PIR7 closed with static measured defaults, host resource budget guardrails' >/tmp/pir2-closeout-plan-missing.txt; then
  pass "PIR2 is closed by measured rejection; PIR4 preserved isolate-scoped constraints"
else
  fail "PIR2 closeout/reroute proof is incomplete" \
    "$(cat /tmp/pir2-closeout-proof-missing.txt 2>/dev/null) $(cat /tmp/pir2-closeout-plan-missing.txt 2>/dev/null)"
fi

step 67 "PIR4 host-call session binding is task-scoped and fail-closed"
if [ -f "${RUNTIME_BOOTSTRAP_OPS_SHARED}" ] &&
  [ -f "${RUNTIME_BOOTSTRAP_OPS}" ] &&
  [ -f "${RUNTIME_BOOTSTRAP_SOURCE}" ] &&
  [ -f "crates/nimbus-runtime/src/runtime/bootstrap/state.rs" ] &&
  contains_all "crates/nimbus-runtime/src/runtime/bootstrap/state.rs" \
    'RuntimeInvocationHostCallBinding' \
    'fn for_context\(context: &RuntimeInvocationContext\)' \
    'format!\("\{\}:\{\}", context.kind, context.function_name\)' \
    'tenant_label: context.tenant_label.clone\(\)' \
    'RuntimeInvocationHostCallBinding::inactive' >/tmp/pir4-binding-state-missing.txt &&
  contains_all "${RUNTIME_BOOTSTRAP_OPS_SHARED}" \
    'op_nimbus_runtime_host_call_session_id' \
    'RuntimeInvocationHostCallBinding' \
    'enforce_live_host_call_session\(operation, &payload_value, &host_call_binding\)' \
    'fn enforce_live_host_call_session' \
    'operation_requires_host_call_session' \
    'runtime host-call session is stale or forged' \
    'expected_session' \
    'tenant' \
    'HostCallOperation::HttpRoute | HostCallOperation::RuntimeExtensionCall' >/tmp/pir4-binding-ops-shared-missing.txt &&
  contains_all "${RUNTIME_BOOTSTRAP_OPS}" \
    'op_nimbus_runtime_host_call_session_id' \
    'nimbus_runtime_ext' >/tmp/pir4-binding-ops-missing.txt &&
  contains_all "${RUNTIME_BOOTSTRAP_SOURCE}" \
    '__nimbusContextHostCallOps' \
    'op_nimbus_runtime_host_call_session_id' \
    '__nimbusCurrentHostCallSessionId' \
    '__nimbusBindHostCallPayload' \
    'Nimbus runtime host-call session is stale or forged for \$\{opName\}' \
    'host_call_session_id: currentSessionId' \
    '__nimbusCreateContext' >/tmp/pir4-binding-source-missing.txt; then
  pass "PIR4 host-call sessions are bound in OpState, added by bootstrap transport, and rechecked before host dispatch"
else
  fail "PIR4 host-call session binding is incomplete" \
    "$(cat /tmp/pir4-binding-state-missing.txt 2>/dev/null) $(cat /tmp/pir4-binding-ops-shared-missing.txt 2>/dev/null) $(cat /tmp/pir4-binding-ops-missing.txt 2>/dev/null) $(cat /tmp/pir4-binding-source-missing.txt 2>/dev/null)"
fi

step 68 "PIR4 mutations/actions bypass the cooperative read-safe scheduler"
if [ -f "${RUNTIME_INVOCATION_KIND}" ] &&
  [ -f "${REC_EXECUTION_PLAN}" ] &&
  [ -f "${COOPERATIVE_WORKER_RUN}" ] &&
  [ -f "${COOPERATIVE_WORKER_EXECUTION}" ] &&
  contains_all "${RUNTIME_INVOCATION_KIND}" \
    'is_convex_read_semantic_candidate' \
    'matches!\(self, Self::Query | Self::PaginatedQuery\)' >/tmp/pir4-invocation-kind-missing.txt &&
  contains_all "${REC_EXECUTION_PLAN}" \
    'permits_cooperative_scheduler_admission' \
    'is_convex_read_semantic_candidate' \
    'CooperativeIneligibilityReason::EffectfulKind' >/tmp/pir4-execution-plan-missing.txt &&
  contains_all "${COOPERATIVE_WORKER_RUN}" \
    'permits_cooperative_scheduler_admission' \
    '!self.scheduler.is_idle\(\)' \
    'pending_admissions.push_front\(job\)' \
    'self.scheduler.has_parked\(\)' >/tmp/pir4-worker-run-missing.txt &&
  contains_all "${COOPERATIVE_WORKER_EXECUTION}" \
    'enum CooperativeAdmissionStart' \
    'DirectResult' \
    'permits_cooperative_scheduler_admission' \
    'invoke_bundle_unmanaged\(Some\(v8_runtime_pool\), invocation\)' \
    'CooperativeAdmissionStart::DirectResult' \
    'queue.complete_job\(job, result, ready_jobs\)' >/tmp/pir4-worker-execution-missing.txt &&
  ! grep -R 'job.request.kind.is_convex_read_semantic_candidate' crates/nimbus-runtime/src/worker_loop >/dev/null 2>&1; then
  pass "PIR4 mutation/action exclusion is preserved through the REC execution-plan gate"
else
  fail "PIR4 mutation/action scheduler exclusion is incomplete" \
    "$(cat /tmp/pir4-invocation-kind-missing.txt 2>/dev/null) $(cat /tmp/pir4-execution-plan-missing.txt 2>/dev/null) $(cat /tmp/pir4-worker-run-missing.txt 2>/dev/null) $(cat /tmp/pir4-worker-execution-missing.txt 2>/dev/null)"
fi

step 69 "PIR4 focused tests cover forged sessions, interleaving, and mutation exclusion"
if [ -f "${RUNTIME_COOPERATIVE_TEST}" ] &&
  contains_all "${RUNTIME_COOPERATIVE_TEST}" \
    'PIR4_FORGED_HOST_CALL_SESSION_CASE' \
    'PIR4_INTERLEAVED_HOST_CALL_SESSION_CASE' \
    'PIR4_MUTATION_EXCLUSION_CASE' \
    'ImmediateRecordingAsyncHost' \
    'DeferredRecordingAsyncHost' \
    'MutationGateHost' \
    'pir4_rejects_forged_host_call_session' \
    'pir4_interleaved_queries_preserve_host_call_sessions' \
    'pir4_mutations_do_not_enter_multiplexed_read_safe_scheduler' \
    'forged-session' \
    'query:messages:first' \
    'query:messages:second' \
    'mutation:messages:write' \
    'query host work must not run while mutation is suspended' >/tmp/pir4-tests-missing.txt; then
  pass "PIR4 focused cooperative tests assert stale-session rejection, delayed query interleaving, and mutation exclusion"
else
  fail "PIR4 focused tests are incomplete" "$(cat /tmp/pir4-tests-missing.txt 2>/dev/null)"
fi

step 70 "PIR4 proof and plan record exact verification for this slice"
if [ -f "${PIR4_PROOF}" ] &&
  contains_all "${PIR4_PROOF}" \
    'PIR4 status: done' \
    'RuntimeInvocationHostCallBinding' \
    'op_nimbus_runtime_host_call_session_id' \
    '__nimbusBindHostCallPayload' \
    'enforce_live_host_call_session' \
    'InvocationKind::is_convex_read_semantic_candidate' \
    'DirectResult' \
    'cargo test -p nimbus-runtime --lib --no-run' \
    'Finished `test` profile \[unoptimized \+ debuginfo\] target\(s\) in 18\.00s' \
    'cargo test -p nimbus-runtime pir4_rejects_forged_host_call_session --lib -- --nocapture' \
    'cargo test -p nimbus-runtime pir4_interleaved_queries_preserve_host_call_sessions --lib -- --nocapture' \
    'cargo test -p nimbus-runtime pir4_mutations_do_not_enter_multiplexed_read_safe_scheduler --lib -- --nocapture' \
    'cargo test -p nimbus-runtime runtime::tests::cooperative --lib -- --nocapture' \
    '8 passed; 0 failed; 8 ignored; 0 measured; 1013 filtered out' \
    'cargo check -p nimbus-runtime --lib' \
    'bash scripts/verify-profile-aware-isolate-runtime.sh' \
    'Summary: 74 passed, 0 failed' \
    'cargo fmt --all --check' \
    'git diff --check' \
    '1 passed; 0 failed; 1 ignored; 0 measured; 1020 filtered out' \
    'Forged host-call sessions are rejected before host dispatch' \
    'DocumentInsert` with `mutation:messages:write` followed by `DocumentGet` with' \
    'system-wall enforcement' >/tmp/pir4-proof-missing.txt &&
  contains_all "${PLAN}" \
    'PIR4 host-call session binding and mutation-exclusion slice' \
    'pir4-host-call-session-binding.md' \
    'RuntimeInvocationHostCallBinding' \
    'mutations/actions bypass the cooperative read-safe scheduler' \
    'pir4_rejects_forged_host_call_session' \
    'pir4_interleaved_queries_preserve_host_call_sessions' \
    'pir4_mutations_do_not_enter_multiplexed_read_safe_scheduler' \
    'PIR4 remains `in_progress` for timeout/accounting and final band-closeout' >/tmp/pir4-plan-missing.txt; then
  pass "PIR4 proof records exact focused verification and leaves the remaining gates active"
else
  fail "PIR4 proof/plan evidence is incomplete" \
    "$(cat /tmp/pir4-proof-missing.txt 2>/dev/null) $(cat /tmp/pir4-plan-missing.txt 2>/dev/null)"
fi

step 71 "PIR4 user-timeout accounting pauses while async host work is in flight"
if [ -f "${EXECUTOR_ADMISSION_PERMIT}" ] &&
  contains_all "${EXECUTOR_ADMISSION_PERMIT}" \
    'pub\(crate\) async fn begin_async_host_call' \
    'timeout_controller.pause\(\).await' \
    'timeout_controller.resume\(\)\?' >/tmp/pir4-timeout-permit-missing.txt &&
  contains_all "${RUNTIME_BOOTSTRAP_OPS_SHARED}" \
    'async fn new\(permit: SharedInvocationPermit\)' \
    'enforce_host_call_grants\(operation, &payload_value, &contract\)\?' \
    'HostCallPermitLease::new\(permit.clone\(\)\).await' \
    'normalize_completed_host_call_result\(result, permit_lease.complete\(\).await\)' \
    'permit_result\?' \
    'result = host_call.await' >/tmp/pir4-timeout-op-missing.txt &&
  contains_all "${RUNTIME_TEST_SUPPORT}" \
    'struct DelayedAsyncEnvelopeHost' \
    'tokio::time::sleep\(delay\).await' \
    'sync host bridge path should not be used for delayed async ops' >/tmp/pir4-timeout-support-missing.txt &&
  contains_all "${RUNTIME_TIMEOUT_TEST}" \
    'struct FailingAsyncEnvelopeHost' \
    'pir4_user_timeout_pauses_during_slow_async_host_ops' \
    'pir4_user_timeout_resumes_after_catchable_async_host_error' \
    'execution_timeout = std::time::Duration::from_millis\(50\)' \
    'std::time::Duration::from_millis\(200\)' \
    'runtime watchdog should fire before the outer test timeout' \
    'timed_out_invocations, 0' \
    'runtime_times_out_infinite_loops' >/tmp/pir4-timeout-test-missing.txt &&
  contains_all "${PIR4_PROOF}" \
    'user-timeout pause around async host work' \
    'cargo test -p nimbus-runtime pir4_user_timeout_pauses_during_slow_async_host_ops --lib -- --nocapture' \
    'test result: ok\. 1 passed; 0 failed; 0 ignored; 0 measured; 1021 filtered out' \
    'cargo test -p nimbus-runtime pir4_user_timeout_resumes_after_catchable_async_host_error --lib -- --nocapture' \
    'test result: ok\. 1 passed; 0 failed; 0 ignored; 0 measured; 1022 filtered out' \
    'cargo test -p nimbus-runtime complete_async_host_call_decrements_queue --lib -- --nocapture' \
    'test result: ok\. 2 passed; 0 failed; 0 ignored; 0 measured; 1020 filtered out' \
    'resumes timeout enforcement before user JavaScript continues' \
    'This proof closes the user-timeout pause, system-wall enforcement,' \
    'runtime-internal waitUntil drain, and response-ready executor/server boundary' >/tmp/pir4-timeout-proof-missing.txt &&
  contains_all "${PLAN}" \
    'PIR4 user-timeout accounting slice' \
    'RuntimeInvocationTimeoutController' \
    'HostCallPermitLease::new' \
    'DelayedAsyncEnvelopeHost' \
    'FailingAsyncEnvelopeHost' \
    'pir4_user_timeout_pauses_during_slow_async_host_ops' \
    'pir4_user_timeout_resumes_after_catchable_async_host_error' \
    'PIR4 remains' \
    'explicit system-wall budget, two-phase `waitUntil`, and' >/tmp/pir4-timeout-plan-missing.txt; then
  pass "PIR4 user execution timeout pauses during async host wall time and resumes on active runtime reacquire"
else
  fail "PIR4 user-timeout accounting slice is incomplete" \
    "$(cat /tmp/pir4-timeout-permit-missing.txt 2>/dev/null) $(cat /tmp/pir4-timeout-op-missing.txt 2>/dev/null) $(cat /tmp/pir4-timeout-support-missing.txt 2>/dev/null) $(cat /tmp/pir4-timeout-test-missing.txt 2>/dev/null) $(cat /tmp/pir4-timeout-proof-missing.txt 2>/dev/null) $(cat /tmp/pir4-timeout-plan-missing.txt 2>/dev/null)"
fi

step 72 "PIR4 system-wall timeout bounds cumulative async host wall time"
if [ -f "${RUNTIME_LIMITS_RESOURCES}" ] &&
  contains_all "${RUNTIME_LIMITS_RESOURCES}" \
    'pub system_timeout: Duration' \
    'system_timeout: self.system_timeout' \
    'self.system_timeout = source.system_timeout' \
    'system_timeout: Duration::from_secs\(30\)' >/tmp/pir4-system-limits-missing.txt &&
  contains_all "${RUNTIME_INVOCATION_DRIVER}" \
    'system_timeout_watchdog' \
    'system_timeout_triggered' \
    'let system_timeout = self.policy.limits\(\).system_timeout' \
    'watchdog.register_timeout' \
    'cancellation_signal.cancel\(\)' >/tmp/pir4-system-driver-missing.txt &&
  contains_all "${RUNTIME_HELPERS}" \
    'system_timeout_triggered' \
    'NimbusRuntimeError::SystemTimeout\(limits.system_timeout\)' >/tmp/pir4-system-helper-missing.txt &&
  contains_all "${RUNTIME_ERROR}" \
    'SystemTimeout\(Duration\)' \
    'runtime system wall time timed out after' >/tmp/pir4-system-error-missing.txt &&
  contains_all "${EXECUTOR_LIFECYCLE}" \
    'NimbusRuntimeError::ExecutionTimeout\(_\) \| NimbusRuntimeError::SystemTimeout\(_\)' >/tmp/pir4-system-lifecycle-missing.txt &&
  contains_all "${COOPERATIVE_WORKER_EXECUTION}" \
    'NimbusRuntimeError::ExecutionTimeout\(_\) \| NimbusRuntimeError::SystemTimeout\(_\)' >/tmp/pir4-system-cooperative-missing.txt &&
  contains_all "${RUNTIME_TIMEOUT_TEST}" \
    'pir4_system_timeout_bounds_slow_async_host_wall_time' \
    'limits.execution_timeout = std::time::Duration::from_secs\(1\)' \
    'limits.system_timeout = std::time::Duration::from_millis\(50\)' \
    'NimbusRuntimeError::SystemTimeout\(timeout\)' \
    'slow async host op should trip system wall timeout' >/tmp/pir4-system-test-missing.txt &&
  contains_all "${RUNTIME_LIMITS_TEST}" \
    'limits.system_timeout = Duration::from_secs\(13\)' \
    'budget.system_timeout' >/tmp/pir4-system-limits-test-missing.txt &&
  contains_all "${BUN_JSC_BACKEND}" \
    'execution_timeout \{execution_timeout:\?\} or system_timeout \{system_timeout:\?\}' \
    'limits.system_timeout = Duration::ZERO' >/tmp/pir4-system-bun-missing.txt &&
  contains_all "${PIR4_PROOF}" \
    'system-wall enforcement' \
    'RuntimeLimits::system_timeout' \
    'NimbusRuntimeError::SystemTimeout' \
    'cargo test -p nimbus-runtime runtime::tests::timeout_cancellation --lib -- --nocapture' \
    'test result: ok\. 9 passed; 0 failed; 0 ignored; 0 measured; 1020 filtered out' \
    'cargo test -p nimbus-runtime runtime_limits_expose_tenant_budget_from_normalized_limits --lib -- --nocapture' \
    'cargo test -p nimbus-runtime bun_jsc_linked_backend_rejects_timeout_policy_before_guest_entry --lib -- --nocapture' \
    'cargo test -p nimbus-runtime bun_jsc_runtime_backend_dispatches_through_no_timeout_linked_adapter_seam --lib -- --nocapture' \
    'SystemTimeout\(50 ms\)' \
    'Tenant budget export, resource overrides, and linked Bun/JSC fail-closed' >/tmp/pir4-system-proof-missing.txt &&
  contains_all "${PLAN}" \
    'PIR4 system-wall timeout slice' \
    'RuntimeLimits::system_timeout' \
    'RuntimeTenantBudget::system_timeout' \
    'NimbusRuntimeError::SystemTimeout' \
    'Linked Bun/JSC remains' \
    'PIR4 remains `in_progress` for two-phase `waitUntil` and final' >/tmp/pir4-system-plan-missing.txt; then
  pass "PIR4 system timeout enforces async host wall time separately from user execution timeout"
else
  fail "PIR4 system-wall timeout slice is incomplete" \
    "$(cat /tmp/pir4-system-limits-missing.txt 2>/dev/null) $(cat /tmp/pir4-system-driver-missing.txt 2>/dev/null) $(cat /tmp/pir4-system-helper-missing.txt 2>/dev/null) $(cat /tmp/pir4-system-error-missing.txt 2>/dev/null) $(cat /tmp/pir4-system-lifecycle-missing.txt 2>/dev/null) $(cat /tmp/pir4-system-cooperative-missing.txt 2>/dev/null) $(cat /tmp/pir4-system-test-missing.txt 2>/dev/null) $(cat /tmp/pir4-system-limits-test-missing.txt 2>/dev/null) $(cat /tmp/pir4-system-bun-missing.txt 2>/dev/null) $(cat /tmp/pir4-system-proof-missing.txt 2>/dev/null) $(cat /tmp/pir4-system-plan-missing.txt 2>/dev/null)"
fi

step 73 "PIR4 waitUntil drains after response-ready on fresh background budgets"
if [ -f "${RUNTIME_BOOTSTRAP_SOURCE}" ] &&
  contains_all "${RUNTIME_BOOTSTRAP_SOURCE}" \
    '__nimbusWaitUntilQueue' \
    'globalThis.__nimbusWaitUntil = function' \
    'Promise.resolve\(promise\)' \
    'globalThis.__nimbusDrainWaitUntil = async function' \
    'Promise.allSettled\(batch\)' \
    'globalThis.__nimbusResetWaitUntil = function' \
    '__nimbusResetWaitUntil\(\);' >/tmp/pir4-waituntil-bootstrap-missing.txt &&
  contains_all "${RUNTIME_INVOCATION_DRIVER}" \
    'begin_wait_until_phase' \
    'timeout_controller.reset\(limits.execution_timeout\).await' \
    'system_timeout_watchdog.disarm\(\).await' \
    'system_timeout_callback' \
    'wait_until_phase_timeout_error' \
    'drain_wait_until_with_trace' >/tmp/pir4-waituntil-driver-missing.txt &&
  contains_all "${RUNTIME_LOADING}" \
    'invoke_loaded_bundle:response_ready' \
    'invoke_recycled_context:response_ready' \
    'wait_until:drain:start' \
    'wait_until:drain:complete' \
    '__nimbusDrainWaitUntil\(\)' \
    'ensure_wait_until_drain_succeeded' >/tmp/pir4-waituntil-loading-missing.txt &&
  contains_all "${RUNTIME_COOPERATIVE}" \
    'ResponseReady' \
    'wait_until: Option' \
    'response_ready: Option' \
    'start_wait_until_phase' \
    '__nimbusDrainWaitUntil\(\)' \
    'begin_wait_until_phase' \
    'wait_until_phase_timeout_error' \
    'ensure_wait_until_drain_succeeded' >/tmp/pir4-waituntil-cooperative-missing.txt &&
  contains_all "${RUNTIME_HELPERS}" \
    'ensure_wait_until_drain_succeeded' \
    'runtime waitUntil drain result must carry a rejected count' \
    'waitUntil background drain rejected' >/tmp/pir4-waituntil-helper-missing.txt &&
  contains_all "${COOPERATIVE_WORKER_RUN}" \
    'CooperativeRuntimeSlotPoll::ResponseReady' >/tmp/pir4-waituntil-run-missing.txt &&
  contains_all "${RUNTIME_COOPERATIVE_TEST}" \
    'CooperativeRuntimeSlotPoll::ResponseReady' >/tmp/pir4-waituntil-cooperative-test-missing.txt &&
  contains_all "${RUNTIME_TEST_SUPPORT}" \
    'struct CountingDelayedAsyncEnvelopeHost' \
    'counting delayed async ops' \
    'tokio::time::sleep\(delay\).await' >/tmp/pir4-waituntil-support-missing.txt &&
  contains_all "${RUNTIME_TIMEOUT_TEST}" \
    'pir4_wait_until_drains_background_work_after_response_ready' \
    'pir4_wait_until_system_budget_is_fresh_after_response_ready' \
    'pir4_wait_until_system_timeout_bounds_background_work' \
    'pir4_wait_until_drains_on_cooperative_queries' \
    'globalThis.__nimbusWaitUntil' \
    'std::time::Duration::from_millis\(120\)' \
    'limits.system_timeout = std::time::Duration::from_millis\(180\)' \
    'limits.system_timeout = std::time::Duration::from_millis\(50\)' >/tmp/pir4-waituntil-timeout-test-missing.txt &&
  contains_all "${PIR4_PROOF}" \
    'runtime-internal two-phase `waitUntil` draining' \
    'pir4_wait_until_drains_background_work_after_response_ready' \
    'pir4_wait_until_drains_on_cooperative_queries' \
    'pir4_wait_until_system_timeout_bounds_background_work' \
    'pir4_wait_until_system_budget_is_fresh_after_response_ready' \
    'test result: ok\. 9 passed; 0 failed; 0 ignored; 0 measured; 1020 filtered out' \
    'test result: ok\. 8 passed; 0 failed; 8 ignored; 0 measured; 1013 filtered out' \
    'runtime-internal waitUntil drain, and response-ready executor/server boundary' >/tmp/pir4-waituntil-proof-missing.txt &&
  contains_all "${PLAN}" \
    'PIR4 waitUntil runtime-substrate slice' \
    '__nimbusWaitUntil' \
    'response_ready' \
    'RuntimeInvocationTimeoutController' \
    'public server/executor call boundary still returns one `Result<Value>`' \
    'public HTTP response/background API' \
    'split decision' >/tmp/pir4-waituntil-plan-missing.txt; then
  pass "PIR4 waitUntil work drains after response-ready with fresh user and system timeout phases"
else
  fail "PIR4 waitUntil runtime-substrate slice is incomplete" \
    "$(cat /tmp/pir4-waituntil-bootstrap-missing.txt 2>/dev/null) $(cat /tmp/pir4-waituntil-driver-missing.txt 2>/dev/null) $(cat /tmp/pir4-waituntil-loading-missing.txt 2>/dev/null) $(cat /tmp/pir4-waituntil-cooperative-missing.txt 2>/dev/null) $(cat /tmp/pir4-waituntil-helper-missing.txt 2>/dev/null) $(cat /tmp/pir4-waituntil-run-missing.txt 2>/dev/null) $(cat /tmp/pir4-waituntil-cooperative-test-missing.txt 2>/dev/null) $(cat /tmp/pir4-waituntil-support-missing.txt 2>/dev/null) $(cat /tmp/pir4-waituntil-timeout-test-missing.txt 2>/dev/null) $(cat /tmp/pir4-waituntil-proof-missing.txt 2>/dev/null) $(cat /tmp/pir4-waituntil-plan-missing.txt 2>/dev/null)"
fi

step 74 "PIR4 response-ready executor/server boundary is explicit"
if [ -f "${EXECUTOR_INVOKE}" ] &&
  contains_all "${EXECUTOR_INVOKE}" \
    'pub struct RuntimeInvocationResponse' \
    'RuntimeInvocationCompletion::Pending' \
    'wait_until_complete' \
    'invoke_on_worker_response_ready' \
    'response_ready_tx: Some\(response_ready_tx\)' \
    'response_ready_rx' >/tmp/pir4-response-ready-executor-missing.txt &&
  ! contains 'pub fn into_response' "${EXECUTOR_INVOKE}" &&
  contains_all "${EXECUTOR_QUEUE_JOB}" \
    'response_ready_tx: Option<oneshot::Sender<Value>>' >/tmp/pir4-response-ready-job-missing.txt &&
  contains_all "${RUNTIME_INVOCATION_DRIVER}" \
    'response_ready_tx' \
    'response_ready_tx.send\(response.clone\(\)\)' \
    'begin_wait_until_phase' >/tmp/pir4-response-ready-driver-missing.txt &&
  contains_all "${RUNTIME_COOPERATIVE}" \
    'response_ready_tx: Option<oneshot::Sender<Value>>' \
    'response_ready_tx.send\(response.clone\(\)\)' \
    'CooperativeRuntimeSlotPoll::ResponseReady' >/tmp/pir4-response-ready-cooperative-missing.txt &&
  contains_all "${COOPERATIVE_WORKER_EXECUTION}" \
    'response_ready_tx: job.response_ready_tx.take\(\)' >/tmp/pir4-response-ready-worker-execution-missing.txt &&
  contains_all "${SERVER_INVOCATION_WORKER}" \
    'invoke_runtime_bundle_on_worker_response_ready_with_host' \
    'RuntimeInvocationResponse' \
    'invoke_on_worker_response_ready' \
    'wait_until_complete' >/tmp/pir4-response-ready-server-missing.txt &&
  contains_all "${EXECUTOR_TEST_SUPPORT}" \
    'write_wait_until_bundle' \
    'write_rejected_wait_until_bundle' \
    '__nimbusWaitUntil' \
    'slow-background' >/tmp/pir4-response-ready-support-missing.txt &&
  contains_all "${COOPERATIVE_EXECUTOR_TEST}" \
    'pir4_response_ready_returns_before_wait_until_background_completion' \
    'response_ready_completion_reports_rejected_wait_until_background_work' \
    'invoke_on_worker_response_ready' \
    'response_ready.response\(\)' \
    'wait_until_complete' \
    'waitUntil background drain rejected 1 promise' \
    'waitUntil completion should remain pending while background host work is blocked' >/tmp/pir4-response-ready-test-missing.txt &&
  contains_all "${PIR4_PROOF}" \
    'public executor/server response-ready boundary' \
    'RuntimeExecutor::invoke_on_worker_response_ready' \
    'RuntimeInvocationResponse' \
    'no consuming `into_response\(\)` helper' \
    'nimbus-server`'\''s async runtime-bundle host helper now crosses that split' \
    'cargo test -p nimbus-runtime executor::tests::cooperative --lib -- --nocapture' \
    'test result: ok\. 7 passed; 0 failed; 0 ignored; 0 measured; 1039 filtered out' \
    'cargo check -p nimbus-server --lib' \
    'Finished `dev` profile \[unoptimized \+ debuginfo\] target\(s\) in 6\.14s' \
    'response-ready executor/server boundary' >/tmp/pir4-response-ready-proof-missing.txt &&
  contains_all "${PLAN}" \
    'PIR4 response-ready public-boundary slice' \
    'RuntimeExecutor::invoke_on_worker_response_ready' \
    'RuntimeInvocationResponse' \
    'response-ready sender into `RuntimeInvocationExecution`' \
    'nimbus-server`'\''s async runtime-bundle host helper now crosses that' \
    'cargo test -p nimbus-runtime executor::tests::cooperative --lib --' \
    'cargo check -p' \
    'nimbus-server --lib' >/tmp/pir4-response-ready-plan-missing.txt; then
  pass "PIR4 exposes response-ready separately from bounded background completion at the executor and server invocation boundary"
else
  fail "PIR4 response-ready executor/server boundary is incomplete" \
    "$(cat /tmp/pir4-response-ready-executor-missing.txt 2>/dev/null) $(cat /tmp/pir4-response-ready-job-missing.txt 2>/dev/null) $(cat /tmp/pir4-response-ready-driver-missing.txt 2>/dev/null) $(cat /tmp/pir4-response-ready-cooperative-missing.txt 2>/dev/null) $(cat /tmp/pir4-response-ready-worker-execution-missing.txt 2>/dev/null) $(cat /tmp/pir4-response-ready-server-missing.txt 2>/dev/null) $(cat /tmp/pir4-response-ready-support-missing.txt 2>/dev/null) $(cat /tmp/pir4-response-ready-test-missing.txt 2>/dev/null) $(cat /tmp/pir4-response-ready-proof-missing.txt 2>/dev/null) $(cat /tmp/pir4-response-ready-plan-missing.txt 2>/dev/null)"
fi

step 75 "PIR4 closeout promotes PIR7 as the single active band"
if [ -f "${PLAN}" ] &&
  [ -f "${PIR4_PROOF}" ] &&
  contains_all "${PLAN}" \
    '\| PIR4 \| `done`' \
    '\| PIR7 \| `done`' \
    'PIR4 closeout and PIR7 activation' \
    'Universal band promotion checklist' \
    'PIR7 is now `in_progress`' \
    'RuntimeInvocationResponse` intentionally has no consuming' \
    'scripts/verify-profile-aware-isolate-runtime.sh` with 75 passed, 0 failed' \
    'static measured defaults, host resource budget guardrails' >/tmp/pir4-closeout-plan-missing.txt &&
  contains_all "${PIR4_PROOF}" \
    'PIR4 status: done' \
    'PIR4 Promotion Audit' \
    'Cohesion:' \
    'Maintainability:' \
    'Testability:' \
    'Security:' \
    'Resilience:' \
    'Canonicality:' \
    'Rust idiom:' \
    'Verifiability/autonomous state:' \
    'PIR7 is now the active band' \
    'Summary: 75 passed, 0 failed' >/tmp/pir4-closeout-proof-missing.txt &&
  ! contains 'pub fn into_response' "${EXECUTOR_INVOKE}"; then
  pass "PIR4 is closed with promotion-audit evidence and PIR7 is active"
else
  fail "PIR4 closeout/PIR7 activation evidence is incomplete" \
    "$(cat /tmp/pir4-closeout-plan-missing.txt 2>/dev/null) $(cat /tmp/pir4-closeout-proof-missing.txt 2>/dev/null)"
fi

step 76 "PIR7 host-resource budget policy is pure, conservative, and QoS-aware"
if [ -f "${RUNTIME_PRESSURE}" ] &&
  [ -f "${RUNTIME_LIMITS_TEST}" ] &&
  [ -f "${RUNTIME_LIB}" ] &&
  [ -f "${PIR7_HOST_BUDGET_PROOF}" ] &&
  contains_all "${RUNTIME_PRESSURE}" \
    'pub struct RuntimeHostResourceBudget' \
    'host_millicpus' \
    'system_reserved_millicpus' \
    'nimbus_control_plane_reserved_millicpus' \
    'runtime_hard_ceiling_millicpus' \
    'runtime_allocatable_millicpus' \
    'nominal_dispatch_seats' \
    'pub struct RuntimeHostPressureSample' \
    'control_plane_lag_high' \
    'pub enum RuntimeHostWorkClass' \
    'Guaranteed' \
    'Burstable' \
    'BestEffort' \
    'pub enum RuntimeHostAdmissionAction' \
    'Queue' \
    'Shed' \
    'admission_for_in_flight' \
    'over_capacity_action_for' \
    'over_capacity_action' \
    'tenant_quota_remaining' \
    'current_host_in_flight' \
    'effective_dispatch_seats' \
    'pub fn unavailable' \
    'RuntimeHostPressureLevel::Critical' >/tmp/pir7-host-budget-pressure-missing.txt &&
  contains_all "${RUNTIME_LIMITS_TEST}" \
    'runtime_host_resource_budget_reserves_system_and_control_plane_capacity' \
    'runtime_host_pressure_overrides_unused_tenant_quota_for_lower_qos_work' \
    'runtime_host_pressure_degrades_conservatively_without_cpu_source' \
    'host pressure should admit burstable work while a reduced host seat remains available' \
    'host pressure can queue burstable work when tenant quota remains but reduced host seats are full' \
    'host pressure can shed best-effort work even when tenant quota remains and a reduced host seat is available' >/tmp/pir7-host-budget-tests-missing.txt &&
  contains_all "${EXECUTOR_ADMISSION}" \
    'host_admission_for_in_flight' \
    'host_admission_action_for_in_flight' \
    'admission_for_in_flight' >/tmp/pir7-host-budget-executor-missing.txt &&
  ! rg -q 'host_admission_action_for_decision|\.admission_for\(' "${EXECUTOR_ADMISSION}" "${EXECUTOR_TENANT_FAIRNESS}" "${RUNTIME_PRESSURE}" &&
  contains_all "${QUEUE_FAIRNESS_EXECUTOR_TEST}" \
    'acquire_runtime_suite_lock' \
    'host_pressure_reduces_runtime_dispatch_seats_before_tenant_quota_exhaustion' \
    'host_pressure_queue_promotion_respects_effective_dispatch_seats' \
    'host_pressure_sheds_burstable_work_under_critical_pressure' >/tmp/pir7-host-budget-executor-tests-missing.txt &&
  contains_all "${RUNTIME_LIB}" \
    'RuntimeHostResourceBudget' \
    'RuntimeHostResourceDecision' \
    'RuntimeHostAdmissionAction' \
    'RuntimeHostWorkClass' >/tmp/pir7-host-budget-exports-missing.txt &&
  contains_all "${PIR7_HOST_BUDGET_PROOF}" \
    'RuntimeHostResourceBudget' \
    'host_capacity - system_reserve - nimbus_control_plane_reserve' \
    'dispatch-aware admission interface' \
    'Host pressure admits burstable work while a reduced host seat remains' \
    'sheds best-effort work even' \
    'cargo test -p nimbus-runtime host_pressure --lib -- --nocapture' \
    '8 passed; 0 failed; 0 ignored; 0 measured; 1111 filtered out' \
    'cargo test -p nimbus-runtime runtime_host_resource_budget --lib -- --nocapture' \
    '1 passed; 0 failed; 0 ignored; 0 measured; 1031 filtered out' \
    'cargo check -p nimbus-runtime --lib' \
    'Summary: 76 passed, 0 failed' >/tmp/pir7-host-budget-proof-missing.txt &&
  contains_all "${PLAN}" \
    'PIR7 host-resource budget policy slice' \
    'RuntimeHostResourceBudget' \
    'nimbus_control_plane_reserve' \
    'RuntimeHostWorkClass' \
    'high pressure' \
    'admission_for_in_flight' \
    'queues burstable work once those seats are full' >/tmp/pir7-host-budget-plan-missing.txt; then
  pass "PIR7 has a tested pure host-resource budget policy before CLI/server/node lowering"
else
  fail "PIR7 host-resource budget policy slice is incomplete" \
    "$(cat /tmp/pir7-host-budget-pressure-missing.txt 2>/dev/null) $(cat /tmp/pir7-host-budget-tests-missing.txt 2>/dev/null) $(cat /tmp/pir7-host-budget-executor-missing.txt 2>/dev/null) $(cat /tmp/pir7-host-budget-executor-tests-missing.txt 2>/dev/null) $(cat /tmp/pir7-host-budget-exports-missing.txt 2>/dev/null) $(cat /tmp/pir7-host-budget-proof-missing.txt 2>/dev/null) $(cat /tmp/pir7-host-budget-plan-missing.txt 2>/dev/null)"
fi

step 77 "PIR7 lowers host-resource budget policy through nimbus start"
if [ -f "${START_COMMAND}" ] &&
  [ -f "${START_BOOT}" ] &&
  [ -f "${START_RUNTIME_LIMITS}" ] &&
  [ -f "${START_CLI_TEST}" ] &&
  [ -f "${PIR7_HOST_BUDGET_PROOF}" ] &&
  contains_all "${START_COMMAND}" \
    'runtime_host_millicpus' \
    'long = "runtime-host-millicpus"' \
    'runtime_system_reserve_millicpus' \
    'long = "runtime-system-reserve-millicpus"' \
    'runtime_control_plane_reserve_millicpus' \
    'long = "runtime-control-plane-reserve-millicpus"' \
    'runtime_hard_ceiling_millicpus' \
    'long = "runtime-hard-ceiling-millicpus"' \
    'runtime_seat_millicpus' \
    'long = "runtime-seat-millicpus"' >/tmp/pir7-start-command-missing.txt &&
  contains_all "${START_RUNTIME_LIMITS}" \
    'RuntimeHostResourceBudget' \
    'default_runtime_host_millicpus' \
    'default_runtime_system_reserve_millicpus' \
    'default_runtime_control_plane_reserve_millicpus' \
    'default_runtime_seat_millicpus' \
    'runtime_host_resource_budget_from_command' \
    'std::thread::available_parallelism' \
    'NonZeroU32::new' \
    'command.runtime_seat_millicpus' \
    'runtime_hard_ceiling_millicpus' >/tmp/pir7-start-lowerer-missing.txt &&
  contains_all "${START_BOOT}" \
    'runtime_host_resource_budget_from_command' \
    'runtime_host_budget_summary_line' \
    'runtime host budget:' \
    'runtime_allocatable_millicpus' \
    'Nimbus control-plane reserve' >/tmp/pir7-start-boot-missing.txt &&
  contains_all "${START_CLI_TEST}" \
    'cli_parses_runtime_host_budget_policy_flags' \
    'cli_rejects_zero_runtime_host_capacity_or_seat' \
    'runtime_host_resource_budget_from_command_applies_operator_policy' \
    'start_command_default_has_conservative_runtime_host_budget' \
    'start_startup_summary_mentions_runtime_host_budget' \
    'start_does_not_accept_runtime_efficiency_profile_knobs' >/tmp/pir7-start-tests-missing.txt &&
  contains_all "${PIR7_HOST_BUDGET_PROOF}" \
    'nimbus start host-budget lowering' \
    'runtime host budget:' \
    'cargo test -p nimbus-bin runtime_host_budget -- --nocapture' \
    '5 passed; 0 failed; 0 ignored; 0 measured; 729 filtered out' \
    'cargo test -p nimbus-bin runtime_host_resource_budget -- --nocapture' \
    '1 passed; 0 failed; 0 ignored; 0 measured; 731 filtered out' \
    'cargo test -p nimbus-bin cli_rejects_zero_runtime_host_capacity_or_seat -- --nocapture' \
    '1 passed; 0 failed; 0 ignored; 0 measured; 731 filtered out' \
    'Summary: 77 passed, 0 failed' >/tmp/pir7-start-proof-missing.txt &&
  contains_all "${PLAN}" \
    'PIR7 nimbus start host-budget lowering slice' \
    '`--runtime-host-millicpus`' \
    '`--runtime-system-reserve-millicpus`' \
    '`--runtime-control-plane-reserve-millicpus`' \
    '`--runtime-hard-ceiling-millicpus`' \
    '`--runtime-seat-millicpus`' >/tmp/pir7-start-plan-missing.txt; then
  pass "PIR7 host-resource budget policy is lowered through nimbus start with tests and banner evidence"
else
  fail "PIR7 nimbus start host-budget lowering slice is incomplete" \
    "$(cat /tmp/pir7-start-command-missing.txt 2>/dev/null) $(cat /tmp/pir7-start-lowerer-missing.txt 2>/dev/null) $(cat /tmp/pir7-start-boot-missing.txt 2>/dev/null) $(cat /tmp/pir7-start-tests-missing.txt 2>/dev/null) $(cat /tmp/pir7-start-proof-missing.txt 2>/dev/null) $(cat /tmp/pir7-start-plan-missing.txt 2>/dev/null)"
fi

step 78 "PIR7 carries host-resource budget policy through nimbus dev"
if [ -f "${DEV_PLAN_TEST}" ] &&
  [ -f "${PIR7_HOST_BUDGET_PROOF}" ] &&
  contains_all "${DEV_PLAN_TEST}" \
    'dev_start_command_inherits_conservative_runtime_host_budget' \
    'StartCommand::default' \
    'runtime_host_millicpus' \
    'runtime_system_reserve_millicpus' \
    'runtime_control_plane_reserve_millicpus' \
    'runtime_hard_ceiling_millicpus' \
    'runtime_seat_millicpus' \
    'dev must not invent a separate runtime hard-ceiling policy' >/tmp/pir7-dev-test-missing.txt &&
  contains_all "${PIR7_HOST_BUDGET_PROOF}" \
    'nimbus dev host-budget inheritance' \
    'dev_start_command_inherits_conservative_runtime_host_budget' \
    'cargo test -p nimbus-bin dev_start_command_inherits_conservative_runtime_host_budget -- --nocapture' \
    '1 passed; 0 failed; 0 ignored; 0 measured; 732 filtered out' \
    'Summary: 78 passed, 0 failed' >/tmp/pir7-dev-proof-missing.txt &&
  contains_all "${PLAN}" \
    'PIR7 nimbus dev host-budget inheritance slice' \
    'DevPlan' \
    'StartCommand::default()' \
    'dev_start_command_inherits_conservative_runtime_host_budget' >/tmp/pir7-dev-plan-missing.txt; then
  pass "PIR7 host-resource budget policy is carried into nimbus dev without a separate model"
else
  fail "PIR7 nimbus dev host-budget inheritance slice is incomplete" \
    "$(cat /tmp/pir7-dev-test-missing.txt 2>/dev/null) $(cat /tmp/pir7-dev-proof-missing.txt 2>/dev/null) $(cat /tmp/pir7-dev-plan-missing.txt 2>/dev/null)"
fi

step 79 "PIR7 renders host-resource budget policy through native nimbus node services"
if [ -f "${NODE_SERVICE}" ] &&
  [ -f "${PIR7_HOST_BUDGET_PROOF}" ] &&
  contains_all "${NODE_SERVICE}" \
    'default_runtime_host_budget_start_args' \
    'StartCommand::default' \
    'args.extend' \
    'default_runtime_host_budget_start_args' \
    '--runtime-host-millicpus' \
    '--runtime-system-reserve-millicpus' \
    '--runtime-control-plane-reserve-millicpus' \
    '--runtime-seat-millicpus' \
    '--runtime-hard-ceiling-millicpus' \
    'native_systemd_renders_runtime_host_budget_start_flags' \
    'default native node service should omit unset optional hard ceiling' >/tmp/pir7-node-service-missing.txt &&
  contains_all "${PIR7_HOST_BUDGET_PROOF}" \
    'nimbus node host-budget render' \
    'native_systemd_renders_runtime_host_budget_start_flags' \
    'cargo test -p nimbus-bin native_systemd_renders_runtime_host_budget_start_flags -- --nocapture' \
    '1 passed; 0 failed; 0 ignored; 0 measured; 733 filtered out' \
    'cargo test -p nimbus-bin native_socket_activation_renders_matching_socket_and_service -- --nocapture' \
    'Summary: 79 passed, 0 failed' >/tmp/pir7-node-proof-missing.txt &&
  contains_all "${PLAN}" \
    'PIR7 nimbus node host-budget render slice' \
    'NativeSystemdNodeService' \
    'default_runtime_host_budget_start_args' \
    'native_systemd_renders_runtime_host_budget_start_flags' >/tmp/pir7-node-plan-missing.txt; then
  pass "PIR7 host-resource budget policy is rendered into native nimbus node services"
else
  fail "PIR7 nimbus node host-budget render slice is incomplete" \
    "$(cat /tmp/pir7-node-service-missing.txt 2>/dev/null) $(cat /tmp/pir7-node-proof-missing.txt 2>/dev/null) $(cat /tmp/pir7-node-plan-missing.txt 2>/dev/null)"
fi

step 80 "PIR7 carries host-resource budget through server construction and AppState"
if [ -f "${SERVER_CONSTRUCTION}" ] &&
  [ -f "${SERVER_ROUTER}" ] &&
  [ -f "${SERVER_STATE}" ] &&
  [ -f "${START_BOOT}" ] &&
  [ -f "${PIR7_HOST_BUDGET_PROOF}" ] &&
  contains_all "${SERVER_CONSTRUCTION}" \
    'ServeOptions' \
    'with_runtime_host_resource_budget' \
    'RuntimeHostResourceBudget' \
    'Tenant quotas remain separate' >/tmp/pir7-server-construction-missing.txt &&
  contains_all "${SERVER_ROUTER}" \
    'RouterOptions' \
    'runtime_host_resource_budget: RuntimeHostResourceBudget' \
    'default_runtime_host_resource_budget' \
    'RouterBuildConfig' \
    'with_runtime_host_resource_budget' \
    'AppStateConfig' \
    'configured runtime host resource budget' \
    'runtime_allocatable_millicpus' >/tmp/pir7-server-router-missing.txt &&
  contains_all "${SERVER_STATE}" \
    'runtime_host_resource_budget: RuntimeHostResourceBudget' \
    'fn runtime_host_resource_budget' \
    'app_state_carries_runtime_host_resource_budget' \
    'fixture CPU count is nonzero' >/tmp/pir7-server-state-missing.txt &&
  contains_all "${START_BOOT}" \
    'runtime_host_resource_budget = runtime_host_resource_budget_from_command' \
    'with_runtime_host_resource_budget' >/tmp/pir7-server-start-missing.txt &&
  contains_all "${PIR7_HOST_BUDGET_PROOF}" \
    'server construction host-budget state seam' \
    'ServeOptions::with_runtime_host_resource_budget' \
    'RouterOptions::with_runtime_host_resource_budget' \
    'AppState::runtime_host_resource_budget' \
    'cargo test -p nimbus-server app_state_carries_runtime_host_resource_budget --lib -- --nocapture' \
    '1 passed; 0 failed; 0 ignored; 0 measured; 475 filtered out' \
    'cargo check -p nimbus-server --lib' \
    'cargo check -p nimbus-bin' \
    'Summary: 80 passed, 0 failed' >/tmp/pir7-server-proof-missing.txt &&
  contains_all "${PLAN}" \
    'PIR7 server construction host-budget state seam' \
    'ServeOptions::with_runtime_host_resource_budget' \
    'RouterOptions::with_runtime_host_resource_budget' \
    'AppState::runtime_host_resource_budget' >/tmp/pir7-server-plan-missing.txt; then
  pass "PIR7 host-resource budget is carried through server construction as typed state"
else
  fail "PIR7 server construction host-budget state seam is incomplete" \
    "$(cat /tmp/pir7-server-construction-missing.txt 2>/dev/null) $(cat /tmp/pir7-server-router-missing.txt 2>/dev/null) $(cat /tmp/pir7-server-state-missing.txt 2>/dev/null) $(cat /tmp/pir7-server-start-missing.txt 2>/dev/null) $(cat /tmp/pir7-server-proof-missing.txt 2>/dev/null) $(cat /tmp/pir7-server-plan-missing.txt 2>/dev/null)"
fi

step 81 "PIR7 gates runtime admission through an injected host-pressure source"
if [ -f "${RUNTIME_PRESSURE}" ] &&
  [ -f "${RUNTIME_LIMITS_POLICY}" ] &&
  [ -f "${EXECUTOR_ADMISSION}" ] &&
  [ -f "${EXECUTOR_TENANT_FAIRNESS}" ] &&
  [ -f "${QUEUE_FAIRNESS_EXECUTOR_TEST}" ] &&
  [ -f "${PIR7_HOST_BUDGET_PROOF}" ] &&
  contains_all "${RUNTIME_PRESSURE}" \
    'pub trait RuntimeHostPressureSource' \
    'NominalRuntimeHostPressureSource' \
    'fn sample' >/tmp/pir7-pressure-source-missing.txt &&
  contains_all "${RUNTIME_LIMITS_POLICY}" \
    'with_host_resource_governor' \
    'host_resource_governor_enabled' \
    'host_resource_decision' \
    'RuntimeHostPressureSource' \
    'NominalRuntimeHostPressureSource' >/tmp/pir7-policy-source-missing.txt &&
  contains_all "${EXECUTOR_ADMISSION}" \
    'host_admission_action_for_in_flight' \
    'host_admission_for_in_flight' \
    'RuntimeHostAdmissionDecision' \
    'HostResourcePressureShed' \
    'runtime_host_work_class_for_job' \
    'admission_for_in_flight' \
    'host_resource_governor_enabled' >/tmp/pir7-admission-gate-missing.txt &&
  contains_all "${EXECUTOR_TENANT_FAIRNESS}" \
    'total_in_flight' \
    'current_host_in_flight' \
    'host_admission_action_for_in_flight' >/tmp/pir7-tenant-promotion-missing.txt &&
  contains_all "${QUEUE_FAIRNESS_EXECUTOR_TEST}" \
    'FixedRuntimeHostPressureSource' \
    'host_pressure_reduces_runtime_dispatch_seats_before_tenant_quota_exhaustion' \
    'host_pressure_queue_promotion_respects_effective_dispatch_seats' \
    'host_pressure_sheds_burstable_work_under_critical_pressure' \
    'with_host_resource_governor' \
    'HostResourcePressureShed' \
    'tenant quota is exhausted' >/tmp/pir7-host-admission-tests-missing.txt &&
  contains_all "${PIR7_HOST_BUDGET_PROOF}" \
    'runtime injected host-pressure admission slice' \
    'RuntimeHostPressureSource' \
    'RuntimePolicy::with_host_resource_governor' \
    'host_pressure_reduces_runtime_dispatch_seats_before_tenant_quota_exhaustion' \
    'host_pressure_sheds_burstable_work_under_critical_pressure' \
    'cargo test -p nimbus-runtime host_pressure --lib -- --nocapture' \
    '8 passed; 0 failed; 0 ignored; 0 measured; 1111 filtered out' \
    'cargo test -p nimbus-runtime executor::tests::cooperative --lib -- --nocapture' \
    '7 passed; 0 failed; 0 ignored; 0 measured; 1039 filtered out' \
    'cargo test -p nimbus-runtime executor::tests::queue_fairness --lib -- --nocapture' \
    '9 passed; 0 failed; 0 ignored; 0 measured; 1037 filtered out' \
    'Summary: 81 passed, 0 failed' >/tmp/pir7-host-admission-proof-missing.txt &&
  contains_all "${PLAN}" \
    'PIR7 runtime injected host-pressure admission slice' \
    'RuntimePolicy::with_host_resource_governor' \
    'RuntimeHostPressureSource' \
    'HostResourcePressureShed' >/tmp/pir7-host-admission-plan-missing.txt; then
  pass "PIR7 runtime admission consumes an injected host-pressure source without changing default policy behavior"
else
  fail "PIR7 runtime injected host-pressure admission slice is incomplete" \
    "$(cat /tmp/pir7-pressure-source-missing.txt 2>/dev/null) $(cat /tmp/pir7-policy-source-missing.txt 2>/dev/null) $(cat /tmp/pir7-admission-gate-missing.txt 2>/dev/null) $(cat /tmp/pir7-tenant-promotion-missing.txt 2>/dev/null) $(cat /tmp/pir7-host-admission-tests-missing.txt 2>/dev/null) $(cat /tmp/pir7-host-admission-proof-missing.txt 2>/dev/null) $(cat /tmp/pir7-host-admission-plan-missing.txt 2>/dev/null)"
fi

step 82 "PIR7 injects the host-resource governor into server runtime registries"
if [ -f "${SERVER_ROUTER}" ] &&
  [ -f "${CONVEX_LIB}" ] &&
  [ -f "${CONVEX_REGISTRY_LOADING}" ] &&
  [ -f "${CONVEX_RUNTIME_ACCESS}" ] &&
  [ -f "${CLOUD_FUNCTIONS_REGISTRY}" ] &&
  [ -f "${PIR7_HOST_BUDGET_PROOF}" ] &&
  contains_all "${SERVER_ROUTER}" \
    'let runtime_host_resource_budget = self.runtime_host_resource_budget' \
    'let runtime_host_pressure_source = self.runtime_host_pressure_source' \
    'self.convex_registry.map' \
    'self.system_convex_registry.map' \
    'self.cloud_functions_registry.map' \
    'with_runtime_host_governor' \
    'runtime_host_resource_budget' \
    'runtime_host_pressure_source.clone' \
    'AppStateConfig' >/tmp/pir7-registry-router-missing.txt &&
  contains_all "${CONVEX_LIB}" \
    'fn from_policy' \
    'fn with_runtime_host_governor' \
    'RuntimeHostPressureSource' \
    'clone_with_host_resource_governor' \
    'OnceLock::new' >/tmp/pir7-registry-convex-lane-missing.txt &&
  contains_all "${CONVEX_REGISTRY_LOADING}" \
    'pub fn with_runtime_host_resource_budget' \
    'pub fn with_runtime_host_governor' \
    'pressure_source.clone' \
    'self.runtime_lane' \
    'self.node20_runtime_lane' \
    'self.node22_runtime_lane' \
    'self.node24_runtime_lane' \
    'self.node26_runtime_lane' \
    'self.bun_jsc_runtime_lane' >/tmp/pir7-registry-convex-loading-missing.txt &&
  contains_all "${CONVEX_RUNTIME_ACCESS}" \
    'convex_registry_applies_runtime_host_resource_budget_to_runtime_policy' \
    'convex_node_runtime_lanes_follow_lts_registry_targets' \
    'FixedRuntimeHostPressureSource' \
    'critical_runtime_host_pressure_source' \
    'runtime_lane_policy_for_function' \
    'function_name' \
    'host_resource_budget' \
    'host_resource_decision' \
    'effective_dispatch_seats' >/tmp/pir7-registry-convex-tests-missing.txt &&
  contains_all "${CLOUD_FUNCTIONS_REGISTRY}" \
    'pub fn with_runtime_host_resource_budget' \
    'pub fn with_runtime_host_governor' \
    'RuntimeHostPressureSource' \
    'clone_with_host_resource_governor' \
    'RuntimeExecutor::new' \
    'runtime_policy' \
    'FixedRuntimeHostPressureSource' \
    'critical_runtime_host_pressure_source' \
    'cloud_functions_registry_applies_runtime_host_resource_budget_to_runtime_policy' \
    'cloud_functions_registry_host_governor_preserves_runtime_metrics_identity' \
    'host_resource_decision' >/tmp/pir7-registry-cloud-functions-missing.txt &&
  contains_all "${PIR7_HOST_BUDGET_PROOF}" \
    'server registry host-budget injection slice' \
    'RuntimePolicy::clone_with_host_resource_governor' \
    'ConvexRegistry::with_runtime_host_resource_budget' \
    'ConvexRegistry::with_runtime_host_governor' \
    'CloudFunctionsRegistry::with_runtime_host_resource_budget' \
    'CloudFunctionsRegistry::with_runtime_host_governor' \
    'RouterBuildConfig::build' \
    'FixedRuntimeHostPressureSource' \
    'convex_node_runtime_lanes_follow_lts_registry_targets' \
    'cargo test -p nimbus-convex convex_registry_applies_runtime_host_resource_budget_to_runtime_policy --lib -- --nocapture' \
    'cargo test -p nimbus-convex convex_registry_host_governor_preserves_runtime_metrics_identity --lib -- --nocapture' \
    '1 passed; 0 failed; 0 ignored; 0 measured; 23 filtered out' \
    'cargo test -p nimbus-convex convex_node_runtime_lanes_follow_lts_registry_targets --lib -- --nocapture' \
    'cargo test -p nimbus-cloud-functions cloud_functions_registry_applies_runtime_host_resource_budget_to_runtime_policy --lib -- --nocapture' \
    'cargo test -p nimbus-cloud-functions cloud_functions_registry_host_governor_preserves_runtime_metrics_identity --lib -- --nocapture' \
    '1 passed; 0 failed; 0 ignored; 0 measured; 28 filtered out' \
    'Summary: 82 passed, 0 failed' >/tmp/pir7-registry-proof-missing.txt &&
  contains_all "${PLAN}" \
    'PIR7 server registry host-budget injection slice' \
    'ConvexRegistry::with_runtime_host_resource_budget' \
    'ConvexRegistry::with_runtime_host_governor' \
    'CloudFunctionsRegistry::with_runtime_host_resource_budget' \
    'CloudFunctionsRegistry::with_runtime_host_governor' \
    'RouterBuildConfig::build' >/tmp/pir7-registry-plan-missing.txt; then
  pass "PIR7 host-resource budget is injected into Convex and Cloud Functions runtime registries"
else
  fail "PIR7 server registry host-budget injection slice is incomplete" \
    "$(cat /tmp/pir7-registry-router-missing.txt 2>/dev/null) $(cat /tmp/pir7-registry-convex-lane-missing.txt 2>/dev/null) $(cat /tmp/pir7-registry-convex-loading-missing.txt 2>/dev/null) $(cat /tmp/pir7-registry-convex-tests-missing.txt 2>/dev/null) $(cat /tmp/pir7-registry-cloud-functions-missing.txt 2>/dev/null) $(cat /tmp/pir7-registry-proof-missing.txt 2>/dev/null) $(cat /tmp/pir7-registry-plan-missing.txt 2>/dev/null)"
fi

step 83 "PIR7 reads real cgroup v2 host pressure for runtime admission"
if [ -f "${CGROUP_PRESSURE_SOURCE}" ] &&
  [ -f "${SERVER_ROUTER}" ] &&
  [ -f "${PIR7_HOST_BUDGET_PROOF}" ] &&
  contains_all "${CGROUP_PRESSURE_SOURCE}" \
    'pub struct CgroupV2HostPressureSource' \
    'pub struct CgroupV2CpuPressureThresholds' \
    'impl RuntimeHostPressureSource for CgroupV2HostPressureSource' \
    'fn sample' \
    'cpu.pressure' \
    'cpu.stat' \
    'parse_cpu_pressure' \
    'parse_cpu_stat' \
    'for_current_process' \
    'ensure_required_host_pressure_files' \
    'cgroup_v2_host_pressure_observes_high_cpu_pressure' \
    'cgroup_v2_host_pressure_observes_critical_cpu_pressure' \
    'cgroup_v2_host_pressure_degrades_when_cpu_pressure_is_unavailable' \
    'cpu_pressure_parser_accepts_one_decimal_centipercent' >/tmp/pir7-cgroup-source-missing.txt &&
  contains_all "${SERVER_ROUTER}" \
    'default_runtime_host_pressure_source' \
    'CgroupV2HostPressureSource::for_current_process' \
    'NominalRuntimeHostPressureSource' \
    'cgroup v2 host pressure source unavailable; using nominal runtime host pressure source' \
    'runtime_host_pressure_source: Arc<dyn RuntimeHostPressureSource>' >/tmp/pir7-cgroup-router-missing.txt &&
  contains_all "${PIR7_HOST_BUDGET_PROOF}" \
    'node cgroup v2 host-pressure source slice' \
    'CgroupV2HostPressureSource' \
    'CgroupV2CpuPressureThresholds' \
    'cpu.pressure' \
    'cpu.stat' \
    'default_runtime_host_pressure_source' \
    'cargo test -p nimbus-node host_pressure --lib -- --nocapture' \
    '3 passed; 0 failed; 0 ignored; 0 measured; 43 filtered out' \
    'cargo test -p nimbus-node cpu_pressure --lib -- --nocapture' \
    '5 passed; 0 failed; 0 ignored; 0 measured; 41 filtered out' \
    'cargo check -p nimbus-server --lib' \
    'Summary: 83 passed, 0 failed' >/tmp/pir7-cgroup-proof-missing.txt &&
  contains_all "${PLAN}" \
    'PIR7 node cgroup v2 host-pressure source slice' \
    'CgroupV2HostPressureSource' \
    'cpu.pressure' \
    'cpu.stat' \
    'default_runtime_host_pressure_source' >/tmp/pir7-cgroup-plan-missing.txt; then
  pass "PIR7 has a node-owned cgroup v2 host-pressure source feeding server runtime admission"
else
  fail "PIR7 node cgroup v2 host-pressure source slice is incomplete" \
    "$(cat /tmp/pir7-cgroup-source-missing.txt 2>/dev/null) $(cat /tmp/pir7-cgroup-router-missing.txt 2>/dev/null) $(cat /tmp/pir7-cgroup-proof-missing.txt 2>/dev/null) $(cat /tmp/pir7-cgroup-plan-missing.txt 2>/dev/null)"
fi

step 84 "PIR7 exposes low-cardinality host-pressure telemetry"
if [ -f "${RUNTIME_METRICS}" ] &&
  [ -f "${RUNTIME_METRICS_GLOBAL}" ] &&
  [ -f "${RUNTIME_LIMITS_POLICY}" ] &&
  [ -f "${RUNTIME_LIMITS_TEST}" ] &&
  [ -f "${PIR7_HOST_BUDGET_PROOF}" ] &&
  contains_all "${RUNTIME_METRICS}" \
    'pub struct RuntimeHostPressureMetricsSnapshot' \
    'pub host_pressure: RuntimeHostPressureMetricsSnapshot' \
    'record_host_resource_decision' \
    'host_pressure_metrics_snapshot_is_low_cardinality_global_state' \
    'host pressure metrics must not add tenant-cardinality labels' \
    'RuntimeHostPressureMetricsSnapshot::default' >/tmp/pir7-telemetry-metrics-missing.txt &&
  contains_all "${RUNTIME_METRICS_GLOBAL}" \
    'host_pressure_decisions' \
    'host_pressure_nominal_decisions' \
    'host_pressure_high_decisions' \
    'host_pressure_critical_decisions' \
    'host_pressure_cpu_source_unavailable_decisions' \
    'host_pressure_memory_source_unavailable_decisions' \
    'encode_host_pressure_level' \
    'decode_memory_pressure_source_status' >/tmp/pir7-telemetry-global-missing.txt &&
  contains_all "${RUNTIME_LIMITS_POLICY}" \
    'if self.host_resource_governor_enabled' \
    'record_host_resource_decision' \
    'decision' >/tmp/pir7-telemetry-policy-missing.txt &&
  contains_all "${RUNTIME_LIMITS_TEST}" \
    'runtime_policy_records_low_cardinality_host_pressure_metrics' \
    'host pressure telemetry must not add tenant-cardinality labels' \
    'latest_effective_dispatch_seats' >/tmp/pir7-telemetry-policy-test-missing.txt &&
  contains_all "${PIR7_HOST_BUDGET_PROOF}" \
    'low-cardinality host-pressure telemetry slice' \
    'RuntimeHostPressureMetricsSnapshot' \
    'record_host_resource_decision' \
    'cargo test -p nimbus-runtime host_pressure_metrics --lib -- --nocapture' \
    '2 passed; 0 failed; 0 ignored; 0 measured; 1034 filtered out' \
    'cargo test -p nimbus-runtime unattributed_metrics_do_not_create_tenant_entries --lib -- --nocapture' \
    '1 passed; 0 failed; 0 ignored; 0 measured; 1035 filtered out' \
    'cargo check -p nimbus-runtime --lib' \
    'Summary: 84 passed, 0 failed' >/tmp/pir7-telemetry-proof-missing.txt &&
  contains_all "${PLAN}" \
    'PIR7 low-cardinality host-pressure telemetry slice' \
    'RuntimeHostPressureMetricsSnapshot' \
    'record_host_resource_decision' >/tmp/pir7-telemetry-plan-missing.txt; then
  pass "PIR7 host-pressure telemetry is global, low-cardinality, and wired through RuntimePolicy"
else
  fail "PIR7 low-cardinality host-pressure telemetry slice is incomplete" \
    "$(cat /tmp/pir7-telemetry-metrics-missing.txt 2>/dev/null) $(cat /tmp/pir7-telemetry-global-missing.txt 2>/dev/null) $(cat /tmp/pir7-telemetry-policy-missing.txt 2>/dev/null) $(cat /tmp/pir7-telemetry-policy-test-missing.txt 2>/dev/null) $(cat /tmp/pir7-telemetry-proof-missing.txt 2>/dev/null) $(cat /tmp/pir7-telemetry-plan-missing.txt 2>/dev/null)"
fi

step 85 "PIR7 runs scheduled/manual PIR0/PIR2 crossover smoke benches in CI"
if [ -f "${CROSSOVER_SCRIPT}" ] &&
  [ -f "${MAKEFILE_PATH}" ] &&
  [ -f "${CI_WORKFLOW}" ] &&
  [ -f "${PIR7_HOST_BUDGET_PROOF}" ] &&
  contains_all "${CROSSOVER_SCRIPT}" \
    'runtime_pool_modes_pir0_profile_matrix/node22/hostless_trivial/cooperative_locker' \
    'runtime_pool_modes_pir2_context_recycle_impact/web_standard/hostless_trivial/cooperative_locker' \
    'NIMBUS_PIR0_TRACE_PATH' \
    'NIMBUS_PIR_CROSSOVER_SAMPLE_SIZE' \
    'warm_context_recycle' \
    'Profile-aware isolate runtime crossover smoke: pass' >/tmp/pir7-crossover-script-missing.txt &&
  contains_all "${MAKEFILE_PATH}" \
    'verify-profile-aware-runtime-crossover' \
    'scripts/verify-profile-aware-isolate-runtime-crossover.sh' \
    'bash -n scripts/verify-profile-aware-isolate-runtime-crossover.sh' >/tmp/pir7-crossover-make-missing.txt &&
  contains_all "${CI_WORKFLOW}" \
    'profile-aware-runtime-crossover' \
    'Profile-Aware Runtime Crossover' \
    'github.event_name == .schedule.' \
    'github.event_name == .workflow_dispatch.' \
    'ci-ubuntu-stable-profile-aware-runtime-crossover-no-bin-v1' \
    'make verify-profile-aware-runtime-crossover' >/tmp/pir7-crossover-ci-missing.txt &&
  contains_all "${PIR7_HOST_BUDGET_PROOF}" \
    'CI crossover guard slice' \
    'verify-profile-aware-isolate-runtime-crossover.sh' \
    'profile-aware-runtime-crossover' \
    'runtime_pool_modes_pir0_profile_matrix/node22/hostless_trivial/cooperative_locker' \
    'runtime_pool_modes_pir2_context_recycle_impact/web_standard/hostless_trivial/cooperative_locker' \
    'bash -n scripts/verify-profile-aware-isolate-runtime-crossover.sh' \
    'Summary: 85 passed, 0 failed' >/tmp/pir7-crossover-proof-missing.txt &&
  contains_all "${PLAN}" \
    'PIR7 CI crossover guard slice' \
    'profile-aware-runtime-crossover' \
    'make verify-profile-aware-runtime-crossover' >/tmp/pir7-crossover-plan-missing.txt; then
  pass "PIR7 has a scheduled/manual CI crossover smoke gate for PIR0/PIR2 benchmark drift"
else
  fail "PIR7 CI crossover guard slice is incomplete" \
    "$(cat /tmp/pir7-crossover-script-missing.txt 2>/dev/null) $(cat /tmp/pir7-crossover-make-missing.txt 2>/dev/null) $(cat /tmp/pir7-crossover-ci-missing.txt 2>/dev/null) $(cat /tmp/pir7-crossover-proof-missing.txt 2>/dev/null) $(cat /tmp/pir7-crossover-plan-missing.txt 2>/dev/null)"
fi

step 86 "PIR7 lowers safe cgroup controls into process-backed service workloads"
if [ -f "${MACHINE_SERVICE_WORKLOADS}" ] &&
  [ -f "${HOST_LIFECYCLE}" ] &&
  [ -f "${SYSTEMD_TRANSIENT}" ] &&
  [ -f "${PIR7_HOST_BUDGET_PROOF}" ] &&
  contains_all "${MACHINE_SERVICE_WORKLOADS}" \
    'SERVICE_WORKLOAD_DEFAULT_CPU_WEIGHT' \
    'SERVICE_WORKLOAD_CPU_WEIGHT_PER_VCPU' \
    'SERVICE_WORKLOAD_MAX_CPU_WEIGHT' \
    'SERVICE_WORKLOAD_DEFAULT_TASKS_MAX' \
    'with_cpu_weight.service_workload_cpu_weight.resources' \
    'with_tasks_max.SERVICE_WORKLOAD_DEFAULT_TASKS_MAX' \
    'service_workload_cpu_weight' \
    'service workload cpu_count must be greater than zero' \
    'guest_node_workload_service_uses_node_agent_and_typed_container_runner' \
    'service_container_runner_request_rejects_zero_cpu_count' \
    'HostLifecycleProperty::CpuWeight' \
    'HostLifecycleProperty::TasksMax' >/tmp/pir7-process-cgroup-machine-missing.txt &&
  contains_all "${HOST_LIFECYCLE}" \
    'HostLifecycleProperty::CpuWeight' \
    'HostLifecycleProperty::TasksMax' \
    'with_cpu_weight' \
    'with_tasks_max' \
    '"CPUWeight"' \
    '"TasksMax"' >/tmp/pir7-process-cgroup-host-missing.txt &&
  contains_all "${SYSTEMD_TRANSIENT}" \
    'Self::CpuWeight' \
    'Self::TasksMax' >/tmp/pir7-process-cgroup-systemd-missing.txt &&
  contains_all "${PIR7_HOST_BUDGET_PROOF}" \
    'process-backed service cgroup control slice' \
    'SERVICE_WORKLOAD_DEFAULT_TASKS_MAX' \
    'service_workload_cpu_weight' \
    'HostLifecycleProperty::CpuWeight' \
    'HostLifecycleProperty::TasksMax' \
    'cargo test -p nimbus-bin service_container_runner -- --nocapture' \
    '2 passed; 0 failed; 0 ignored; 0 measured; 733 filtered out' \
    'cargo test -p nimbus-bin guest_node_workload_service_uses_node_agent_and_typed_container_runner -- --nocapture' \
    '1 passed; 0 failed; 0 ignored; 0 measured; 734 filtered out' \
    'cargo test -p nimbus-node runner_spec --lib -- --nocapture' \
    '3 passed; 0 failed; 0 ignored; 0 measured; 43 filtered out' \
    'Summary: 86 passed, 0 failed' >/tmp/pir7-process-cgroup-proof-missing.txt &&
  contains_all "${PLAN}" \
    'PIR7 process-backed service cgroup control slice' \
    'CPUWeight' \
    'TasksMax' >/tmp/pir7-process-cgroup-plan-missing.txt; then
  pass "PIR7 process-backed service workloads get safe systemd cgroup controls without capping the Nimbus daemon"
else
  fail "PIR7 process-backed service cgroup control slice is incomplete" \
    "$(cat /tmp/pir7-process-cgroup-machine-missing.txt 2>/dev/null) $(cat /tmp/pir7-process-cgroup-host-missing.txt 2>/dev/null) $(cat /tmp/pir7-process-cgroup-systemd-missing.txt 2>/dev/null) $(cat /tmp/pir7-process-cgroup-proof-missing.txt 2>/dev/null) $(cat /tmp/pir7-process-cgroup-plan-missing.txt 2>/dev/null)"
fi

step 87 "PIR7 exposes fixed-bucket runtime-profile telemetry"
if [ -f "${RUNTIME_METRICS}" ] &&
  [ -f "${RUNTIME_METRICS_PROFILES}" ] &&
  [ -f "${RUNTIME_LIMITS_POLICY}" ] &&
  [ -f "${EXECUTOR_ADMISSION_PERMIT}" ] &&
  [ -f "${EXECUTOR_LIFECYCLE}" ] &&
  [ -f "${COOPERATIVE_WORKER_EXECUTION}" ] &&
  [ -f "${WARM_POOL}" ] &&
  [ -f "${RUNTIME_INVOCATION_DRIVER}" ] &&
  [ -f "${PIR7_HOST_BUDGET_PROOF}" ] &&
  contains_all "${RUNTIME_METRICS}" \
    'RuntimeProfileTelemetryRegistry' \
    'RuntimeProfileTelemetrySnapshot' \
    'pub profiles: RuntimeProfileTelemetrySnapshot' \
    'record_profile_invocation_started' \
    'record_profile_queue_wait' \
    'record_profile_execution' \
    'record_profile_runtime_pool_hit' \
    'profile_metrics_snapshot_is_fixed_bucket_runtime_state' \
    'profile metrics must not add tenant-cardinality labels' >/tmp/pir7-profile-telemetry-metrics-missing.txt &&
  contains_all "${RUNTIME_METRICS_PROFILES}" \
    'pub\(super\) struct RuntimeProfileTelemetryRegistry' \
    'pub struct RuntimeProfileCountersSnapshot' \
    'web_lean' \
    'node_full' \
    'unprofiled' \
    'started_invocations' \
    'completed_invocations' \
    'queue_wait_nanos_total' \
    'execution_nanos_total' \
    'runtime_pool_replacements' \
    'duration_to_nanos' >/tmp/pir7-profile-telemetry-profiles-missing.txt &&
  contains_all "${RUNTIME_LIMITS_POLICY}" \
    'pub\(crate\) fn runtime_profile' \
    'RuntimeProfile::for_limits' >/tmp/pir7-profile-telemetry-policy-missing.txt &&
  contains_all "${EXECUTOR_ADMISSION_PERMIT}" \
    'record_profile_invocation_started' \
    'record_profile_queue_wait' \
    'record_profile_invocation_completed' \
    'profiles.node_full.started_invocations' \
    'profiles.node_full.completed_invocations' >/tmp/pir7-profile-telemetry-permit-missing.txt &&
  contains_all "${EXECUTOR_LIFECYCLE}" \
    'let runtime_profile = policy.runtime_profile' \
    'record_profile_execution' >/tmp/pir7-profile-telemetry-lifecycle-missing.txt &&
  contains_all "${COOPERATIVE_WORKER_EXECUTION}" \
    'let runtime_profile = policy.runtime_profile' \
    'record_profile_execution' >/tmp/pir7-profile-telemetry-cooperative-missing.txt &&
  contains_all "${WARM_POOL}" \
    'record_profile_runtime_pool_hit' \
    'record_profile_runtime_pool_miss' >/tmp/pir7-profile-telemetry-warm-pool-missing.txt &&
  contains_all "${RUNTIME_INVOCATION_DRIVER}" \
    'record_profile_runtime_pool_replacement' >/tmp/pir7-profile-telemetry-invocation-missing.txt &&
  contains_all "${PIR7_HOST_BUDGET_PROOF}" \
    'fixed-bucket runtime-profile telemetry slice' \
    'RuntimeProfileTelemetrySnapshot' \
    'web_lean' \
    'node_full' \
    'unprofiled' \
    'recorded at the policy-owned admission, execution, warm-pool, and replacement call sites' \
    'cargo test -p nimbus-runtime profile_metrics_snapshot_is_fixed_bucket_runtime_state -- --nocapture' \
    '1 passed; 0 failed; 0 ignored; 0 measured; 1036 filtered out' \
    'cargo test -p nimbus-runtime try_acquire_initial_would_block_without_starting_invocation -- --nocapture' \
    'Summary: 87 passed, 0 failed' >/tmp/pir7-profile-telemetry-proof-missing.txt &&
  contains_all "${PLAN}" \
    'PIR7 fixed-bucket runtime-profile telemetry slice' \
    'RuntimeProfileTelemetrySnapshot' \
    'web_lean' \
    'node_full' \
    'unprofiled' >/tmp/pir7-profile-telemetry-plan-missing.txt; then
  pass "PIR7 runtime-profile telemetry is fixed-bucket and wired through policy-owned runtime paths"
else
  fail "PIR7 fixed-bucket runtime-profile telemetry slice is incomplete" \
    "$(cat /tmp/pir7-profile-telemetry-metrics-missing.txt 2>/dev/null) $(cat /tmp/pir7-profile-telemetry-profiles-missing.txt 2>/dev/null) $(cat /tmp/pir7-profile-telemetry-policy-missing.txt 2>/dev/null) $(cat /tmp/pir7-profile-telemetry-permit-missing.txt 2>/dev/null) $(cat /tmp/pir7-profile-telemetry-lifecycle-missing.txt 2>/dev/null) $(cat /tmp/pir7-profile-telemetry-cooperative-missing.txt 2>/dev/null) $(cat /tmp/pir7-profile-telemetry-warm-pool-missing.txt 2>/dev/null) $(cat /tmp/pir7-profile-telemetry-invocation-missing.txt 2>/dev/null) $(cat /tmp/pir7-profile-telemetry-proof-missing.txt 2>/dev/null) $(cat /tmp/pir7-profile-telemetry-plan-missing.txt 2>/dev/null)"
fi

step 88 "PIR7 closes with replay-gated adaptivity and live defaults off"
if [ -f "${RUNTIME_CONTROLLER_REPLAY}" ] &&
  [ -f "${RUNTIME_ADAPTIVE_CONTROLLER}" ] &&
  [ -f "${RUNTIME_LIB}" ] &&
  [ -f "${PIR7_HOST_BUDGET_PROOF}" ] &&
  contains_all "${RUNTIME_CONTROLLER_REPLAY}" \
    'pub struct RuntimeControllerReplayConfig' \
    'stable_window_observations' \
    'panic_window_observations' \
    'scale_down_hysteresis_observations' \
    'max_warm_runtimes_per_authority' \
    'max_warm_runtimes_per_tenant' \
    'compute_stall_signal_per_mille' \
    'pub fn replay_runtime_controller' \
    'apply_tenant_cap' \
    'controller_replay_uses_stable_and_panic_windows_for_burst_targets' \
    'controller_replay_holds_scale_down_until_hysteresis_expires' \
    'controller_replay_pauses_and_shrinks_under_memory_pressure' \
    'controller_replay_separates_compute_stall_from_warm_capacity_stall' \
    'controller_replay_applies_tenant_caps_to_zipf_hot_cold_mix' \
    'controller_replay_decays_periodic_load_with_rate_limit_after_hysteresis' >/tmp/pir7-controller-replay-missing.txt &&
  contains_all "${RUNTIME_ADAPTIVE_CONTROLLER}" \
    'pub struct RuntimeAdaptiveControllerSettings' \
    'live_adaptive_defaults_enabled: false' \
    'pub fn live_adaptive_defaults_enabled' >/tmp/pir7-controller-replay-adaptive-missing.txt &&
  contains_all "${RUNTIME_LIMITS_TEST}" \
    'runtime_policy_carries_adaptive_controller_settings_without_enabling_defaults' \
    'live_adaptive_defaults_enabled' >/tmp/pir7-controller-replay-policy-test-missing.txt &&
  contains_all "${RUNTIME_LIB}" \
    'RuntimeAdaptiveControllerSettings' \
    'RuntimeControllerReplayConfig' \
    'RuntimeControllerReplayDecision' \
    'replay_runtime_controller' >/tmp/pir7-controller-replay-export-missing.txt &&
  contains_all "${PIR7_HOST_BUDGET_PROOF}" \
    'controller replay closeout slice' \
    'RuntimeAdaptiveControllerSettings' \
    'live adaptive defaults remain off by default' \
    'stable and panic windows' \
    'hysteresis' \
    'rate limits' \
    'tenant caps' \
    'Zipf hot/cold' \
    'periodic load' \
    'cargo test -p nimbus-runtime adaptive_controller --lib -- --nocapture' \
    '8 passed; 0 failed; 0 ignored; 0 measured; 1118 filtered out' \
    'cargo test -p nimbus-runtime controller_replay --lib -- --nocapture' \
    '6 passed; 0 failed; 0 ignored; 0 measured; 1120 filtered out' \
    'Summary: 88 passed, 0 failed' \
    'PIR7 status: done' >/tmp/pir7-controller-replay-proof-missing.txt &&
  contains_all "${PLAN}" \
    '\| PIR7 \| `done`' \
    '\| PIR8 \| `deferred`' \
    'PIR7 controller replay closeout' \
    'RuntimeAdaptiveControllerSettings' \
    'live adaptive defaults remain off by default' \
    'controller_replay' \
    'Summary: 88 passed, 0 failed' >/tmp/pir7-controller-replay-plan-missing.txt; then
  pass "PIR7 controller replay is proven and live adaptive defaults remain off"
else
  fail "PIR7 controller replay closeout is incomplete" \
    "$(cat /tmp/pir7-controller-replay-missing.txt 2>/dev/null) $(cat /tmp/pir7-controller-replay-adaptive-missing.txt 2>/dev/null) $(cat /tmp/pir7-controller-replay-policy-test-missing.txt 2>/dev/null) $(cat /tmp/pir7-controller-replay-export-missing.txt 2>/dev/null) $(cat /tmp/pir7-controller-replay-proof-missing.txt 2>/dev/null) $(cat /tmp/pir7-controller-replay-plan-missing.txt 2>/dev/null)"
fi

step 89 "Final architecture records target-bounded pointer-compression defaults and exemplar import rules"
if [ -f "${FINAL_ARCH_PLAN}" ] &&
  contains_all "${FINAL_ARCH_PLAN}" \
    'target-bounded pointer' \
    'Pointer Compression As Target-Bounded Production Path' \
    'Exemplar Pattern Import Rules' \
    'Thread-pinned isolate ownership' \
    'Authority-keyed warm reuse' \
    'Bounded fairness and backpressure' \
    'Two-phase completion' \
    'Reset-or-discard lifecycle' \
    'Startup snapshot versus module code cache' \
    'Explicit host-value transfer interface' \
    'prebuilt-supported ptrcomp' \
    'supported Linux ARM64 ptrcomp release artifact' \
    'not a wholesale target-set contract' \
    'production-default targets' \
    'skips only full `nextest` under user-mode QEMU' \
    'aarch64-apple-darwin' \
    'x86_64-unknown-linux-gnu' \
    'aarch64-unknown-linux-gnu' \
    'Windows MSVC \| non-ptrcomp' \
    'native Linux ARM64 Nimbus release build lane' \
	    'not production behavior' \
	    'skips only full `nextest` under user-mode QEMU' \
	    'release aarch64-unknown-linux-gnu ptrcomp simdutf' \
	    'Clippy ptrcomp simdutf' \
    'published release has 22 assets' \
    'QEMU runtime success or failure as production proof for Linux ARM64 ptrcomp' \
    'Node/Deno warm reuse must remain fail-closed' \
    'Nimbus.s runtime Interface should stay deeper than the exemplars' \
    'Isolate Lifecycle Blueprint' \
    'Process/platform startup' \
    'Admission and pool selection' \
    'Acquire' \
    'Create' \
    'Execute' \
    'Background drain' \
    'Return and retain' \
    'Delete and retire' \
    'Pressure cleanup' \
    'WebStandard and Deno/Node Lifecycle Split' \
    'Each active request owns one live context/global at a time' \
    'Do not copy the WebStandard reset Implementation into Node/Deno' \
    'extension-JS replay, module maps, op state, async hooks' \
    'OpenWorkers. strongest lesson is not "copy every WebStandard context-reuse' \
    'thread-pinned, authority-keyed, reset-or-discard' >/tmp/pir-final-architecture-missing.txt; then
  pass "final architecture distinguishes ptrcomp target policy and canonical isolate lifecycle import rules"
else
  fail "final architecture target-bounded pointer-compression decision is incomplete" \
    "$(cat /tmp/pir-final-architecture-missing.txt 2>/dev/null)"
fi

step 90 "PIR release builds enable pointer compression only on supported targets"
if [ -f "${RELEASE_FEATURE_SCRIPT}" ] &&
  [ -f "${RELEASE_WORKFLOW}" ] &&
  [ -f "${CI_WORKFLOW}" ] &&
  [ -f "${MAKEFILE_PATH}" ] &&
  [ -f "${NIMBUS_CARGO}" ] &&
  [ -f "${NIMBUS_BIN_CARGO}" ] &&
  [ -f "${PIR5_POINTER_COMPRESSION_PROOF}" ] &&
  [ -f "${FINAL_ARCH_PLAN}" ] &&
  bash -n "${RELEASE_FEATURE_SCRIPT}" &&
  linux_args="$(bash "${RELEASE_FEATURE_SCRIPT}" --target x86_64-unknown-linux-gnu --format cargo-args)" &&
  darwin_args="$(bash "${RELEASE_FEATURE_SCRIPT}" --target aarch64-apple-darwin --format cargo-args)" &&
  linux_arm_args="$(bash "${RELEASE_FEATURE_SCRIPT}" --target aarch64-unknown-linux-gnu --format cargo-args)" &&
  windows_args="$(bash "${RELEASE_FEATURE_SCRIPT}" --target x86_64-pc-windows-msvc --format cargo-args)" &&
  [ "${linux_args}" = "--features v8-pointer-compression" ] &&
  [ "${darwin_args}" = "--features v8-pointer-compression" ] &&
  [ "${linux_arm_args}" = "--features v8-pointer-compression" ] &&
  [ -z "${windows_args}" ] &&
  contains_all "${RELEASE_FEATURE_SCRIPT}" \
    'x86_64-unknown-linux-gnu' \
    'aarch64-apple-darwin' \
    'aarch64-unknown-linux-gnu' \
    'x86_64-pc-windows-msvc' \
    'v8-pointer-compression' \
    'cargo-args' \
    'feature-list' >/tmp/pir-release-features-script-missing.txt &&
  contains_all "${NIMBUS_CARGO}" \
    'v8-pointer-compression = .*nimbus-runtime/v8-pointer-compression' >/tmp/pir-release-features-nimbus-missing.txt &&
  contains_all "${NIMBUS_BIN_CARGO}" \
    'v8-pointer-compression = .*nimbus/v8-pointer-compression' >/tmp/pir-release-features-bin-missing.txt &&
  contains_all "${MAKEFILE_PATH}" \
    'scripts/nimbus-release-rust-features.sh --format cargo-args' \
    'cargo build --release -p nimbus-bin .*\$\$cargo_features' \
    'bash -n scripts/nimbus-release-rust-features.sh' >/tmp/pir-release-features-make-missing.txt &&
  contains_all "${RELEASE_WORKFLOW}" \
    'Verify release runtime feature selector' \
    'scripts/nimbus-release-rust-features.sh --target x86_64-unknown-linux-gnu' \
    'scripts/nimbus-release-rust-features.sh --target aarch64-apple-darwin' \
    'scripts/nimbus-release-rust-features.sh --target aarch64-unknown-linux-gnu' \
    'scripts/nimbus-release-rust-features.sh --target x86_64-pc-windows-msvc' \
    'runtime_feature_mode: ptrcomp' \
    'runtime_feature_mode: non-ptrcomp' \
    'release-\$\{\{ matrix.target \}\}-\$\{\{ matrix.runtime_feature_mode \}\}-no-bin-v1' \
    'release-aarch64-unknown-linux-gnu-ptrcomp-no-bin-v1' \
    'cargo build --release -p nimbus-bin \$\{cargo_features\}' >/tmp/pir-release-features-workflow-missing.txt &&
  contains_all "${CI_WORKFLOW}" \
    'rust-runtime-ptrcomp-check' \
    'Rust Runtime Ptrcomp Check' \
    'ci-ubuntu-stable-runtime-ptrcomp-no-bin-v1' \
    'env -u V8_FROM_SOURCE cargo check -p nimbus-runtime --lib --features v8-pointer-compression' \
    'v8 feature "v8_enable_pointer_compression"' \
    'v8 feature "simdutf"' >/tmp/pir-release-features-ci-missing.txt &&
  contains_all "${PIR5_POINTER_COMPRESSION_PROOF}" \
    'Target-Specific Release Default Stabilizer' \
    'scripts/nimbus-release-rust-features.sh' \
    'x86_64-unknown-linux-gnu` -> `--features v8-pointer-compression' \
    'aarch64-apple-darwin` -> `--features v8-pointer-compression' \
    'aarch64-unknown-linux-gnu` -> `--features v8-pointer-compression' \
    'x86_64-pc-windows-msvc` -> no additional Cargo feature' \
    'rust-runtime-ptrcomp-check' \
    'Summary: 108 passed, 0 failed' >/tmp/pir-release-features-proof-missing.txt &&
  contains_all "${FINAL_ARCH_PLAN}" \
    'Target-specific pointer-compression release-default policy is implemented' \
    'scripts/nimbus-release-rust-features.sh' \
    'Release builds enable `v8-pointer-compression` for' \
    'x86_64-unknown-linux-gnu' \
    'aarch64-apple-darwin' \
    'aarch64-unknown-linux-gnu' \
    'Release builds intentionally emit no pointer-compression feature for' \
    'x86_64-pc-windows-msvc' \
    'rust-runtime-ptrcomp-check' >/tmp/pir-release-features-arch-missing.txt &&
  contains_all "${PLAN}" \
    'PIR target-specific pointer-compression release-default' \
    'stabilizer: added `scripts/nimbus-release-rust-features.sh`' \
    'scripts/nimbus-release-rust-features.sh' \
    'Summary: 108 passed, 0 failed' >/tmp/pir-release-features-plan-missing.txt; then
  pass "release feature policy is target-specific, supported-target ptrcomp, and unsupported-target non-ptrcomp"
else
  fail "PIR target-specific pointer-compression release-default stabilizer is incomplete" \
    "$(cat /tmp/pir-release-features-script-missing.txt 2>/dev/null) $(cat /tmp/pir-release-features-nimbus-missing.txt 2>/dev/null) $(cat /tmp/pir-release-features-bin-missing.txt 2>/dev/null) $(cat /tmp/pir-release-features-make-missing.txt 2>/dev/null) $(cat /tmp/pir-release-features-workflow-missing.txt 2>/dev/null) $(cat /tmp/pir-release-features-ci-missing.txt 2>/dev/null) $(cat /tmp/pir-release-features-proof-missing.txt 2>/dev/null) $(cat /tmp/pir-release-features-arch-missing.txt 2>/dev/null) $(cat /tmp/pir-release-features-plan-missing.txt 2>/dev/null)"
fi

step 91 "Post-PIR optimization benchmark backlog is scaffolded and guarded"
if [ -f "${FINAL_ARCH_PLAN}" ] &&
  [ -f "${PLAN}" ] &&
  [ -f "${POST_PIR_OPTIMIZATION_PROOF}" ] &&
  contains_all "${FINAL_ARCH_PLAN}" \
    'Post-PIR Optimization Benchmark Backlog' \
    'WebStandard warm-pool parity versus OpenWorkers-style path' \
    'Warm-hit overhead breakdown' \
    'Hot-tail prewarm policy' \
    'Pool sizing and eviction curves' \
    'Cooperative scheduler under mixed I/O and CPU' \
    'Exact-key fragmentation cost' \
    'NodeFull lazy initialization' \
    'Replay-based adaptive controller' \
    'OpenWorkers-style owner-keyed diagnostic path' \
    'never becomes a shipping' \
    'do not benchmark fresh-realm pooling again as a default candidate' >/tmp/pir-post-optimization-arch-missing.txt &&
  contains_all "${PLAN}" \
    'Post-PIR optimization handoff' \
    'post-pir-optimization-benchmarks.md' \
    'WebStandard exact-key warm-pool parity' \
    'warm-hit overhead attribution' \
    'fixed-window hot-tail prewarm behavior' \
    'pool sizing / pressure-eviction curves' \
    'cooperative scheduler mixed I/O/CPU rows' \
    'cannot weaken Nimbus' \
    'Once PIR0-PIR7 are closed, continue into the Post-PIR optimization benchmark backlog' >/tmp/pir-post-optimization-plan-missing.txt &&
  contains_all "${POST_PIR_OPTIMIZATION_PROOF}" \
    'Status: first-wave WebStandard parity matrix recorded' \
    'No benchmark row in this file is accepted as complete until raw JSONL' \
    'WebStandard exact-key WarmPool' \
    'OpenWorkers-style owner-keyed diagnostic path' \
    'StartupSnapshotCache' \
    'WarmContextRecycle' \
    'hostless-trivial' \
    'setup-heavy large-module' \
    'async host-call' \
    'CPU-bound JIT-hot' \
    'multi-tenant Zipf' \
    'high authority-key fragmentation' \
    'host-pressure or memory-pressure' \
    'Do not weaken authority-key dimensions' \
    'Do not reopen NodeFull fresh-realm pooling' \
    'Condition 91' >/tmp/pir-post-optimization-proof-missing.txt; then
  pass "post-PIR optimization backlog has proof and verifier guardrails"
else
  fail "Post-PIR optimization benchmark backlog scaffold is incomplete" \
    "$(cat /tmp/pir-post-optimization-arch-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-plan-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-proof-missing.txt 2>/dev/null)"
fi

step 92 "Post-PIR optimization benchmark harness is wired and opt-in"
if [ -f "${BENCH}" ] &&
  [ -f "${POST_PIR_BENCH_MODULE}" ] &&
  [ -f "${POST_PIR_OPTIMIZATION_PROOF}" ] &&
  contains_all "${BENCH}" \
    'runtime_pool_modes/post_pir.rs' \
    'runtime_pool_modes_post_pir::post_pir_optimization_benchmark' \
    'build_runtime_with_config' >/tmp/pir-post-optimization-bench-missing.txt &&
  contains_all "${POST_PIR_BENCH_MODULE}" \
    'nimbus.profile_aware_isolate_runtime.post_pir.optimization.v1' \
    'NIMBUS_POST_PIR_OPTIMIZATION_BENCH' \
    'NIMBUS_POST_PIR_TRACE_PATH' \
    'webstandard_exact_key_warm_pool' \
    'openworkers_owner_keyed_diagnostic' \
    'startup_snapshot_cache' \
    'warm_context_recycle_diagnostic' \
    'single_tenant' \
    'zipf_hot_tenant' \
    'high_authority_fragmentation' \
    'RuntimeRoutingAffinity::None' \
    'RuntimeRoutingAffinity::Function' \
    'authority_relaxed_diagnostic' \
    'latency_p999_nanos' \
    'request_correlation_nanos_total' \
    'execution_plan_build_nanos_total' \
    'admission_decision_nanos_total' \
    'worker_router_dispatch_nanos_total' \
    'bundle_integrity_verify_nanos_total' \
    'host_bridge_call_nanos_total' \
    'host_pressure_decisions' \
    'latest_effective_dispatch_seats' >/tmp/pir-post-optimization-module-missing.txt &&
  contains_all "${POST_PIR_OPTIMIZATION_PROOF}" \
    'Harness support slice 1' \
    'crates/nimbus-runtime/benches/runtime_pool_modes/post_pir.rs' \
    'NIMBUS_POST_PIR_OPTIMIZATION_BENCH' \
    'NIMBUS_POST_PIR_TRACE_PATH' \
    'authority_relaxed_diagnostic' \
    'cargo check -p nimbus-runtime --benches' \
    'benchmark rows are opt-in' >/tmp/pir-post-optimization-harness-proof-missing.txt; then
  pass "post-PIR optimization benchmark harness is opt-in and traceable"
else
  fail "Post-PIR optimization benchmark harness is incomplete" \
    "$(cat /tmp/pir-post-optimization-bench-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-module-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-harness-proof-missing.txt 2>/dev/null)"
fi

step 93 "Post-PIR optimization benchmark smoke trace is recorded"
if [ -f "${POST_PIR_OPTIMIZATION_SMOKE_TRACE}" ] &&
  [ -f "${POST_PIR_OPTIMIZATION_PROOF}" ] &&
  contains_all "${POST_PIR_OPTIMIZATION_SMOKE_TRACE}" \
    'nimbus.profile_aware_isolate_runtime.post_pir.optimization.v1' \
    'hostless_trivial/single_tenant/webstandard_exact_key_warm_pool' \
    '"authority_relaxed_diagnostic":false' \
    '"measured_iterations":32' \
    '"runtime_pool_hits":32' \
    '"runtime_pool_misses":1' \
    '"warm_pool_hits":32' \
    '"warm_pool_misses":1' \
    '"retained_runtime_pool_entries":1' \
    '"host_pressure_decisions":0' >/tmp/pir-post-optimization-smoke-trace-missing.txt &&
  contains_all "${POST_PIR_OPTIMIZATION_PROOF}" \
    'Smoke trace slice 1' \
    'post-pir-optimization-smoke.jsonl' \
    'runtime_pool_modes_post_pir_optimization/hostless_trivial/single_tenant/webstandard_exact_key_warm_pool' \
    '6 JSONL records' \
    'time:   \[978.23' \
    '1 warm miss, 32 warm hits' \
    'Summary: 93 passed, 0 failed' >/tmp/pir-post-optimization-smoke-proof-missing.txt; then
  pass "post-PIR optimization smoke trace records exact-key warm-pool execution"
else
  fail "Post-PIR optimization benchmark smoke trace is incomplete" \
    "$(cat /tmp/pir-post-optimization-smoke-trace-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-smoke-proof-missing.txt 2>/dev/null)"
fi

step 94 "Post-PIR optimization first-wave matrix is recorded"
if [ -f "${POST_PIR_OPTIMIZATION_FIRST_WAVE_TRACE}" ] &&
  [ "$(wc -l < "${POST_PIR_OPTIMIZATION_FIRST_WAVE_TRACE}" | tr -d ' ')" -eq 202 ] &&
  [ -f "${POST_PIR_OPTIMIZATION_PROOF}" ] &&
  contains_all "${POST_PIR_OPTIMIZATION_FIRST_WAVE_TRACE}" \
    'hostless_trivial/single_tenant/webstandard_exact_key_warm_pool' \
    'hostless_trivial/single_tenant/openworkers_owner_keyed_diagnostic' \
    'hostless_trivial/single_tenant/startup_snapshot_cache' \
    'hostless_trivial/single_tenant/warm_context_recycle_diagnostic' \
    'setup_heavy_large_module/zipf_hot_tenant/webstandard_exact_key_warm_pool' \
    'compute_bound_jit_hot/high_authority_fragmentation/openworkers_owner_keyed_diagnostic' \
    'async_host_call/high_authority_fragmentation/warm_context_recycle_diagnostic' \
    '"authority_relaxed_diagnostic":true' \
    '"authority_relaxed_diagnostic":false' \
    '"tenant_distribution":"high_authority_fragmentation"' \
    '"latency_p999_nanos"' >/tmp/pir-post-optimization-first-wave-trace-missing.txt &&
  contains_all "${POST_PIR_OPTIMIZATION_PROOF}" \
    'First-wave matrix slice 1' \
    'post-pir-optimization-first-wave.jsonl' \
    '202 JSONL records' \
    '48 final rows' \
    'Exact-key WebStandard warm pool | 1.320 ms' \
    'OpenWorkers-style owner-keyed diagnostic | 1.196 ms' \
    'Startup snapshot cache | 2.189 ms' \
    'Warm-context recycle diagnostic | 10.569 ms' \
    'Exact-key WebStandard warm pools are in the same class as the owner-keyed' \
    'does not justify weakening authority keys' \
    'Warm-context recycle remains diagnostic only' \
    'High-authority fragmentation did not create catastrophic warm-pool overhead' \
    'Summary: 94 passed, 0 failed' >/tmp/pir-post-optimization-first-wave-proof-missing.txt; then
  pass "post-PIR first-wave matrix records exact-key warm-pool comparison results"
else
  fail "Post-PIR optimization first-wave matrix is incomplete" \
    "$(cat /tmp/pir-post-optimization-first-wave-trace-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-first-wave-proof-missing.txt 2>/dev/null)"
fi

step 95 "Post-PIR warm-hit attribution trace is recorded"
if [ -f "${POST_PIR_OPTIMIZATION_ATTRIBUTION_TRACE}" ] &&
  [ "$(wc -l < "${POST_PIR_OPTIMIZATION_ATTRIBUTION_TRACE}" | tr -d ' ')" -eq 10 ] &&
  [ -f "${POST_PIR_OPTIMIZATION_PROOF}" ] &&
  [ -f "${FINAL_ARCH_PLAN}" ] &&
  contains_all "${POST_PIR_OPTIMIZATION_ATTRIBUTION_TRACE}" \
    'hostless_trivial/single_tenant/webstandard_exact_key_warm_pool' \
    'async_host_call/single_tenant/webstandard_exact_key_warm_pool' \
    '"request_correlation_nanos_total"' \
    '"execution_plan_build_nanos_total"' \
    '"admission_decision_nanos_total"' \
    '"worker_router_dispatch_nanos_total"' \
    '"bundle_integrity_verify_nanos_total"' \
    '"host_bridge_call_nanos_total"' \
    '"host_bridge_calls":0' \
    '"host_bridge_calls":9' \
    '"warm_pool_hits":32' \
    '"warm_pool_hits":8' >/tmp/pir-post-optimization-attribution-trace-missing.txt &&
  contains_all "${POST_PIR_OPTIMIZATION_PROOF}" \
    'Warm-Hit Attribution Slice 2' \
    'request-correlation metrics bookkeeping' \
    'execution-plan construction' \
    'tenant/host admission' \
    'worker router dispatch' \
    'bundle integrity verification' \
    'HostBridge call/wait duration' \
    'post-pir-warm-hit-attribution.jsonl' \
    '10 JSONL records' \
    '2 final rows' \
    'hostless_trivial/single_tenant/webstandard_exact_key_warm_pool' \
    'async_host_call/single_tenant/webstandard_exact_key_warm_pool' \
    'does not show a large unexplained enterprise' \
    'substrate bucket in policy' \
    'dominated by HostBridge wait/call time' \
    'Condition 95' >/tmp/pir-post-optimization-attribution-proof-missing.txt &&
  contains_all "${FINAL_ARCH_PLAN}" \
    'First-wave calibration result' \
    'not justify weakening Nimbus authority keys' \
    'Initial warm-hit attribution shows policy bookkeeping' \
    'Fanout retained-density rows now show' \
    'authority-key collapse' >/tmp/pir-post-optimization-attribution-arch-missing.txt; then
  pass "post-PIR warm-hit attribution records runtime-owned layer counters"
else
  fail "Post-PIR warm-hit attribution trace is incomplete" \
    "$(cat /tmp/pir-post-optimization-attribution-trace-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-attribution-proof-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-attribution-arch-missing.txt 2>/dev/null)"
fi

step 96 "Startup snapshots partition service-extension state and host-bridge sessions are live-bound"
if [ -f "${V8_STARTUP_KEY}" ] &&
  [ -f "${V8_STARTUP}" ] &&
  [ -f "${BOOTSTRAP_EXTENSIONS}" ] &&
  [ -f "${RUNTIME_CONSTRUCTION}" ] &&
  [ -f "${RUNTIME_HOST_BRIDGE_TEST}" ] &&
  [ -f "${POST_PIR_OPTIMIZATION_PROOF}" ] &&
  contains_all "${V8_STARTUP_KEY}" \
    'WebLeanService' \
    'NodeFullService' \
    'service_capability_enabled && limits.grants.has_service_grants' \
    'service_extension_enabled' \
    'startup_snapshot_key_partitions_optional_service_extension' >/tmp/pir-post-optimization-snapshot-key-missing.txt &&
  contains_all "${V8_STARTUP}" \
    'deno_core::shared_ro_heap_serialize_lock\(\)' \
    'create_v8_startup_snapshot' \
    'service_extension_enabled: bool' \
    'snapshot_extensions\(compatibility_target, service_extension_enabled\)' >/tmp/pir-post-optimization-startup-missing.txt &&
  contains_all "${BOOTSTRAP_EXTENSIONS}" \
    'snapshot_extensions' \
    'service_extension_enabled: bool' \
    'extensions.push\(service_extension\(\)\)' >/tmp/pir-post-optimization-bootstrap-missing.txt &&
  contains_all "${RUNTIME_CONSTRUCTION}" \
    'WEB_STANDARD_SERVICE_BOOTSTRAP_SNAPSHOT' \
    'NODE_FULL_SERVICE_BOOTSTRAP_SNAPSHOT' \
    'RuntimeStartupSnapshotKey::for_limits' \
    'snapshot_key.service_extension_enabled\(\)' >/tmp/pir-post-optimization-construction-missing.txt &&
  contains_all "${RUNTIME_HOST_BRIDGE_TEST}" \
    'acquire_runtime_suite_lock\(\)' \
    'query:services:get' \
    'mutation:messages:write' \
    'query:messages:listPage' \
    'action:messages:outer' >/tmp/pir-post-optimization-host-bridge-missing.txt &&
  ! contains 'host-call-session-1' "${RUNTIME_HOST_BRIDGE_TEST}" &&
  contains_all "${POST_PIR_OPTIMIZATION_PROOF}" \
    'Host-Bridge Session And Snapshot Safety Closeout' \
    'WebLeanService' \
    'NodeFullService' \
    'deno_core::shared_ro_heap_serialize_lock' \
    'cargo test -p nimbus-runtime backends::v8::startup_key --lib' \
    '3 passed; 0 failed; 1115 filtered out' \
    'cargo test -p nimbus-runtime runtime::tests::host_bridge --lib' \
    '16 passed; 0 failed; 1102 filtered out' \
    'Condition 96' >/tmp/pir-post-optimization-safety-proof-missing.txt; then
  pass "service-enabled startup snapshots are partitioned and host-bridge sessions use live invocation context"
else
  fail "Post-PIR host-bridge/session snapshot safety closeout is incomplete" \
    "$(cat /tmp/pir-post-optimization-snapshot-key-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-startup-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-bootstrap-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-construction-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-host-bridge-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-safety-proof-missing.txt 2>/dev/null)"
fi

step 97 "Post-PIR fanout retained-density curve is recorded"
if [ -f "${POST_PIR_OPTIMIZATION_FANOUT_TRACE}" ] &&
  [ "$(wc -l < "${POST_PIR_OPTIMIZATION_FANOUT_TRACE}" | tr -d ' ')" -eq 64 ] &&
  [ -f "${POST_PIR_BENCH_MODULE}" ] &&
  [ -f "${POST_PIR_OPTIMIZATION_PROOF}" ] &&
  [ -f "${FINAL_ARCH_PLAN}" ] &&
  contains_all "${POST_PIR_BENCH_MODULE}" \
    'nimbus.profile_aware_isolate_runtime.post_pir.fanout.v1' \
    'POST_PIR_FANOUT_GROUP' \
    'NIMBUS_POST_PIR_FANOUT_BENCH' \
    'PostPirFanoutShape' \
    'rss_after_prime_bytes' \
    'rss_prime_delta_bytes' \
    'rss_per_retained_entry_bytes' \
    'routing_affinity_max_entries' \
    'RuntimeRoutingAffinity::Function' >/tmp/pir-post-optimization-fanout-module-missing.txt &&
  contains_all "${POST_PIR_OPTIMIZATION_FANOUT_TRACE}" \
    'nimbus.profile_aware_isolate_runtime.post_pir.fanout.v1' \
    'runtime_pool_modes_post_pir_fanout' \
    'hostless_trivial/fanout_8_retained_cap_1/webstandard_exact_key_warm_pool' \
    'hostless_trivial/fanout_8_retained_cap_8/webstandard_exact_key_warm_pool' \
    'setup_heavy_large_module/fanout_64_retained_cap_64/webstandard_exact_key_warm_pool' \
    'openworkers_owner_keyed_diagnostic' \
    '"authority_fanout":64' \
    '"retained_cap":64' \
    '"rss_after_prime_bytes"' \
    '"rss_prime_delta_bytes"' \
    '"rss_per_retained_entry_bytes"' \
    '"retained_runtime_pool_entries":64' \
    '"retained_runtime_pool_evictions":0' \
    '"retained_runtime_pool_evictions":49' >/tmp/pir-post-optimization-fanout-trace-missing.txt &&
  contains_all "${POST_PIR_OPTIMIZATION_PROOF}" \
    'Fanout Retained Density Slice 3' \
    'runtime_pool_modes_post_pir_fanout' \
    'post-pir-fanout-retained-density-current-rss.jsonl' \
    '64 JSONL records' \
    '24 final rows' \
    'hostless_trivial | 8 | 8 | 0.986' \
    'setup_heavy_large_module | 64 | 64 | 1.541' \
    '107.422 MiB' \
    'undersized exact-key pools can thrash under fanout' \
    'When retained cap covers fanout' \
    'Condition 97' >/tmp/pir-post-optimization-fanout-proof-missing.txt &&
  contains_all "${FINAL_ARCH_PLAN}" \
    'Fanout retained-density rows now show' \
    'cap-equals-' \
    '107.422 MiB' \
    'Fixed-window hot-tail prewarm rows show' \
    'authority-key collapse' >/tmp/pir-post-optimization-fanout-arch-missing.txt; then
  pass "post-PIR fanout retained-density rows quantify exact-key thrash and RSS scaling"
else
  fail "Post-PIR fanout retained-density curve is incomplete" \
    "$(cat /tmp/pir-post-optimization-fanout-module-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-fanout-trace-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-fanout-proof-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-fanout-arch-missing.txt 2>/dev/null)"
fi

step 98 "Post-PIR hot-tail prewarm policy curve is recorded"
if [ -f "${POST_PIR_OPTIMIZATION_HOT_TAIL_TRACE}" ] &&
  [ "$(wc -l < "${POST_PIR_OPTIMIZATION_HOT_TAIL_TRACE}" | tr -d ' ')" -eq 12 ] &&
  [ -f "${POST_PIR_BENCH_MODULE}" ] &&
  [ -f "${POST_PIR_OPTIMIZATION_PROOF}" ] &&
  [ -f "${FINAL_ARCH_PLAN}" ] &&
  contains_all "${POST_PIR_BENCH_MODULE}" \
    'nimbus.profile_aware_isolate_runtime.post_pir.hot_tail_prewarm.v1' \
    'POST_PIR_HOT_TAIL_GROUP' \
    'NIMBUS_POST_PIR_HOT_TAIL_BENCH' \
    'POST_PIR_TRACE_MIN_HOT_TAIL_ITERATIONS' \
    'PostPirHotTailPrewarmShape' \
    'hot_tail_authority_index' \
    'prewarm_paused_by_memory_pressure' \
    'post_pir_trace_iterations' >/tmp/pir-post-optimization-hot-tail-module-missing.txt &&
  contains_all "${POST_PIR_OPTIMIZATION_HOT_TAIL_TRACE}" \
    'nimbus.profile_aware_isolate_runtime.post_pir.hot_tail_prewarm.v1' \
    'runtime_pool_modes_post_pir_hot_tail_prewarm' \
    'hostless_trivial/prewarm_0_cap_16_pressure_nominal/webstandard_exact_key_warm_pool' \
    'hostless_trivial/prewarm_16_cap_16_pressure_nominal/webstandard_exact_key_warm_pool' \
    'setup_heavy_large_module/prewarm_8_cap_16_pressure_critical_memory/webstandard_exact_key_warm_pool' \
    '"measured_iterations":128' \
    '"admitted_prewarm_entries":16' \
    '"admitted_prewarm_entries":0' \
    '"prewarm_paused_by_memory_pressure":true' \
    '"prewarm_memory_pressure_level":"critical"' \
    '"retained_runtime_pool_evictions":24' \
    '"warm_pool_hits":104' >/tmp/pir-post-optimization-hot-tail-trace-missing.txt &&
  contains_all "${POST_PIR_OPTIMIZATION_PROOF}" \
    'Hot-Tail Prewarm Slice 4' \
    'runtime_pool_modes_post_pir_hot_tail_prewarm' \
    'post-pir-hot-tail-prewarm-fixed.jsonl' \
    '12 JSONL records and 12 final rows' \
    'Each final row records 128 measured invocations' \
    'hostless_trivial | 16 | 16 | nominal | 1.016 | 2.900 | 3.092' \
    'setup_heavy_large_module | 8 | 0 | critical | 1.474 | 3.991 | 4.611' \
    'Tail cold misses still dominate p95/p99' \
    'admit zero requested prewarm entries' \
    'Condition 98' >/tmp/pir-post-optimization-hot-tail-proof-missing.txt &&
  contains_all "${FINAL_ARCH_PLAN}" \
    'Fixed-window hot-tail prewarm rows show' \
    'not the main p95/p99 lever' \
    'admits zero speculative prewarm entries' \
    'demand-driven retention and cap sizing' >/tmp/pir-post-optimization-hot-tail-arch-missing.txt; then
  pass "post-PIR fixed-window hot-tail prewarm rows prove pressure-safe prewarm and tail-limited p95/p99"
else
  fail "Post-PIR hot-tail prewarm policy curve is incomplete" \
    "$(cat /tmp/pir-post-optimization-hot-tail-module-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-hot-tail-trace-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-hot-tail-proof-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-hot-tail-arch-missing.txt 2>/dev/null)"
fi

step 99 "Post-PIR pool sizing and pressure-eviction curve is recorded"
if [ -f "${POST_PIR_OPTIMIZATION_POOL_SIZING_TRACE}" ] &&
  [ "$(wc -l < "${POST_PIR_OPTIMIZATION_POOL_SIZING_TRACE}" | tr -d ' ')" -eq 16 ] &&
  [ -f "${POST_PIR_BENCH_MODULE}" ] &&
  [ -f "${RUNTIME_PRESSURE}" ] &&
  [ -f "${V8_LIFECYCLE}" ] &&
  [ -f "crates/nimbus-runtime/src/limits/tests.rs" ] &&
  [ -f "${POST_PIR_OPTIMIZATION_PROOF}" ] &&
  [ -f "${FINAL_ARCH_PLAN}" ] &&
  contains_all "${POST_PIR_BENCH_MODULE}" \
    'nimbus.profile_aware_isolate_runtime.post_pir.pool_sizing.v1' \
    'POST_PIR_POOL_SIZING_GROUP' \
    'NIMBUS_POST_PIR_POOL_SIZING_BENCH' \
    'PostPirPoolSizingShape' \
    'warm_hit_ratio' \
    'high_pressure_eviction_target' \
    'critical_pressure_eviction_target' >/tmp/pir-post-optimization-pool-sizing-module-missing.txt &&
  contains_all "${RUNTIME_PRESSURE}" \
    'retained_runtime_eviction_target' \
    'RuntimeMemoryPressureLevel::High => retained_entries.div_ceil\(2\)' \
    'RuntimeMemoryPressureLevel::Critical => retained_entries' >/tmp/pir-post-optimization-pool-sizing-pressure-missing.txt &&
  contains_all "${V8_LIFECYCLE}" \
    'retained_entry_eviction_count_for_pressure' \
    'retained_runtime_eviction_target' >/tmp/pir-post-optimization-pool-sizing-lifecycle-missing.txt &&
  contains_all "crates/nimbus-runtime/src/limits/tests.rs" \
    'runtime_memory_pressure_decision_sizes_retained_evictions' \
    'high pressure evicts the oldest half' \
    'critical pressure evicts every idle retained runtime' >/tmp/pir-post-optimization-pool-sizing-tests-missing.txt &&
  contains_all "${POST_PIR_OPTIMIZATION_POOL_SIZING_TRACE}" \
    'nimbus.profile_aware_isolate_runtime.post_pir.pool_sizing.v1' \
    'runtime_pool_modes_post_pir_pool_sizing' \
    'hostless_trivial/hot_tail_fanout_64_retained_cap_4/webstandard_exact_key_warm_pool' \
    'hostless_trivial/hot_tail_fanout_64_retained_cap_16/webstandard_exact_key_warm_pool' \
    'setup_heavy_large_module/hot_tail_fanout_64_retained_cap_64/webstandard_exact_key_warm_pool' \
    '"measured_iterations":128' \
    '"warm_hit_ratio":0.0' \
    '"warm_hit_ratio":0.7058823529411765' \
    '"retained_runtime_pool_evictions":132' \
    '"high_pressure_eviction_target":8' \
    '"critical_pressure_eviction_target":16' >/tmp/pir-post-optimization-pool-sizing-trace-missing.txt &&
  contains_all "${POST_PIR_OPTIMIZATION_PROOF}" \
    'Pool Sizing And Pressure-Eviction Slice 5' \
    'runtime_pool_modes_post_pir_pool_sizing' \
    'post-pir-pool-sizing-curve-fixed.jsonl' \
    '16 JSONL records and 16 final rows' \
    'Each final row records 128 measured invocations' \
    'hostless_trivial | 4 | 2.729 | 2.992 | 3.234 | 4 | 132 | 0.000 | 2 | 4' \
    'setup_heavy_large_module | 16 | 1.435 | 3.951 | 4.174 | 16 | 24 | 0.706 | 8 | 16' \
    'Cap 4 is below the hot set and thrashes' \
    'Cap 12 to 16 is the first useful knee' \
    'policy-owned' \
    'Condition 99' >/tmp/pir-post-optimization-pool-sizing-proof-missing.txt &&
  contains_all "${FINAL_ARCH_PLAN}" \
    'Fixed-window pool sizing rows show' \
    'first useful retained-cap knee around' \
    'Cap 4 thrashes' \
    'gated by density budget' >/tmp/pir-post-optimization-pool-sizing-arch-missing.txt; then
  pass "post-PIR fixed-window pool sizing rows quantify the hot-tail cap knee and pressure eviction targets"
else
  fail "Post-PIR pool sizing and pressure-eviction curve is incomplete" \
    "$(cat /tmp/pir-post-optimization-pool-sizing-module-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-pool-sizing-pressure-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-pool-sizing-lifecycle-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-pool-sizing-tests-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-pool-sizing-trace-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-pool-sizing-proof-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-pool-sizing-arch-missing.txt 2>/dev/null)"
fi

step 100 "Post-PIR cooperative mixed I/O/CPU scheduler curve is recorded"
if [ -f "${POST_PIR_OPTIMIZATION_COOPERATIVE_MIXED_TRACE}" ] &&
  [ "$(wc -l < "${POST_PIR_OPTIMIZATION_COOPERATIVE_MIXED_TRACE}" | tr -d ' ')" -eq 4 ] &&
  [ -f "${POST_PIR_BENCH_MODULE}" ] &&
  [ -f "${POST_PIR_OPTIMIZATION_PROOF}" ] &&
  [ -f "${FINAL_ARCH_PLAN}" ] &&
  contains_all "${POST_PIR_BENCH_MODULE}" \
    'nimbus.profile_aware_isolate_runtime.post_pir.cooperative_mixed.v1' \
    'POST_PIR_COOPERATIVE_MIXED_GROUP' \
    'NIMBUS_POST_PIR_COOPERATIVE_MIXED_BENCH' \
    'POST_PIR_TRACE_MIN_COOPERATIVE_MIXED_WAVES' \
    'PostPirCooperativeMixedShape' \
    'PostPirCooperativeMixedScenario' \
    'tokio::runtime::Builder::new_current_thread' \
    'async_host_latency_p95_nanos' \
    'compute_latency_p95_nanos' \
    'max_concurrent_runtime_instances = 1' >/tmp/pir-post-optimization-cooperative-mixed-module-missing.txt &&
  contains_all "${POST_PIR_OPTIMIZATION_COOPERATIVE_MIXED_TRACE}" \
    'nimbus.profile_aware_isolate_runtime.post_pir.cooperative_mixed.v1' \
    'runtime_pool_modes_post_pir_cooperative_mixed' \
    'io_only_4x1ms/webstandard_cooperative_exact_key_warm_pool' \
    'balanced_io_first_2io_2cpu/webstandard_cooperative_exact_key_warm_pool' \
    'balanced_cpu_first_2cpu_2io/webstandard_cooperative_exact_key_warm_pool' \
    'cpu_heavy_cpu_first_1io_3cpu/webstandard_cooperative_exact_key_warm_pool' \
    '"measured_waves":32' \
    '"measured_invocations":128' \
    '"measured_async_host_invocations":64' \
    '"measured_compute_invocations":96' \
    '"active_runtime_instances":0' \
    '"queued_invocations":0' \
    '"worker_dispatched_invocations":132' \
    '"warm_pool_hits":128' \
    '"warm_pool_misses":4' >/tmp/pir-post-optimization-cooperative-mixed-trace-missing.txt &&
  contains_all "${POST_PIR_OPTIMIZATION_PROOF}" \
    'Cooperative Mixed I/O/CPU Slice 6' \
    'runtime_pool_modes_post_pir_cooperative_mixed' \
    'post-pir-cooperative-mixed-fixed.jsonl' \
    '4 JSONL records and 4 final rows' \
    '32 measured waves and 128 measured invocations' \
    'zero active runtime instances and zero queued invocations' \
    'balanced_io_first_2io_2cpu | io_first | 2 | 2 | 3.060 | 4.756 | 6.452' \
    'cpu_heavy_cpu_first_1io_3cpu | cpu_first | 1 | 3 | 2.822 | 6.839 | 7.254' \
    'not CPU preemption' \
    'Condition 100' >/tmp/pir-post-optimization-cooperative-mixed-proof-missing.txt &&
  contains_all "${FINAL_ARCH_PLAN}" \
    'Cooperative mixed I/O/CPU rows show' \
    'one worker and one active runtime slot' \
    'zero queued invocations' \
    'CPU-first rows deliberately raise async-host p95' \
    'host-pressure controls' >/tmp/pir-post-optimization-cooperative-mixed-arch-missing.txt; then
  pass "post-PIR cooperative mixed scheduler rows quantify host-wait parking and CPU-first delay"
else
  fail "Post-PIR cooperative mixed I/O/CPU scheduler curve is incomplete" \
    "$(cat /tmp/pir-post-optimization-cooperative-mixed-module-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-cooperative-mixed-trace-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-cooperative-mixed-proof-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-cooperative-mixed-arch-missing.txt 2>/dev/null)"
fi

step 101 "Post-PIR exact-key fragmentation curve is recorded"
if [ -f "${POST_PIR_OPTIMIZATION_FRAGMENTATION_TRACE}" ] &&
  [ "$(wc -l < "${POST_PIR_OPTIMIZATION_FRAGMENTATION_TRACE}" | tr -d ' ')" -eq 12 ] &&
  [ -f "${POST_PIR_BENCH_MODULE}" ] &&
  [ -f "${POST_PIR_OPTIMIZATION_PROOF}" ] &&
  [ -f "${FINAL_ARCH_PLAN}" ] &&
  contains_all "${POST_PIR_BENCH_MODULE}" \
    'nimbus.profile_aware_isolate_runtime.post_pir.fragmentation.v1' \
    'POST_PIR_FRAGMENTATION_GROUP' \
    'NIMBUS_POST_PIR_FRAGMENTATION_BENCH' \
    'POST_PIR_TRACE_MIN_FRAGMENTATION_ITERATIONS' \
    'PostPirFragmentationShape' \
    'PostPirFragmentationDimension' \
    'RuntimeRoutingAffinity::Script' \
    'script_bundle_count' \
    'exact_key_partition_dimensions' \
    'write_named_bundle' >/tmp/pir-post-optimization-fragmentation-module-missing.txt &&
  contains_all "${POST_PIR_OPTIMIZATION_FRAGMENTATION_TRACE}" \
    'nimbus.profile_aware_isolate_runtime.post_pir.fragmentation.v1' \
    'runtime_pool_modes_post_pir_fragmentation' \
    'hostless_trivial/tenant_fanout_32_retained_cap_16/webstandard_exact_key_warm_pool' \
    'hostless_trivial/script_fanout_32_retained_cap_32/webstandard_exact_key_warm_pool' \
    'setup_heavy_large_module/function_fanout_32_retained_cap_16/webstandard_exact_key_warm_pool' \
    'setup_heavy_large_module/script_fanout_32_retained_cap_32/webstandard_exact_key_warm_pool' \
    '"measured_iterations":128' \
    '"fragmentation_dimension":"script"' \
    '"script_bundle_count":32' \
    '"exact_key_partition_dimensions":"bundle_identity,affinity_key,runtime_limits,permission_profile,construction_mode,exact_service_grants"' \
    '"warm_hit_ratio":0.0' \
    '"warm_hit_ratio":0.8' \
    '"retained_runtime_pool_evictions":144' \
    '"warm_pool_hits":128' \
    '"warm_pool_misses":32' >/tmp/pir-post-optimization-fragmentation-trace-missing.txt &&
  contains_all "${POST_PIR_OPTIMIZATION_PROOF}" \
    'Exact-Key Fragmentation Slice 7' \
    'runtime_pool_modes_post_pir_fragmentation' \
    'post-pir-exact-key-fragmentation-fixed.jsonl' \
    '12 JSONL records and 12 final rows' \
    '128 measured invocations after 32 prime invocations' \
    'hostless_trivial | script | 32 | 1.017 | 1.143 | 1.197' \
    'setup_heavy_large_module | function | 16 | 3.541 | 3.857 | 4.027' \
    'zero warm hits and 144 evictions' \
    '0.800 warm-hit ratio' \
    'not benchmark knobs' \
    'Condition 101' >/tmp/pir-post-optimization-fragmentation-proof-missing.txt &&
  contains_all "${FINAL_ARCH_PLAN}" \
    'Exact-key fragmentation rows extend the fanout result' \
    'fanout 32 / cap 16' \
    'zero warm hits and 144 evictions' \
    '0.800 warm-hit ratio' \
    'not runtime speed knobs' >/tmp/pir-post-optimization-fragmentation-arch-missing.txt; then
  pass "post-PIR exact-key fragmentation rows quantify tenant, function, and script authority fanout"
else
  fail "Post-PIR exact-key fragmentation curve is incomplete" \
    "$(cat /tmp/pir-post-optimization-fragmentation-module-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-fragmentation-trace-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-fragmentation-proof-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-fragmentation-arch-missing.txt 2>/dev/null)"
fi

step 102 "Post-PIR WebStandard code-cache variants are recorded"
if [ -f "${POST_PIR_OPTIMIZATION_CODE_CACHE_TRACE}" ] &&
  [ "$(wc -l < "${POST_PIR_OPTIMIZATION_CODE_CACHE_TRACE}" | tr -d ' ')" -eq 4 ] &&
  [ -f "${POST_PIR_BENCH_MODULE}" ] &&
  [ -f "${POST_PIR_OPTIMIZATION_PROOF}" ] &&
  [ -f "${FINAL_ARCH_PLAN}" ] &&
  contains_all "${POST_PIR_BENCH_MODULE}" \
    'nimbus.profile_aware_isolate_runtime.post_pir.code_cache.v1' \
    'POST_PIR_CODE_CACHE_GROUP' \
    'NIMBUS_POST_PIR_CODE_CACHE_BENCH' \
    'POST_PIR_TRACE_MIN_CODE_CACHE_ITERATIONS' \
    'PostPirCodeCacheScenario' \
    'CodeCacheState::FreshBundleEachInvocation' \
    'CodeCacheState::PrimedBundleCodeCache' \
    'PostPirCodeCacheTraceRecord' \
    'maybe_emit_post_pir_code_cache_trace_record' >/tmp/pir-post-optimization-code-cache-module-missing.txt &&
  contains_all "${POST_PIR_OPTIMIZATION_CODE_CACHE_TRACE}" \
    'nimbus.profile_aware_isolate_runtime.post_pir.code_cache.v1' \
    'runtime_pool_modes_post_pir_code_cache' \
    'hostless_trivial/startup_snapshot_cache/fresh_bundle_each_invocation' \
    'hostless_trivial/startup_snapshot_cache/primed_bundle_code_cache' \
    'setup_heavy_large_module/startup_snapshot_cache/fresh_bundle_each_invocation' \
    'setup_heavy_large_module/startup_snapshot_cache/primed_bundle_code_cache' \
    '"measured_iterations":64' \
    '"code_cache_state":"fresh_bundle_each_invocation"' \
    '"code_cache_state":"primed_bundle_code_cache"' \
    '"bundle_module_loads":65' \
    '"runtime_pool_hits":64' \
    '"runtime_pool_misses":1' >/tmp/pir-post-optimization-code-cache-trace-missing.txt &&
  contains_all "${POST_PIR_OPTIMIZATION_PROOF}" \
    'WebStandard Code-Cache Variant Slice 8' \
    'runtime_pool_modes_post_pir_code_cache' \
    'post-pir-webstandard-code-cache-fixed.jsonl' \
    '4 JSONL records and 4 final rows' \
    '64 measured invocations after one prime invocation' \
    'hostless_trivial | fresh_bundle_each_invocation | 1.959 | 2.255 | 2.299' \
    'setup_heavy_large_module | primed_bundle_code_cache | 2.420 | 2.626 | 2.704' \
    'p50 moved from 2.614 ms to 2.420 ms' \
    'parse/compile reuse inside the module-load path' \
    'Condition 102' >/tmp/pir-post-optimization-code-cache-proof-missing.txt &&
  contains_all "${FINAL_ARCH_PLAN}" \
    'WebStandard code-cache variant rows show' \
    '2.614 ms to 2.420 ms and p99 moved from 3.042 ms to 2.704 ms' \
    'safe secondary layer' \
    'Code cache may still reduce cold or semi-warm setup-heavy Web rows' \
    'secondary default layer' >/tmp/pir-post-optimization-code-cache-arch-missing.txt; then
  pass "post-PIR WebStandard code-cache rows quantify cold and semi-warm secondary benefit"
else
  fail "Post-PIR WebStandard code-cache variant curve is incomplete" \
    "$(cat /tmp/pir-post-optimization-code-cache-module-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-code-cache-trace-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-code-cache-proof-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-code-cache-arch-missing.txt 2>/dev/null)"
fi

step 103 "Post-PIR NodeFull lazy-init closeout is recorded"
if [ -f "${POST_PIR_OPTIMIZATION_NODE_LAZY_INIT_TRACE}" ] &&
  [ "$(wc -l < "${POST_PIR_OPTIMIZATION_NODE_LAZY_INIT_TRACE}" | tr -d ' ')" -eq 6 ] &&
  [ -f "${POST_PIR_BENCH_MODULE}" ] &&
  [ -f "${BOOTSTRAP_EXTENSIONS}" ] &&
  [ -f "${POST_PIR_OPTIMIZATION_PROOF}" ] &&
  [ -f "${FINAL_ARCH_PLAN}" ] &&
  contains_all "${POST_PIR_BENCH_MODULE}" \
    'nimbus.profile_aware_isolate_runtime.post_pir.node_lazy_init.v1' \
    'POST_PIR_NODE_LAZY_INIT_GROUP' \
    'NIMBUS_POST_PIR_NODE_LAZY_INIT_BENCH' \
    'POST_PIR_TRACE_MIN_NODE_LAZY_INIT_ITERATIONS' \
    'PostPirNodeLazyInitScenario' \
    'NodeFullNfr6WorkloadKind::LoaderHookDynamicBuiltin' \
    'snapshot_extension_init_mode' \
    'node_lazy_contract' \
    'maybe_emit_post_pir_node_lazy_init_trace_record' >/tmp/pir-post-optimization-node-lazy-module-missing.txt &&
  contains_all "${BOOTSTRAP_EXTENSIONS}" \
    'snapshot_extension' \
    'deno_node::deno_node::lazy_init' \
    'execution_extension' \
    'deno_node::deno_node::init' >/tmp/pir-post-optimization-node-lazy-bootstrap-missing.txt &&
  contains_all "${POST_PIR_OPTIMIZATION_NODE_LAZY_INIT_TRACE}" \
    'nimbus.profile_aware_isolate_runtime.post_pir.node_lazy_init.v1' \
    'runtime_pool_modes_post_pir_node_lazy_init' \
    'node22/setup_heavy_large_module/startup_snapshot_cache' \
    'node22/loader_hook_dynamic_builtin/startup_snapshot_cache' \
    'node24/node24_cjs_translator_boundary/startup_snapshot_cache' \
    '"measured_iterations":32' \
    '"snapshot_extension_init_mode":"lazy_init"' \
    '"execution_extension_init_mode":"init"' \
    '"node_lazy_contract":"snapshot_extensions_lazy_init_execution_extensions_init"' \
    '"runtime_pool_hits":32' \
    '"runtime_pool_misses":1' \
    '"fresh_realm_creates":0' >/tmp/pir-post-optimization-node-lazy-trace-missing.txt &&
  contains_all "${POST_PIR_OPTIMIZATION_PROOF}" \
    'NodeFull Lazy-Init Slice 9' \
    'runtime_pool_modes_post_pir_node_lazy_init' \
    'post-pir-nodefull-lazy-init-fixed-window.jsonl' \
    '6 JSONL records and 6 final rows' \
    '32 measured invocations after one prime invocation' \
    'node22 | setup_heavy_large_module | 9.534 | 9.734 | 10.536' \
    'node24 | node24_cjs_translator_boundary | 19.072 | 20.311 | 22.016' \
    'canonical lazy-init pattern is already implemented' \
    'import-set manifest/classifier proof' \
    'Condition 103' >/tmp/pir-post-optimization-node-lazy-proof-missing.txt &&
  contains_all "${FINAL_ARCH_PLAN}" \
    'NodeFull lazy-init rows show the canonical baseline is already present' \
    'snapshot extension slots use `lazy_init`' \
    'record zero' \
    'fresh-realm creates' \
    'Pure setup-heavy NodeFull rows sit around 9.3-9.5 ms p50' \
    'Import-set extension pruning needs a separate classifier' \
    'post-PIR benchmark backlog is recorded' >/tmp/pir-post-optimization-node-lazy-arch-missing.txt; then
  pass "post-PIR NodeFull lazy-init rows close the baseline and defer unsafe import-set pruning"
else
  fail "Post-PIR NodeFull lazy-init closeout is incomplete" \
    "$(cat /tmp/pir-post-optimization-node-lazy-module-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-node-lazy-bootstrap-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-node-lazy-trace-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-node-lazy-proof-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-node-lazy-arch-missing.txt 2>/dev/null)"
fi

step 104 "Post-PIR replay-based adaptive controller rows are recorded"
if [ -f "${POST_PIR_OPTIMIZATION_CONTROLLER_REPLAY_TRACE}" ] &&
  [ "$(wc -l < "${POST_PIR_OPTIMIZATION_CONTROLLER_REPLAY_TRACE}" | tr -d ' ')" -eq 5 ] &&
  [ -f "${POST_PIR_BENCH_MODULE}" ] &&
  [ -f "${POST_PIR_OPTIMIZATION_PROOF}" ] &&
  [ -f "${FINAL_ARCH_PLAN}" ] &&
  contains_all "${POST_PIR_BENCH_MODULE}" \
    'nimbus.profile_aware_isolate_runtime.post_pir.controller_replay.v1' \
    'POST_PIR_CONTROLLER_REPLAY_GROUP' \
    'NIMBUS_POST_PIR_CONTROLLER_REPLAY_BENCH' \
    'POST_PIR_TRACE_MIN_CONTROLLER_REPLAY_ITERATIONS' \
    'PostPirControllerReplayShape' \
    'steady_nominal' \
    'burst_spillover' \
    'memory_pressure_panic' \
    'zipf_tenant_cap' \
    'periodic_decay' \
    'PostPirControllerReplayScenario' \
    'RuntimeAdaptiveControllerSettings::default' \
    'replay_runtime_controller' \
    'PostPirControllerReplayTraceRecord' \
    'maybe_emit_post_pir_controller_replay_trace_record' \
    'record.measured_replays != POST_PIR_TRACE_MIN_CONTROLLER_REPLAY_ITERATIONS' >/tmp/pir-post-optimization-controller-replay-module-missing.txt &&
  contains_all "${POST_PIR_OPTIMIZATION_CONTROLLER_REPLAY_TRACE}" \
    'nimbus.profile_aware_isolate_runtime.post_pir.controller_replay.v1' \
    'runtime_pool_modes_post_pir_controller_replay' \
    '"benchmark_id":"steady_nominal"' \
    '"benchmark_id":"burst_spillover"' \
    '"benchmark_id":"memory_pressure_panic"' \
    '"benchmark_id":"zipf_tenant_cap"' \
    '"benchmark_id":"periodic_decay"' \
    '"measured_replays":512' \
    '"live_adaptive_defaults_enabled":false' \
    '"input_authorities":2' \
    '"desired_warm_target":11' \
    '"desired_warm_target":0' \
    '"replayed_warm_target":3' \
    '"replayed_warm_target":1' \
    '"prewarming_paused":true' \
    '"evict_idle_retained_runtimes":true' \
    '"isolate_stall_signal":true' \
    '"spillover_signal":true' \
    '"tenant_cap_limited":true' \
    '"rate_limited":true' \
    '"max_scale_down_step":2' >/tmp/pir-post-optimization-controller-replay-trace-missing.txt &&
  contains_all "${POST_PIR_OPTIMIZATION_PROOF}" \
    'Replay-Based Adaptive Controller Slice 10' \
    'runtime_pool_modes_post_pir_controller_replay' \
    'post-pir-controller-replay-fixed.jsonl' \
    '5 JSONL records and 5 final rows' \
    '512 measured replays' \
    'live_adaptive_defaults_enabled: false' \
    'steady_nominal | 1 | 5,565,217 | desired 2, replayed 2' \
    'zipf_tenant_cap | 2 | 2,348,171 | hot desired 11 and cold desired 2 both capped to replayed 1' \
    'periodic_decay | 1 | 5,973,771 | desired 0, replayed 3, rate limited true' \
    'Live adaptive defaults remain off' \
    'Condition 104' >/tmp/pir-post-optimization-controller-replay-proof-missing.txt &&
  contains_all "${FINAL_ARCH_PLAN}" \
    'Replay-based adaptive controller rows show' \
    'stable demand, burst spillover, memory-pressure panic, Zipf tenant caps' \
    'every row records live adaptive defaults off' \
    'memory or host pressure pauses prewarm' \
    'evicts idle retained runtimes' \
    'hot desired 11 and cold desired 2 to replayed 1' \
    'rate-limits desired 0 to replayed 3' \
    'post-PIR benchmark backlog is recorded' >/tmp/pir-post-optimization-controller-replay-arch-missing.txt; then
  pass "post-PIR replay controller rows prove conservative offline adaptivity decisions with live defaults off"
else
  fail "Post-PIR replay-based adaptive controller curve is incomplete" \
    "$(cat /tmp/pir-post-optimization-controller-replay-module-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-controller-replay-trace-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-controller-replay-proof-missing.txt 2>/dev/null) $(cat /tmp/pir-post-optimization-controller-replay-arch-missing.txt 2>/dev/null)"
fi

step 105 "PIR7L live adaptive autoscaling plan is gated, modularly scoped, and non-default"
if [ -f "${PLAN}" ] &&
  [ -f "${FINAL_ARCH_PLAN}" ] &&
  [ -f "${PLANS_INDEX}" ] &&
  [ -f "${POST_PIR_OPTIMIZATION_PROOF}" ] &&
  [ -f "${RUNTIME_CONTROLLER_REPLAY}" ] &&
  [ -f "${RUNTIME_PRESSURE}" ] &&
  [ -f "${EXECUTOR_ADMISSION}" ] &&
  contains_all "${PLAN}" \
    'PIR7L — Live Adaptive Autoscaling Promotion Gate' \
    'File-size ownership exceptions' \
    'RuntimeAdaptiveWarmPoolController' \
    "Little's Law" \
    'stable/panic windows' \
    'bounded lending/borrowing' \
    'request parser may enable adaptive scaling' \
    'Authority keys remain exact' \
    'captured production-style traces' \
    'shadow mode before actuation' \
    'operator rollback' >/tmp/pir7l-plan-missing.txt &&
  contains_all "${FINAL_ARCH_PLAN}" \
    'Do Not Default Live Adaptive Autoscaling Until PIR7L Passes' \
    'PIR7L live adaptive autoscaling follow-up' \
    'Shadow mode before actuation' \
    'Operator rollback' \
    'captured production-style traces' \
    'pressure-oscillation checks' \
    'Any SDK, manifest, request parser' \
    'owner-key collapse' >/tmp/pir7l-final-arch-missing.txt &&
  contains_all "${PLANS_INDEX}" \
    'PIR7L live adaptive autoscaling is a named PIR follow-on' \
    'replay/host-budget proof plus REC'\''s derived execution-plan seam' \
    'Execute PIR7L only after REC closeout' \
    'captured traces, shadow mode, p99/fairness/pressure' >/tmp/pir7l-plan-index-missing.txt &&
  contains_all "${POST_PIR_OPTIMIZATION_PROOF}" \
    'Live Adaptive Autoscaling Plan Audit' \
    'Knative KPA' \
    'Kubernetes HPA' \
    'API Priority and Fairness' \
    'Node Allocatable' \
    'Supabase Edge Runtime' \
    'OpenWorkers is not a proven adaptive autoscaler' \
    'Serverless in the Wild' \
    'Huawei public serverless traces' \
    'RuntimeAdaptiveWarmPoolController' \
    'No SDK, manifest, request parser' \
    'plan-readiness gate, not proof' \
    'Condition 105' >/tmp/pir7l-proof-missing.txt &&
  contains 'Ownership-size exception' "${BASH_SOURCE[0]}" &&
  contains_all "${RUNTIME_ADAPTIVE_CONTROLLER}" \
    'RuntimeAdaptiveControllerSettings' \
    'live_adaptive_defaults_enabled: false' \
    'pub fn live_adaptive_defaults_enabled' >/tmp/pir7l-controller-settings-missing.txt &&
  contains_all "${RUNTIME_CONTROLLER_REPLAY}" \
    'replay_runtime_controller' \
    'stable_window_observations' \
    'panic_window_observations' \
    'max_scale_down_step' >/tmp/pir7l-controller-missing.txt &&
  contains_all "${RUNTIME_PRESSURE}" \
    'RuntimeHostResourceBudget' \
    'RuntimeHostPressureSource' \
    'RuntimeHostAdmissionAction' \
    'RuntimeHostWorkClass' >/tmp/pir7l-pressure-missing.txt &&
  contains_all "${EXECUTOR_ADMISSION}" \
    'host_admission_for_in_flight' \
    'tenant_fairness' \
    'RuntimeHostAdmissionAction::Shed' \
    'RuntimeHostAdmissionAction::Admit' >/tmp/pir7l-admission-missing.txt; then
  pass "PIR7L plan names pure controller ownership and adapter seams while live actuation remains gated off"
else
  fail "PIR7L live adaptive autoscaling gate is incomplete" \
    "$(cat /tmp/pir7l-plan-missing.txt 2>/dev/null) $(cat /tmp/pir7l-final-arch-missing.txt 2>/dev/null) $(cat /tmp/pir7l-plan-index-missing.txt 2>/dev/null) $(cat /tmp/pir7l-proof-missing.txt 2>/dev/null) $(cat /tmp/pir7l-controller-settings-missing.txt 2>/dev/null) $(cat /tmp/pir7l-controller-missing.txt 2>/dev/null) $(cat /tmp/pir7l-pressure-missing.txt 2>/dev/null) $(cat /tmp/pir7l-admission-missing.txt 2>/dev/null)"
fi

step 106 "PIR7L live adaptive controller is implemented, benchmarked, and operator-gated"
if [ -f "${RUNTIME_ADAPTIVE_CONTROLLER}" ] &&
  [ -f "${POST_PIR_OPTIMIZATION_LIVE_ADAPTIVE_TRACE}" ] &&
  contains_all "${RUNTIME_ADAPTIVE_CONTROLLER}" \
    'pub enum RuntimeAdaptiveControllerMode' \
    'Disabled' \
    'Shadow' \
    'Canary' \
    'Live' \
    'pub struct RuntimeAdaptiveControllerSettings' \
    'pub trait RuntimeAdaptiveClock' \
    'pub trait RuntimeAdaptivePressureAdapter' \
    'pub trait RuntimeAdaptiveObservationSource' \
    'pub trait RuntimeAdaptiveMetricsSink' \
    'pub trait RuntimeAdaptiveActuator' \
    'pub struct RuntimeAdaptiveWarmPoolController' \
    'run_with_adapters' \
    'rollback_to_static_defaults' \
    'RuntimeAdaptiveWarmPoolActuationKind::CanarySkipped' \
    'RuntimeAdaptiveWarmPoolActuationKind::RollbackToStatic' \
    'replay_input_with_pressure' >/tmp/pir7l-impl-controller-missing.txt &&
  contains_all "${RUNTIME_LIB}" \
    'RuntimeAdaptiveWarmPoolController' \
    'RuntimeAdaptiveControllerMetricsSnapshot' \
    'RuntimeAdaptiveActuator' \
    'RuntimeAdaptivePressureAdapter' >/tmp/pir7l-impl-exports-missing.txt &&
  contains_all "${START_COMMAND}" \
    'runtime_adaptive_mode' \
    'runtime_adaptive_canary_percent' \
    'runtime_adaptive_rollback' \
    'long = "runtime-adaptive-mode"' \
    'long = "runtime-adaptive-canary-percent"' \
    'long = "runtime-adaptive-rollback"' >/tmp/pir7l-impl-start-command-missing.txt &&
  contains_all "${START_RUNTIME_LIMITS}" \
    'runtime_adaptive_controller_settings_from_command' \
    'RuntimeAdaptiveControllerSettings::disabled' \
    'RuntimeAdaptiveControllerSettings::shadow' \
    'RuntimeAdaptiveControllerSettings::canary' \
    'RuntimeAdaptiveControllerSettings::live' \
    'with_rollback_to_static_defaults' >/tmp/pir7l-impl-start-lowerer-missing.txt &&
  contains_all "${START_CLI_TEST}" \
    'cli_parses_runtime_adaptive_operator_controls' \
    'cli_rejects_runtime_adaptive_canary_percent_above_one_hundred' \
    'runtime_adaptive_controller_settings_from_command_applies_operator_policy' >/tmp/pir7l-impl-start-tests-missing.txt &&
  contains_all "${DEV_PLAN_TEST}" \
    'dev_start_command_inherits_disabled_runtime_adaptive_controller' >/tmp/pir7l-impl-dev-tests-missing.txt &&
  contains_all "${NODE_SERVICE}" \
    'runtime_adaptive_mode_start_arg' \
    '--runtime-adaptive-mode' \
    'disabled' \
    '--runtime-adaptive-canary-percent' >/tmp/pir7l-impl-node-missing.txt &&
  contains_all "${SERVER_CONSTRUCTION}" \
    'with_runtime_adaptive_controller_settings' >/tmp/pir7l-impl-server-construction-missing.txt &&
  contains_all "${SERVER_ROUTER}" \
    'with_runtime_adaptive_controller_settings' \
    'runtime_adaptive_controller_settings' \
    'configured runtime adaptive controller' >/tmp/pir7l-impl-server-router-missing.txt &&
  contains_all "${SERVER_STATE}" \
    'runtime_adaptive_controller_settings' >/tmp/pir7l-impl-server-state-missing.txt &&
  contains_all "${CONVEX_LIB}" \
    'RuntimeAdaptiveControllerSettings' \
    'with_adaptive_controller_settings' >/tmp/pir7l-impl-convex-lib-missing.txt &&
  contains_all "${CONVEX_REGISTRY_LOADING}" \
    'RuntimeAdaptiveControllerSettings' \
    'with_runtime_host_governor' \
    'adaptive_settings' >/tmp/pir7l-impl-convex-loading-missing.txt &&
  contains_all "${CONVEX_RUNTIME_ACCESS}" \
    'RuntimeAdaptiveControllerMode::Shadow' >/tmp/pir7l-impl-convex-runtime-access-missing.txt &&
  contains_all "${CLOUD_FUNCTIONS_REGISTRY}" \
    'RuntimeAdaptiveControllerSettings' \
    'with_adaptive_controller_settings' \
    'RuntimeAdaptiveControllerMode::Shadow' >/tmp/pir7l-impl-cloud-functions-missing.txt &&
  contains_all "${RUNTIME_METRICS}" \
    'RuntimeAdaptiveControllerMetricsSnapshot' \
    'record_adaptive_controller_evaluation' \
    'adaptive_controller_metrics_snapshot_is_low_cardinality_global_state' >/tmp/pir7l-impl-metrics-api-missing.txt &&
  contains_all "${RUNTIME_METRICS_GLOBAL}" \
    'latest_recommended_warm_target_total' \
    'latest_effective_warm_target_total' >/tmp/pir7l-impl-metrics-global-missing.txt &&
  contains_all "${POST_PIR_BENCH_MODULE}" \
    'POST_PIR_LIVE_ADAPTIVE_TRACE_SCHEMA' \
    'NIMBUS_POST_PIR_LIVE_ADAPTIVE_BENCH' \
    'PostPirLiveAdaptiveShape' \
    'DisabledStatic' \
    'ShadowBurst' \
    'CanaryAdmittedBurst' \
    'CanaryExcludedBurst' \
    'LiveMemoryPressure' \
    'RollbackPeriodic' \
    'LiveZipfTenantCap' \
    'PostPirLiveAdaptiveScenario' \
    'RuntimeAdaptiveWarmPoolController::new' \
    'run_with_adapters' \
    'PostPirLiveAdaptiveTraceRecord' \
    'maybe_emit_post_pir_live_adaptive_trace_record' \
    'record.measured_iterations != POST_PIR_TRACE_MIN_LIVE_ADAPTIVE_ITERATIONS' >/tmp/pir7l-impl-bench-missing.txt &&
  contains_all "${POST_PIR_OPTIMIZATION_LIVE_ADAPTIVE_TRACE}" \
    'nimbus.profile_aware_isolate_runtime.post_pir.live_adaptive_controller.v1' \
    'runtime_pool_modes_post_pir_live_adaptive_controller' \
    '"benchmark_id":"disabled_static"' \
    '"benchmark_id":"shadow_burst"' \
    '"benchmark_id":"canary_admitted_burst"' \
    '"benchmark_id":"canary_excluded_burst"' \
    '"benchmark_id":"live_memory_pressure"' \
    '"benchmark_id":"rollback_periodic"' \
    '"benchmark_id":"live_zipf_tenant_cap"' \
    '"measured_iterations":512' \
    '"controller_mode":"disabled"' \
    '"controller_mode":"shadow"' \
    '"controller_mode":"canary"' \
    '"controller_mode":"live"' \
    '"live_adaptive_defaults_enabled":false' \
    '"live_adaptive_defaults_enabled":true' \
    '"rollback_to_static_defaults":true' \
    '"host_pressure_level":"critical"' \
    '"memory_pressure_level":"critical"' \
    '"shadow_only_decisions":1' \
    '"canary_skipped_decisions":1' \
    '"rollback_to_static_decisions":1' \
    '"tenant_cap_limited_decisions":2' \
    '"prewarming_paused_decisions":1' \
    '"evict_idle_decisions":1' \
    '"attempted_actuations":0' \
    '"attempted_actuations":2' \
    '"latest_effective_warm_target_total":5' >/tmp/pir7l-impl-trace-missing.txt &&
  contains_all "${POST_PIR_OPTIMIZATION_PROOF}" \
    'Live Adaptive Controller Slice 11' \
    'runtime_pool_modes_post_pir_live_adaptive_controller' \
    'post-pir-live-adaptive-controller-fixed.jsonl' \
    '7 JSONL records and 7 final rows' \
    '512 measured evaluations' \
    'disabled_static | disabled | false | false | nominal | nominal | 1 | 2 | 1 | 0 | 0 | 0 | 0 | 0 | 0' \
    'shadow_burst | shadow | false | false | nominal | nominal | 1 | 5 | 1 | 0 | 0 | 1 | 0 | 0 | 0' \
    'canary_admitted_burst | canary | true | false | nominal | nominal | 1 | 5 | 5 | 1 | 1 | 0 | 0 | 0 | 0' \
    'canary_excluded_burst | canary | true | false | nominal | nominal | 1 | 5 | 1 | 0 | 0 | 0 | 1 | 0 | 0' \
    'live_memory_pressure | live | true | false | critical | critical | 1 | 0 | 0 | 1 | 1 | 0 | 0 | 0 | 0 | 1 | 1' \
    'rollback_periodic | live | true | true | nominal | nominal | 1 | 0 | 5 | 1 | 1 | 0 | 0 | 1 | 0' \
    'live_zipf_tenant_cap | live | true | false | nominal | nominal | 2 | 2 | 2 | 2 | 2 | 0 | 0 | 0 | 2' \
    'Condition 106' >/tmp/pir7l-impl-proof-missing.txt &&
  contains_all "${PLAN}" \
    '\| PIR7L \| `done`' \
    'PIR7L live adaptive controller implementation' \
    'Condition 106' \
    'runtime_pool_modes_post_pir_live_adaptive_controller' >/tmp/pir7l-impl-plan-missing.txt &&
  contains_all "${FINAL_ARCH_PLAN}" \
    'PIR7L live adaptive controller is implemented as an operator-gated capability' \
    'disabled, shadow, canary, live, and rollback modes' \
    'does not make live adaptivity a default' >/tmp/pir7l-impl-arch-missing.txt; then
  pass "PIR7L live adaptive controller is implemented with operator gates, proof artifacts, and no live default"
else
  fail "PIR7L live adaptive implementation proof is incomplete" \
    "$(cat /tmp/pir7l-impl-controller-missing.txt 2>/dev/null) $(cat /tmp/pir7l-impl-exports-missing.txt 2>/dev/null) $(cat /tmp/pir7l-impl-start-command-missing.txt 2>/dev/null) $(cat /tmp/pir7l-impl-start-lowerer-missing.txt 2>/dev/null) $(cat /tmp/pir7l-impl-start-tests-missing.txt 2>/dev/null) $(cat /tmp/pir7l-impl-dev-tests-missing.txt 2>/dev/null) $(cat /tmp/pir7l-impl-node-missing.txt 2>/dev/null) $(cat /tmp/pir7l-impl-server-construction-missing.txt 2>/dev/null) $(cat /tmp/pir7l-impl-server-router-missing.txt 2>/dev/null) $(cat /tmp/pir7l-impl-server-state-missing.txt 2>/dev/null) $(cat /tmp/pir7l-impl-convex-lib-missing.txt 2>/dev/null) $(cat /tmp/pir7l-impl-convex-loading-missing.txt 2>/dev/null) $(cat /tmp/pir7l-impl-convex-runtime-access-missing.txt 2>/dev/null) $(cat /tmp/pir7l-impl-cloud-functions-missing.txt 2>/dev/null) $(cat /tmp/pir7l-impl-metrics-api-missing.txt 2>/dev/null) $(cat /tmp/pir7l-impl-metrics-global-missing.txt 2>/dev/null) $(cat /tmp/pir7l-impl-bench-missing.txt 2>/dev/null) $(cat /tmp/pir7l-impl-trace-missing.txt 2>/dev/null) $(cat /tmp/pir7l-impl-proof-missing.txt 2>/dev/null) $(cat /tmp/pir7l-impl-plan-missing.txt 2>/dev/null) $(cat /tmp/pir7l-impl-arch-missing.txt 2>/dev/null)"
fi

step 107 "TFA supersedes PIR7M with inferred autoscaling and resource-first operator envelopes"
if contains_all "${PLAN}" \
    '\| PIR7M \| `done`' \
    'tenant-function-autoscaling-plan.md` supersedes PIR7M' \
    'pool-first `live_scaling` vocabulary and public `activation_warm` field' >/tmp/tfa-plan-routing-missing.txt &&
  contains_all "${TFA_PLAN}" \
    'Public v1 function scaling knobs are `preset`, `min_warm`, `max_warm`, and `scale_down_delay`' \
    'Autoscaling is inferred' \
    '`min_warm: 0` is the default for `nimbus start`' \
    '`nimbus dev` also uses `min_warm: 0`' \
    'Resource-First Operator Envelope' \
    'runtime_safety' \
    'Tenant-inferred autoscaling does not mean "turn on live adaptive controller mode"' \
    'TFA0 — Scaffold And Failing Verifier' \
    'TFA6 — Closeout' >/tmp/tfa-plan-contract-missing.txt &&
  contains_all "${FINAL_ARCH_PLAN}" \
    'TFA supersedes PIR7M' \
    'autoscaling is inferred' \
    'Current TFA operator envelope' \
    'runtime_safety' >/tmp/tfa-arch-missing.txt; then
  pass "TFA records the current public function-scaling contract and PIR7M supersession"
else
  fail "TFA/PIR7M function scaling contract is incomplete" \
    "$(cat /tmp/tfa-plan-routing-missing.txt 2>/dev/null) $(cat /tmp/tfa-plan-contract-missing.txt 2>/dev/null) $(cat /tmp/tfa-arch-missing.txt 2>/dev/null)"
fi

step 108 "TFA inferred autoscaling implementation is present and proven"
if contains_all "${RUNTIME_LIMITS_SCALING}" \
    'RequestedRuntimeScalingTarget' \
    'pub fn inferred_autoscaling' \
    'RuntimeScalingTarget' \
    'pub autoscaling: bool' \
    'pub fn autoscaling_inferred' \
    'pub struct RuntimeScalingPlanSet' \
    'pub fn plan_for_function' >/tmp/tfa-impl-runtime-scaling-missing.txt &&
  ! grep -E 'pub (activation_warm|live_scaling)' "${RUNTIME_LIMITS_SCALING}" >/tmp/tfa-impl-runtime-forbidden.txt 2>/dev/null &&
  contains_all "${RUNTIME_LIMITS_POLICY}" \
    'effective_scaling_plans: RuntimeScalingPlanSet' \
    'clone_with_effective_scaling_plans' \
    'effective_scaling_plan_for_function' >/tmp/tfa-impl-runtime-policy-missing.txt &&
  contains_all "${TENANT_OPERATOR_POLICY}" \
    'runtime_resources: OperatorRuntimeResourceEnvelope' \
    'runtime_safety: OperatorRuntimeSafetyCaps' \
    'pub struct OperatorRuntimeResourceEnvelope' \
    'pub struct OperatorRuntimeSafetyCaps' \
    'runtime_scaling: OperatorRuntimeScalingQuota' >/tmp/tfa-impl-tenant-policy-missing.txt &&
  ! grep -E 'allow_live_scaling|OperatorRuntimeScalingLimits|runtime_scaling_limits:' "${TENANT_OPERATOR_POLICY}" >/tmp/tfa-impl-tenant-forbidden.txt 2>/dev/null &&
  contains_all "${TENANT_OPERATOR_VALIDATION}" \
    'defaults.runtime_resources.cpu_millicpus must be non-zero' \
    'defaults.runtime_safety.max_min_warm_total must be <= max_total_warm' \
    'quotas.runtime_scaling.max_min_warm must be <= max_warm' >/tmp/tfa-impl-tenant-validation-missing.txt &&
  contains_all "${TENANT_RUNTIME_SCALING}" \
    'pub struct TenantRuntimeScalingRequest' \
    'pub fn autoscaling_inferred' \
    'pub fn admit_runtime_scaling' \
    'derived_from_resources' \
    'runtime_safety.max_min_warm_total' \
    'effective max_warm_per_function' \
    'runtime_safety:' \
    'fixed_range_disables_admitted_autoscaling' \
    'admits_auto_inside_resource_derived_operator_envelope' >/tmp/tfa-impl-tenant-runtime-missing.txt &&
  contains_all "${NIMBUS_BIN_FUNCTION_SCALING}" \
    'NimbusFunctionsFileConfig' \
    'FunctionScalingFileConfig' \
    'classes: BTreeMap' \
    'overrides: BTreeMap' \
    'resolve_function_scaling_intent' \
    'admit_function_scaling_plans' \
    'fn from_host_budget' \
    'policy_for_function' \
    'render_resolved_effective_plan' \
    'autoscaling: inferred' \
    'no_yaml_dev_uses_zero_min_warm_with_retention' \
    'unknown_public_activation_warm_rejects' \
    'unknown_public_autoscaling_rejects' \
    'unknown_public_live_scaling_rejects' \
    'fixed_preset_derives_missing_bound_and_disables_inferred_autoscaling' \
    'selectors_do_not_fan_out_to_theoretical_authority_keys' >/tmp/tfa-impl-bin-lowering-missing.txt &&
  contains_all "${NIMBUS_BIN_EXPLAIN}" \
    'ExplainResource::Functions' \
    'ExplainResource::Config' \
    'autoscaling: inferred' >/tmp/tfa-impl-bin-explain-missing.txt &&
  contains_all "${NIMBUS_BIN_VALIDATE}" \
    'ValidateResource::Functions' \
    'ValidateResource::Policy' \
    'policy: Option<PathBuf>' \
    'validate_functions_uses_explicit_operator_policy_for_quota_admission' >/tmp/tfa-impl-bin-validate-missing.txt &&
  contains_all "${NIMBUS_BIN_RUN}" \
    'RunResource::Functions' \
    'RunResource::Exec' \
    'run_functions_uses_explicit_operator_policy_for_quota_admission' >/tmp/tfa-impl-bin-run-missing.txt &&
  contains_all "${START_BOOT}" \
    'resolve_function_scaling_intent' \
    'load_optional_policy' \
    'admit_start_function_scaling_plans' \
    'admit_function_scaling_plans' \
    'FunctionScalingAdmissionEnvelope::from_host_budget' \
    'with_effective_runtime_scaling_plans' \
    'default_function_scaling_summary_line' >/tmp/tfa-impl-start-boot-missing.txt &&
  ! grep -E 'effective_runtime_scaling_plan_from_intent' "${START_BOOT}" >/tmp/tfa-impl-start-forbidden.txt 2>/dev/null &&
  contains_all "${START_CLI_TEST}" \
    'start_startup_summary_mentions_baked_function_scaling_defaults' \
    'cli_parses_start_operator_policy_path' \
    'start_function_scaling_admission_keeps_selector_overrides' \
    'start_function_scaling_admission_uses_explicit_operator_policy' \
    'min_warm=0, max_warm=auto, scale_down_delay=600s, autoscaling inferred=true' \
    'min_warm=0, max_warm=auto, scale_down_delay=120s, autoscaling inferred=true' >/tmp/tfa-impl-start-test-missing.txt &&
  contains_all "${SERVER_ROUTER}" \
    'effective_runtime_scaling_plans' \
    'with_effective_runtime_scaling_plans' \
    'autoscaling = effective_runtime_scaling_plan.effective.autoscaling' \
    'configured effective runtime scaling plans' >/tmp/tfa-impl-server-router-missing.txt &&
  contains_all "${CONVEX_RUNTIME_ACCESS}" \
    'convex_registry_applies_selector_scaling_plan_set_to_runtime_lanes' \
    'effective_scaling_plan_for_function' >/tmp/tfa-impl-convex-runtime-missing.txt &&
  contains_all "${TFA_PROOF}" \
    'TFA0' \
    'TFA1' \
    'TFA2' \
    'TFA3' \
    'TFA4' \
    'TFA5' \
    'TFA6' >/tmp/tfa-impl-proof-missing.txt; then
  pass "TFA implementation, proof, and verifier coverage are complete"
else
  fail "TFA implementation proof is incomplete" \
    "$(cat /tmp/tfa-impl-runtime-scaling-missing.txt 2>/dev/null) $(cat /tmp/tfa-impl-runtime-forbidden.txt 2>/dev/null) $(cat /tmp/tfa-impl-tenant-policy-missing.txt 2>/dev/null) $(cat /tmp/tfa-impl-tenant-forbidden.txt 2>/dev/null) $(cat /tmp/tfa-impl-tenant-validation-missing.txt 2>/dev/null) $(cat /tmp/tfa-impl-tenant-runtime-missing.txt 2>/dev/null) $(cat /tmp/tfa-impl-bin-lowering-missing.txt 2>/dev/null) $(cat /tmp/tfa-impl-bin-explain-missing.txt 2>/dev/null) $(cat /tmp/tfa-impl-bin-validate-missing.txt 2>/dev/null) $(cat /tmp/tfa-impl-bin-run-missing.txt 2>/dev/null) $(cat /tmp/tfa-impl-start-boot-missing.txt 2>/dev/null) $(cat /tmp/tfa-impl-start-forbidden.txt 2>/dev/null) $(cat /tmp/tfa-impl-start-test-missing.txt 2>/dev/null) $(cat /tmp/tfa-impl-server-router-missing.txt 2>/dev/null) $(cat /tmp/tfa-impl-proof-missing.txt 2>/dev/null)"
fi

printf '\nSummary: %d passed, %d failed\n' "${PASS}" "${FAIL}"
if [ "${FAIL}" -ne 0 ]; then
  printf '\nFailing conditions:\n'
  for detail in "${FAIL_DETAIL[@]}"; do
    printf '  - %s\n' "${detail}"
  done
  exit 1
fi
