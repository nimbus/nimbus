# Bun/JSC Gate 33: Optional Backend Admission

Date: 2026-05-23

Nimbus plan: `docs/plans/bun-jsc-embedder-api-and-pool-plan.md`

## Status

Status: local proof passed; `BEP8` is complete.

Bun/JSC is now an explicit optional runtime backend lane in Nimbus metadata and
policy admission. The admitted shape is intentionally narrow:

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

This is admission, not a linked Bun execution engine. Current builds still
return a contract error if invocation reaches `backends::bun_jsc` without a Bun
embedder execution adapter:

```text
Bun/JSC runtime backend is admitted only for the proven fresh/discard lockdown
profile, but this Nimbus build does not link a Bun embedder execution adapter yet
```

## What Changed

- `RuntimeCompatibilityTarget::BunJsc` and
  `RuntimeExecutionModel::BackendOwnedEventLoop` make Bun/JSC distinct from
  Node/V8 instead of overloading a Node target.
- `RuntimeLimits::application_bun_jsc()` names the only product-admissible
  profile: untrusted in-process, fresh/discard, outer quota required, no
  host-sensitive grants. `RuntimeMemoryEnforcement` now makes that explicit:
  V8 lanes report `v8_isolate_heap_limit`, while Bun/JSC reports
  `outer_quota_required` until a hard per-VM heap boundary exists.
- Runtime policy validation accepts that exact profile and still rejects V8
  with Bun targets, Bun/JSC with V8/Node targets, proof-only profiles,
  retained trusted profiles, memory-enforcement mismatches, and profile
  mismatches.
- The Convex registry owns a separate Bun/JSC runtime lane beside default V8
  and Node20/22/24 V8 lanes. Runtime policies are constructed eagerly for
  diagnostics, but executors are lazy; the Bun/JSC lane remains
  `not_linked`, so selecting it for execution fails closed instead of
  starting V8 worker threads with Bun-shaped policy.
- Convex manifest validation accepts only `runtime_environment: "bun"` with
  `runtime_engine: "bun_jsc"`, target `bun_jsc`, program-wrapper evaluation,
  and `bun_self_contained` package resolution.
- Codegen now publishes the `bunJsc` runtime lane metadata at the top level so
  generated artifacts have an explicit, non-Node Bun lane available. A
  per-function `"use bun"` directive remains withheld until a linked Bun
  embedder adapter can execute that lane instead of returning the
  adapter-not-linked contract error.
- Architecture docs now describe the exact BEP8 state: admitted profile,
  fail-closed execution adapter boundary, and retained/proof profiles still
  blocked.
- `/debug/runtime/metrics` now exposes per-lane diagnostics for default V8,
  Node20/22/24, and Bun/JSC, including executor-started state, adapter link
  state, metrics, reset capabilities, memory-enforcement semantics, and the
  default-lane compatibility fields.

## Rejected Shapes

The BEP8 tests keep these combinations fail-closed before invocation:

- V8 with `RuntimeCompatibilityTarget::BunJsc`
- Bun/JSC with a Node compatibility target
- Bun/JSC with `node_external_packages`
- Bun/JSC proof-only or retained trusted pool profiles as product routes
- Bun/JSC with V8/Deno execution model or pool metadata
- Bun/JSC with V8 isolate heap-limit memory semantics
- V8 with Bun/JSC program-wrapper metadata
- V8 with Bun/JSC outer-quota-only memory semantics

## Local Verification

Passed in `/Users/jack/src/github.com/nimbus/nimbus`:

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

The reusable gate passed all 10 local steps after running outside the Codex
sandbox so runtime-metrics tests could bind local test sockets:

- Nimbus format
- UI build prerequisites
- 11 runtime policy tests
- 4 Bun/JSC pool scaffold tests
- 11 registry/runtime metadata admission tests
- 2 runtime diagnostics tests
- 1 ignored Bun source proof test
- Nimbus whitespace check
- Bun `cargo fmt --all --check`
- Bun native `check-bun-embed-probe`
- Bun whitespace check

## Outcome

`BEP8` is complete. Bun/JSC is now an optional backend lane beside Deno/V8 at
the policy, registry, and generated-metadata seams, but only for the proven
fresh/discard lockdown profile. The next gate is `BEP9`: close the plan with
repeatable local and Linux/minicloud evidence, residual risks, and the product
go/no-go decision for linking a real Bun embedder adapter.
