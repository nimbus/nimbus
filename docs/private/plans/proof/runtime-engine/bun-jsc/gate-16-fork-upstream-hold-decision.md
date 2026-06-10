# Bun/JSC Gate 16: Fork Upstream Hold Decision

Date: 2026-05-23

Nimbus prior proof revision: `8437f86c` (`Add Bun runtime metadata rejection gate`)

Bun worktree: `/Users/jack/src/github.com/oven-sh/bun`

Bun proof commit: `65cdc97796` (`Add Bun embed lifecycle reuse proof`)

Bun upstream base in local worktree: `f161e0311d`
(`shell: wrap only component-leading ! when neutralizing glob metachars (#31272)`)

Bun patch status: committed locally on Bun `main`, not upstreamed.

## Decision

Do not fork Bun yet, and do not open an upstream proposal yet.

Keep the local Bun delta as proof evidence. The patch proves that a non-CLI
Bun/JSC embed target is possible, but the measured gates still show that
Nimbus does not have the permission hooks, Bun package resolver policy, hard
memory isolation story, or production CI lane needed to justify a maintained
runtime fork.

## Current Local Bun Delta

Commands reviewed:

```sh
cd /Users/jack/src/github.com/oven-sh/bun
git log --oneline --decorate origin/main..HEAD
git diff --stat origin/main..HEAD
git diff --name-status origin/main..HEAD
git status --short --branch
```

Result:

- `origin/main`: `f161e0311d`
- local proof `HEAD`: `65cdc97796`
- branch status: `main...origin/main [ahead 10]`
- shortstat: 12 files changed, 2731 insertions, 22 deletions

Local proof commits:

```text
65cdc97796 Add Bun embed lifecycle reuse proof
f0cee692c0 Add Bun embed package module policy proof
f6c87be47e Add Bun embed memory behavior proof
9e20ac28a2 Add Bun embed permission inventory proof
c57f7e58c0 Add Bun embed timeout cancel proof
d0e63e03af Use generated Nimbus program bundle in embed proof
58c6378713 Add Bun embed program bundle proof
34e71eec57 Add Bun embed async host-call proof
31334f9b6e Add Bun embed sync host-call proof
5385b59549 Add Bun JSC embed probe target
```

Touched Bun files:

| Path | Delta type | Decision relevance |
| --- | --- | --- |
| `scripts/build/bun.ts` | modified | Adds the opt-in native `check-bun-embed-probe` target and generated driver. Plausibly upstreamable later as an embedder test shape, but not yet a stable API. |
| `scripts/build/rust.ts` | modified | Parameterizes Rust archive emission for a non-`bun_bin` staticlib root. Plausibly upstreamable if Bun wants embedders. |
| `scripts/build/configure.ts` | modified | Wires the opt-in proof target into configuration. Proof-only. |
| `src/link_bridge/` | added/moved | Splits process-neutral C ABI link roots out of `bun_bin`. Plausibly upstreamable as build hygiene, but still motivated by the proof target. |
| `src/embed_probe/` | added | Nimbus-specific proof harness, generated fixture, and measurements. Not upstreamable as product code. |
| `Cargo.toml`, `Cargo.lock`, `src/bun_bin/*` | modified | Workspace plumbing for the proof target and link bridge. |

## What Is Upstreamable Later

Possible upstream proposal candidates after the remaining blockers are clearer:

- stable embeddable build target below `bun_bin`
- Rust archive emitter that supports non-CLI staticlib roots
- process-neutral link bridge for symbols that are not inherently CLI-owned
- documented owner-thread/API-lock requirements for JSC termination, promise
  driving, footprint shrinking, and VM teardown

These are build/API shape improvements, not enough to make Bun/JSC a Nimbus
runtime backend.

## What Is Not Upstreamable As-Is

- Nimbus-generated program-wrapper fixture
- Nimbus proof host-call functions
- permission inventory assertions
- memory and lifecycle measurement code
- Nimbus-specific product policy decisions

Those should stay in Nimbus proof documentation or a future Nimbus-owned
conformance harness.

## Fork Triggers

A Nimbus-maintained Bun fork becomes justified only if all of these become
true:

- Nimbus chooses to ship Bun/JSC as a product runtime backend.
- Permission hooks exist for Bun filesystem, network, environment,
  subprocess, worker, dynamic import, FFI/native-addon, and package-loading
  surfaces.
- A Nimbus-owned Bun package resolver policy exists and does not reuse
  Deno/V8 `node_external_packages` semantics blindly.
- Memory containment has an outer hard limit and a documented fresh/discard
  lifecycle policy.
- The required embeddable APIs cannot be consumed from upstream Bun or landed
  upstream in a usable form.
- The fork delta remains small enough to maintain across Bun, JSC/WebKit,
  native build graph, and security-update churn.
- CI can continuously prove the embed target on supported macOS and Linux
  platforms.

None of those trigger conditions are met today.

## Required CI Before Any Fork

Before a fork could become a dependency, Nimbus would need at least:

- Bun upstream-sync/rebase check
- macOS native `check-bun-embed-probe`
- Linux native `check-bun-embed-probe`
- proof target `cargo fmt --all --check`
- proof target build with the same debug/no-ASAN profile used here
- Nimbus-side runtime metadata and server rejection tests
- vulnerability/security-update tracking for Bun, JSC/WebKit, and native
  dependencies
- release artifact policy for generated/native Bun build products

## Decision Record

Status: hold local proof delta.

The current local Bun branch is valuable because it gives Nimbus executable
evidence for build/link, VM construction, host calls, generated-wrapper load,
timeout/cancel, permission inventory, memory behavior, package policy, and
lifecycle reuse. It is not a maintainable product fork yet. The right next
step is to close the proof plan with this decision, keep Bun/JSC rejected in
Nimbus product metadata, and revisit upstream/fork work only after a product
backend is explicitly chosen and the containment APIs are specified.
