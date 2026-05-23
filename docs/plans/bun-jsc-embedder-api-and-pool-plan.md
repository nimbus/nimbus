# Plan: Bun/JSC Embedder API And Pool

This plan owns the next product-moving Bun/JSC wave after
`docs/plans/bun-jsc-in-process-lockdown-plan.md`.

The decision from the proof work is: continue pursuing Bun/JSC as an optional
in-process runtime backend beside the existing Deno/V8/Node-compatible lane,
with its own Bun pool. It is not a replacement for Deno/V8, not a Bun binary
inside a microVM, and not selectable for tenant code until the gates below
prove deny-by-default containment.

## Status

- **Status:** active; `BEP0` through `BEP2` are complete, `BEP3` is next
- **Primary owner:** this plan
- **Nimbus worktree:** `/Users/jack/src/github.com/nimbus/nimbus`
- **Bun worktree:** `/Users/jack/src/github.com/oven-sh/bun`
- **Current Bun proof head:** `ce5aa2a389`
  (`Stabilize Bun embed cancellation proof on Linux`)
- **Product posture:** optional backend candidate; not selectable
- **Fork posture:** upstream-first; no Nimbus Bun fork yet
- **Completed predecessor:** `docs/plans/bun-jsc-in-process-lockdown-plan.md`

## Product Shape

```text
Runtime admission
  -> RuntimeBackendKind::{DenoV8, BunJsc}
      -> Deno/V8 pool for current Node-compatible runtime
      -> Bun/JSC pool for Bun-compatible runtime
          -> Bun embedder construction profile
          -> Nimbus resolver policy
          -> Nimbus native permission hooks
          -> HostBridge transport
          -> cancellation and lifecycle owner
          -> memory and discard policy
```

The Bun pool is separate because Bun/JSC has different VM construction,
event-loop, resolver, cancellation, memory, and teardown semantics. Sharing the
Deno/V8 pool abstraction is fine only at the worker/backend seam; VM internals
must stay backend-owned.

## Non-Goals

- Do not ship Bun/JSC as a selectable runtime before all gates pass.
- Do not route Bun through `node_external_packages` or label it as a Node22
  runtime.
- Do not rely on OCI/microVM isolation to satisfy in-process gates.
- Do not fork Bun until the missing embedder API surface is precise and small.
- Do not add compatibility shims for legacy runtime metadata; Nimbus is
  pre-launch, so clean metadata is preferred.

## Execution Gates

| Gate | Status | Goal | Verifiable success criteria |
| --- | --- | --- | --- |
| BEP0 | `done` | Establish the cross-platform proof baseline. | `scripts/verify-bun-jsc-in-process-lockdown.sh` passes locally and on Debian 13 `minicloud`; Gate 25 records the Linux toolchain, command, result, and Bun proof commit `ce5aa2a389`; no system apt trust changes are required. |
| BEP1 | `done` | Write the concrete Bun embedder API proposal. | `docs/plans/proof/runtime-engine/bun-jsc/gate-26-embedder-api-proposal.md` maps every unsafe Bun/JSC surface to a construction profile field, resolver hook, native permission hook, lifecycle hook, audit contract, or explicit unsupported state; it includes thread stack-bound, termination-state reset, event-loop, worker propagation, and teardown requirements learned from Gate 25. |
| BEP2 | `done` | Choose upstream-first versus fork threshold for implementation. | `docs/plans/proof/runtime-engine/bun-jsc/gate-27-upstream-fork-threshold.md` records the decision matrix, acceptable patch surface, release/tag format, pre-fork checklist, no-fork conditions, and exact trigger for creating `nimbus/bun`. |
| BEP3 | `pending` | Make the Nimbus runtime seam ready for a real Bun pool without enabling it. | Nimbus has typed backend/pool config and diagnostics for `BunJsc` that fail closed by default; tests prove unsupported Bun metadata is rejected before invocation and that Deno/V8 behavior is unchanged. |
| BEP4 | `pending` | Define and scaffold the dedicated Bun/JSC pool owner. | A concept-owned design/code scaffold exists for Bun pool lifecycle, fresh/discard versus retained trusted reuse, cancellation handles, explicit lifecycle state/ack transitions, event-loop progress, teardown, and metrics; it has no Deno/V8 internals in its public envelope and remains disabled until Bun hooks exist. Product cancellation must not rely on elapsed-time sleeps. |
| BEP5 | `pending` | Prove resolver/package policy denial or hookability. | The Bun proof target demonstrates policy control over dynamic import, `Bun.resolve*`, CommonJS if enabled, Node builtins, package roots, plugins, and native addons; Nimbus docs and tests keep Bun package resolution separate from Node external packages. |
| BEP6 | `pending` | Prove native permission denial or hookability. | Filesystem, network, env/process, subprocess, FFI, plugin, worker, timer, fetch/WebSocket, and dynamic-code surfaces are absent, denied by default, or routed through a typed Nimbus policy hook with audit evidence; unsafe bypasses fail the gate. |
| BEP7 | `pending` | Prove memory, cancellation, teardown, and reuse policy. | The proof records hard memory boundaries or keeps untrusted Bun on fresh/discard with an outer quota; cancellation interrupts runaway code through state/ack-driven lifecycle control rather than sleep timing, recovery is deterministic on macOS and Linux, retained reuse stays trusted-only unless hard isolation is proven, and teardown loops pass. |
| BEP8 | `pending` | Integrate Bun/JSC as an optional runtime backend only after containment passes. | Runtime admission can select Bun/JSC only with the proven lockdown profile and pool policy; generated artifact metadata is explicit; server/registry/codegen tests cover accepted and rejected combinations; V8/Deno remains the default. |
| BEP9 | `pending` | Add repeatable CI/operator lanes and close the plan. | A reusable verification command covers Nimbus tests plus Bun proof lanes on macOS and Linux; docs record local and minicloud evidence, residual risks, fork status, and the exact product go/no-go decision. |

## Embedder API Requirements

The Bun-side API must support a named construction profile that can omit or
deny host-sensitive globals before tenant code runs. Wrapper-level deletion is
defense in depth, not the primary control.

Minimum API surface:

- global construction profile for `Bun`, Web APIs, Node globals, `process`,
  workers, timers, dynamic code, and module loaders
- resolver policy for static/dynamic import, `Bun.resolve*`,
  `import.meta.resolve*`, CommonJS, Node builtins, package roots, plugins,
  native addons, and virtual modules
- filesystem policy for reads, writes, metadata, watches, file descriptors,
  path canonicalization, and directory creation
- network policy for DNS, TCP, UDP, TLS, HTTP/fetch, WebSocket, `Bun.connect`,
  `Bun.listen`, and `Bun.serve`
- env/process policy for projected env, cwd, argv, pid/uid/gid, and system
  metadata
- subprocess policy for executable, args, env, cwd, stdio, IPC, descriptors,
  timeout, and audit identity
- native loading policy for FFI, `dlopen`, N-API, native addons, and plugins
- worker policy with identity, HostBridge grants, cancellation, teardown,
  memory, and audit propagation
- dynamic-code policy that separates host-authored generated wrapper
  compilation from tenant-visible `eval`, `Function`, Node `vm`, and REPL
  evaluation
- lifecycle policy for API-lock use, thread stack-bound initialization,
  termination-state reset, event-loop progress, cancellation, VM teardown,
  discard-on-pressure, and outer quota coordination

## Nimbus Pool Requirements

The Bun pool must be a backend-owned component. It can share worker-loop
interfaces with Deno/V8, but not VM internals.

Required behavior:

- `BunJscTrustedRetainedPool` remains trusted-only until hard isolation exists.
- `BunJscFreshDiscardPoolOuterQuotaRequired` is the first possible untrusted
  policy unless Bun/JSC exposes a hard per-VM heap boundary.
- Every invocation carries backend kind, trust tier, lockdown profile,
  lifecycle policy, tenant identity, workload identity, decision ID, and audit
  context.
- Cancellation and teardown are pool-owned, state/ack-driven rather than
  elapsed-time-sleep-driven, and verified on macOS and Linux.
- Cancellation proofs cover before guest entry, after guest entry, sync CPU
  loops, promise/microtask progress, HostBridge in-flight work, normal
  completion, teardown, retained trusted reuse, and fresh/discard untrusted
  policy.
- Runtime diagnostics expose Bun pool state without presenting Bun as the
  default JavaScript runtime.

## Verification Baseline

Every implementation batch should finish with focused checks plus:

```sh
cargo fmt --all --check
cargo test -p nimbus-runtime limits::tests --lib
cargo test -p nimbus-server registry_and_license::registry --lib
cargo test -p nimbus-server registry_and_license::runtime_metrics --lib
bash scripts/verify-bun-jsc-in-process-lockdown.sh
git diff --check
```

When Bun source changes are part of a batch, also run in
`/Users/jack/src/github.com/oven-sh/bun`:

```sh
cargo fmt --all --check
bun scripts/build.ts --profile=debug-no-asan \
  --build-dir=/private/tmp/nimbus-bun-embed-native \
  --cache-dir=/private/tmp/nimbus-bun-cache \
  --target=check-bun-embed-probe
git diff --check
```

For Linux proof, use the minicloud lane recorded in
`docs/plans/proof/runtime-engine/bun-jsc/gate-25-linux-minicloud-verification.md`.

## Progress Log

| Date | Gate | Status | Notes | Verification | Next |
| --- | --- | --- | --- | --- | --- |
| 2026-05-23 | BEP0 | `done` | Established the cross-platform proof baseline. The Bun proof now passes on macOS and Debian 13 `minicloud`; the Linux fix taught us to avoid host-wide apt trust, use user-local LLVM 21.1.8, seat stack bounds on JSC-touching proof threads, clear termination request state after priming, and avoid a 10 ms cancellation race in debug Linux builds. | `bash scripts/verify-bun-jsc-in-process-lockdown.sh` passed on minicloud: 9 runtime tests, 10 registry tests, 2 diagnostics tests, 1 ignored Bun source proof, Bun format, Bun native `check-bun-embed-probe`, and whitespace checks. Bun commit `ce5aa2a389` records the proof-harness source fix. | Start BEP1 by writing the concrete upstream/fork Bun embedder API proposal with every unsafe surface mapped to construction, resolver, permission, lifecycle, or unsupported state. |
| 2026-05-23 | BEP1 | `done` | Added Gate 26, the concrete embedder API proposal. The proposed API is upstream-first and keeps Nimbus-specific tenant/HostBridge state behind an opaque embedder context while requiring construction profiles, resolver policy, native permission hooks, lifecycle policy, and audit events. | Documentation-only gate; `bash -n scripts/verify-bun-jsc-in-process-lockdown.sh` passed, `cargo fmt --all --check` passed, and `git diff --check` passed. | Start BEP2 by turning the API proposal into an upstream-first versus fork decision matrix with exact fork trigger and tag/release convention. |
| 2026-05-23 | BEP2 | `done` | Added Gate 27, the upstream-first versus fork threshold. A fork is justified only after local BEP5-BEP7 proof patches pass, upstream cannot provide an equivalent stable API, the patch surface is narrow, and Nimbus is ready to make Bun/JSC product-selectable through BEP8. | Documentation-only gate; `bash -n scripts/verify-bun-jsc-in-process-lockdown.sh` passed, `cargo fmt --all --check` passed, and `git diff --check` passed. | Start BEP3 by preparing Nimbus typed runtime/backend/pool diagnostics for a real Bun pool while keeping Bun/JSC fail-closed. |

## References

- `docs/plans/bun-jsc-in-process-lockdown-plan.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-18-in-process-lockdown-source-map.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-19-resolver-package-lockdown-decision.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-20-permission-lockdown-decision.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-21-memory-lifecycle-policy.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-22-reproducible-verification-lane.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-23-fork-upstream-decision.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-24-closeout.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-25-linux-minicloud-verification.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-26-embedder-api-proposal.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-27-upstream-fork-threshold.md`
- `docs/architecture/runtime/engine-seam.md`
- `docs/architecture/runtime/new-engine-proof-harness.md`
