# Plan: Bun/JSC Runtime Proof

Focused execution plan for the next Bun/JSC proof wave. This plan starts from
the completed runtime engine seam and execution isolation baselines, keeps
Bun/JSC proof-only, and decides whether a Nimbus-maintained Bun fork is
justified only after the remaining production blockers are measured.

## Status

- **Status:** in execution; `BJ0` through `BJ4` are complete and `BJ5` is next.
- **Primary owner:** this plan
- **Current trust tier:** `in_process_trusted_only`
- **Current product posture:** proof-only, not selectable, no production route
- **Bun worktree:** `/Users/jack/src/github.com/oven-sh/bun`
- **Current local Bun proof commit:** `65cdc97796`
  (`Add Bun embed lifecycle reuse proof`)
- **Current upstream base in local Bun worktree:** `f161e0311d`
  (`shell: wrap only component-leading ! when neutralizing glob metachars (#31272)`)

## Decision

Do not fork Bun yet.

The current local Bun delta is valuable proof evidence, not a product
dependency. A fork becomes justified only if Nimbus decides to ship Bun/JSC as
a runtime backend and the remaining gates show that the required embeddable
target, policy hooks, memory controls, package loading contract, and lifecycle
APIs cannot land upstream or be consumed without a long-lived patch.

## Current Evidence

The local proof chain has shown:

- Bun's Rust/JSC build graph can produce the required generated Rust artifacts.
- A non-CLI native target can final-link below `bun_bin`.
- The proof can construct and destroy a Bun/JSC VM in process.
- Guest JavaScript can call sync and async Rust host functions through JSC.
- The proof can drive Bun/JSC promise and event-loop progress.
- A real Nimbus-generated program wrapper can load and invoke through
  `globalThis.__nimbusInvoke`.
- Host-owned timeout and external cancellation can interrupt generated guest
  execution, clear termination, and recover the same VM for follow-up
  evaluation.
- The same non-CLI generated-wrapper proof now records the BJ1 permission
  inventory: Bun filesystem/process/network/plugin/FFI/env surfaces, process
  globals, Web/network APIs, timers, workers, `eval`, and `new Function` are
  `unsafe_bypass`; `require` and require-based Node module/native-addon lanes
  are `absent_by_default`; dynamic `import(...)` syntax is
  `policy_hook_missing`; the Nimbus proof host hooks and generated wrapper are
  `policy_hook_available`.
- The generated-wrapper proof now records BJ2 memory behavior: 16 generated
  Nimbus invocations produced a measurable JSC heap pressure signal through
  `VM::heap_size()` and sync GC, retained memory dropped after releasing the
  retained graph and shrinking the footprint, and no hard per-VM heap limit was
  observed in the embed path.
- The generated-wrapper proof now records BJ3 package/module behavior:
  self-contained program wrapper remains the selected next lane, static ESM
  import is rejected by the program evaluator, `require` is absent by default,
  generated Node builtin and external-package maps deny missing entries by
  default, but dynamic `import("node:fs")` fulfills and `Bun.resolve` /
  `Bun.resolveSync` are exposed as unmediated resolver authority.
- The generated-wrapper proof now records BJ4 lifecycle behavior: the proof
  creates/invokes/destroys four fresh VMs, runs eight retained generated
  invocations, interrupts and recovers the retained VM through three external
  cancellation cycles, then invokes successfully once more after cancellation.
  This proves trusted generated-wrapper reuse is viable, while product posture
  remains fresh VM or discard until containment is solved.

Fresh verification after the local Bun pull on 2026-05-23:

```sh
cd /Users/jack/src/github.com/oven-sh/bun
bun scripts/build.ts --profile=debug-no-asan \
  --build-dir=/private/tmp/nimbus-bun-embed-native \
  --cache-dir=/private/tmp/nimbus-bun-cache \
  --target=check-bun-embed-probe
```

Result: passed. The build configured `bun-debug` at revision `c57f7e58c0`,
compiled `bun_embed_probe`, linked `bun-embed-probe`, ran the smoke target, and
emitted `[build] check-bun-embed-probe done`.

BJ1 verification on 2026-05-23:

```sh
cd /Users/jack/src/github.com/oven-sh/bun
cargo fmt --all --check
bun scripts/build.ts --profile=debug-no-asan \
  --build-dir=/private/tmp/nimbus-bun-embed-native \
  --cache-dir=/private/tmp/nimbus-bun-cache \
  --target=check-bun-embed-probe
git diff --check
```

Result: passed against Bun proof commit `9e20ac28a2`. The native proof printed
the Gate 11 inventory recorded in
`docs/plans/proof/runtime-engine/bun-jsc/gate-11-permission-surface-inventory.md`.

BJ2 verification on 2026-05-23:

```sh
cd /Users/jack/src/github.com/oven-sh/bun
cargo fmt --all --check
bun scripts/build.ts --profile=debug-no-asan \
  --build-dir=/private/tmp/nimbus-bun-embed-native \
  --cache-dir=/private/tmp/nimbus-bun-cache \
  --target=check-bun-embed-probe
git diff --check
```

Result: passed against Bun proof commit `f6c87be47e`. The native proof printed
the Gate 12 memory sample recorded in
`docs/plans/proof/runtime-engine/bun-jsc/gate-12-memory-behavior.md`.

BJ3 verification on 2026-05-23:

```sh
cd /Users/jack/src/github.com/oven-sh/bun
cargo fmt --all --check
bun scripts/build.ts --profile=debug-no-asan \
  --build-dir=/private/tmp/nimbus-bun-embed-native \
  --cache-dir=/private/tmp/nimbus-bun-cache \
  --target=check-bun-embed-probe
git diff --check
```

Result: passed against Bun proof commit `f0cee692c0`. The native proof printed
the Gate 13 package/module policy sample recorded in
`docs/plans/proof/runtime-engine/bun-jsc/gate-13-package-module-policy.md`.

BJ4 verification on 2026-05-23:

```sh
cd /Users/jack/src/github.com/oven-sh/bun
cargo fmt --all --check
bun scripts/build.ts --profile=debug-no-asan \
  --build-dir=/private/tmp/nimbus-bun-embed-native \
  --cache-dir=/private/tmp/nimbus-bun-cache \
  --target=check-bun-embed-probe
git diff --check
```

Result: passed against Bun proof commit `65cdc97796`. The native proof printed
the Gate 14 lifecycle/reuse stress sample recorded in
`docs/plans/proof/runtime-engine/bun-jsc/gate-14-lifecycle-reuse-stress.md`.

## Remaining Blockers

Bun/JSC cannot become a selectable Nimbus backend until these are resolved:

- permission containment for Bun, Node, Web, package-loading, worker,
  subprocess, FFI, native-addon, filesystem, network, and environment surfaces
- memory enforcement if Bun/JSC is ever promoted: Gate 12 found a usable
  pressure signal but no hard per-VM heap limit, so the safe first product
  policy is fresh VM or discard-on-pressure plus an outer sandbox/process hard
  limit
- package and module loading enforcement if Bun/JSC is ever promoted: Gate 13
  selected program-wrapper as the safe next artifact lane and proved generated
  Node package maps deny missing entries by default, but dynamic `node:fs`
  import and `Bun.resolve*` remain unmediated without a Nimbus-owned Bun
  resolver policy
- VM lifecycle policy if Bun/JSC is ever promoted: Gate 14 proves trusted
  generated-wrapper retained reuse can survive cancellation recovery, but the
  safe first product policy remains fresh VM or discard until permission and
  resolver containment are enforced
- reproducible artifact strategy for generated and native Bun build products
- explicit runtime artifact metadata and server/codegen rejection of unsupported
  Bun combinations
- final fork/upstream/hold decision with maintenance cost recorded

## Execution Gates

| Gate | Status | Goal | Verification |
| --- | --- | --- | --- |
| BJ0 | `done` | Reconcile current proof evidence, local Bun delta, and merge baseline. | `bun scripts/build.ts --profile=debug-no-asan --build-dir=/private/tmp/nimbus-bun-embed-native --cache-dir=/private/tmp/nimbus-bun-cache --target=check-bun-embed-probe` passed on 2026-05-23 after the local Bun pull against Bun `c57f7e58c0` on upstream base `f161e0311d`; Nimbus `main` contains merge `8c5f2697`. |
| BJ1 | `done` | Gate 11: permission-surface containment inventory. | Bun proof commit `9e20ac28a2` extends the non-CLI `bun_embed_probe` with `nimbus_bun_embed_probe_permission_surface_inventory()` and records each Bun, Node, Web/network, package-loading, worker/concurrency, subprocess, FFI/native-addon, filesystem, network, and environment surface as `absent_by_default`, `policy_hook_available`, `policy_hook_missing`, or `unsafe_bypass`; `cargo fmt --all --check`, `bun scripts/build.ts --profile=debug-no-asan --build-dir=/private/tmp/nimbus-bun-embed-native --cache-dir=/private/tmp/nimbus-bun-cache --target=check-bun-embed-probe`, and `git diff --check` passed on 2026-05-23. |
| BJ2 | `done` | Gate 12: memory behavior and safe first policy. | Bun proof commit `f6c87be47e` extends the non-CLI `bun_embed_probe` with `nimbus_bun_embed_probe_memory_behavior()`. It runs 16 generated Nimbus invocations under retained allocation load, records `VM::heap_size()` / sync-GC pressure samples, observes no hard per-VM heap limit, and sets the safe first policy to fresh VM or discard-on-pressure; `cargo fmt --all --check`, `bun scripts/build.ts --profile=debug-no-asan --build-dir=/private/tmp/nimbus-bun-embed-native --cache-dir=/private/tmp/nimbus-bun-cache --target=check-bun-embed-probe`, and `git diff --check` passed on 2026-05-23. |
| BJ3 | `done` | Gate 13: package/module loading and resolver policy. | Bun proof commit `f0cee692c0` extends the non-CLI `bun_embed_probe` with `nimbus_bun_embed_probe_package_module_policy()`. It records `self_contained_program_wrapper` via `Bun__REPL__evaluate` as the selected next lane, proves static ESM import is rejected in that evaluator, proves `require` is absent by default, proves generated Node builtin and external-package empty maps deny missing entries by default, and records dynamic `import("node:fs")` plus `Bun.resolve` / `Bun.resolveSync` as unsafe resolver bypasses requiring a Nimbus-owned Bun package resolver; `cargo fmt --all --check`, `bun scripts/build.ts --profile=debug-no-asan --build-dir=/private/tmp/nimbus-bun-embed-native --cache-dir=/private/tmp/nimbus-bun-cache --target=check-bun-embed-probe`, and `git diff --check` passed on 2026-05-23. |
| BJ4 | `done` | Gate 14: lifecycle, reuse, teardown, and stress. | Bun proof commit `65cdc97796` extends the non-CLI `bun_embed_probe` with `nimbus_bun_embed_probe_lifecycle_reuse_stress()`. It creates/invokes/destroys four fresh VMs, runs eight retained generated `messages:sendAndSchedule` invocations, interrupts generated `messages:spinForever` through three external cancellation/recovery cycles, and successfully invokes once more after cancellation; `cargo fmt --all --check`, `bun scripts/build.ts --profile=debug-no-asan --build-dir=/private/tmp/nimbus-bun-embed-native --cache-dir=/private/tmp/nimbus-bun-cache --target=check-bun-embed-probe`, and `git diff --check` passed on 2026-05-23. |
| BJ5 | `todo` | Gate 15: Nimbus artifact metadata and server rejection. | Add or verify explicit engine/content/evaluation-format metadata and tests that registries reject unsupported Bun combinations before invocation. No production Bun route may be added. |
| BJ6 | `todo` | Gate 16: fork, upstream, or hold decision. | Compare required hooks against the current Bun delta and upstream shape; record no-fork, upstream-proposal, or Nimbus-fork decision with maintenance cost and CI requirements. |
| BJ7 | `todo` | Closeout and next implementation handoff. | Update proof docs, this plan, `docs/plans/README.md`, and runtime architecture references; all verification commands pass; Bun remains proof-only unless every promotion gate is satisfied. |

## BJ1 Permission Inventory

Status: complete. Full evidence lives in
`docs/plans/proof/runtime-engine/bun-jsc/gate-11-permission-surface-inventory.md`.

BJ1 should probe these surfaces in the same non-CLI generated-program wrapper
shape used by Gate 10:

- Bun globals: `Bun`, `Bun.file`, `Bun.write`, `Bun.spawn`, `Bun.serve`,
  `Bun.listen`, `Bun.connect`, `Bun.plugin`, `Bun.dlopen`
- Node globals and modules: `process`, `process.env`, `require`, `node:fs`,
  `fs`, `node:child_process`, `node:worker_threads`, `node:net`, `node:dgram`,
  `node:ffi`, native-addon lanes
- Web/network surfaces: `fetch`, `WebSocket`, socket constructors, timers that
  can escape invocation lifecycle
- Dynamic code and package paths: dynamic `import(...)`, CommonJS require,
  package resolver entrypoints, external package descriptors
- Worker/concurrency surfaces: `Worker`, worker-thread APIs, Bun worker helpers

Success criteria:

- present host-sensitive surfaces either have a concrete Nimbus policy hook or
  keep Bun/JSC in `in_process_trusted_only`
- missing hooks are named with exact source ownership in Bun
- no production Nimbus selector, server route, or codegen target is added

BJ1 result: Bun/JSC stays `in_process_trusted_only`. Present unhooked
host-sensitive surfaces dominate the current VM. The only policy-hooked row is
the Nimbus proof host-call/generated-wrapper path.

## BJ2 Memory Behavior

Status: complete. Full evidence lives in
`docs/plans/proof/runtime-engine/bun-jsc/gate-12-memory-behavior.md`.

BJ2 result: Bun/JSC still stays `in_process_trusted_only` and proof-only. The
embed path exposes a pressure signal through `VM::heap_size()` and
`VirtualMachine::garbage_collect(true)`, but the proof did not find a hard
per-VM heap limit. If this backend is ever promoted, Nimbus must treat
Bun/JSC memory containment as an outer process/sandbox quota plus
fresh-per-invocation or discard-on-pressure VM lifecycle policy, not a
retained in-process tenant-isolation guarantee.

## BJ3 Package And Module Policy

Status: complete. Full evidence lives in
`docs/plans/proof/runtime-engine/bun-jsc/gate-13-package-module-policy.md`.

BJ3 result: Bun/JSC still stays `in_process_trusted_only` and proof-only. The
next proof lane should keep using the self-contained generated program wrapper,
not a Bun ESM artifact, until a safe Bun module-loader API exists. Generated
Node builtin and external-package helper maps deny missing entries by default,
but dynamic `import("node:fs")` and `Bun.resolve*` are available without a
Nimbus resolver hook, so a product backend needs a first-class
`nimbus_owned_bun_package_resolver` before tenant code can load packages.

## BJ4 Lifecycle Reuse Stress

Status: complete. Full evidence lives in
`docs/plans/proof/runtime-engine/bun-jsc/gate-14-lifecycle-reuse-stress.md`.

BJ4 result: Bun/JSC still stays `in_process_trusted_only` and proof-only. The
trusted generated-wrapper lane can reuse a retained VM across generated
invocations and recover the same VM after host-owned cancellation. That is a
positive lifecycle signal, but it does not override Gate 11 permission exposure
or Gate 13 resolver exposure. The safe first product policy remains fresh VM
or discard on pressure, timeout, cancellation, or package-loader use until
containment is enforced.

## Fork Criteria

Keep holding the local Bun patch unless all of these become true:

- Nimbus chooses to ship Bun/JSC as a product backend.
- The remaining gates identify the exact embeddable APIs and policy hooks
  required.
- Upstream Bun cannot or will not accept those hooks in a usable shape.
- The fork delta is small enough to maintain across Bun, WebKit/JSC, native
  build, and security-update churn.
- A repeatable CI lane exists for the embed target and Nimbus proof gates on
  the supported platforms.

If any of those are false, the right answer is no fork yet.

## Required Verification

Before closing this plan:

```sh
cargo fmt --all --check
cargo clippy -p nimbus-runtime -p nimbus-server -p nimbus-bin -- -D warnings
cargo test -p nimbus-runtime --test engine_proofs bun_jsc_build_gate_reproduces_from_bun_build_graph --ignored -- --nocapture
bun scripts/build.ts --profile=debug-no-asan --build-dir=/private/tmp/nimbus-bun-embed-native --cache-dir=/private/tmp/nimbus-bun-cache --target=check-bun-embed-probe
git diff --check
```

The Bun build command runs in `/Users/jack/src/github.com/oven-sh/bun`. Nimbus
commands run in `/Users/jack/src/github.com/nimbus/nimbus`.

## References

- `docs/architecture/runtime/engine-seam.md`
- `docs/architecture/runtime/new-engine-proof-harness.md`
- `docs/architecture/runtime/permission-model.md`
- `docs/plans/archive/runtime-engine-seam-plan.md`
- `docs/plans/archive/execution-isolation-and-runtime-backends-plan.md`
- `docs/plans/proof/runtime-engine/bun-jsc/eib3-viability-and-fork-decision.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-10-timeout-cancel.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-11-permission-surface-inventory.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-12-memory-behavior.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-13-package-module-policy.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-14-lifecycle-reuse-stress.md`
