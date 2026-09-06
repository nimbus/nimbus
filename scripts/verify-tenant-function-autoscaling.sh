#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PASS=0
FAIL=0

pass() {
  printf 'PASS %s\n' "$1"
  PASS=$((PASS + 1))
}

fail() {
  printf 'FAIL %s\n' "$1"
  FAIL=$((FAIL + 1))
}

require_file() {
  local path="$1"
  local label="$2"
  if [[ -f "$path" ]]; then
    pass "$label"
  else
    fail "$label"
  fi
}

require_contains() {
  local path="$1"
  local pattern="$2"
  local label="$3"
  if rg -q "$pattern" "$path"; then
    pass "$label"
  else
    fail "$label"
  fi
}

require_absent() {
  local path="$1"
  local pattern="$2"
  local label="$3"
  if rg -q "$pattern" "$path"; then
    fail "$label"
  else
    pass "$label"
  fi
}

PLAN_ACTIVE="docs/private/plans/tenant-function-autoscaling-plan.md"
PLAN_ARCHIVED="docs/private/plans/archive/tenant-function-autoscaling-plan.md"
if [[ -f "$PLAN_ACTIVE" ]]; then
  PLAN="$PLAN_ACTIVE"
else
  PLAN="$PLAN_ARCHIVED"
fi
PROOF_DIR="docs/private/plans/proof/tenant-function-autoscaling"
PROOF="$PROOF_DIR/README.md"
SCALING="crates/nimbus-runtime/src/limits/scaling.rs"
FUNCTION_SCALING="crates/nimbus-cli/src/function_scaling.rs"
OPERATOR_POLICY="crates/nimbus-tenant/src/operator_policy.rs"
OPERATOR_ADMISSION="crates/nimbus-tenant/src/operator_policy/runtime_scaling.rs"
START_BOOT="crates/nimbus-cli/src/start/boot.rs"
EXPLAIN="crates/nimbus-cli/src/explain.rs"
ROUTER="crates/nimbus-server/src/router.rs"
RUNTIME_POLICY="crates/nimbus-runtime/src/limits/policy.rs"
START_TESTS="crates/nimbus-cli/src/start/tests/cli_surface.rs"
COMPUTE_RUNTIME_CONFIG="crates/nimbus-compute/src/config/runtime.rs"

require_file "$PLAN" "tenant function autoscaling plan exists"
require_file "$PROOF" "tenant function autoscaling proof exists"
require_contains "$PLAN" 'Public v1 function scaling knobs are `preset`, `min_warm`, `max_warm`, and `scale_down_delay`' "plan names the v1 public knobs"
require_contains "$PLAN" 'Autoscaling is inferred' "plan states autoscaling is inferred"

require_absent "$FUNCTION_SCALING" 'pub\(crate\) activation_warm' "public function scaling config rejects activation_warm"
require_absent "$FUNCTION_SCALING" 'pub\(crate\) live_scaling' "public function scaling config rejects live_scaling"
require_contains "$FUNCTION_SCALING" 'unknown_public_activation_warm_rejects' "function config tests reject activation_warm"
require_contains "$FUNCTION_SCALING" 'unknown_public_autoscaling_rejects' "function config tests reject autoscaling"
require_contains "$FUNCTION_SCALING" 'unknown_public_live_scaling_rejects' "function config tests reject live_scaling"
require_contains "$FUNCTION_SCALING" 'no_yaml_dev_uses_zero_min_warm_with_retention' "dev default stays scale-to-zero until traffic"
require_contains "$FUNCTION_SCALING" 'autoscaling_inferred' "function diagnostics expose inferred autoscaling"

require_absent "$SCALING" 'pub activation_warm' "runtime public scaling structs do not expose activation_warm"
require_absent "$SCALING" 'pub live_scaling' "runtime public scaling structs do not expose live_scaling"
require_contains "$SCALING" 'inferred_autoscaling' "runtime derives autoscaling from preset and range"
require_contains "$SCALING" 'RuntimeScalingPlanSet' "runtime carries selector-aware scaling plan sets"
require_contains "$RUNTIME_POLICY" 'effective_scaling_plan_for_function' "runtime policy resolves scaling by function selector"

require_absent "$OPERATOR_POLICY" 'allow_live_scaling' "operator policy does not expose allow_live_scaling"
require_contains "$OPERATOR_POLICY" 'OperatorRuntimeResourceEnvelope' "operator policy has resource-first runtime envelope"
require_contains "$OPERATOR_POLICY" 'OperatorRuntimeSafetyCaps' "operator policy keeps pool caps as safety guardrails"
require_contains "$OPERATOR_ADMISSION" 'derived_from_resources' "operator admission derives pool ceilings from resources"
require_contains "$FUNCTION_SCALING" 'fn from_host_budget' "CLI admission derives function envelope from host budget"

require_absent "$START_BOOT" 'effective_runtime_scaling_plan_from_intent' "start boot no longer bypasses policy admission"
require_contains "$START_BOOT" 'load_optional_policy' "start boot loads the optional operator policy source"
require_contains "$START_BOOT" 'admit_start_function_scaling_plans' "start boot routes scaling through one start admission seam"
require_contains "$START_BOOT" 'FunctionScalingAdmissionEnvelope::from_host_budget' "start boot uses host-budget-derived function envelope"
require_contains "$START_TESTS" 'start_function_scaling_admission_keeps_selector_overrides' "start tests prove selector overrides reach admitted plans"
require_contains "$START_TESTS" 'start_function_scaling_admission_uses_explicit_operator_policy' "start tests prove explicit operator policy gates admission"
require_contains "$EXPLAIN" 'autoscaling: inferred' "explain output shows inferred autoscaling"
require_contains "$ROUTER" 'with_effective_runtime_scaling_plans' "server router carries selector-aware scaling plan sets"
require_contains "$COMPUTE_RUNTIME_CONFIG" 'with_effective_scaling_plans' "compute runtime policy receives selector-aware scaling plans"

require_contains "$PROOF" 'TFA0' "proof records TFA0"
require_contains "$PROOF" 'TFA6' "proof records TFA6 closeout criteria"

printf '\nverify-tenant-function-autoscaling: %d passed, %d failed\n' "$PASS" "$FAIL"
if ((FAIL > 0)); then
  exit 1
fi
