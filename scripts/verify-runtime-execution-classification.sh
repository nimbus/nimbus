#!/usr/bin/env bash
# Verification gate for the Runtime Execution Classification plan. The
# completed plan lives under docs/private/plans/archive.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PLAN_ACTIVE="docs/private/plans/runtime-execution-classification-plan.md"
PLAN_ARCHIVE="docs/private/plans/archive/runtime-execution-classification-plan.md"
if [ -f "${PLAN_ACTIVE}" ]; then
  PLAN="${PLAN_ACTIVE}"
else
  PLAN="${PLAN_ARCHIVE}"
fi
PROOF="docs/private/plans/proof/runtime-execution-classification/rec0-baseline.md"
REC1_PROOF="docs/private/plans/proof/runtime-execution-classification/rec1-execution-plan.md"
REC2_PROOF="docs/private/plans/proof/runtime-execution-classification/rec2-host-effects.md"
REC3_PROOF="docs/private/plans/proof/runtime-execution-classification/rec3-scheduler-consumption.md"
REC4_PROOF="docs/private/plans/proof/runtime-execution-classification/rec4-context-codegen-alignment.md"
REC5_PROOF="docs/private/plans/proof/runtime-execution-classification/rec5-numeric-closeout.md"
REC5_PIR0_TRACE="docs/private/plans/proof/runtime-execution-classification/artifacts/rec5-pir0-selected-trace.jsonl"
REC5_PIR0_WARM_EXCEPTION_TRACE="docs/private/plans/proof/runtime-execution-classification/artifacts/rec5-pir0-current-trace-after-waituntil-phase-gate.jsonl"
REC5_PIR5_RSS_TRACE="docs/private/plans/proof/runtime-execution-classification/artifacts/rec5-pir5-retained-density-current-rss.jsonl"
EXECUTION_PLAN="crates/nimbus-runtime/src/execution_plan.rs"
INVOCATION="crates/nimbus-runtime/src/runtime/invocation.rs"
COOP_RUN="crates/nimbus-runtime/src/worker_loop/cooperative/run.rs"
COOP_EXECUTION="crates/nimbus-runtime/src/worker_loop/cooperative/execution.rs"
COOP_TESTS="crates/nimbus-runtime/src/runtime/tests/cooperative.rs"
HOST="crates/nimbus-runtime/src/host.rs"
BOOTSTRAP_SOURCE="crates/nimbus-runtime/src/runtime/bootstrap/source.rs"
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

step 1 "REC ledger records REC0 through REC5 closed"
if [ ! -f "${PLAN_ACTIVE}" ] &&
  [ -f "${PLAN_ARCHIVE}" ] &&
  contains '\*\*Status:\*\* `done`' "${PLAN}" &&
  contains 'Archive state' "${PLAN}" &&
  contains '\| REC0 \| `done`' "${PLAN}" &&
  contains '\| REC1 \| `done`' "${PLAN}" &&
  contains '\| REC2 \| `done`' "${PLAN}" &&
  contains '\| REC3 \| `done`' "${PLAN}" &&
  contains '\| REC4 \| `done`' "${PLAN}" &&
  contains '\| REC5 \| `done`' "${PLAN}" &&
  contains 'Current PIR baseline' "${PLAN}" &&
  contains '90 passed, 0 failed' "${PLAN}" &&
  contains 'REC0 baseline audit closeout' "${PLAN}" &&
  contains 'REC1 internal execution-plan vocabulary closeout' "${PLAN}" &&
  contains 'REC2 host-operation effect closeout' "${PLAN}" &&
  contains 'REC3 scheduler-consumption closeout' "${PLAN}" &&
  contains 'REC4 runtime-context and codegen-alignment closeout' "${PLAN}" &&
  contains 'REC5 numeric validation and closeout' "${PLAN}"; then
  pass "archived plan ledger closes REC0/REC1/REC2/REC3/REC4/REC5"
else
  fail "REC phase ledger is not in the expected closed state" \
    "expected archived plan, top-level done, REC0/REC1/REC2/REC3/REC4/REC5 done, and PIR 90/0 baseline"
fi

step 2 "REC0 proof records baseline, diagram, and verifier closeout"
if [ -f "${PROOF}" ] &&
  contains_all "${PROOF}" \
    'REC0 Baseline Audit And Verifier Scaffold' \
    'Status: `done`' \
    'bash scripts/verify-profile-aware-isolate-runtime.sh' \
    'Summary: 90 passed, 0 failed' \
    'Current-State Diagram' \
    'RuntimeExecutionPlan' \
    'Summary: 8 passed, 0 failed' >/tmp/rec0-proof-baseline-missing.txt; then
  pass "REC0 proof has the baseline and closeout surface"
else
  fail "REC0 proof baseline is incomplete" \
    "$(cat /tmp/rec0-proof-baseline-missing.txt 2>/dev/null)"
fi

step 3 "REC0 inventory covers required symbols and ownership seams"
if [ -f "${PROOF}" ] &&
  [ -f "${TENANT_EFFICIENCY}" ] &&
  [ -f "${ADMISSION}" ] &&
  [ -f "${AFFINITY}" ] &&
  [ -f "${WARM_POOL}" ] &&
  [ -f "${HOST_STATE}" ] &&
  [ -f "${CONVEX_DISPATCH}" ] &&
  [ -f "${CODEGEN_CONTEXT}" ] &&
  contains_all "${PROOF}" \
    'InvocationKind' \
    'RuntimeWorkerJob' \
    'RuntimeProfile' \
    'RuntimeEfficiencyPlan' \
    'RuntimeTenantBudget' \
    'RuntimeHostWorkClass' \
    'RuntimeHostResourceDecision' \
    'RuntimeAffinityKey' \
    'RuntimePoolPartitionKey' \
    'HostCallOperation' \
    'HostCallPayload::operation' \
    'HostCallEnvelope' \
    'RuntimeHostState' \
    'packages/codegen/src/planner/context_api.mjs' >/tmp/rec0-inventory-missing.txt &&
  contains 'pub struct RuntimeEfficiencyPlan' "${TENANT_EFFICIENCY}" &&
  contains 'fn runtime_host_work_class_for_job' "${ADMISSION}" &&
  contains 'pub\(crate\) enum RuntimeAffinityKey' "${AFFINITY}" &&
  contains 'struct RuntimePoolPartitionKey' "${WARM_POOL}" &&
  contains 'pub struct RuntimeHostState' "${HOST_STATE}"; then
  pass "inventory covers semantic, substrate, effect, budget, affinity, and enforcement seams"
else
  fail "REC0 current-state inventory is incomplete" \
    "$(cat /tmp/rec0-inventory-missing.txt 2>/dev/null)"
fi

step 4 "Direct cooperative scheduler consumers were inventoried and are now removed"
if [ -f "${INVOCATION}" ] &&
  [ -f "${COOP_RUN}" ] &&
  [ -f "${COOP_EXECUTION}" ] &&
  contains 'pub\(crate\) const fn is_convex_read_semantic_candidate' "${INVOCATION}" &&
  contains_all "${PROOF}" \
    'crates/nimbus-runtime/src/runtime/invocation.rs' \
    'crates/nimbus-runtime/src/worker_loop/cooperative/run.rs' \
    'crates/nimbus-runtime/src/worker_loop/cooperative/execution.rs' \
    'Current production direct consumers' >/tmp/rec0-scheduler-missing.txt &&
  ! grep -R 'job.request.kind.is_convex_read_semantic_candidate' crates/nimbus-runtime/src/worker_loop >/dev/null 2>&1; then
  pass "REC0 names the old direct consumers and REC3 removes them from worker scheduling"
else
  fail "direct scheduler consumer inventory/removal is incomplete" \
    "$(cat /tmp/rec0-scheduler-missing.txt 2>/dev/null)"
fi

step 5 "Host operation inventory is exhaustive enough for REC2"
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
    'RuntimeExtensionCall' >/tmp/rec0-host-code-missing.txt &&
  contains_all "${PROOF}" \
    'Host Operation Inventory' \
    'pure/local read, observable read, write, scheduler, service or' \
    'HttpRoute' \
    'CtxRuntimeEnterNestedCall' \
    'RuntimeExtensionCall' >/tmp/rec0-host-proof-missing.txt; then
  pass "host operation enum and proof carry the REC2 exhaustiveness baseline"
else
  fail "host operation inventory is incomplete" \
    "$(cat /tmp/rec0-host-code-missing.txt 2>/dev/null) $(cat /tmp/rec0-host-proof-missing.txt 2>/dev/null)"
fi

step 6 "REC0 answers all open validation questions with fail-closed defaults"
if [ -f "${PROOF}" ] &&
  contains_all "${PROOF}" \
    'OVQ-01' \
    'OVQ-02' \
    'OVQ-03' \
    'OVQ-04' \
    'OVQ-05' \
    'OVQ-06' \
    'OVQ-07' \
    'OVQ-08' \
    'OVQ-09' \
    'OVQ-10' \
    'OVQ-11' \
    'OVQ-12' \
    'OVQ-13' \
    'OVQ-14' \
    'OVQ-15' \
    'OVQ-16' \
    'unknown posture is ineligible' \
    'Unknown or unclassified operations are cooperative-ineligible' \
    'NodeFull starts ineligible for cooperative reuse' >/tmp/rec0-validation-missing.txt; then
  pass "REC0 records validation answers and conservative defaults"
else
  fail "REC0 validation answers are incomplete" \
    "$(cat /tmp/rec0-validation-missing.txt 2>/dev/null)"
fi

step 7 "REC0 carries canonical patterns, complexity pockets, and deletion test"
if [ -f "${PROOF}" ] &&
  contains_all "${PROOF}" \
    'Canonical Pattern Carry-Forward' \
    'Workerd' \
    'OpenWorkers' \
    'Wasmtime' \
    'Kubernetes' \
    'Deletion test' \
    'Scheduler predicate spread' \
    'Job/admission coupling' \
    'Host-call ABI/adapter dispatch' \
    'Routing locality versus authority reuse' \
    'Tenant-admission versus runtime efficiency' \
    'Context narrowing versus JS ambient authority' \
    'Verifier drift' >/tmp/rec0-patterns-missing.txt; then
  pass "REC0 proof preserves the architecture audit findings"
else
  fail "REC0 architecture audit carry-forward is incomplete" \
    "$(cat /tmp/rec0-patterns-missing.txt 2>/dev/null)"
fi

step 8 "REC verifier is wired into helper syntax gates"
if [ -f "${MAKEFILE_PATH}" ] &&
  [ -f "${CI_WORKFLOW}" ] &&
  contains 'bash -n scripts/verify-runtime-execution-classification.sh' "${MAKEFILE_PATH}" &&
  contains 'bash -n scripts/verify-runtime-execution-classification.sh' "${CI_WORKFLOW}"; then
  pass "REC verifier has Makefile and CI syntax coverage"
else
  fail "REC verifier is not wired into proof-helper syntax gates" \
    "expected Makefile proof-helpers and CI proof-helpers to run bash -n"
fi

step 9 "REC1 typed execution-plan vocabulary exists"
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

step 10 "REC1 semantic helper rename is behavior-preserving"
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

step 11 "REC1 proof records focused tests and REC2 handoff"
if [ -f "${REC1_PROOF}" ] &&
  contains_all "${REC1_PROOF}" \
    'REC1 Internal Runtime Execution Plan Proof' \
    'Status: `done`' \
    'RuntimeEffectClass' \
    'CooperativeEligibility' \
    'RuntimePoolAuthorityKey' \
    'RuntimeAdmissionOutcome' \
    'InvocationKind::is_convex_read_semantic_candidate' \
    'NodeFull remains ineligible with `NodeFullUnproven`' \
    'cargo test -p nimbus-runtime runtime_execution_plan --lib -- --nocapture' \
    '5 passed; 0 failed; 0 ignored; 0 measured; 1050 filtered out' \
    'Summary: 11 passed, 0 failed' \
    'REC2 must classify every `HostCallOperation` exhaustively' >/tmp/rec1-proof-missing.txt; then
  pass "REC1 proof records exact tests and next-band contract"
else
  fail "REC1 proof artifact is incomplete" \
    "$(cat /tmp/rec1-proof-missing.txt 2>/dev/null)"
fi

step 12 "REC2 host operation effect classifier is enum-owned and exhaustive"
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

step 13 "REC2 observed host effects are guarded through execution-plan state"
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

step 14 "REC2 proof records focused tests and REC3 handoff"
if [ -f "${REC2_PROOF}" ] &&
  contains_all "${REC2_PROOF}" \
    'REC2 Host Operation Effect Classification Proof' \
    'Status: `done`' \
    'HostCallOperation::runtime_effect_class' \
    'RuntimeObservedEffectViolation' \
    'RuntimeInvocationExecutionPlanBinding' \
    'default binding is inactive' \
    'cargo test -p nimbus-runtime runtime_execution_plan --lib -- --nocapture' \
    '6 passed; 0 failed; 0 ignored; 0 measured; 1051 filtered out' \
    'cargo test -p nimbus-runtime host_call_operations_have_exhaustive_runtime_effect_classes --lib -- --nocapture' \
    '1 passed; 0 failed; 0 ignored; 0 measured; 1056 filtered out' \
    'Summary: 14 passed, 0 failed' \
    'REC3 must install `RuntimeExecutionPlan`' >/tmp/rec2-proof-missing.txt; then
  pass "REC2 proof records exact tests and next-band contract"
else
  fail "REC2 proof artifact is incomplete" \
    "$(cat /tmp/rec2-proof-missing.txt 2>/dev/null)"
fi

step 15 "REC3 scheduler consumes RuntimeExecutionPlan instead of InvocationKind"
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
  contains_all "${COOP_RUN}" \
    'permits_cooperative_scheduler_admission' >/tmp/rec3-coop-run-missing.txt &&
  contains_all "${COOP_EXECUTION}" \
    'permits_cooperative_scheduler_admission' \
    'execution_plan: job.execution_plan.clone\(\)' >/tmp/rec3-coop-execution-missing.txt &&
  contains_all "${ADMISSION}" \
    'job.execution_plan.host_work_class\(\)' >/tmp/rec3-admission-missing.txt &&
  ! grep -R 'is_convex_read_semantic_candidate' crates/nimbus-runtime/src/worker_loop >/dev/null 2>&1; then
  pass "cooperative scheduler and host work-class admission consume RuntimeExecutionPlan"
else
  fail "REC3 scheduler consumption is incomplete" \
    "$(cat /tmp/rec3-plan-missing.txt 2>/dev/null) $(cat /tmp/rec3-worker-job-missing.txt 2>/dev/null) $(cat /tmp/rec3-executor-invoke-missing.txt 2>/dev/null) $(cat /tmp/rec3-coop-run-missing.txt 2>/dev/null) $(cat /tmp/rec3-coop-execution-missing.txt 2>/dev/null) $(cat /tmp/rec3-admission-missing.txt 2>/dev/null)"
fi

step 16 "REC3 runtime negative and compatibility tests exist"
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

step 17 "REC3 proof records focused tests and REC4 handoff"
if [ -f "${REC3_PROOF}" ] &&
  contains_all "${REC3_PROOF}" \
    'REC3 Scheduler Consumption Proof' \
    'Status: `done`' \
    'RuntimeExecutionPlan::for_invocation' \
    'RuntimeExecutionPlan::permits_cooperative_scheduler_admission' \
    'ObservableRead' \
    'NodeFullUnproven' \
    'cargo test -p nimbus-runtime runtime_execution_plan --lib -- --nocapture' \
    '12 passed; 0 failed; 0 ignored; 0 measured; 1051 filtered out' \
    'cargo test -p nimbus-runtime cooperative_execution_model --lib -- --nocapture' \
    '4 passed; 0 failed; 0 ignored; 0 measured; 1059 filtered out' \
    'cargo test -p nimbus-runtime rec3_query_write_effect_violation_rejects_before_host_dispatch --lib -- --nocapture' \
    '1 passed; 0 failed; 1 ignored; 0 measured; 1063 filtered out' \
    'cargo test -p nimbus-runtime pir4_mutations_do_not_enter_multiplexed_read_safe_scheduler --lib -- --nocapture' \
    'Summary: 17 passed, 0 failed' \
    'REC4 must align runtime context shape' >/tmp/rec3-proof-missing.txt; then
  pass "REC3 proof records exact tests and next-band contract"
else
  fail "REC3 proof artifact is incomplete" \
    "$(cat /tmp/rec3-proof-missing.txt 2>/dev/null)"
fi

step 18 "REC4 runtime context shape is request-kind capability aware"
if [ -f "${BOOTSTRAP_SOURCE}" ] &&
  [ -f "${CODEGEN_CONTEXT}" ] &&
  contains_all "${BOOTSTRAP_SOURCE}" \
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

step 19 "REC4 context and raw host-op regression tests exist"
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

step 20 "REC4 proof records tests and REC5 handoff"
if [ -f "${REC4_PROOF}" ] &&
  contains_all "${REC4_PROOF}" \
    'REC4 Runtime Context And Codegen Alignment Proof' \
    'Status: `done`' \
    '__nimbusCreateContext\(\{ request \}\)' \
    'request-kind capability shape' \
    'cargo test -p nimbus-runtime runtime_query_context_is_reader_only_when_request_kind_is_present --lib -- --nocapture' \
    '1 passed; 0 failed; 0 ignored; 0 measured; 1232 filtered out' \
    'cargo test -p nimbus-runtime runtime_mutation_context_exposes_query_and_mutation_nested_calls --lib -- --nocapture' \
    'cargo test -p nimbus-runtime runtime_action_context_exposes_nested_calls_without_direct_db --lib -- --nocapture' \
    'cargo test -p nimbus-runtime rec3_query_write_effect_violation_rejects_before_host_dispatch --lib -- --nocapture' \
    '1 passed; 0 failed; 1 ignored; 0 measured; 1065 filtered out' \
    'npm run test --workspace @nimbus/codegen' \
    'Convex nested-call matrix' \
    'runtime remap fixtures: ok \(4 cases\)' \
    'Summary: 20 passed, 0 failed' \
    'REC5 must run the PIR-aligned numeric validation' >/tmp/rec4-proof-missing.txt; then
  pass "REC4 proof records exact tests and REC5 closeout contract"
else
  fail "REC4 proof artifact is incomplete" \
    "$(cat /tmp/rec4-proof-missing.txt 2>/dev/null)"
fi

step 21 "REC5 waitUntil and warm-pool hot-path cleanup is present"
if [ -f "${BOOTSTRAP_STATE}" ] &&
  [ -f "${OPS_SHARED}" ] &&
  [ -f "${BOOTSTRAP_SOURCE}" ] &&
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
  contains_all "${BOOTSTRAP_SOURCE}" \
    'op_nimbus_runtime_wait_until_pending' \
    'markPending\(\)' >/tmp/rec5-wait-js-missing.txt &&
  contains_all "crates/nimbus-runtime/src/runtime/driver/invocation.rs" \
    'take_runtime_wait_until_pending' \
    'begin_wait_until_phase' \
    'RuntimePoolKind::WarmContextRecycle' >/tmp/rec5-invocation-hotpath-missing.txt; then
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

step 22 "REC5 benchmark artifacts exist with expected row counts"
if [ -f "${REC5_PIR0_TRACE}" ] &&
  [ -f "${REC5_PIR0_WARM_EXCEPTION_TRACE}" ] &&
  [ -f "${REC5_PIR5_RSS_TRACE}" ] &&
  [ "$(wc -l < "${REC5_PIR0_TRACE}")" -ge 38 ] &&
  [ "$(wc -l < "${REC5_PIR0_WARM_EXCEPTION_TRACE}")" -ge 10 ] &&
  [ "$(wc -l < "${REC5_PIR5_RSS_TRACE}")" -eq 1 ] &&
  contains '"benchmark_id":"web_standard/hostless_trivial/run_to_completion/startup_snapshot_cache"' "${REC5_PIR0_TRACE}" &&
  contains '"benchmark_id":"node24/hostless_trivial/run_to_completion/startup_snapshot_cache"' "${REC5_PIR0_TRACE}" &&
  contains '"benchmark_id":"web_standard/compute_bound_jit_hot/cooperative_locker/warm_pool"' "${REC5_PIR0_TRACE}" &&
  contains '"benchmark_id":"web_standard/await_1ms/cooperative_locker_four_tenants/warm_pool"' "${REC5_PIR0_TRACE}" &&
  contains '"benchmark_id":"web_standard/hostless_trivial/cooperative_locker/warm_pool"' "${REC5_PIR0_WARM_EXCEPTION_TRACE}" &&
  contains '"measured_per_runtime_rss_bytes":1245184' "${REC5_PIR5_RSS_TRACE}"; then
  pass "REC5 PIR0/PIR5 artifacts cover selected lanes and retained RSS"
else
  fail "REC5 benchmark artifacts are missing or incomplete" \
    "expected selected trace >=38 rows, focused warm exception trace >=10 rows, retained RSS trace exactly 1 row"
fi

step 23 "REC5 proof records numeric exception and optimization plan"
if [ -f "${REC5_PROOF}" ] &&
  contains_all "${REC5_PROOF}" \
    'REC5 Numeric Validation And Closeout Proof' \
    'Status: `done`' \
    'RuntimeWaitUntilState' \
    'prepare_warm_runtime_for_retention' \
    'rec5-pir0-selected-trace.jsonl' \
    'rec5-pir0-current-trace-after-waituntil-phase-gate.jsonl' \
    'rec5-pir5-retained-density-current-rss.jsonl' \
    'WebStandard hostless run-to-completion' \
    '\+0.89%' \
    'WebStandard cooperative warm-pool path remains a measured latency exception' \
    '890.72-908.86 us' \
    '29.058 us 29.279 us 29.509 us' \
    '1,245,184 bytes/runtime' \
    '4 passed; 0 failed; 0 ignored; 0 measured; 1063 filtered out' \
    '19 passed; 0 failed; 9 ignored; 0 measured; 1039 filtered out' \
    'blocks using REC as performance justification for broader cooperative defaults' \
    'Optimization Plan' >/tmp/rec5-proof-missing.txt; then
  pass "REC5 proof records exact evidence, measured exception, and follow-up constraint"
else
  fail "REC5 proof artifact is incomplete" \
    "$(cat /tmp/rec5-proof-missing.txt 2>/dev/null)"
fi

step 24 "REC5 final closeout keeps scheduler and host-admission ownership clean"
if [ -f "${EXECUTION_PLAN}" ] &&
  [ -f "${COOP_RUN}" ] &&
  [ -f "${COOP_EXECUTION}" ] &&
  [ -f "${ADMISSION}" ] &&
  contains 'permits_cooperative_scheduler_admission' "${COOP_RUN}" &&
  contains 'permits_cooperative_scheduler_admission' "${COOP_EXECUTION}" &&
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
