#!/usr/bin/env bash
# Static architecture gate for runtime-owner isolation. Behavioral tests prove
# execution semantics; this gate prevents the ownership seams from drifting
# back into adapter configuration, routing labels, or backend-local keys.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

passed=0

pass() {
  passed=$((passed + 1))
  printf 'ok %d - %s\n' "${passed}" "$1"
}

fail() {
  printf 'not ok - %s\n' "$1" >&2
  exit 1
}

reject_pattern() {
  local description="$1"
  local pattern="$2"
  shift 2
  if rg -n --glob '*.rs' "${pattern}" "$@"; then
    fail "${description}"
  fi
  pass "${description}"
}

require_pattern() {
  local description="$1"
  local pattern="$2"
  shift 2
  if ! rg -n --glob '*.rs' "${pattern}" "$@" >/dev/null; then
    fail "${description}"
  fi
  pass "${description}"
}

printf 'Nimbus runtime tenant-isolation static gate\n\n'

reject_pattern \
  'adapter registries do not own RuntimeExecutor fields' \
  'RuntimeExecutor' \
  crates/nimbus-convex/src/registry \
  crates/nimbus-cloud-functions/src/registry.rs

reject_pattern \
  'production adapters do not construct runtime executors' \
  'RuntimeExecutor::new' \
  crates/nimbus-convex/src \
  crates/nimbus-cloud-functions/src \
  crates/nimbus-server/src/adapters

reject_pattern \
  'adapter registries do not own canonical runtime governor or policy' \
  'Runtime(GovernorConfig|Policy)' \
  crates/nimbus-convex/src/registry \
  crates/nimbus-cloud-functions/src/registry.rs

require_pattern \
  'compute state owns the canonical RuntimeManager' \
  'runtime_manager:[[:space:]]*Arc<RuntimeManager>' \
  crates/nimbus-compute/src/state.rs

require_pattern \
  'compute projects canonical base limits into adapter execution requirements' \
  'with_runtime_limits\(base_runtime_limits\.clone\(\)\)' \
  crates/nimbus-compute/src/state.rs

reject_pattern \
  'retained-state keys do not use an optional tenant label as ownership' \
  '(tenant(_label)?[[:space:]]*:[[:space:]]*Option|Option<[^>]+>[[:space:]]*,?[[:space:]]*//.*tenant)' \
  crates/nimbus-runtime/src/retained_state.rs \
  crates/nimbus-runtime/src/backends/v8/warm_pool.rs \
  crates/nimbus-runtime/src/backends/wasmtime/store_pool.rs

require_pattern \
  'runtime owner IDs carry an owner class' \
  'class:[[:space:]]*RuntimeOwnerClass' \
  crates/nimbus-runtime/src/retained_state.rs
require_pattern \
  'runtime owner IDs carry an opaque stable subject' \
  'stable_subject:[[:space:]]*Arc<str>' \
  crates/nimbus-runtime/src/retained_state.rs
require_pattern \
  'runtime owner IDs carry a nonzero incarnation' \
  'incarnation:[[:space:]]*NonZeroU64' \
  crates/nimbus-runtime/src/retained_state.rs

reject_pattern \
  'compute does not allocate a parallel tenant-owner incarnation' \
  '(NEXT_[A-Z_]*(OWNER|INCARNATION)|TENANT_[A-Z_]*INCARNATION|owner_incarnation[[:space:]]*\.fetch_add)' \
  crates/nimbus-compute/src

if rg -n --glob '*.rs' --glob '!runtime_manager.rs' \
  'RuntimeOwnerId::tenant' crates/nimbus-compute/src; then
  fail 'tenant owners are lowered only by the compute RuntimeManager'
fi
pass 'tenant owners are lowered only by the compute RuntimeManager'

reject_pattern \
  'routing affinity never constructs runtime owners' \
  'RuntimeOwner(Id|Lease|Class)|runtime_owner_lease' \
  crates/nimbus-runtime/src/affinity.rs
reject_pattern \
  'bundle loading never derives runtime owners from bundle tenant metadata' \
  'RuntimeOwnerId::tenant|RuntimeOwnerLeaseIssuer' \
  crates/nimbus-runtime/src/runtime/driver/loading.rs \
  crates/nimbus-runtime/src/runtime.rs

for pool_kind in WarmPool RetainedStorePool BunJscTrustedRetained; do
  if ! sed -n '/pub(crate) fn validate_retained_state_admission/,/^}/p' \
    crates/nimbus-runtime/src/retained_state.rs | grep -F "RuntimePoolKind::${pool_kind}" >/dev/null; then
    fail "${pool_kind} is missing from the common retained-state admission guard"
  fi
done
pass 'every mutable retained pool kind requires the common owner lease guard'

require_pattern \
  'executor admission invokes the common retained-state guard' \
  'validate_retained_state_admission' \
  crates/nimbus-runtime/src/executor/invoke.rs
require_pattern \
  'Bun/JSC invokes the common retained-state guard before backend entry' \
  'validate_retained_state_admission' \
  crates/nimbus-runtime/src/backends/bun_jsc/mod.rs
require_pattern \
  'Bun/JSC retained mode remains non-product-selectable' \
  'product_selectable:[[:space:]]*false' \
  crates/nimbus-runtime/src/backends/bun_jsc/pool.rs

reject_pattern \
  'startup snapshots and anchors do not consume tenant or invocation ownership state' \
  'tenant_label|runtime_owner_lease|RuntimeInvocationContext' \
  crates/nimbus-runtime/src/backends/v8/startup.rs \
  crates/nimbus-runtime/src/backends/v8/startup_key.rs \
  crates/nimbus-runtime/src/runtime/driver/anchor.rs
require_pattern \
  'the NodeFull anchor uses the platform-owned virtual anchor bundle' \
  'RuntimeBundle::virtual_anchor' \
  crates/nimbus-runtime/src/runtime/driver/anchor.rs

printf '\nruntime tenant-isolation static gate: %d passed, 0 failed\n' "${passed}"
