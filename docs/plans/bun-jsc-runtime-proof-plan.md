# Plan: Bun/JSC Runtime Proof

Focused execution plan for the next Bun/JSC proof wave. This plan starts from
the completed runtime engine seam and execution isolation baselines, keeps
Bun/JSC proof-only, and decides whether a Nimbus-maintained Bun fork is
justified only after the remaining production blockers are measured.

## Status

- **Status:** ready for execution; `BJ0` is complete and `BJ1` is next.
- **Primary owner:** this plan
- **Current trust tier:** `in_process_trusted_only`
- **Current product posture:** proof-only, not selectable, no production route
- **Bun worktree:** `/Users/jack/src/github.com/oven-sh/bun`
- **Current local Bun proof commit:** `c57f7e58c0`
  (`Add Bun embed timeout cancel proof`)
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

## Remaining Blockers

Bun/JSC cannot become a selectable Nimbus backend until these are resolved:

- permission containment for Bun, Node, Web, package-loading, worker,
  subprocess, FFI, native-addon, filesystem, network, and environment surfaces
- memory behavior under generated Nimbus invocation load
- package and module loading contract, including explicit Bun resolver policy
- VM lifecycle policy: retained reuse versus fresh-per-invocation discard
- reproducible artifact strategy for generated and native Bun build products
- explicit runtime artifact metadata and server/codegen rejection of unsupported
  Bun combinations
- final fork/upstream/hold decision with maintenance cost recorded

## Execution Gates

| Gate | Status | Goal | Verification |
| --- | --- | --- | --- |
| BJ0 | `done` | Reconcile current proof evidence, local Bun delta, and merge baseline. | `bun scripts/build.ts --profile=debug-no-asan --build-dir=/private/tmp/nimbus-bun-embed-native --cache-dir=/private/tmp/nimbus-bun-cache --target=check-bun-embed-probe` passed on 2026-05-23 after the local Bun pull against Bun `c57f7e58c0` on upstream base `f161e0311d`; Nimbus `main` contains merge `8c5f2697`. |
| BJ1 | `todo` | Gate 11: permission-surface containment inventory. | Extend the non-CLI `bun_embed_probe` to classify each host-sensitive surface as `absent_by_default`, `denied_by_default`, `policy_hook_available`, `policy_hook_missing`, or `unsafe_bypass`; record source snippet, expected result, actual result, and hook path. |
| BJ2 | `todo` | Gate 12: memory behavior and safe first policy. | Run generated Nimbus invocation loops with memory pressure; prove a per-VM heap limit or pressure signal, or record `fresh_per_invocation`/discard-on-pressure as the only safe policy. |
| BJ3 | `todo` | Gate 13: package/module loading and resolver policy. | Prove the selected Bun artifact shape, decide ESM versus program wrapper for the next lane, reject Node external packages by default, and identify any explicit Bun package resolver API needed. |
| BJ4 | `todo` | Gate 14: lifecycle, reuse, teardown, and stress. | Run create/invoke/cancel/drop loops and retained-VM reuse loops; decide whether the product path can reuse VMs or must start fresh per invocation. |
| BJ5 | `todo` | Gate 15: Nimbus artifact metadata and server rejection. | Add or verify explicit engine/content/evaluation-format metadata and tests that registries reject unsupported Bun combinations before invocation. No production Bun route may be added. |
| BJ6 | `todo` | Gate 16: fork, upstream, or hold decision. | Compare required hooks against the current Bun delta and upstream shape; record no-fork, upstream-proposal, or Nimbus-fork decision with maintenance cost and CI requirements. |
| BJ7 | `todo` | Closeout and next implementation handoff. | Update proof docs, this plan, `docs/plans/README.md`, and runtime architecture references; all verification commands pass; Bun remains proof-only unless every promotion gate is satisfied. |

## BJ1 Permission Inventory

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
