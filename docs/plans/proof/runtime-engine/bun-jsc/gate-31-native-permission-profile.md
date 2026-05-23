# Bun/JSC Gate 31: Native Permission Profile Proof

Date: 2026-05-23

Nimbus plan: `docs/plans/bun-jsc-embedder-api-and-pool-plan.md`

## Decision

Status: native permission denial and hookability proven for the proof profile.

Bun proof commit `0c132cff81964e9cf85014e6955eee4ef013d942`
(`Add Bun embedder native permission deny profile proof`) adds a native
construction-profile helper for the embed proof target. The helper applies a
deny profile after trusted generated-wrapper setup and before tenant-visible
probe code. The production version must replace the test-only deny profile
with typed Nimbus policy decisions, audit events, and tenant/workload identity.

## Bun Hook Surface

The proof patch touches these Bun paths:

- `src/jsc/bindings/ZigGlobalObject.cpp`: adds a native deny-profile helper
  that mutates the current Bun/JSC global object, marks profile-owned objects,
  replaces dangerous globals with denied host functions, and disables
  tenant-visible string code generation through JSC `setEvalEnabled(false, ...)`
- `src/embed_probe/lib.rs`: turns the permission inventory into a hard gate;
  any `policy_hook_missing` or `unsafe_bypass` classification fails the proof
  target before it can be treated as a passing containment result

The proof deliberately loads the trusted generated wrapper before applying the
deny profile because the current wrapper path uses host-authored dynamic
compilation. Tenant-visible dynamic code is then denied by native JSC policy.

## Proof Results

The native embed probe now reports:

```text
nimbus bun embed permission surface inventory:
  Bun global: policy_hook_available
  Bun.file: denied_by_default
  Bun.write: denied_by_default
  Bun.spawn: denied_by_default
  Bun.spawnSync: denied_by_default
  Bun.serve: denied_by_default
  Bun.listen: denied_by_default
  Bun.connect: denied_by_default
  Bun.plugin: denied_by_default
  Bun.FFI: policy_hook_available
  Bun.dlopen: absent_by_default
  Bun.FFI.dlopen: denied_by_default
  Bun.env: absent_by_default
  process: policy_hook_available
  process.env: absent_by_default
  require: absent_by_default
  Node builtin modules via require: absent_by_default
  node:fs via require: absent_by_default
  fs via require: absent_by_default
  node:child_process via require: absent_by_default
  node:worker_threads via require: absent_by_default
  node:net via require: absent_by_default
  node:dgram via require: absent_by_default
  node:ffi via require: absent_by_default
  native addon via require: absent_by_default
  fetch: denied_by_default
  WebSocket: denied_by_default
  setTimeout: denied_by_default
  Worker: denied_by_default
  new Function: denied_by_default
  Function constructor escape: denied_by_default
  eval: denied_by_default
  dynamic import syntax: denied_by_default
  Nimbus host hooks and generated wrapper: policy_hook_available
```

This covers filesystem entry points, network/server entry points,
env/process exposure, subprocess creation, FFI/native loading, plugins,
workers, timers, fetch/WebSocket, CommonJS/native-addon require surfaces, and
tenant-visible dynamic code generation.

## Product Implications

This proof supports the Bun/JSC product direction only if the production
embedder API makes the profile first-class:

- apply the profile during VM/global construction, before tenant code
- route denied capabilities through typed Nimbus policy hooks instead of
  test-only deny functions
- attach tenant identity, workload identity, decision ID, and audit context to
  every permission decision
- keep trusted generated-wrapper compilation separate from tenant-visible
  dynamic code
- propagate the same profile to future worker or retained-pool contexts

The proof does not make Bun/JSC product-selectable yet. BEP7 still has to close
the memory, cancellation, teardown, and reuse policy before BEP8 can wire
runtime admission to the optional Bun backend.

## Verification

Passed in `/Users/jack/src/github.com/oven-sh/bun`:

```sh
cargo fmt --all --check
bun scripts/build.ts --profile=debug-no-asan \
  --build-dir=/private/tmp/nimbus-bun-embed-native \
  --cache-dir=/private/tmp/nimbus-bun-cache \
  --target=check-bun-embed-probe
git diff --check
```

Result:

```text
Bun native check-bun-embed-probe: pass
Permission inventory: no policy_hook_missing or unsafe_bypass classifications
```

Passed in `/Users/jack/src/github.com/nimbus/nimbus`:

```sh
cargo fmt --all --check
git diff --check
bash scripts/verify-bun-jsc-in-process-lockdown.sh
```

The reusable gate passed all 10 steps, including Nimbus runtime/backend policy
tests, Bun/JSC pool scaffold tests, registry rejection tests, runtime
diagnostics tests, the ignored Bun source proof lane, Bun native
`check-bun-embed-probe`, and whitespace checks.

## Outcome

`BEP6` is complete. The next gate is `BEP7`: prove memory, cancellation,
teardown, and reuse policy across macOS and Linux.
