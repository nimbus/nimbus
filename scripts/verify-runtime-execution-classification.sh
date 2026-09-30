#!/usr/bin/env bash
# Verification gate for the Runtime Execution Classification code contracts.
# The plan, proof, and benchmark artifacts are private working state, so this
# gate reads tracked code and CI wiring only.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

EXECUTION_PLAN="crates/nimbus-runtime/src/execution_plan.rs"
INVOCATION="crates/nimbus-runtime/src/runtime/invocation.rs"
COOP_RUN="crates/nimbus-runtime/src/worker_loop/cooperative/run.rs"
COOP_EXECUTION="crates/nimbus-runtime/src/worker_loop/cooperative/execution.rs"
COOP_BACKEND="crates/nimbus-runtime/src/worker_loop/cooperative/backend.rs"
COOP_TESTS="crates/nimbus-runtime/src/runtime/tests/cooperative.rs"
HOST="crates/nimbus-runtime/src/host.rs"
BOOTSTRAP_CONTEXT_SOURCE="crates/nimbus-runtime/src/runtime/bootstrap/js/nimbus_context_contract.js"
BOOTSTRAP_HOST_CALL_TRANSPORT="crates/nimbus-runtime/src/runtime/bootstrap/js/deno_host_call_transport.js"
BOOTSTRAP_STATE="crates/nimbus-runtime/src/runtime/bootstrap/state.rs"
OPS_SHARED="crates/nimbus-runtime/src/runtime/bootstrap/ops/shared.rs"
HOST_BRIDGE_TESTS="crates/nimbus-runtime/src/runtime/tests/host_bridge.rs"
WORKER_JOB="crates/nimbus-runtime/src/executor/queue/job.rs"
EXECUTOR_INVOKE="crates/nimbus-runtime/src/executor/invoke.rs"
ADMISSION="crates/nimbus-runtime/src/executor/admission.rs"
AFFINITY="crates/nimbus-runtime/src/affinity.rs"
V8_LIFECYCLE="crates/nimbus-runtime/src/backends/v8/lifecycle.rs"
WARM_POOL="crates/nimbus-runtime/src/backends/v8/warm_pool.rs"
TENANT_EFFICIENCY="crates/nimbus-tenant/src/runtime_profile.rs"
HOST_STATE="crates/nimbus-bridge/src/state.rs"
CONVEX_DISPATCH="crates/nimbus-server/src/adapters/convex/host_bridge/async_bridge/dispatch.rs"
CODEGEN_CONTEXT="packages/codegen/src/planner/context_api.mjs"
MAKEFILE_PATH="Makefile"
CI_WORKFLOW=".github/workflows/ci.yml"

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

printf '\033[1mREC verification gate -- runtime execution classification\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

step 1 "REC0 inventory seams exist in code"
if [ -f "${TENANT_EFFICIENCY}" ] &&
  [ -f "${ADMISSION}" ] &&
  [ -f "${AFFINITY}" ] &&
  [ -f "${WARM_POOL}" ] &&
  [ -f "${HOST_STATE}" ] &&
  [ -f "${CONVEX_DISPATCH}" ] &&
  [ -f "${CODEGEN_CONTEXT}" ] &&
  contains 'pub struct RuntimeEfficiencyPlan' "${TENANT_EFFICIENCY}" &&
  contains 'fn runtime_host_work_class_for_job' "${ADMISSION}" &&
  contains_all "${AFFINITY}" \
    'pub\(crate\) enum RuntimeRouteKey' \
    'pub\(crate\) enum RuntimeReuseLocalityKey' >/tmp/rec0-current-affinity-missing.txt &&
  contains 'struct V8RetainedAuthorityKey' "${WARM_POOL}" &&
  contains 'pub struct RuntimeHostState' "${HOST_STATE}"; then
  pass "code carries the semantic, substrate, effect, budget, affinity, and enforcement seams"
else
  fail "REC0 current-state inventory is incomplete" \
    "$(cat /tmp/rec0-current-affinity-missing.txt 2>/dev/null)"
fi

step 2 "Direct cooperative scheduler consumers are removed"
if [ -f "${INVOCATION}" ] &&
  [ -f "${COOP_RUN}" ] &&
  [ -f "${COOP_EXECUTION}" ] &&
  [ -f "${COOP_BACKEND}" ] &&
  contains 'pub\(crate\) const fn is_convex_read_semantic_candidate' "${INVOCATION}" &&
  ! grep -R 'job.request.kind.is_convex_read_semantic_candidate' crates/nimbus-runtime/src/worker_loop >/dev/null 2>&1; then
  pass "worker scheduling has no direct InvocationKind consumer"
else
  fail "direct scheduler consumer removal is incomplete" \
    "expected is_convex_read_semantic_candidate in invocation.rs and no direct worker_loop consumer"
fi

step 3 "Host operation enum is exhaustive enough for REC2"
if [ -f "${HOST}" ] &&
  contains_all "${HOST}" \
    'pub enum HostCallOperation' \
    'HttpRoute' \
    'CtxQuery' \
    'CtxPaginatedQuery' \
    'CtxMutation' \
    'CtxAction' \
    'CtxRunQuery' \
    'CtxRunMutation' \
    'CtxRunAction' \
    'DocumentGet' \
    'QueryBuilderStart' \
    'QueryBuilderWithIndex' \
    'QueryBuilderFilter' \
    'QueryBuilderOrder' \
    'QueryReadCollect' \
    'QueryReadTake' \
    'QueryReadPaginate' \
    'QueryReadFirst' \
    'QueryReadUnique' \
    'DocumentInsert' \
    'DocumentPatch' \
    'DocumentDelete' \
    'CtxSchedulerRunAfter' \
    'CtxSchedulerRunAt' \
    'CtxSchedulerCancel' \
    'CtxServiceLookup' \
    'CtxRuntimeEnterNestedCall' \
    'RuntimeExtensionCall' >/tmp/rec0-host-code-missing.txt; then
  pass "host operation enum carries the REC2 exhaustiveness baseline"
else
  fail "host operation inventory is incomplete" \
    "$(cat /tmp/rec0-host-code-missing.txt 2>/dev/null)"
fi

step 4 "REC verifier is wired into helper syntax gates"
if [ -f "${MAKEFILE_PATH}" ] &&
  [ -f "${CI_WORKFLOW}" ] &&
  contains 'bash -n scripts/verify-runtime-execution-classification.sh' "${MAKEFILE_PATH}" &&
  contains 'bash -n scripts/verify-runtime-execution-classification.sh' "${CI_WORKFLOW}"; then
  pass "REC verifier has Makefile and CI syntax coverage"
else
  fail "REC verifier is not wired into proof-helper syntax gates" \
    "expected Makefile proof-helpers and CI proof-helpers to run bash -n"
fi

step 5 "REC1 typed execution-plan vocabulary exists"
if [ -f "${EXECUTION_PLAN}" ] &&
  contains_all "${EXECUTION_PLAN}" \
    'enum RuntimeEffectClass' \
    'enum RuntimeSideChannelPosture' \
    'enum CooperativeEligibility' \
    'enum CooperativeIneligibilityReason' \
    'enum RuntimeSchedulingClass' \
    'enum RuntimePoolAuthorityKey' \
    'enum RuntimeAdmissionOutcome' \
    'struct RuntimeExecutionPlan' \
    'struct RuntimeExecutionPlanInput' \
    'fn cooperative_eligibility_for' \
    'RuntimeProfile::NodeFull' \
    'CooperativeIneligibilityReason::NodeFullUnproven' \
    'RuntimeAdmissionOutcome::NotEvaluated' \
    '#!\[expect\(' >/tmp/rec1-execution-plan-missing.txt &&
  ! contains 'RuntimeAffinityKey' "${EXECUTION_PLAN}"; then
  pass "REC1 adds internal classifier vocabulary without reusing routing affinity"
else
  fail "REC1 execution-plan module is incomplete" \
    "$(cat /tmp/rec1-execution-plan-missing.txt 2>/dev/null)"
fi

step 6 "REC1 semantic helper rename is behavior-preserving"
if [ -f "${INVOCATION}" ] &&
  [ -f "${EXECUTION_PLAN}" ] &&
  contains 'is_convex_read_semantic_candidate' "${INVOCATION}" &&
  contains 'matches!\(self, Self::Query | Self::PaginatedQuery\)' "${INVOCATION}" &&
  contains 'is_convex_read_semantic_candidate' "${EXECUTION_PLAN}" &&
  ! grep -R 'is_convex_read_semantic_candidate' crates/nimbus-runtime/src/worker_loop >/dev/null 2>&1 &&
  ! grep -R 'allows_cooperative_multiplexing' crates/nimbus-runtime/src >/dev/null 2>&1; then
  pass "InvocationKind exposes only semantic evidence consumed by RuntimeExecutionPlan"
else
  fail "REC1 semantic helper rename is incomplete" \
    "expected helper to remain classifier-only, with no worker-loop use and no allows_cooperative_multiplexing"
fi

step 7 "REC2 host operation effect classifier is enum-owned and exhaustive"
if [ -f "${HOST}" ] &&
  contains_all "${HOST}" \
    'pub\(crate\) const fn runtime_effect_class' \
    'RuntimeEffectClass::ObservableRead' \
    'RuntimeEffectClass::Write' \
    'RuntimeEffectClass::Scheduler' \
    'RuntimeEffectClass::ServiceExternal' \
    'RuntimeEffectClass::NestedRuntime' \
    'RuntimeEffectClass::HttpRoute' \
    'RuntimeEffectClass::Extension' \
    'host_call_operations_have_exhaustive_runtime_effect_classes' >/tmp/rec2-host-classifier-missing.txt; then
  pass "HostCallOperation owns the exhaustive runtime effect classifier"
else
  fail "REC2 host operation effect classifier is incomplete" \
    "$(cat /tmp/rec2-host-classifier-missing.txt 2>/dev/null)"
fi

step 8 "REC2 observed host effects are guarded through execution-plan state"
if [ -f "${EXECUTION_PLAN}" ] &&
  [ -f "${BOOTSTRAP_STATE}" ] &&
  [ -f "${OPS_SHARED}" ] &&
  contains_all "${EXECUTION_PLAN}" \
    'struct RuntimeObservedEffectViolation' \
    'observed_effect_violation' \
    'cooperative_ineligibility_reason_for_effect_class' \
    'runtime_execution_plan_reports_typed_observed_effect_violations' >/tmp/rec2-plan-effect-missing.txt &&
  contains_all "${BOOTSTRAP_STATE}" \
    'struct RuntimeInvocationExecutionPlanBinding' \
    'fn inactive\(\) -> Self' \
    'fn for_plan\(plan: &RuntimeExecutionPlan\) -> Self' \
    'unwrap_or_else\(RuntimeInvocationExecutionPlanBinding::inactive\)' >/tmp/rec2-state-binding-missing.txt &&
  contains_all "${OPS_SHARED}" \
    'RuntimeInvocationExecutionPlanBinding' \
    'enforce_observed_host_call_effect' \
    'operation.runtime_effect_class\(\)' \
    'plan.observed_effect_violation' \
    'enforce_live_host_call_session' \
    'enforce_host_call_grants' >/tmp/rec2-shared-guard-missing.txt; then
  pass "shared host-call path has an execution-plan observed-effect guard"
else
  fail "REC2 observed-effect guard is incomplete" \
    "$(cat /tmp/rec2-plan-effect-missing.txt 2>/dev/null) $(cat /tmp/rec2-state-binding-missing.txt 2>/dev/null) $(cat /tmp/rec2-shared-guard-missing.txt 2>/dev/null)"
fi

step 9 "REC3 scheduler consumes RuntimeExecutionPlan instead of InvocationKind"
if [ -f "${EXECUTION_PLAN}" ] &&
  [ -f "${WORKER_JOB}" ] &&
  [ -f "${EXECUTOR_INVOKE}" ] &&
  [ -f "${COOP_RUN}" ] &&
  [ -f "${COOP_EXECUTION}" ] &&
  [ -f "${ADMISSION}" ] &&
  contains_all "${EXECUTION_PLAN}" \
    'fn for_invocation' \
    'permits_cooperative_scheduler_admission' \
    'RuntimeEffectClass::ObservableRead' \
    'RuntimeProfile::NodeFull' \
    'CooperativeIneligibilityReason::NodeFullUnproven' \
    'host_work_class_for_context' >/tmp/rec3-plan-missing.txt &&
  contains_all "${WORKER_JOB}" \
    'execution_plan: RuntimeExecutionPlan' >/tmp/rec3-worker-job-missing.txt &&
  contains_all "${EXECUTOR_INVOKE}" \
    'RuntimeExecutionPlan::for_invocation' \
    'execution_plan,' >/tmp/rec3-executor-invoke-missing.txt &&
  contains_all "${COOP_BACKEND}" \
    'fn permits_scheduler_admission' \
    'execution_plan.permits_cooperative_scheduler_admission\(\)' >/tmp/rec3-coop-backend-missing.txt &&
  contains_all "${COOP_RUN}" \
    'driver.permits_scheduler_admission' >/tmp/rec3-coop-run-missing.txt &&
  contains_all "${COOP_EXECUTION}" \
    'driver.permits_scheduler_admission' \
    'execution_plan: job.execution_plan.clone\(\)' >/tmp/rec3-coop-execution-missing.txt &&
  contains_all "${ADMISSION}" \
    'job.execution_plan.host_work_class\(\)' >/tmp/rec3-admission-missing.txt &&
  ! grep -R 'is_convex_read_semantic_candidate' crates/nimbus-runtime/src/worker_loop >/dev/null 2>&1; then
  pass "cooperative scheduler and host work-class admission consume RuntimeExecutionPlan"
else
  fail "REC3 scheduler consumption is incomplete" \
    "$(cat /tmp/rec3-plan-missing.txt 2>/dev/null) $(cat /tmp/rec3-worker-job-missing.txt 2>/dev/null) $(cat /tmp/rec3-executor-invoke-missing.txt 2>/dev/null) $(cat /tmp/rec3-coop-backend-missing.txt 2>/dev/null) $(cat /tmp/rec3-coop-run-missing.txt 2>/dev/null) $(cat /tmp/rec3-coop-execution-missing.txt 2>/dev/null) $(cat /tmp/rec3-admission-missing.txt 2>/dev/null)"
fi

step 10 "REC3 runtime negative and compatibility tests exist"
if [ -f "${COOP_TESTS}" ] &&
  contains_all "${COOP_TESTS}" \
    'REC3_QUERY_WRITE_EFFECT_VIOLATION_CASE' \
    'rec3_query_write_effect_violation_rejects_before_host_dispatch' \
    'runtime host-call effect violation' \
    'HostCallOperation::DocumentInsert' \
    'pir4_mutations_do_not_enter_multiplexed_read_safe_scheduler' >/tmp/rec3-tests-missing.txt; then
  pass "REC3 has query-shaped write denial and mutation exclusion coverage"
else
  fail "REC3 runtime tests are incomplete" \
    "$(cat /tmp/rec3-tests-missing.txt 2>/dev/null)"
fi

step 11 "REC4 runtime context shape is request-kind capability aware"
if [ -f "${BOOTSTRAP_CONTEXT_SOURCE}" ] &&
  [ -f "${CODEGEN_CONTEXT}" ] &&
  contains_all "${BOOTSTRAP_CONTEXT_SOURCE}" \
    'requestKind' \
    'capabilities' \
    'dbWrite' \
    'nestedCalls' \
    'not available for \$\{requestKind' \
    'case "query"' \
    'case "mutation"' \
    'case "action"' >/tmp/rec4-runtime-context-missing.txt &&
  contains_all "${CODEGEN_CONTEXT}" \
    'function contextCapabilities' \
    'dbWrite: false' \
    'dbWrite: true' \
    'scheduler: true' \
    'createUnsupportedContextApi' >/tmp/rec4-codegen-context-missing.txt; then
  pass "runtime bootstrap and codegen share query/mutation/action capability shapes"
else
  fail "REC4 runtime/codegen context shape is incomplete" \
    "$(cat /tmp/rec4-runtime-context-missing.txt 2>/dev/null) $(cat /tmp/rec4-codegen-context-missing.txt 2>/dev/null)"
fi

step 12 "REC4 context and raw host-op regression tests exist"
if [ -f "${HOST_BRIDGE_TESTS}" ] &&
  [ -f "${COOP_TESTS}" ] &&
  contains_all "${HOST_BRIDGE_TESTS}" \
    'runtime_query_context_is_reader_only_when_request_kind_is_present' \
    'runtime_action_context_exposes_nested_calls_without_direct_db' \
    'not available for query handlers' \
    'not available for action handlers' >/tmp/rec4-host-bridge-tests-missing.txt &&
  contains_all "${COOP_TESTS}" \
    'REC3_QUERY_WRITE_EFFECT_VIOLATION_CASE' \
    '__nimbusAsyncHostValue\("op_nimbus_document_insert"' \
    'runtime host-call effect violation' >/tmp/rec4-coop-tests-missing.txt; then
  pass "runtime tests cover context denial and lower-level host-op effect denial"
else
  fail "REC4 regression tests are incomplete" \
    "$(cat /tmp/rec4-host-bridge-tests-missing.txt 2>/dev/null) $(cat /tmp/rec4-coop-tests-missing.txt 2>/dev/null)"
fi

step 13 "REC5 waitUntil and warm-pool hot-path cleanup is present"
if [ -f "${BOOTSTRAP_STATE}" ] &&
  [ -f "${OPS_SHARED}" ] &&
  [ -f "${BOOTSTRAP_HOST_CALL_TRANSPORT}" ] &&
  [ -f "${V8_LIFECYCLE}" ] &&
  [ -f "${WARM_POOL}" ] &&
  [ -f "crates/nimbus-runtime/src/runtime/driver/invocation.rs" ] &&
  contains_all "${BOOTSTRAP_STATE}" \
    'struct RuntimeWaitUntilState' \
    'mark_pending' \
    'take_runtime_wait_until_pending' \
    'clear_runtime_wait_until_pending' \
    'state.put\(RuntimeWaitUntilState::default\(\)\)' >/tmp/rec5-wait-state-missing.txt &&
  contains_all "${OPS_SHARED}" \
    'op_nimbus_runtime_wait_until_pending' \
    'RuntimeWaitUntilState' \
    'mark_pending\(\)' >/tmp/rec5-wait-op-missing.txt &&
  contains_all "${BOOTSTRAP_HOST_CALL_TRANSPORT}" \
    'op_nimbus_runtime_wait_until_pending' \
    'markPending\(\)' >/tmp/rec5-wait-js-missing.txt &&
  contains_all "crates/nimbus-runtime/src/runtime/driver/invocation.rs" \
    'take_runtime_wait_until_pending' \
    'begin_wait_until_phase' \
    'RuntimePoolKind::WarmPool' >/tmp/rec5-invocation-hotpath-missing.txt; then
  if contains_all "${V8_LIFECYCLE}" \
    'prepare_warm_runtime_for_retention' \
    'reset_request_state\(\)' \
    'RequestStateResetFailed' >/tmp/rec5-lifecycle-cleanup-missing.txt &&
    contains_all "${WARM_POOL}" \
      'record_warm_runtime_condemnation' \
      'record_warm_pool_discard_unquiesced' >/tmp/rec5-warm-pool-cleanup-missing.txt &&
    ! contains 'reset_request_state\(\)' "crates/nimbus-runtime/src/runtime/driver/invocation.rs"; then
    pass "waitUntil pending checks and centralized retention cleanup are wired into the hot path"
  else
    fail "REC5 waitUntil/warm-pool hot-path cleanup is incomplete" \
      "$(cat /tmp/rec5-lifecycle-cleanup-missing.txt 2>/dev/null) $(cat /tmp/rec5-warm-pool-cleanup-missing.txt 2>/dev/null)"
  fi
else
  fail "REC5 waitUntil/warm-pool hot-path cleanup is incomplete" \
    "$(cat /tmp/rec5-wait-state-missing.txt 2>/dev/null) $(cat /tmp/rec5-wait-op-missing.txt 2>/dev/null) $(cat /tmp/rec5-wait-js-missing.txt 2>/dev/null) $(cat /tmp/rec5-invocation-hotpath-missing.txt 2>/dev/null)"
fi

step 14 "REC5 final closeout keeps scheduler and host-admission ownership clean"
if [ -f "${EXECUTION_PLAN}" ] &&
  [ -f "${COOP_RUN}" ] &&
  [ -f "${COOP_EXECUTION}" ] &&
  [ -f "${COOP_BACKEND}" ] &&
  [ -f "${ADMISSION}" ] &&
  contains 'execution_plan.permits_cooperative_scheduler_admission\(\)' "${COOP_BACKEND}" &&
  contains 'driver.permits_scheduler_admission' "${COOP_RUN}" &&
  contains 'driver.permits_scheduler_admission' "${COOP_EXECUTION}" &&
  contains 'job.execution_plan.host_work_class\(\)' "${ADMISSION}" &&
  ! grep -R 'job.request.kind.is_convex_read_semantic_candidate' crates/nimbus-runtime/src/worker_loop >/dev/null 2>&1 &&
  ! grep -R 'allows_cooperative_multiplexing' crates/nimbus-runtime/src >/dev/null 2>&1; then
  pass "scheduler consumes REC execution plans and host admission consumes PIR7 work class"
else
  fail "REC5 ownership closeout regressed" \
    "expected no direct InvocationKind scheduler path and no parallel host-admission model"
fi

printf '\nSummary: %d passed, %d failed\n' "${PASS}" "${FAIL}"
if [ "${FAIL}" -ne 0 ]; then
  printf '\nFailing conditions:\n'
  for detail in "${FAIL_DETAIL[@]}"; do
    printf '  - %s\n' "${detail}"
  done
  exit 1
fi
