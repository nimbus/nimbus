# Bun/JSC Gate 23: Fork And Upstream Decision

Date: 2026-05-23

Nimbus plan: `docs/plans/bun-jsc-in-process-lockdown-plan.md`

Inputs:

- `docs/plans/proof/runtime-engine/bun-jsc/gate-16-fork-upstream-hold-decision.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-18-in-process-lockdown-source-map.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-19-resolver-package-lockdown-decision.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-20-permission-lockdown-decision.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-21-memory-lifecycle-policy.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-22-reproducible-verification-lane.md`

Bun worktree: `/Users/jack/src/github.com/oven-sh/bun`

Bun local proof head: `65cdc97796` (`Add Bun embed lifecycle reuse proof`)

Bun upstream base in local worktree: `f161e0311d`
(`shell: wrap only component-leading ! when neutralizing glob metachars (#31272)`)

## Decision

Status: upstream-first; no Nimbus Bun fork yet.

Do not fork Bun for product use today. Keep the local Bun commits as proof
evidence only. If Nimbus decides to productize an in-process Bun/JSC backend,
the first real implementation should seek a small upstream embedder API before
creating a Nimbus-maintained fork.

This decision is compatible with a future Bun runtime pool. A selectable Bun
backend should have a dedicated Bun/JSC pool beside the V8/Deno/Node pool, but
that pool still needs Bun-side resolver, permission, memory, cancellation, and
teardown hooks before it can run untrusted tenant code in process.

## Minimum Patch Surface

The minimum patch surface is not "embed Bun and delete globals." It is a
stable embedder API with these parts:

| API area | Required capability |
| --- | --- |
| Build/embed target | Build Bun/JSC below `bun_bin` without CLI process ownership, process exit, or global runtime setup assumptions. |
| Global construction profile | Install/omit `Bun`, Web, Node, process, worker, dynamic-code, timer, and module globals according to a named profile. |
| Resolver policy | Synchronously deny or allow dynamic import, `Bun.resolve*`, `import.meta.resolve*`, CommonJS, Node builtins, package roots, plugins, virtual modules, and native addons. |
| Filesystem policy | Gate BunFile, NodeFS, watches, file descriptors, path canonicalization, reads, writes, truncates, mkdir/open, and metadata. |
| Network policy | Gate DNS, TCP, UDP, TLS, HTTP/fetch, WebSocket, `Bun.connect`, `Bun.listen`, and `Bun.serve` before native socket/server creation. |
| Process/env policy | Provide a projected process/env object and deny raw host process metadata unless explicitly granted. |
| Subprocess policy | Default deny; if ever granted, check executable, args, env, cwd, stdio, IPC, descriptors, timeout, and audit identity before native spawn. |
| Native loading policy | Keep FFI, `dlopen`, N-API, and native addons absent for untrusted tenants. |
| Worker policy | Default deny; if ever granted, propagate runtime identity, HostBridge policy, cancellation, teardown, memory, and audit into child contexts. |
| Dynamic code policy | Separate host-authored generated wrapper compilation from tenant-visible `eval`, `Function`, Node `vm`, and REPL evaluation. |
| Lifecycle/pool API | Dedicated Bun/JSC pool ownership over concurrency, event-loop progress, cancellation, VM teardown, discard-on-pressure, and outer quota coordination. |

## Maintenance Burden

A Nimbus fork would carry non-trivial risk because the patch touches Bun's
highest-churn and highest-security areas:

- JSC/WebKit VM construction and termination
- Bun global object and native callback registration
- Bun resolver/module loader and plugin integration
- filesystem, network, subprocess, and native loading implementations
- worker/thread lifecycle and event-loop behavior
- build graph and platform-specific native linking

That burden is only justified if upstream cannot expose the embedder API and
Nimbus has already committed to shipping Bun/JSC as a product runtime.

## CI Expectations Before A Fork Or Product Backend

Minimum CI before a fork or product dependency:

- macOS `scripts/verify-bun-jsc-in-process-lockdown.sh`
- Linux/minicloud `scripts/verify-bun-jsc-in-process-lockdown.sh`
- Bun upstream rebase/sync check
- Bun proof `cargo fmt --all --check`
- Bun native `check-bun-embed-probe`
- Nimbus runtime policy, registry rejection, and diagnostics tests
- explicit Linux native linker/JSC verification
- security update watch for Bun, JSC/WebKit, transitive native deps, and
  generated build artifacts
- release provenance/tagging for any consumed Bun fork or binary artifact

## Release And Tagging Implication

If a fork becomes necessary, follow Nimbus fork conventions:

- fork name should be explicit, likely `nimbus/bun` rather than a generic
  mirror
- tags should encode upstream Bun version plus Nimbus patch sequence, for
  example `bun-vX.Y.Z-nimbus.N` once the upstream version baseline is chosen
- Nimbus should pin by immutable revision/tag, not a floating branch
- the fork must publish proof evidence for macOS and Linux before Nimbus
  depends on it
- the Nimbus product should still expose Bun as an optional backend, not as a
  replacement for the V8/Deno/Node lane

## Upstream Proposal Shape

When the time comes, the upstream proposal should be narrow and concrete:

1. stabilized non-CLI embed target below `bun_bin`
2. named embedder security profile during VM/global construction
3. synchronous resolver policy hook with denial evidence
4. native operation policy hooks for filesystem/network/process/subprocess
5. worker and lifecycle propagation contracts
6. documented API-lock, termination, event-loop, and teardown requirements

Do not upstream Nimbus-generated program fixtures or HostBridge details. Those
belong in Nimbus proof harnesses.

## Outcome

`BIL7` is complete. The decision is upstream-first, no fork yet. A future fork
is justified only if Nimbus commits to a selectable Bun/JSC backend, the needed
embedder APIs cannot land upstream, and the patch surface remains small enough
to continuously verify across macOS and Linux.
