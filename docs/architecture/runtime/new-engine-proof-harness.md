# New Runtime Engine Proof Harness

Status: required gate for non-V8 engines

This document defines the proof harness required before Nimbus adds a
selectable runtime engine beyond the current Deno/V8 implementation. The proof
harness is intentionally non-production: it may live behind ignored tests,
Cargo features, or local proof scripts, but it must not change production
defaults or let an experimental engine masquerade as an existing Node target.

The harness is evidence for the runtime engine seam in
[Runtime Engine Seam](engine-seam.md). It is not a shortcut around that seam.

## Location And Isolation

A new engine proof should live under the owning runtime crate, but outside the
default runtime path. Use one of these shapes:

- `crates/nimbus-runtime/tests/engine_proofs/<engine>.rs` for integration
  proofs that can run as ignored tests.
- `crates/nimbus-runtime/benches/engine_proofs/<engine>.rs` for long-running
  lifecycle or stress proofs that need custom harness control.
- `docs/plans/proof/runtime-engine/<engine>/` for captured reports, command
  output summaries, and screenshots if the proof has external build steps.

Proof code must be guarded so normal `cargo test`, `make test`, and production
builds keep using Deno/V8 only. Prefer both:

- a Cargo feature such as `experimental-bun-jsc-proof` or
  `experimental-wasmtime-proof`
- `#[ignore]` on tests that require local engine source, native artifacts, or
  long-running stress loops

The proof may introduce engine-specific dependencies only behind that feature.
It must not add a production manifest target, server lane, generated artifact
selector, or operator-visible runtime option until the promotion gates are
satisfied.

## Required Evidence

Every new engine proof must produce a short report, either as test output or as
a checked-in summary under `docs/plans/proof/runtime-engine/<engine>/`, with
the exact command, date, local engine revision if applicable, and result for
each check below.

| Check | Required evidence |
| --- | --- |
| Build and link | The proof target compiles from a clean Nimbus workspace using documented commands. Native/generated engine artifacts are reproduced or explicitly vendored; no untracked local directory is required. |
| VM construction | Nimbus constructs the engine below the CLI/process entrypoint and can create and drop a VM without taking process-global ownership. |
| Sync host call | Guest code calls a sync host function backed by `HostBridge`, including ABI version validation and operation/payload mismatch rejection. |
| Async host call | Guest code awaits an async host function backed by `HostBridge`; the proof records cancellation behavior and `SharedInvocationPermit` pause/resume behavior or a documented equivalent. |
| Bundle load | The engine loads a Nimbus bundle by explicit content kind and engine config, invokes the entrypoint, settles promises or guest calls, and returns a JSON-compatible value. |
| Runtime extension call | The provider-neutral runtime-extension lane is transported without hard-coding adapter namespaces into `nimbus-runtime`. |
| Timeout and cancel | Timeouts and external cancellation interrupt guest execution. The proof states whether the VM remains reusable after termination. |
| Memory behavior | The proof records the engine memory limit mechanism, or states that the first safe policy is discard-on-pressure/fresh-per-invocation. |
| Permission policy | Every host-sensitive builtin exposed by the compatibility target is denied, wrapped by Nimbus grants, or the backend is marked trusted/sandbox-only. |
| Reuse and teardown | The proof runs create/invoke/cancel/drop loops and records whether retained VM reuse is safe. If reuse is not proven, the backend must start fresh-per-invocation. |
| Artifact metadata | Generated artifact fields name the engine, bundle content kind, compatibility target, and package resolver explicitly. |
| Server routing | Registry loading rejects unsupported engine/content/target combinations before invocation. |

Passing only build, link, and simple JavaScript evaluation is not enough.

## Bun/JSC-Specific Gate

A Bun/JSC proof must avoid Bun's process-owning binary path. The proof is not
valid if it depends on:

- `bun_bin` as the runtime boundary
- Bun's global allocator entrypoint
- crash or signal handler installation
- stdio or parent-death watchdog setup from the CLI path
- Bun code paths that call process exit during normal VM teardown
- untracked generated host export directories or ad hoc local build products

The expected proof surface starts below those process entrypoints, using the
minimal VM/JSC APIs needed to construct a VM, install host functions, load a
bundle, drive promises, cancel execution, and tear down safely. The proof must
document the exact Bun source revision and the commands that reproduce any
required generated Rust/Zig/native artifacts.

For package resolution, Bun/JSC must have its own explicit resolver lane. Do
not route Bun-backed functions through `node_external_packages`, and do not
label a Bun-backed target as `Node22` unless the manifest also names the Bun
engine and the measured compatibility target separately.

## Wasmtime-Specific Gate

A wasmtime proof is a different guest ABI, not a JavaScript compatibility
target. It must prove:

- component or module load keyed by bundle hash and engine config
- typed imports for host operations
- fuel or epoch interruption for timeout/cancel
- Store lifecycle and resource limiter behavior
- memory/table/resource limits
- JSON or typed value conversion at the Nimbus invocation boundary

wasmtime artifacts should use a distinct bundle content kind such as
`wasm_component`; they must not reuse JavaScript bundle fields or Node target
names.

## Promotion Decision

An engine can move from proof to selectable experimental backend only when:

- all required evidence is recorded
- unsupported runtime policy combinations are rejected at construction or
  registry load
- generated artifacts and operator metadata name the engine honestly
- permission behavior is complete, or the backend is explicitly
  trusted/sandbox-only
- reuse semantics are proven, or the production path is fresh-per-invocation
- the proof has focused tests that can be rerun by another developer

If permissions, cancellation, teardown, or reproducible link behavior fails,
the result is still useful evidence, but the backend remains proof-only.
