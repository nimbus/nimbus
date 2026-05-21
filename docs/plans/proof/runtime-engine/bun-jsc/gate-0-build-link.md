# Bun/JSC Gate 0: Build And Link Reproducibility

Date: 2026-05-21

Nimbus revision: `41d88f6b` (`Add Bun JSC ignored build proof`)

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
brew install cmake
brew install ninja

bun scripts/build.ts --profile=ci-rust-only \
  --build-dir=/private/tmp/nimbus-bun-rust-only \
  --cache-dir=/private/tmp/nimbus-bun-cache \
  --target=clone-lolhtml \
  --target=codegen

CARGO_TARGET_DIR=/private/tmp/nimbus-bun-proof-target \
BUN_CODEGEN_DIR=/private/tmp/nimbus-bun-rust-only/codegen \
CARGO_ENCODED_RUSTFLAGS= \
cargo check -p bun_jsc --lib
```

The first sandboxed attempt failed because rustup needed to write under
`~/.rustup`; the command was rerun with approval. rustup installed/synced the
Bun-pinned nightly toolchain, then Cargo started dependency resolution. The
final `cargo check` uses `CARGO_ENCODED_RUSTFLAGS=` to clear Bun's generated
macOS `-fuse-ld=lld` cargo-config flag for this one-off verification. Without
that override, Homebrew clang rejected the linker flag before checking
`bun_jsc`.

## Result

Status: build/codegen/cargo-check gate passed.

The first direct `cargo check` failed while resolving `bun_lolhtml_sys`:

```text
error: failed to get `lol_html_c_api` as a dependency of package `bun_lolhtml_sys`
Caused by: unable to update /Users/jack/src/github.com/oven-sh/bun/vendor/lolhtml/c-api
Caused by: failed to read `/Users/jack/src/github.com/oven-sh/bun/vendor/lolhtml/c-api/Cargo.toml`
Caused by: No such file or directory
```

That failure was expected from a clean Bun worktree because Bun's rust-only
build graph fetches `vendor/lolhtml` before Cargo runs.

The canonical setup slice passed after installing missing local prerequisites:

```text
[build] clone-lolhtml, codegen done
```

It produced:

- `/Users/jack/src/github.com/oven-sh/bun/vendor/lolhtml/c-api/Cargo.toml`
- `/private/tmp/nimbus-bun-rust-only/codegen/generated_classes.rs`
- `/private/tmp/nimbus-bun-rust-only/codegen/generated_host_exports.rs`
- `/private/tmp/nimbus-bun-rust-only/codegen/cpp.rs`

The final cargo check passed:

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 22.04s
```

Observed upstream warnings:

- `bun_crash_handler`: 3 unnecessary `unsafe` warnings
- `bun_spawn`: 1 unused-label warning
- `bun_install`: 1 unused-label warning

Bun worktree status remained clean after the successful setup and check.

## Nimbus Proof Target

The build/codegen/cargo-check gate is captured as an ignored Nimbus integration
test:

```sh
cargo test -p nimbus-runtime --test engine_proofs \
  bun_jsc_build_gate_reproduces_from_bun_build_graph \
  -- --ignored --nocapture
```

Result:

```text
test bun_jsc::bun_jsc_build_gate_reproduces_from_bun_build_graph ... ok
test result: ok. 1 passed; 0 failed
```

Optional environment overrides:

- `NIMBUS_BUN_REPO`: Bun checkout path; defaults to
  `$HOME/src/github.com/oven-sh/bun`
- `NIMBUS_BUN_BUILD_DIR`: Bun rust-only build directory; defaults under
  `/private/tmp` when available
- `NIMBUS_BUN_CACHE_DIR`: Bun dependency cache directory; defaults under
  `/private/tmp` when available
- `NIMBUS_BUN_CARGO_TARGET_DIR`: Cargo target directory for the Bun check;
  defaults under `/private/tmp` when available

The test removes Cargo-test toolchain environment variables before spawning
the nested Bun `cargo check` so Bun's own `rust-toolchain.toml` selects the
pinned nightly.

This proof target intentionally stops at build/codegen/cargo-check
reproducibility. VM construction, host functions, invocation, cancellation,
and teardown remain separate gates.

## VM Construction Scout Notes

The likely next proof surface is below `bun_bin`:

- `src/jsc/VirtualMachine.rs::VirtualMachine::init(InitOptions)` allocates the
  per-thread VM, installs Bun's TLS VM pointer, initializes event-loop state,
  and creates the `JSGlobalObject` through `Zig__GlobalObject__create`.
- `InitOptions::is_main_thread = true` installs Bun's parent-death watchdog.
  The first Nimbus VM proof should keep this false unless the proof is
  explicitly testing process ownership behavior.
- `src/runtime/jsc_hooks.rs` provides `__BUN_RUNTIME_HOOKS`, the high-tier
  runtime hook table consumed by `bun_jsc`. A proof that needs Bun runtime
  state, timers, module generation, or preloads must link `bun_runtime`, not
  only `bun_jsc`.
- `src/jsc/JSModuleLoader.rs` exposes direct source/module evaluation helpers.
- `src/jsc/JSFunction.rs::JSFunction::create` is the likely sync host-function
  installation primitive.
- `src/jsc/VM.rs` exposes execution-time-limit and termination hooks that
  should become the timeout/cancel proof surface.

This scout does not prove VM construction yet. `cargo check -p bun_jsc --lib`
does not link a runnable binary against Bun's C++/WebKit symbols, and
`VirtualMachine::init` calls externs such as `Zig__GlobalObject__create`.
The next gate must prove the required C++/WebKit link inputs without falling
back to `bun_bin` as the runtime boundary.

## Canonical Setup Notes

Bun's build graph has a narrower setup path than a full debug binary build:

```sh
bun scripts/build.ts --profile=ci-rust-only \
  --build-dir=/private/tmp/nimbus-bun-rust-only \
  --cache-dir=/private/tmp/nimbus-bun-cache \
  --target=clone-lolhtml \
  --target=codegen
```

This is the intended next setup slice because `ci-rust-only` wires:

- `clone-lolhtml`, which fetches `vendor/lolhtml/.ref`
- `codegen`, which creates the `BUN_CODEGEN_DIR` inputs consumed by Rust
- the same rust-only dependency ordering used for CI's `libbun_rust.a`

The first setup attempt did not reach vendor fetch or codegen because `cmake`
was absent. Installing `cmake 4.3.2` satisfied Bun's `>= 3.24` requirement.
The next attempt configured the graph but failed because `ninja` was absent.
Installing `ninja 1.13.2` satisfied the build-driver requirement.

The setup command installed Bun's ignored `node_modules` dependencies and
fetched Bun's ignored `vendor/lolhtml` source as intended by the Bun build
graph. Those paths are generated inputs, not Nimbus progress state.

## Decision

The Bun/JSC backend remains proof-only and not selectable in Nimbus.

Gate 0 is satisfied for `bun_jsc` build/codegen/cargo-check reproducibility.
Before Bun can become a selectable Nimbus backend, the next proof must still
construct and tear down a VM below Bun's process-owned CLI path. That proof
must avoid:

- `bun_bin` as the runtime boundary
- Bun's process entry `main`
- Bun's global allocator entrypoint
- crash/signal/stdio/parent-death-watchdog setup from the CLI path
- `Cli::start()` and `Global::exit(0)`
- untracked local generated directories

## Follow-Up Evidence

The next VM-construction investigation is recorded in
`docs/plans/proof/runtime-engine/bun-jsc/gate-1-vm-construction.md`.

1. Add a Nimbus-side ignored proof target for VM construction below Bun's CLI
   path.
2. Identify the smallest Bun/JSC API that can construct a VM without
   `bun_bin`, `Cli::start()`, global allocator setup, or process exit.
3. Prove sync host-function installation, guest invocation, promise/event-loop
   progress, cancellation, and teardown before adding any selectable backend.
