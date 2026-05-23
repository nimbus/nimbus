# Bun/JSC EIB3 Viability And Fork Decision

Date: 2026-05-21

Nimbus plan:
`docs/plans/archive/execution-isolation-and-runtime-backends-plan.md`

Bun worktree: `/Users/jack/src/github.com/oven-sh/bun`

Bun proof commit: `65cdc97796` (`Add Bun embed lifecycle reuse proof`)

Nimbus proof baseline:
`docs/plans/proof/runtime-engine/bun-jsc/gate-15-artifact-metadata-server-rejection.md`

## Decision

Bun/JSC remains proof-only and maps to the
`in_process_trusted_only` trust tier.

Do not create a Nimbus-maintained Bun fork yet.

The current local Bun patch should be held as proof evidence. It is not ready
to upstream as a production API proposal because the next required hooks are
not known precisely enough. The embeddable link target and process-neutral link
bridge are plausible upstreamable shapes, but Nimbus should not upstream or
fork until the permission and memory gates identify the exact APIs a real
backend needs.

## Evidence Reviewed

The local Bun proof chain has now shown:

- non-CLI native target below `bun_bin`
- VM construction and teardown through `VirtualMachine::init` and `destroy`
- sync JSC host function transport
- async host function transport with promise settlement and event-loop driving
- self-contained Nimbus program-wrapper evaluation
- generated `globalThis.__nimbusInvoke` dispatch
- generated handler entry
- host-owned timeout and external cancellation through JSC termination
- same-VM recovery after timeout and cancellation

Gate 10 also found that:

- JSC termination must be primed on the VM owner thread before cross-thread
  `notifyNeedTermination()`
- Bun/JSC `setExecutionTimeLimit()` interrupted a loop but was not recoverable
  in the debug embed path because recovery hit a JSC watchdog assertion
- the working cancellation model is a host-owned deadline/cancel handle that
  calls JSC termination, not Bun's watchdog time-limit path

Gate 11 later found that the current non-CLI generated-wrapper VM exposes Bun
filesystem/process/network/plugin/FFI/env surfaces, process globals,
Web/network APIs, timers, workers, `eval`, and `new Function` as
`unsafe_bypass`; `require` and require-based Node/native-addon lanes are
`absent_by_default`; dynamic `import(...)` syntax is `policy_hook_missing`;
only the Nimbus proof host-call/generated-wrapper path is
`policy_hook_available`.

Gate 12 later found that the same non-CLI generated-wrapper VM exposes a
usable memory pressure signal through `VM::heap_size()` and sync GC, but no
hard per-VM heap limit. Under 16 generated Nimbus invocations with retained
allocation load, heap samples grew from `221431` bytes after setup GC to
`5499474` bytes after retained sync GC, then dropped to `285224` bytes after
release GC and `195847` bytes after `shrink_footprint()` under the JSC API
lock.

Gate 13 later found that the self-contained generated program wrapper remains
the right next artifact lane. Static ESM import is rejected by the program
evaluator and `require` remains absent by default. The generated wrapper denies
missing Node builtin and external package map entries by default. However,
dynamic `import("node:fs")` fulfilled, and `Bun.resolve` plus
`Bun.resolveSync` are exposed as unmediated resolver authority.

Gate 14 later found that the trusted generated-wrapper lane can reuse a
retained Bun/JSC VM across eight generated invocations, recover the same VM
after three host-owned external cancellation cycles, and invoke successfully
again after cancellation. This is positive lifecycle evidence, but it does not
change product posture because permission containment and resolver containment
remain unresolved.

Gate 15 later added explicit Nimbus metadata for JavaScript evaluation format.
The current V8 lanes emit and accept `es_module`; Bun/JSC proof artifacts map
to `program_wrapper`, but `bun_jsc` is recognized only so server and runtime
policy construction can reject it clearly before invocation.

## Remaining Production Blockers

Bun/JSC is not selectable because these required evidence rows remain open:

| Blocker | Required evidence before promotion |
| --- | --- |
| Permission containment | Gate 11 inventory is complete, but containment is not. Every present host-sensitive Bun, Node, JSC, and package-loading surface must become absent, denied, wrapped by Nimbus policy, or the backend must remain permanently trusted/sandbox-only. |
| Memory behavior | Gate 12 proved a pressure signal, but no hard per-VM heap limit. A product backend must use fresh VM or discard-on-pressure plus an outer process/sandbox hard quota rather than retained in-process tenant memory isolation. |
| Package and module loading | Gate 13 selected program-wrapper as the safe next artifact lane and proved generated maps deny missing Node package entries by default, but dynamic `node:fs` import and `Bun.resolve*` remain unmediated. A product backend needs a Nimbus-owned Bun resolver policy and must not reuse `node_external_packages` as if it were Deno/V8. |
| VM lifecycle and reuse | Gate 14 proves retained reuse is viable for trusted generated-wrapper proof code, including post-cancel recovery. Product reuse is still blocked until permission and resolver containment are enforced; first product policy remains fresh/discard. |
| Reproducible artifacts | Required Bun generated/native artifacts are reproduced by documented commands without untracked local build products. |
| Server/codegen routing | Gate 15 proves unsupported Bun/content/evaluation-format/target combinations are rejected before invocation. A production Bun route remains absent by design and would require positive containment evidence before selection. |

## Next Proof Gate

Next gate: **Bun/JSC Gate 16: fork, upstream, or hold decision**.

The gate should compare the current local Bun proof delta with the remaining
promotion blockers and record whether Nimbus should keep holding the proof
patch, propose upstream APIs, or prepare a Nimbus-maintained fork. It must
include maintenance cost and CI requirements.

## Fork Posture

A Nimbus-maintained Bun fork becomes justified only if all of these are true:

- Nimbus decides to ship Bun/JSC as a product backend rather than proof
  evidence.
- Upstream Bun cannot or will not accept the embeddable target, link bridge,
  cancellation, memory, package-resolution, or permission hooks required by
  Nimbus.
- The remaining delta is small enough to carry across Bun, WebKit/JSC, native
  build, and security-update churn.
- The fork has repeatable CI for the embed target and the Nimbus proof gates.

Expected fork maintenance cost:

- recurring merge work across Bun's native build graph and generated Rust/C++
  artifacts
- JSC/WebKit API drift for termination, memory, and VM lifecycle hooks
- package-resolution and Node-compat behavior drift
- security-update tracking for Bun, JSC/WebKit, and native dependencies
- cross-platform native-link validation, especially macOS and Linux

Required hooks before an upstream or fork proposal is concrete:

- stable embeddable build target below `bun_bin`
- process-neutral link bridge for required C ABI symbols
- stable JSC host-function registration and async promise driving APIs
- recoverable host-owned termination/cancel API or documented owner-thread
  priming sequence
- memory limit or pressure/discard callback suitable for per-invocation policy
- per-VM policy hooks for filesystem, network, environment, subprocess, worker,
  FFI/native addons, Bun APIs, and package loading
- explicit package resolver and evaluation-format APIs for Nimbus artifacts
- teardown and reuse contract that does not call process exit or install
  process-global handlers from the backend path

## Verification

Decision documentation updated after the Gate 11, Gate 12, Gate 13, Gate 14,
and Gate 15 proof work.

Reviewed:

- Bun commit `65cdc97796`
- `src/embed_probe/lib.rs`
- `src/embed_probe/nimbus_generated_program_bundle.js`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-10-timeout-cancel.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-11-permission-surface-inventory.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-12-memory-behavior.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-13-package-module-policy.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-14-lifecycle-reuse-stress.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-15-artifact-metadata-server-rejection.md`
- `docs/architecture/runtime/new-engine-proof-harness.md`
- `docs/architecture/runtime/engine-seam.md`
- `docs/architecture/runtime/permission-model.md`

Verification command:

```sh
git diff --check -- \
  docs/plans/proof/runtime-engine/bun-jsc/eib3-viability-and-fork-decision.md \
  docs/plans/bun-jsc-runtime-proof-plan.md \
  docs/plans/proof/runtime-engine/bun-jsc/gate-11-permission-surface-inventory.md \
  docs/plans/proof/runtime-engine/bun-jsc/gate-12-memory-behavior.md \
  docs/plans/proof/runtime-engine/bun-jsc/gate-13-package-module-policy.md \
  docs/plans/proof/runtime-engine/bun-jsc/gate-14-lifecycle-reuse-stress.md \
  docs/plans/proof/runtime-engine/bun-jsc/gate-15-artifact-metadata-server-rejection.md
```
