# Bun/JSC EIB3 Viability And Fork Decision

Date: 2026-05-21

Nimbus plan:
`docs/plans/archive/execution-isolation-and-runtime-backends-plan.md`

Bun worktree: `/Users/jack/src/github.com/oven-sh/bun`

Bun proof commit: `ea677357e3` (`Add Bun embed timeout cancel proof`)

Nimbus proof baseline: `docs/plans/proof/runtime-engine/bun-jsc/gate-10-timeout-cancel.md`

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

## Remaining Production Blockers

Bun/JSC is not selectable because these required evidence rows remain open:

| Blocker | Required evidence before promotion |
| --- | --- |
| Permission containment | Gate 11 inventory is complete, but containment is not. Every present host-sensitive Bun, Node, JSC, and package-loading surface must become absent, denied, wrapped by Nimbus policy, or the backend must remain permanently trusted/sandbox-only. |
| Memory behavior | A per-VM heap limit, pressure signal, or discard-on-pressure policy is proven under generated Nimbus invocation load. |
| Package and module loading | Bun/JSC has explicit artifact metadata, evaluation format, package resolver, and external-package policy. It must not reuse `node_external_packages` as if it were Deno/V8. |
| VM lifecycle and reuse | Create/invoke/cancel/drop loops prove retained VM reuse is safe, or the backend starts fresh-per-invocation and records the cost. |
| Reproducible artifacts | Required Bun generated/native artifacts are reproduced by documented commands without untracked local build products. |
| Server/codegen routing | Nimbus registries reject unsupported Bun/content/target/package combinations before invocation. |

## Next Proof Gate

Next gate: **Bun/JSC Gate 12: memory behavior and safe first policy**.

The gate should run generated Nimbus invocation loops with memory pressure in
the same non-CLI generated-wrapper path. It must either prove a per-VM heap
limit or pressure signal usable by Nimbus, or record `fresh_per_invocation` /
discard-on-pressure as the only safe first policy for Bun/JSC.

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

Decision-only documentation update. No Bun or Nimbus code changed for EIB3.

Reviewed:

- Bun commit `ea677357e3`
- `src/embed_probe/lib.rs`
- `src/embed_probe/nimbus_generated_program_bundle.js`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-10-timeout-cancel.md`
- `docs/architecture/runtime/new-engine-proof-harness.md`
- `docs/architecture/runtime/engine-seam.md`
- `docs/architecture/runtime/permission-model.md`

Verification command:

```sh
git diff --check -- \
  docs/plans/proof/runtime-engine/bun-jsc/eib3-viability-and-fork-decision.md \
  docs/plans/archive/execution-isolation-and-runtime-backends-plan.md
```
