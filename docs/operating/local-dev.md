# Local Development Build Contract

This document is the canonical reference for how a fresh clone of
`nimbus/nimbus` becomes a working build. The short version: **`make` is the
single entry point**; you should not have to know which Rust crate depends
on which JS package or which generator emits which artifact.

If you find yourself reading this because Cargo gave you an error about a
missing file under `packages/nimbus-ui/`, you are in the right place — keep
reading.

## Build contract

The repo's build is a heterogeneous graph that spans both toolchains:

- **Rust crates** in `crates/*` compile via Cargo.
- **JavaScript packages** in `packages/*` build via npm workspaces.
- **`nimbus-assets` and the Convex registry path** have compile-time
  dependencies on artifacts produced by a JS workspace:
  - `crates/nimbus-convex/src/registry/loading.rs` `include_str!`s and
    `include_bytes!`s files under `packages/nimbus-ui/.nimbus/convex/`
    (output of `convex codegen`).
  - `crates/nimbus-assets/src/ui.rs` embeds `packages/nimbus-ui/dist/`
    via `rust-embed`; `crates/nimbus-server/src/http/ui.rs` serves those
    bytes and owns route/auth behavior.

Make owns this cross-toolchain graph. The promise is:

> Run any `make` target and the prerequisites it needs — UI codegen, SPA
> build, anything else — are built on demand. You never need to run a
> targeted `npm` command yourself before running `make`.

The graph lives at the top of the `Makefile` (`UI_PKG`,
`UI_CODEGEN_SOURCES`, `UI_CODEGEN_OUTPUTS`, `UI_CODEGEN_SENTINEL`,
`UI_SPA_SOURCES`, `UI_DIST_INDEX`). Every Make target that compiles
`nimbus-server` or `nimbus-assets` lists `$(UI_DIST_INDEX)` as a prerequisite
so the SPA build (which itself depends on codegen) fires automatically when
the inputs change.

## Prerequisites

- **Node** 22+ (LTS). Required for `npm`, `convex codegen`, and the Vite
  SPA build — i.e. for any Rust target that touches `nimbus-server`.
- **Rust toolchain** as pinned in `rust-toolchain.toml`.
- **GNU Make** 3.81+ (the macOS system Make is fine; the build graph is
  intentionally portable to Make 3.81 and does not depend on grouped
  targets, `.RECIPEPREFIX`, or other newer features).

`nimbus-runtime` is the one Rust crate with zero workspace dependencies,
so `cargo test -p nimbus-runtime` works without Node. Everything else
needs Node available on the `PATH`.

## Common commands

```bash
make ci                   # full CI suite (clippy + tests + deny + JS + harness)
make check                # cargo check --workspace
make clippy               # workspace clippy with -D warnings
make test                 # cargo test --workspace
make build-ui             # only build the nimbus-ui dist (codegen + Vite)
make verify-desktop-ui    # browser smoke walk for the operator console
make release              # release-profile builds
```

All of these depend on the UI graph and will trigger codegen + SPA build
on demand if their inputs changed.

## Why Make orchestrates

`build.rs` is the canonical place to express *intra-crate* prebuild for a
single Rust crate, but it is poorly suited for orchestrating a
multi-process JS toolchain (npm install, codegen, Vite build) inside a
Cargo build. Doing so would make every `cargo` invocation own that work
even when invoked transitively from `rust-analyzer`, IDE save hooks, or
tests that don't need the SPA — and would smuggle Node into the build's
hot path.

Instead, the build contract uses the pattern Tauri / Tabby / Meilisearch
use: Make owns the cross-toolchain orchestration, `build.rs` honestly
asserts inputs exist (and errors actionably if they don't). See
`docs/plans/archive/local-dev-canonicalization-plan.md` (closed
2026-05-21) for the full discussion of alternatives and the rationale.

## Cargo-direct builds

Running `cargo build`, `cargo check`, or `cargo test` against
`nimbus-server` directly works **iff** the UI artifacts already exist on
disk. If they don't, `nimbus-assets/build.rs` returns:

```
nimbus-ui dist is missing — <path> does not exist.
Run any `make` target (e.g. `make build-ui`, `make check`, `make test`);
Make's dependency graph will build the SPA on demand. Cargo-direct
builds that compile nimbus-assets with the ui feature require dist to exist
beforehand.
```

This is intentional — the asset-crate `build.rs` does not run `npm` itself
(that would make Cargo own the JS toolchain), but it tells you exactly what
to run instead. The most common case is a fresh clone: run `make ci-required`
(or any other Make target) once and then continue with `cargo` as normal.
