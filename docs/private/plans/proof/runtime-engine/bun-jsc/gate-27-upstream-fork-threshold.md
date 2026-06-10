# Bun/JSC Gate 27: Upstream-First Versus Fork Threshold

Date: 2026-05-23

Nimbus plan: `docs/plans/archive/bun-jsc-embedder-api-and-pool-plan.md`

Inputs:

- `docs/plans/proof/runtime-engine/bun-jsc/gate-23-fork-upstream-decision.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-25-linux-minicloud-verification.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-26-embedder-api-proposal.md`

## Decision

Status: upstream-first; fork threshold defined.

Do not create a Nimbus Bun fork yet. Nimbus should first try to shape the
embedder API as a small upstreamable Bun surface. A fork becomes justified only
if all of these are true:

1. A local Bun proof demonstrates the required resolver, permission, lifecycle,
   memory, and teardown controls from Gate 26.
2. The required API cannot be landed upstream, or upstream cannot provide a
   stable API on a timeline compatible with Nimbus productization.
3. The remaining patch surface is narrow, source-owned, and continuously
   verifiable on macOS and Linux.
4. The fork can be shipped as an optional backend dependency pinned by immutable
   tag/revision with release evidence.

If those conditions are not all true, Nimbus keeps Bun/JSC proof-only and uses
the existing OCI/microVM path for arbitrary Bun applications.

## Fork Trigger

The exact trigger for creating a Nimbus-owned Bun repository is:

```text
BEP5 + BEP6 + BEP7 have passing local proof patches
AND the patch surface is limited to the approved hook areas below
AND upstream Bun does not expose an equivalent accepted/stable API
AND Nimbus is ready to make Bun/JSC product-selectable behind BEP8
```

This is intentionally stricter than "we need a patch." A local proof patch can
exist before a fork. The fork is only for a product dependency.

## Decision Matrix

| API area | Upstream-first target | Fork acceptable if | Fork no-go if |
| --- | --- | --- | --- |
| Non-CLI embed target | Stable build target below `bun_bin` that constructs/destroys a VM without process ownership. | Patch is limited to build graph and process-neutral link roots. | Requires rewriting CLI startup, allocator, crash/signal, or process-exit behavior broadly. |
| Construction profile | Named security profile can omit/limit `Bun`, Node, process, worker, plugin, dynamic-code, and module globals. | Patch is centralized around global construction/property registration. | Requires scattered post-construction monkey-patching or fragile JS wrapper deletion as the primary control. |
| Resolver policy | Synchronous policy hook before dynamic import, `Bun.resolve*`, `import.meta.resolve*`, CommonJS, plugins, and native addons resolve. | Patch is centralized in shared resolver/module-loader entrypoints. | Requires maintaining a separate resolver or duplicating Bun package semantics in Nimbus. |
| Filesystem policy | Native hook before BunFile/NodeFS/read/write/stat/watch/open effects. | Patch has a small common filesystem-policy shim used by Bun and NodeFS paths. | Must patch every individual filesystem callsite with no shared owner. |
| Network policy | Native hook before DNS/socket/fetch/WebSocket/listen/server effects. | Patch centralizes connect/listen/fetch/DNS checks near native creation. | Requires wrapping only JS APIs while native socket/server owners remain reachable. |
| Env/process policy | Projected env/process object or native hook for every host-process read/write. | Patch centralizes env/process projection at construction and native getters. | Raw host `process` object remains installed for untrusted tenants. |
| Subprocess policy | Default-deny native spawn hook before process creation. | Patch is limited to spawn owner and Node child-process bridge. | Any subprocess path can reach native spawn without policy. |
| Native loading | FFI, `dlopen`, N-API, plugins, native addons absent or default-denied. | Patch cleanly omits or denies the loaders in the untrusted profile. | Native code loading cannot be fully disabled for the profile. |
| Workers | Worker creation default-denied or launched with child profile, identity, memory, cancellation, and teardown propagation. | Patch is localized to worker construction and lifecycle propagation. | Child contexts can inherit raw host authority or outlive invocation teardown. |
| Dynamic code | Tenant-visible `eval`, `Function`, Node `vm`, and REPL compile paths default-denied while host-generated wrapper compilation remains possible. | Patch cleanly distinguishes host-authored embedder evaluation from tenant-authored dynamic code. | JSC intrinsic lockdown cannot be made reliable without broad JSC/WebKit changes. |
| Memory/lifecycle | Fresh/discard plus outer quota, or proven hard per-VM heap limit; deterministic cancellation and teardown on macOS/Linux. | Patch only adds embedder lifecycle hooks and documented thread/termination contracts. | Requires invasive JSC/WebKit memory-manager changes or retained untrusted reuse without hard bounds. |

## Repository And Release Convention If Forked

If the fork trigger fires:

- repository name: `nimbus/bun`
- creation shape: push Nimbus-owned history; do not depend on a GitHub web-fork
  relationship
- branch shape: `nimbus/<upstream-version>` for maintained release lines
- tag shape: `bun-vX.Y.Z-nimbus.N`, matching the upstream Bun version baseline
  and Nimbus patch sequence
- Nimbus dependency pin: immutable tag and revision, never a floating branch
- required release evidence: macOS proof gate, Linux/minicloud proof gate,
  Bun source `cargo fmt --all --check`, native `check-bun-embed-probe`,
  `git diff --check`, patch-surface summary, and security update notes

## Pre-Fork Checklist

Before creating `nimbus/bun`, all items must be true:

- Gate 26 API proposal has an implementation sketch for every required hook.
- BEP5 resolver proof denies dynamic import and `Bun.resolve*` by policy.
- BEP6 permission proof turns every `unsafe_bypass` into absent, denied, or
  policy-hooked.
- BEP7 lifecycle proof passes cancellation, teardown, memory policy, and reuse
  rules on macOS and Linux.
- Patch surface is reviewed and small enough for continuous rebase.
- Nimbus runtime admission still rejects Bun/JSC unless the proven profile and
  pool policy are selected.
- Distribution/provenance plans can consume a pinned fork artifact if needed.

## No-Fork Conditions

Do not fork if:

- Bun can expose the Gate 26 API upstream in a stable form.
- The required patch crosses too many unrelated Bun/JSC/WebKit owners.
- The only available containment is JavaScript wrapper deletion.
- Memory, worker, subprocess, or native-loading isolation cannot be proven.
- Nimbus only needs arbitrary Bun applications in sandboxes; that is already
  served by OCI/microVM workloads.

## Outcome

`BEP2` is complete. The next gate is `BEP3`: make the Nimbus runtime seam ready
for a real Bun pool while keeping Bun/JSC fail-closed and non-selectable.
