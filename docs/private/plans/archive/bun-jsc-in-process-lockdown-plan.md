# Plan: Bun/JSC In-Process Lockdown

This plan owns the next Bun/JSC runtime wave after the completed proof plan.
The target is specifically an in-process Bun/JSC backend with Nimbus-owned
security lockdown. Running the `bun` binary inside an OCI image and microVM is
already covered by Nimbus sandbox orchestration; that is a workload mode, not
this runtime-engine plan.

## Status

- **Status:** complete; `BIL0` through `BIL8` are complete.
- **Primary owner:** this plan
- **Bun worktree:** `/Users/jack/src/github.com/oven-sh/bun`
- **Nimbus worktree:** `/Users/jack/src/github.com/nimbus/nimbus`
- **Current product posture:** proof-only; not selectable
- **Current trust tier:** modeled only as `proof_only` or
  `in_process_trusted_only`, never `in_process_untrusted`
- **Target trust tier:** `in_process_untrusted` only after every host-sensitive
  Bun/JSC surface is absent, denied by default, or routed through Nimbus policy
- **Completed predecessor:** `docs/plans/archive/bun-jsc-runtime-proof-plan.md`

## Product Decision

Do not treat "Bun in a microVM" as the fallback for this proof.

If a customer wants to run arbitrary Bun applications safely today, Nimbus can
package Bun in an OCI image and run it through the existing tenant-isolated
`nimbus-crun` / libkrun microVM path. That path provides process and kernel
isolation outside the runtime-engine seam.

This plan is about a different product capability: whether Nimbus can embed
Bun/JSC in process, expose the Nimbus JavaScript context and HostBridge, and
still deny or mediate every host-sensitive capability before tenant code can
reach the host.

## Non-Goals

- Do not add a production Bun selector before the lockdown gates pass.
- Do not claim Bun as a Node22 replacement. Bun compatibility target and
  engine backend remain separate metadata axes.
- Do not route Bun-backed functions through `node_external_packages`.
- Do not depend on OCI/microVM isolation to satisfy in-process runtime gates.
- Do not fork Bun until this plan identifies an exact, maintainable patch set
  that upstream cannot provide in a usable shape.

## Required Architecture Shape

```text
Nimbus runtime admission
  -> RuntimeBackendKind::BunJsc
  -> in-process lockdown profile
      -> Bun/JSC VM construction below Bun CLI/process ownership
      -> Nimbus host-call transport through JSC host functions
      -> Nimbus context bootstrap
      -> Bun policy hooks for host-sensitive builtins
      -> Nimbus resolver/package policy
      -> memory/lifecycle policy
      -> runtime diagnostics and audit evidence
  -> invocation
```

The mandatory lockdown rule is:

```text
present host-sensitive surface
  = denied by default
  OR policy-hooked through Nimbus grants
  OR absent in the selected VM/evaluation mode
```

Any surface outside those states keeps Bun/JSC in `in_process_trusted_only`.

## Execution Gates

| Gate | Status | Goal | Verifiable success criteria |
| --- | --- | --- | --- |
| BIL0 | `done` | Rebaseline scope and evidence around the in-process-only product target. | This plan, `docs/plans/README.md`, `docs/architecture/runtime/engine-seam.md`, and the Bun proof docs clearly distinguish in-process Bun/JSC from OCI/microVM Bun workloads; the proof command reproduced against the current local Bun worktree; `cargo fmt --all --check` and `git diff --check` passed in Nimbus. |
| BIL1 | `done` | Define the concrete Nimbus containment API and admission states for Bun/JSC. | Added `RuntimeBackendTrustTier` and `RuntimeBackendLockdownProfile`; diagnostics and runtime bundle cache keys now carry the axes; tests prove V8 stays `in_process_untrusted` on `v8_deno_core` while Bun/JSC with no profile, proof-only profile, trusted generated-wrapper profile, or future untrusted profile all fail closed; no server or codegen product route was added. |
| BIL2 | `done` | Build a Bun source ownership map for every unsafe surface found in Gate 11. | `docs/plans/proof/runtime-engine/bun-jsc/gate-18-in-process-lockdown-source-map.md` names the Bun source owner/path or API family for filesystem, network, env, subprocess, FFI, plugin, worker, package resolver, dynamic import, process globals, timers, and dynamic-code surfaces; each row states whether upstream API, fork patch, or Nimbus wrapper is plausible. |
| BIL3 | `done` | Choose and prove the resolver/package lockdown strategy. | `docs/plans/proof/runtime-engine/bun-jsc/gate-19-resolver-package-lockdown-decision.md` records the source blocker: dynamic import and `Bun.resolve*` route below Nimbus wrapper maps into Bun's module loader/resolver, so an upstream or forked Bun embedder resolver policy is required before `in_process_untrusted`. |
| BIL4 | `done` | Choose and prove permission lockdown for host-sensitive builtins. | `docs/plans/proof/runtime-engine/bun-jsc/gate-20-permission-lockdown-decision.md` records the source blocker: current filesystem, network, env/process, subprocess, FFI, plugin, worker, timer, dynamic-code, and resolver surfaces remain `unsafe_bypass` or `policy_hook_missing`, so Bun/JSC cannot become `in_process_untrusted` without construction, resolver, and native permission hooks. |
| BIL5 | `done` | Finalize memory, cancellation, reuse, and teardown policy for the first selectable backend. | Added `RuntimeBackendLifecyclePolicy`, carried it through diagnostics, cache keys, facade exports, and tenant runtime decisions, and documented the policy in `docs/plans/proof/runtime-engine/bun-jsc/gate-21-memory-lifecycle-policy.md`: retained Bun/JSC VMs are trusted proof-only; untrusted promotion requires fresh/discard lifecycle plus an outer hard memory quota unless Bun/JSC exposes a hard per-VM heap boundary. |
| BIL6 | `done` | Add reproducible CI lanes for the in-process lockdown proof. | Added and ran `scripts/verify-bun-jsc-in-process-lockdown.sh`; it reproduces the Nimbus tests and Bun embed proof on macOS, builds the embedded UI prerequisite before server tests, and passes on the Linux/minicloud lane. |
| BIL7 | `done` | Make the fork/upstream decision with evidence. | `docs/plans/proof/runtime-engine/bun-jsc/gate-23-fork-upstream-decision.md` records `upstream-first; no fork yet`, including minimum patch surface, maintenance burden, CI expectations, release/tagging implications, and future Bun pool expectations. |
| BIL8 | `done` | Close the plan or promote Bun/JSC to the next implementation plan. | `docs/plans/proof/runtime-engine/bun-jsc/gate-24-closeout.md` records the final state: proof-only, upstream-first, not selectable, future dedicated Bun/JSC pool required before product promotion, and Linux/minicloud verification passed for the proof baseline. |

## Selected Containment API

`BIL1` selected a small typed Rust shape, not a stringly runtime flag:

```rust
RuntimeBackendKind::BunJsc
RuntimeBackendTrustTier::{ProofOnly, InProcessTrustedOnly, InProcessUntrusted}
RuntimeBackendLockdownProfile::{
    BunJscProofOnly,
    BunJscTrustedGeneratedWrapper,
    BunJscInProcessUntrusted,
}
RuntimeBackendLifecyclePolicy::{
    BunJscTrustedRetainedPool,
    BunJscFreshDiscardPoolOuterQuotaRequired,
}
```

These axes are serialized in diagnostics, included in runtime bundle cache
keys, and captured in tenant runtime policy decisions. They do not make Bun/JSC
selectable; every Bun/JSC combination still fails closed until the matching
Bun-side lockdown and pool implementation exists.

## Bun Hook Requirements

`BIL2` through `BIL4` should determine whether upstream Bun can expose stable
embedder hooks for:

- filesystem reads, writes, metadata, watches, and path resolution
- TCP, UDP, TLS, HTTP, WebSocket, `fetch`, `Bun.serve`, `Bun.listen`, and
  `Bun.connect`
- environment reads and writes, `process.env`, argv, cwd, pid, uid, gid, and
  system metadata
- subprocess creation, shell execution, IPC, and stdio inheritance
- FFI, `dlopen`, native addons, plugin loading, and dynamic library paths
- worker creation, threads, nested event loops, and background tasks
- dynamic import, CommonJS require, Node builtin resolution, package
  resolution, and `Bun.resolve*`
- timers, cancellation points, and runaway microtask/event-loop progress
- dynamic code evaluation through `eval`, `new Function`, and equivalent
  compiler entrypoints

The preferred result is a narrow upstreamable embedder policy API. A fork is
acceptable only if that API cannot land upstream and the patch surface remains
small enough for Nimbus to maintain across Bun/WebKit/JSC updates.

## Resolver Policy

The safe starting point remains self-contained generated program wrappers with
bundled Nimbus code only. Any package-loading mode must have a Bun-owned
resolver policy, independent of the Deno/V8 Node external package lane.

Before `in_process_untrusted`, these must be denied or policy-hooked:

- `import("node:fs")` and every Node builtin import
- `Bun.resolve` and `Bun.resolveSync`
- CommonJS require if it becomes present in the chosen evaluator
- package roots outside the Nimbus-generated bundle root
- native addon and plugin resolution

## Memory And Lifecycle Policy

Gate 12 found a useful JSC pressure signal but no hard per-VM heap limit in the
current embed path. Therefore, `BIL5` must not assume retained in-process
isolation is enough for untrusted tenants.

The first selectable Bun/JSC policy, if any, should be a dedicated Bun/JSC pool
beside the existing V8/Deno/Node pool. For untrusted tenants, that Bun pool
must use fresh/discard VM entries and an outer hard memory quota until Bun/JSC
can prove a hard in-process heap boundary. Trusted retained Bun/JSC pool reuse
remains proof-only.

Only a verified hard memory boundary can justify retained untrusted VM reuse.

## Verification

This plan is complete only when the relevant gate-specific commands plus this
baseline pass:

```sh
cargo fmt --all --check
cargo clippy -p nimbus-runtime -p nimbus-server -p nimbus-bin -- -D warnings
cargo test -p nimbus-runtime limits::tests --lib
cargo test -p nimbus-server registry_and_license::registry --lib
cargo test -p nimbus-runtime --test engine_proofs \
  bun_jsc_build_gate_reproduces_from_bun_build_graph \
  -- --ignored --nocapture
git diff --check
```

And in `/Users/jack/src/github.com/oven-sh/bun`:

```sh
cargo fmt --all --check
bun scripts/build.ts --profile=debug-no-asan \
  --build-dir=/private/tmp/nimbus-bun-embed-native \
  --cache-dir=/private/tmp/nimbus-bun-cache \
  --target=check-bun-embed-probe
git diff --check
```

If a gate cannot pass because Bun lacks the required embedder API, the gate can
only close by recording the exact source-level blocker and the fork/upstream
decision it implies.

## Progress Log

| Date | Gate | Status | Notes | Verification | Next |
| --- | --- | --- | --- | --- | --- |
| 2026-05-23 | BIL0 | `done` | Opened the follow-on plan and narrowed the product target to in-process Bun/JSC lockdown. OCI/microVM Bun remains an existing sandbox workload mode, not this runtime-engine proof. Updated the active plan index plus runtime seam and proof-harness docs. | `cargo fmt --all --check` passed; `git diff --check` passed; `bun scripts/build.ts --profile=debug-no-asan --build-dir=/private/tmp/nimbus-bun-embed-native --cache-dir=/private/tmp/nimbus-bun-cache --target=check-bun-embed-probe` passed in `/Users/jack/src/github.com/oven-sh/bun` and emitted `[build] check-bun-embed-probe done`. | Start BIL1 by adding typed containment/admission states while keeping Bun/JSC non-selectable. |
| 2026-05-23 | BIL1 | `done` | Added `RuntimeBackendTrustTier` (`proof_only`, `in_process_trusted_only`, `in_process_untrusted`) and `RuntimeBackendLockdownProfile` (`v8_deno_core`, Bun/JSC proof/trusted/untrusted profiles). Runtime diagnostics, tenant runtime decisions, and runtime bundle cache keys now carry the axes. Bun/JSC remains rejected for every profile. | `cargo check -p nimbus-runtime -p nimbus-server` passed; `cargo test -p nimbus-runtime limits::tests --lib` passed 9 tests; `cargo test -p nimbus-server registry_and_license::registry --lib` passed 10 tests; `cargo test -p nimbus-server registry_and_license::runtime_metrics --lib` passed 2 tests; `cargo fmt --all --check` passed; `git diff --check` passed. | Start BIL2 by mapping Gate 11 unsafe surfaces to Bun source ownership and deciding which need upstream hooks versus a fork patch. |
| 2026-05-23 | BIL2 | `done` | Added Gate 18, a source ownership map covering Bun global registration, filesystem, network, environment/process, subprocess, FFI/native addons, plugins, resolver/package loading, CommonJS/Node globals, workers, timers, dynamic code, and Nimbus host calls. The map records that wrappers are useful only as defense in depth; `in_process_untrusted` needs Bun-side construction, resolver, and native operation hooks. | Read-only `rg` searches across the local Bun worktree; no Bun files modified. | Start BIL3 by turning the Gate 13 dynamic-import and `Bun.resolve*` findings into a precise resolver/package-lockdown blocker or proof strategy. |
| 2026-05-23 | BIL3 | `done` | Added Gate 19, the resolver/package lockdown decision. The selected artifact lane stays a self-contained generated wrapper, but dynamic import and `Bun.resolve*` remain source-blocked below Nimbus wrapper maps. `require` stays absent only while Nimbus avoids `Bun__REPL__setupGlobalRequire`; enabling CommonJS later needs the same resolver policy. | Gate 13 proof output recorded `dynamic_import_node_fs: unsafe_import_fulfilled`, `Bun.resolve: unsafe_bypass`, and `Bun.resolveSync: unsafe_bypass`; read-only source searches mapped the shared resolver path through `ZigGlobalObject.cpp`, `BunObject.rs`, `ImportMetaObject.cpp`, `ExposeNodeModuleGlobals.cpp`, `CommonJS.ts`, `ModuleLoader.cpp`, `JSModuleLoader.{rs,zig}`, and `jsc_hooks.rs`. | Start BIL4 by turning the Gate 11 permission inventory into a precise construction/native-hook blocker for filesystem, network, env/process, subprocess, FFI, plugins, workers, timers, and dynamic code. |
| 2026-05-23 | BIL4 | `done` | Added Gate 20, the host permission lockdown decision. The current proof lane remains blocked for untrusted tenants because Bun filesystem, network, env/process, subprocess, FFI, plugin, worker, timer, and dynamic-code surfaces are present without Nimbus policy hooks. The decision separates construction absence from native denial/mediation as the required Bun embedder security profile. | Gate 11 inventory plus Gate 18 source map and Gate 19 resolver decision; no Bun files modified. | Start BIL5 by making the lifecycle policy explicit: retained Bun/JSC VMs are trusted-only, untrusted promotion requires fresh/discard behavior plus an outer hard memory quota unless Bun/JSC exposes a hard per-VM heap boundary. |
| 2026-05-23 | BIL5 | `done` | Added `RuntimeBackendLifecyclePolicy` (`v8_deno_core_pool`, `bun_jsc_trusted_retained_pool`, `bun_jsc_fresh_discard_pool_outer_quota_required`) and carried it through runtime limits, diagnostics, tenant runtime policy decisions, facade exports, and runtime bundle cache keys. Added Gate 21 to make retained Bun/JSC VMs trusted-only and require fresh/discard lifecycle plus an outer hard memory quota for untrusted promotion. | `cargo check -p nimbus-runtime -p nimbus-server` passed; `cargo test -p nimbus-runtime limits::tests --lib` passed 9 tests. | Start BIL6 by adding a reusable verification command/script for the Nimbus-side gates plus the local Bun `check-bun-embed-probe` lane and naming the required Linux/minicloud lane. |
| 2026-05-23 | BIL6 | `done` | Added `scripts/verify-bun-jsc-in-process-lockdown.sh`, a ten-step local gate covering Nimbus formatting, UI build prerequisites for `nimbus-server`, runtime policy tests, server registry/runtime diagnostics tests, ignored Bun source proof lane, Nimbus whitespace, Bun format, Bun native embed probe, and Bun whitespace. | `bash scripts/verify-bun-jsc-in-process-lockdown.sh` passed locally and on Debian 13 `minicloud`. It ran 9 runtime policy tests, 10 registry tests, 2 runtime diagnostics tests, the ignored Bun source proof test, Bun `cargo fmt --all --check`, Bun `check-bun-embed-probe`, and whitespace checks in both repos. A clean Linux clone exposed the missing UI prerequisite, so the script now runs `npm ci` when needed and `make build-ui` before server tests. | Start BIL7 by recording the fork/upstream decision with patch surface, maintenance burden, CI, release/tagging, and explicit future Bun pool expectations. |
| 2026-05-23 | BIL7 | `done` | Added Gate 23: upstream-first, no fork yet. A selectable Bun backend should eventually have a dedicated Bun/JSC pool beside V8/Deno/Node, but the pool must prove resolver, permission, memory, cancellation, and teardown isolation before it can run untrusted tenant code in process. | Source decisions from Gates 18-21 plus the reproducible Gate 22 verification lane. | Start BIL8 by running final verification, updating close-out docs, and leaving the plan in an explicit proof-only/upstream-first state. |
| 2026-05-23 | BIL8 | `done` | Added Gate 24 closeout. Bun/JSC remains proof-only, upstream-first, and not selectable. A future selectable Bun backend should have a dedicated Bun/JSC pool beside V8/Deno/Node, but that pool must prove resolver, permission, memory, cancellation, reuse, and teardown isolation before untrusted tenant promotion. | Nimbus: `cargo fmt --all --check` passed; `cargo clippy -p nimbus-runtime -p nimbus-server -p nimbus-bin -- -D warnings` passed; `cargo test -p nimbus-runtime limits::tests --lib` passed 9 tests; `cargo test -p nimbus-server registry_and_license::registry --lib` passed 10 tests; ignored Bun source proof test passed 1 test; `git diff --check` passed. Bun: `cargo fmt --all --check` passed; `bun scripts/build.ts --profile=debug-no-asan --build-dir=/private/tmp/nimbus-bun-embed-native --cache-dir=/private/tmp/nimbus-bun-cache --target=check-bun-embed-probe` passed; `git diff --check` passed. Reusable gate `bash scripts/verify-bun-jsc-in-process-lockdown.sh` passed locally and on Debian 13 `minicloud` with Bun proof commit `ce5aa2a389`. | Plan complete. Next work is `docs/plans/archive/bun-jsc-embedder-api-and-pool-plan.md`, the upstream/fork embedder API and dedicated Bun pool plan. |

## References

- `docs/plans/archive/bun-jsc-runtime-proof-plan.md`
- `docs/architecture/runtime/engine-seam.md`
- `docs/architecture/runtime/new-engine-proof-harness.md`
- `docs/architecture/runtime/permission-model.md`
- `docs/plans/archive/runtime-engine-seam-plan.md`
- `docs/plans/archive/execution-isolation-and-runtime-backends-plan.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-11-permission-surface-inventory.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-12-memory-behavior.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-13-package-module-policy.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-14-lifecycle-reuse-stress.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-16-fork-upstream-hold-decision.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-18-in-process-lockdown-source-map.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-19-resolver-package-lockdown-decision.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-20-permission-lockdown-decision.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-21-memory-lifecycle-policy.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-22-reproducible-verification-lane.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-23-fork-upstream-decision.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-24-closeout.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-25-linux-minicloud-verification.md`
