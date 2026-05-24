# Bun/JSC Gate 35: Linked Adapter Rebaseline

Date: 2026-05-23

Nimbus plan: `docs/plans/bun-jsc-linked-adapter-plan.md`

## Status

Status: BJA0 rebaseline complete.

This gate anchors the linked-adapter wave to current source state. The previous
BEP wave proved containment and created the fail-closed Bun/JSC lane. This wave
starts only after recording that the next product step is not more metadata: it
is a real `BunJscExecutionAdapter` behind the existing `RuntimeBackend` seam.

## Source State

| Item | Value |
| --- | --- |
| Nimbus worktree | `/Users/jack/src/github.com/nimbus/nimbus` |
| Nimbus baseline | `9b575308` (`Plan linked Bun runtime adapter work`) |
| Bun worktree | `/Users/jack/src/github.com/oven-sh/bun` |
| Bun upstream baseline | `origin/main` at `f161e0311d` (`bun-v1.3.14-156-gf161e0311d`) |
| Bun proof head | `4b5de5ee5d` (`bun-v1.3.14-172-g4b5de5ee5d`) |
| Bun local dirty state | clean |
| Bun local proof delta | 16 commits ahead of `origin/main` |
| Product source status | not reproducible yet; local proof commits are evidence, not a shipping dependency |

The current local Bun proof delta touches the expected hook/proof owners:

```text
Cargo.toml / Cargo.lock
scripts/build/{bun,configure,rust}.ts
src/bun_bin/{Cargo.toml,lib.rs}
src/embed_probe/{Cargo.toml,lib.rs,nimbus_generated_program_bundle.js}
src/jsc/ModuleLoader.rs
src/jsc/bindings/{ZigGlobalObject.cpp,bindings.cpp}
src/link_bridge/{Cargo.toml,lib.rs}
src/runtime/api/BunObject.rs
```

The proof commits currently ahead of upstream are:

```text
5385b59549 Add Bun JSC embed probe target
31334f9b6e Add Bun embed sync host-call proof
34e71eec57 Add Bun embed async host-call proof
58c6378713 Add Bun embed program bundle proof
d0e63e03af Use generated Nimbus program bundle in embed proof
c57f7e58c0 Add Bun embed timeout cancel proof
9e20ac28a2 Add Bun embed permission inventory proof
f6c87be47e Add Bun embed memory behavior proof
f0cee692c0 Add Bun embed package module policy proof
65cdc97796 Add Bun embed lifecycle reuse proof
ce5aa2a389 Stabilize Bun embed cancellation proof on Linux
c5bafa6d73 Add Bun embedder resolver denial proof
0c132cff81 Add Bun embedder native permission deny profile proof
7bcb026409 Make Bun embedder cancellation proof ack-driven
44540674fc Clarify Bun embedder lifecycle proof coverage
4b5de5ee5d Add Bun embedder pre-entry cancellation gate proof
```

## Current Upstream Sufficiency

Current upstream Bun at `origin/main` is not sufficient for a Nimbus product
dependency. It does not contain the local proof target or the local hook
surfaces that proved:

- VM construction/destruction below the CLI path
- sync and async host-call callbacks into embedder-owned functions
- self-contained Nimbus program-wrapper evaluation
- timeout and explicit cancellation proof
- native permission denial/profile marking for Bun/process/FFI/timers/workers
- resolver denial before dynamic import, lower module-load/evaluate paths,
  `Bun.resolve*`, package roots, plugin virtual modules, and native addons
- lifecycle proof for before-entry cancellation, after-entry spin-entry ack,
  recovery, teardown, and retained trusted reuse

This is exactly why BJA8 requires either an upstream Bun release/tag that
contains equivalent APIs or a Nimbus-owned Bun fork/tag. The product must not
depend on `~/src/github.com/oven-sh/bun` carrying local-only proof commits.

## Required Bun-Side APIs For BJA2-BJA8

The linked adapter needs a small product API, not the proof binary itself:

- non-CLI VM construction and destruction
- named lockdown construction profile for the untrusted Bun/JSC lane
- host callback registration for synchronous and asynchronous HostBridge calls
- self-contained program-wrapper evaluation with JSON args/result transport
- resolver policy hook before dynamic import, load/evaluate, `Bun.resolve*`,
  package roots, plugins, virtual modules, and native addons resolve
- native permission policy or construction-time omission for filesystem,
  network/server, env/process, subprocess, FFI/native loading, plugin, worker,
  timer, fetch/WebSocket, and tenant-visible dynamic-code surfaces
- cancellation entry gate before guest code, after-entry termination hook, and
  state/ack lifecycle evidence
- teardown/discard signal for the untrusted fresh/discard pool
- memory-pressure signal and outer-quota coordination evidence
- audit/evidence output that Nimbus can normalize into runtime diagnostics

## Nimbus Linked-Adapter Seam

The Nimbus seam should remain:

```text
RuntimeBackendInvocation
  -> BunJscRuntimeBackend
  -> BunJscPool
  -> BunJscExecutionAdapter
  -> Bun/JSC embedder API
```

`BunJscExecutionAdapter` should be smaller than `RuntimeBackend`; it should
not know about the worker loop or Deno/V8 internals. The default adapter remains
`not_linked` and returns the existing contract error. A linked adapter may only
report `linked` after the Bun source dependency is reproducible and the linked
gate proves execution, HostBridge grants, cancellation, teardown, diagnostics,
and memory policy.

## Fork Trigger For This Wave

Create a Nimbus-owned Bun source only when all of these are true:

1. BJA2 proves the required execution API locally.
2. The API surface is limited to the owners listed in this proof or a smaller
   equivalent set.
3. Upstream Bun does not expose an equivalent accepted/stable API in time for
   BJA3-BJA8.
4. Nimbus can pin the dependency by immutable upstream tag/revision or
   `nimbus/bun` tag using the format `bun-vX.Y.Z-nimbus.N`.

The fork trigger is intentionally not "local proof commits exist." Local proof
commits already exist. The fork trigger is product dependency ownership.

## Verification

Required BJA0 checks:

```sh
cargo fmt --all --check
make verify-bun-jsc-runtime-contract
git diff --check
```

Result: passed on 2026-05-23.

The runtime-contract gate passed with:

```text
runtime policy and memory semantics: 11 passed
Bun/JSC pool scaffold contract: 4 passed
Convex runtime lane registry contract: 13 passed
runtime diagnostics API contract: 2 passed
tenant admission for proven Bun/JSC profile: 1 passed
operator UI runtime diagnostics contract: 2 files, 5 tests passed
```

`make verify-bun-jsc-runtime-contract` was run outside the Codex filesystem
sandbox so the runtime diagnostics route tests could bind local test sockets.
