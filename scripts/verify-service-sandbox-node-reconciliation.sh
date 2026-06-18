#!/usr/bin/env bash
# Aggregate completion-gate verifier for
# docs/private/plans/service-sandbox-node-reconciliation-plan.md (NSR0..NSR11).
#
# Ships in NSR0 so /goal is verifiable from day one. Later phases progressively
# flip conditions from FAIL to PASS. Heavy test gates live in phase proof and CI;
# this script keeps cheap/static assertions and cargo fmt.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PLAN="docs/private/plans/service-sandbox-node-reconciliation-plan.md"
PLANS_README="docs/private/plans/README.md"
PROOF_DIR="docs/private/plans/proof/service-sandbox-node-reconciliation"
VERIFIER="scripts/verify-service-sandbox-node-reconciliation.sh"
BIN_MAIN="crates/nimbus-bin/src/main.rs"
NODE_CARGO="crates/nimbus-node/Cargo.toml"
NODE_SRC="crates/nimbus-node/src"
COMPOSE_SRC="crates/nimbus-bin/src/compose"
SESSION_MODEL="docs/private/architecture/sandbox/service-sandbox-session-model.md"
SDK_PLAN="docs/private/plans/archive/nimbus-sdk-resource-model-plan.md"
CAPABILITY_PLAN="docs/private/plans/archive/nimbus-capability-segregation-plan.md"
NODE_DBUS_DOC="docs/private/operating/node-dbus-binding.md"
MICROVM_DOC="docs/private/architecture/sandbox/microvm-service-baseline.md"

PASS=0
FAIL=0
FAIL_DETAIL=()

pass() {
  PASS=$((PASS + 1))
  printf 'PASS  %s\n' "$1"
}

fail() {
  FAIL=$((FAIL + 1))
  FAIL_DETAIL+=("$1: $2")
  printf 'FAIL  %s\n      %s\n' "$1" "$2"
}

crates_grep() {
  grep -rq --include='*.rs' "$1" crates/ 2>/dev/null
}

plan_grep() {
  grep -q "$1" "${PLAN}" 2>/dev/null
}

# --- 1. NSR0 control-plane registration --------------------------------------
C="1. NSR0 control plane, proof bundle, and verifier are registered"
if [[ -f "${PLAN}" ]] \
  && grep -q 'Status: active control plane' "${PLAN}" \
  && grep -q 'Control Plane Protocol' "${PLAN}" \
  && grep -q 'Verifiable Success Criteria' "${PLAN}" \
  && grep -q 'Autonomous `/goal` Prompt' "${PLAN}" \
  && grep -q 'service-sandbox-node-reconciliation-plan.md' "${PLANS_README}" \
  && [[ -d "${PROOF_DIR}" ]] \
  && [[ -f "${PROOF_DIR}/README.md" ]] \
  && [[ -f "${PROOF_DIR}/nsr0-control-plane.md" ]] \
  && [[ -f "${VERIFIER}" ]]; then
  pass "${C}"
else
  fail "${C}" "missing active plan markers, README entry, proof files, or verifier"
fi

# --- 2. NSR1 node namespace remains workload-free -----------------------------
C="2. NSR1 public nimbus node run is absent and regression test exists"
if crates_grep 'node_run_is_not_a_public_node_subcommand' \
  && ! grep -rq '## nimbus node run' docs/reference/cli.md docs/concepts/architecture 2>/dev/null; then
  pass "${C}"
else
  fail "${C}" "public nimbus node run docs or regression test are missing/stale"
fi

# --- 3. NSR1 public run/sandbox verbs and target resolver ---------------------
C="3. NSR1 nimbus run and nimbus sandbox command surfaces use a shared target resolver"
if grep -qE 'Run\(' "${BIN_MAIN}" \
  && grep -qE 'Sandbox\(' "${BIN_MAIN}" \
  && crates_grep 'TargetContext' \
  && crates_grep 'run_command_resolves_target' \
  && crates_grep 'sandbox_command_resolves_target'; then
  pass "${C}"
else
  fail "${C}" "Run/Sandbox command variants or TargetContext parser tests are not implemented yet"
fi

# --- 4. NSR1 start/dev/compose Compose trigger policy -------------------------
C="4. NSR1 start requires explicit Compose input while dev/compose keep scoped discovery"
if crates_grep 'start_does_not_auto_admit_ambient_compose' \
  && crates_grep 'dev_auto_discovers_compose_for_local_development' \
  && crates_grep 'compose_command_auto_discovers_compose_project'; then
  pass "${C}"
else
  fail "${C}" "canonical Compose trigger-policy tests are missing"
fi

# --- 5. NSR2a desired-state/controller seam -----------------------------------
C="5. NSR2a WorkloadController and DesiredWorkloadStore are implemented"
if crates_grep 'struct WorkloadController' \
  && crates_grep 'DesiredWorkloadStore' \
  && crates_grep 'desired_workload_replay_after_restart'; then
  pass "${C}"
else
  fail "${C}" "WorkloadController/DesiredWorkloadStore or replay test missing"
fi

# --- 6. NSR2b scheduler/placement has no lifecycle side effects ---------------
C="6. NSR2b scheduler and placement engine are side-effect-free at the interface"
if crates_grep 'struct WorkloadScheduler' \
  && crates_grep 'WorkloadPlacementEngine' \
  && crates_grep 'SchedulingExplanation' \
  && crates_grep 'generated_workload_placement' \
  && ! grep -rqE 'SandboxBackend::start|StartTransientUnit|open_channel|write_status' crates/nimbus-server/src/workload_control 2>/dev/null; then
  pass "${C}"
else
  fail "${C}" "scheduler symbols/tests missing or lifecycle side effects appear in workload_control"
fi

# --- 7. NSR2c queue/replay/reservation safety ---------------------------------
C="7. NSR2c WorkloadEventQueue, reservation, and binding-conflict tests exist"
if crates_grep 'WorkloadEventQueue' \
  && crates_grep 'WorkloadEvaluation' \
  && crates_grep 'stale_snapshot_requeues_workload_evaluation' \
  && crates_grep 'reservation_expiry_unblocks_workload' \
  && crates_grep 'binding_conflict_requeues_with_reason'; then
  pass "${C}"
else
  fail "${C}" "queue/replay/reservation safety symbols or tests missing"
fi

# --- 8. NSR2d executor/channel seam -------------------------------------------
C="8. NSR2d WorkloadExecutor seam and fake lifecycle tests exist"
if crates_grep 'trait WorkloadExecutor' \
  && crates_grep 'EmbeddedNodeClient' \
  && crates_grep 'open_channel' \
  && crates_grep 'fake_executor_lifecycle_reaches_ready'; then
  pass "${C}"
else
  fail "${C}" "WorkloadExecutor/EmbeddedNodeClient/open_channel seam or fake tests missing"
fi

# --- 9. NSR3 node-agent invariants --------------------------------------------
C="9. NSR3 node agent keeps nimbus-node server-free and has node state tests"
if ! grep -q 'nimbus-server' "${NODE_CARGO}" \
  && ! grep -rq 'nimbus_server' "${NODE_SRC}" 2>/dev/null \
  && crates_grep 'NodeAgent' \
  && crates_grep 'node_agent_reconciles_multiple_workloads_idempotently' \
  && crates_grep 'node_state_transition_assignment_disposition'; then
  pass "${C}"
else
  fail "${C}" "nimbus-node dependency invariant failed or node-agent tests missing"
fi

# --- 10. NSR4 typed runner/systemd launch -------------------------------------
C="10. NSR4 typed runner specs render systemd plans without raw host commands"
if crates_grep 'RunnerSpec' \
  && crates_grep 'runner_spec_renders_host_lifecycle_request' \
  && crates_grep 'raw_host_command_rejected_by_workload_control'; then
  pass "${C}"
else
  fail "${C}" "typed runner spec or raw-command rejection tests missing"
fi

# --- 11. NSR5 machine-os guest node gates -------------------------------------
C="11. NSR5 machine-os guest-node promotion gates are reachable"
if [[ -x scripts/verify-bootc-default-promotion-gate.sh ]] \
  && [[ -x ../machine-os/scripts/check-selinux-avcs.sh ]] \
  && [[ -f "${PROOF_DIR}/nsr5-machine-os-guest-node.md" ]]; then
  pass "${C}"
else
  fail "${C}" "machine-os gate scripts or NSR5 proof file missing"
fi

# --- 12. NSR6 Compose lifecycle no-bypass -------------------------------------
C="12. NSR6 Compose lifecycle uses an explicit sandbox lifecycle adapter"
if [[ -d "${COMPOSE_SRC}" ]] \
  && ! grep -rqE 'SandboxBackend::(start|stop|inspect)' "${COMPOSE_SRC}" 2>/dev/null \
  && crates_grep 'trait ComposeSandboxLifecycleExecutor' \
  && crates_grep 'compose_lifecycle_uses_explicit_sandbox_lifecycle_adapter'; then
  pass "${C}"
else
  fail "${C}" "Compose direct SandboxBackend lifecycle remains or explicit lifecycle-adapter guard test missing"
fi

# --- 13. NSR7 admitted Compose templates lease sandboxes safely ----------------
C="13. NSR7 admitted Compose templates lease sandboxes safely"
if plan_grep 'sandboxes.create({ template: "agent-browser" })' \
  && plan_grep 'nimbus.yaml' \
  && plan_grep 'nimbus.policy.yaml' \
  && plan_grep 'profiles: \["nimbus-template"\]' \
  && plan_grep 'templates.agentBrowser.start' \
  && plan_grep 'Symbol.asyncDispose' \
  && plan_grep 'createFromTemplate' \
  && plan_grep 'operator/security-owned' \
  && plan_grep 'effective policy' \
  && grep -q 'createFromTemplate' "${CAPABILITY_PLAN}" 2>/dev/null \
  && grep -q 'sandboxes.create({ template' "${SDK_PLAN}" 2>/dev/null \
  && crates_grep 'SandboxTemplate' \
  && crates_grep 'compose_imports_sandbox_template' \
  && crates_grep 'deploy_packages_nimbus_yaml_app_intent' \
  && crates_grep 'prod_deploy_rejects_app_bundled_policy_authority' \
  && crates_grep 'app_intent_is_admitted_against_effective_policy' \
  && crates_grep 'create_from_template_requires_exact_grant' \
  && crates_grep 'sandbox_template_ports_are_channel_only' \
  && crates_grep 'leased_sandbox_ttl_reconciles_deadline' \
  && crates_grep 'leased_sandbox_quota_is_enforced'; then
  pass "${C}"
else
  fail "${C}" "SandboxTemplate import, nimbus.yaml app intent, operator-policy admission, typed template SDK, TTL/quota, or channel-only port tests missing"
fi

# --- 14. NSR9 session channel model and tests ---------------------------------
C="14. NSR9 session channel lifecycle is canonical and tested"
if grep -q 'half-close' "${SESSION_MODEL}" \
  && grep -q 'backpressure' "${SESSION_MODEL}" \
  && crates_grep 'session_channel_target_generation_mismatch' \
  && crates_grep 'session_channel_half_close' \
  && crates_grep 'session_channel_backpressure' \
  && crates_grep 'session_channel_disconnect_audit'; then
  pass "${C}"
else
  fail "${C}" "session model or canonical channel tests missing"
fi

# --- 15. NSR10 Wasm seam remains reserved/fail-closed --------------------------
C="15. NSR10 Wasm runner is reserved without turning invocation isolates into sandboxes"
if plan_grep 'wasmtime-backend-plan.md' \
  && plan_grep 'runtime invocation isolates' \
  && crates_grep 'wasm_sandbox_requests_fail_closed'; then
  pass "${C}"
else
  fail "${C}" "Wasm seam docs or fail-closed test missing"
fi

# --- 16. NSR11 docs/source-map stale claims -----------------------------------
C="16. NSR11 stale docs and command-surface drift are fixed"
if grep -q 'node-workload-executor' "${NODE_DBUS_DOC}" \
  && ! grep -q 'ServiceManager.*nimbus-server' "${MICROVM_DOC}" \
  && ! grep -q '`data`' ARCHITECTURE.md \
  && grep -q '`backup`' ARCHITECTURE.md \
  && grep -q 'node-workload-executor' docs/source-map.md; then
  pass "${C}"
else
  fail "${C}" "node/systemd docs, ARCHITECTURE.md, or source-map command anchors are stale"
fi

# --- 17. formatting ------------------------------------------------------------
C="17. cargo fmt --all --check passes"
if cargo fmt --all --check >/dev/null 2>&1; then
  pass "${C}"
else
  fail "${C}" "cargo fmt --all --check reported diffs"
fi

printf '\n%d/17 conditions green\n' "${PASS}"
if [[ ${FAIL} -gt 0 ]]; then
  exit 1
fi
exit 0
