# Bun/JSC Gate 34: Embedder API And Pool Closeout

Date: 2026-05-23

Nimbus plan: `docs/plans/bun-jsc-embedder-api-and-pool-plan.md`

## Status

Status: local and Linux/minicloud proof passed; `BEP9` is complete.

The BEP0-BEP9 wave closes with Bun/JSC admitted as an optional in-process
runtime backend candidate beside the existing Deno/V8 backend lane. The
admitted product shape remains intentionally narrow:

```text
runtime_environment: bun
runtime_engine: bun_jsc
runtime_bundle_content_kind: javascript
runtime_javascript_evaluation_format: program_wrapper
runtime_compatibility_target: bun_jsc
runtime_package_resolution: bun_self_contained
backend_trust_tier: in_process_untrusted
backend_lockdown_profile: bun_jsc_in_process_untrusted
backend_lifecycle_policy: bun_jsc_fresh_discard_pool_outer_quota_required
runtime_pool_kind: bun_jsc_fresh_discard
execution_model: backend_owned_event_loop
```

Current Nimbus builds still fail closed before Bun/JSC guest execution unless a
Bun embedder execution adapter is linked. The per-function `"use bun"` selector
is intentionally withheld until that adapter exists and can run the admitted
lane.

## Product Decision

The product decision after BEP9 is:

- Continue pursuing Bun/JSC as an optional in-process runtime backend beside
  Deno/V8.
- Keep Bun/JSC behind a dedicated Bun pool because VM construction, resolver
  policy, cancellation, event-loop progress, memory pressure, and teardown are
  backend-owned.
- Admit only the proven untrusted fresh/discard profile with outer quota
  enforcement.
- Keep retained Bun/JSC reuse trusted-only until a hard isolation boundary is
  proven.
- Do not fork Bun yet. Stay upstream-first unless the missing stable embedder
  API surface remains small, product-critical, proven, and unavailable
  upstream.
- Treat OCI/microVM Bun workloads as a separate sandbox mode, not as the answer
  to the in-process runtime question.

## Local Evidence

Passed in `/Users/jack/src/github.com/nimbus/nimbus` at Nimbus commit
`3b6c27bce640b3b0fbd76723185047513534411e`:

```sh
cargo fmt --all --check
cargo test -p nimbus-runtime limits::tests --lib
cargo test -p nimbus-runtime backends::bun_jsc --lib
cargo test -p nimbus-server registry_and_license::registry --lib
cargo test -p nimbus-server tenant_isolation::tests::production_untrusted_runtime_admission_allows_bun_jsc_fresh_discard_policy --lib
cargo test -p nimbus-server registry_and_license::runtime_metrics --lib
npm run test --workspace @nimbus/codegen
git diff --check
bash scripts/verify-bun-jsc-in-process-lockdown.sh
```

Result:

```text
runtime limits: 11 passed
Bun/JSC backend scaffold: 4 passed
server registry: 11 passed
tenant admission focused case: 1 passed
server runtime metrics: 2 passed
codegen selftest: passed
reusable Bun/JSC gate: pass
```

The local reusable gate passed all 10 steps after rerunning outside the Codex
sandbox so runtime-metrics tests could bind local test sockets.

## Linux/Minicloud Evidence

Passed on Debian 13 `minicloud` as `nimbus@192.168.4.29` from isolated proof
worktrees created from local Git bundles:

```text
Nimbus proof worktree: /home/nimbus/src/github.com/nimbus/nimbus-bep9-proof-20260523191300
Nimbus proof commit:   3b6c27bce640b3b0fbd76723185047513534411e
Bun proof worktree:    /home/nimbus/src/github.com/oven-sh/bun-bep7-proof-20260523180348
Bun proof commit:      4b5de5ee5d173975485fd907abe7b6e1457a90c5
Proof root:            /home/nimbus/.cache/nimbus-proof/bep9-3b6c27bc
```

The Linux command was:

```sh
cd /home/nimbus/src/github.com/nimbus/nimbus-bep9-proof-20260523191300
. "$HOME/.cargo/env"
export NVM_DIR="$HOME/.nvm"
. "$NVM_DIR/nvm.sh"
nvm use --lts >/dev/null
export PROOF_ROOT="$HOME/.cache/nimbus-proof/bep9-3b6c27bc"
mkdir -p "$PROOF_ROOT/tmp" \
  "$PROOF_ROOT/bun-embed-native" \
  "$PROOF_ROOT/bun-cache" \
  "$PROOF_ROOT/bun-rust-only" \
  "$PROOF_ROOT/bun-cargo-target"
export TMPDIR="$PROOF_ROOT/tmp"
export PATH="$HOME/.local/toolchains/LLVM-21.1.8-Linux-X64/bin:$HOME/.bun/bin:$PATH"
NIMBUS_BUN_REPO=/home/nimbus/src/github.com/oven-sh/bun-bep7-proof-20260523180348 \
NIMBUS_BUN_BUILD_DIR="$PROOF_ROOT/bun-embed-native" \
NIMBUS_BUN_CACHE_DIR="$PROOF_ROOT/bun-cache" \
NIMBUS_BUN_RUST_ONLY_BUILD_DIR="$PROOF_ROOT/bun-rust-only" \
NIMBUS_BUN_CARGO_TARGET_DIR="$PROOF_ROOT/bun-cargo-target" \
bash scripts/verify-bun-jsc-in-process-lockdown.sh
```

The reusable gate passed all 10 Linux steps:

- Nimbus format
- Nimbus UI build prerequisites
- 11 runtime policy tests
- 4 Bun/JSC pool scaffold tests
- 11 registry/runtime metadata admission tests
- 2 runtime diagnostics tests
- 1 ignored Bun source proof test
- Nimbus whitespace diff check
- Bun `cargo fmt --all --check`
- Bun native `check-bun-embed-probe`
- Bun whitespace diff check

The native probe linked and produced the expected BEP7/BEP8 evidence:

```text
nimbus bun embed cancellation policy:
  before_guest_entry: owner_entry_gate_denied_and_recovered
  after_guest_entry_sync_loop: spin_entered_ack
  recovery_after_deadline_cancel: ok
  recovery_after_external_cancel: ok
  cancellation_timing_policy: state_ack_not_sleep

nimbus bun embed permission surface inventory:
  Bun.file: denied_by_default
  Bun.write: denied_by_default
  Bun.spawn: denied_by_default
  Bun.serve: denied_by_default
  Bun.connect: denied_by_default
  Bun.plugin: denied_by_default
  fetch: denied_by_default
  WebSocket: denied_by_default
  Worker: denied_by_default
  new Function: denied_by_default
  eval: denied_by_default
  dynamic import syntax: denied_by_default

nimbus bun embed memory behavior:
  hard_heap_limit: not_observed
  pressure_signal: vm.heap_size_and_sync_gc
  safe_first_policy: fresh_vm_or_discard_on_pressure

nimbus bun embed package/module policy:
  artifact_shape: self_contained_program_wrapper
  evaluation_format: program_via_Bun__REPL__evaluate
  dynamic_import_node_fs: denied_by_resolver_policy
  dynamic_import_package_root: denied_by_resolver_policy
  plugin_virtual_module_import: denied_by_resolver_policy
  selected_next_lane: program_wrapper
  resolver_policy_hook: native_embedder_deny_all
  required_resolver_api: nimbus_owned_bun_package_resolver

nimbus bun embed lifecycle reuse stress:
  fresh_vm_create_invoke_destroy_iterations: 4
  retained_vm_invocations_before_cancel: 8
  external_cancel_recovery_iterations: 3
  external_cancel_trigger: spin_entered_ack
  retained_vm_post_cancel_invocation: ok
  retained_vm_reuse: trusted_generated_wrapper_ok
  product_first_policy: fresh_vm_or_discard_with_outer_quota_required
```

## Residual Risks

- Nimbus does not yet link a Bun embedder execution adapter, so admitted Bun/JSC
  metadata still reaches a clear adapter-not-linked contract error.
- Bun/JSC has no observed hard per-VM heap limit in this proof. Untrusted
  Bun/JSC therefore remains fresh/discard with outer quota enforcement.
- Function-level `"use bun"` remains withheld from codegen until execution is
  real and the admitted lane can pass end-to-end invocation tests.
- The proof uses a local Bun proof checkout. A fork is not needed yet, but the
  fork threshold from Gate 27 still applies if upstream cannot carry the small
  embedder API surface.
- Future CI should run this gate only on a runner prepared for the Bun native
  build dependencies and WebKit/JSC cache footprint.

## Outcome

`BEP9` is complete and the plan is closed. Nimbus can now treat Bun/JSC as a
real optional backend candidate in product architecture while keeping runtime
execution fail-closed until a verified Bun embedder execution adapter is linked.
The next implementation wave should own that adapter, the Bun pool runtime
execution path, and end-to-end invocation tests for the admitted profile.
