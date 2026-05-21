# Bun/JSC Gate 0: Build And Link Reproducibility

Date: 2026-05-21

Nimbus revision: `d7cdf9f2` (`Define runtime engine seam gates`)

Bun worktree: `/Users/jack/src/github.com/oven-sh/bun`

Bun revision: `0b20408b656f95aa347cb6c06eb03c14a20051cb`

Bun worktree status: clean at the start of the proof.

## Question

Can Nimbus start an in-process Bun/JSC proof from the local Bun Rust port
without using Bun's process-owned CLI/staticlib path or relying on untracked
generated/vendor state?

## Source Shape Reviewed

- `src/bun_bin/Cargo.toml` defines `bun_bin` as a `staticlib` named
  `bun_rust`, not as an embeddable runtime crate.
- `src/bun_bin/lib.rs` owns process entry behavior: global allocator setup,
  crash handler initialization, signal handling, stdio initialization,
  parent-death watchdog installation, `bun_runtime::cli::Cli::start()`, and
  `Global::exit(0)`.
- `src/runtime/Cargo.toml` exposes `bun_runtime` as a library crate, but pulls
  in broad Bun subsystems and generated/runtime JSC surfaces.
- `src/jsc/Cargo.toml` exposes `bun_jsc` as a library crate, but it is not a
  small standalone embedding boundary.
- `src/runtime/generated_host_exports.rs`,
  `src/runtime/generated_classes.rs`, and `src/jsc/cpp.rs` include generated
  files via `BUN_CODEGEN_DIR`.

## Command

```sh
CARGO_TARGET_DIR=/private/tmp/nimbus-bun-proof-target cargo check -p bun_jsc --lib
```

The first sandboxed attempt failed because rustup needed to write under
`~/.rustup`; the command was rerun with approval. rustup installed/synced the
Bun-pinned nightly toolchain, then Cargo started dependency resolution.

## Result

Status: blocked before link/VM proof.

Cargo failed while resolving `bun_lolhtml_sys`:

```text
error: failed to get `lol_html_c_api` as a dependency of package `bun_lolhtml_sys`
Caused by: unable to update /Users/jack/src/github.com/oven-sh/bun/vendor/lolhtml/c-api
Caused by: failed to read `/Users/jack/src/github.com/oven-sh/bun/vendor/lolhtml/c-api/Cargo.toml`
Caused by: No such file or directory
```

## Decision

The Bun/JSC backend remains proof-only and not selectable in Nimbus.

Gate 0 is not satisfied yet. Before testing VM construction, host calls,
bundle loading, cancellation, permissions, or teardown, the Bun proof needs a
documented reproducible setup step for required vendor/generated artifacts.
That setup must still avoid:

- `bun_bin` as the runtime boundary
- Bun's process entry `main`
- Bun's global allocator entrypoint
- crash/signal/stdio/parent-death-watchdog setup from the CLI path
- `Cli::start()` and `Global::exit(0)`
- untracked local generated directories

## Next Required Evidence

1. Document and run the Bun repository's canonical vendor/codegen setup for
   `vendor/lolhtml/c-api` and `BUN_CODEGEN_DIR`.
2. Re-run `cargo check -p bun_jsc --lib` with all generated/vendor inputs
   reproducible from the clean Bun worktree.
3. Only after that check passes, add a Nimbus-side ignored proof target for
   VM construction below the Bun CLI path.
