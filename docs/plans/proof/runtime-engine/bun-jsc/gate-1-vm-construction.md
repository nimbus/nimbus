# Bun/JSC Gate 1: VM Construction Link Surface

Date: 2026-05-21

Nimbus revision: `41d88f6b` (`Add Bun JSC ignored build proof`)

Bun worktree: `/Users/jack/src/github.com/oven-sh/bun`

Bun revision: `0b20408b656f95aa347cb6c06eb03c14a20051cb`

Bun worktree status: clean at the start and end of the proof.

## Question

Can Nimbus prove Bun/JSC VM construction below Bun's process-owned CLI path by
using a normal Rust `bun_jsc` test or binary as the smallest linkable surface?

## Source Shape Reviewed

- `src/jsc/VirtualMachine.rs::VirtualMachine::init(InitOptions)` is the likely
  VM constructor below the CLI path. It allocates the per-thread
  `VirtualMachine`, installs Bun's TLS VM pointer, initializes event-loop
  state, calls high-tier `RuntimeHooks::init_runtime_state`, then creates the
  `JSGlobalObject` through `Zig__GlobalObject__create`.
- `InitOptions::is_main_thread = true` installs
  `ParentDeathWatchdog::install_on_event_loop`. A Nimbus embed proof should
  keep this false unless it is explicitly proving process ownership behavior.
- `src/runtime/jsc_hooks.rs` defines `__BUN_RUNTIME_HOOKS`, which `bun_jsc`
  reads through a link-time resolved extern. Any proof that needs timers,
  module generation, preloads, or other high-tier runtime state must link
  `bun_runtime`, not just `bun_jsc`.
- `src/jsc/VM.rs` records that direct VM construction through
  `JSC__VM__create` was removed; Bun's VM is created through
  `Zig::GlobalObject::create -> WebWorker__createVM`.
- `src/jsc/JSModuleLoader.rs` exposes source/module evaluation helpers.
- `src/jsc/JSFunction.rs::JSFunction::create` is the likely primitive for a
  sync host-function proof after VM construction is linkable.
- `scripts/build/rust.ts::emitRust` hard-codes `cargo build -p bun_bin --lib`
  as the Rust build product for Bun's native graph.
- `src/bun_bin/Cargo.toml` makes `bun_bin` a `staticlib`, and
  `src/bun_bin/lib.rs` owns `main`, global allocator installation, crash/signal
  setup, stdio setup, `Cli::start()`, and `Global::exit(0)`.
- `scripts/build/bun.ts::emitBun` links the final executable from C/C++
  objects, `libbun_rust.a`, dependency archives, WebKit/JSC libraries, system
  libraries, and smoke-tests that executable with `--revision`.

## Rust Test Link Probe

Baseline command:

```sh
CARGO_TARGET_DIR=/private/tmp/nimbus-bun-proof-target \
BUN_CODEGEN_DIR=/private/tmp/nimbus-bun-rust-only/codegen \
CARGO_ENCODED_RUSTFLAGS= \
cargo test -p bun_jsc --lib --no-run
```

Result: the probe did not reach the linker. Bun's workspace lint config denies
dead code for the `bun_jsc` lib test target, and the crate's compile-time macro
smoke type is intentionally never constructed:

```text
error: struct `Smoke` is never constructed
   --> src/jsc/lib.rs:978:16
    |
978 |     pub struct Smoke {
    |                ^^^^^
    |
    = note: requested on the command line with `-D dead-code`

error: associated items `constructor`, `get_n`, `set_n`, and `do_thing` are never used
```

Second command, only to get past that lint and observe the link surface:

```sh
CARGO_TARGET_DIR=/private/tmp/nimbus-bun-proof-target \
BUN_CODEGEN_DIR=/private/tmp/nimbus-bun-rust-only/codegen \
CARGO_ENCODED_RUSTFLAGS=-Adead_code \
cargo test -p bun_jsc --lib --no-run
```

Result: the probe reached the linker and failed before any Nimbus VM proof
could run:

```text
error: linking with `/opt/homebrew/opt/llvm@21/bin/clang++` failed: exit status: 1
...
= note: ld: warning: ignoring duplicate libraries: '-lkernel32', '-lntdll'
        ld: library 'ntdll' not found
        clang++: error: linker command failed with exit code 1
```

Observed upstream warnings before both failures:

- `bun_crash_handler`: 3 unnecessary `unsafe` warnings
- `bun_spawn`: 1 unused-label warning
- `bun_install`: 1 unused-label warning

## Bun Native Graph Probe

Command:

```sh
bun scripts/build.ts --profile=debug \
  --build-dir=/private/tmp/nimbus-bun-vm-link-probe \
  --cache-dir=/private/tmp/nimbus-bun-cache \
  --configure-only
```

Result:

```text
[configured] bun-debug
  target       darwin-aarch64
  build type   Debug
  build dir    ./../../../../../../private/tmp/nimbus-bun-vm-link-probe
  revision     0b20408b65
  features     asan, assertions, logs

21 deps, 95 codegen, 1178 objects in 923ms
run: ninja -C /private/tmp/nimbus-bun-vm-link-probe
```

Target inspection of the generated `build.ninja` found these relevant top-level
surfaces:

```text
build rust-target/aarch64-apple-darwin/debug/libbun_rust.a: rust_build_cross
build bun-rust: phony rust-target/aarch64-apple-darwin/debug/libbun_rust.a
build bun-debug: link obj/unified/UnifiedSource-packages_bun_usockets_src_crypto-0.cpp.o ...
build bun: phony bun-debug
build bun-debug.smoke-test-passed: smoke_test bun-debug
build check: phony bun-debug.smoke-test-passed
default bun check
```

The `bun-debug` link line includes Bun's unified C++ objects, generated C++
bindings, `libbun_rust.a`, WebKit/JSC archives, dependency archives, system
libraries, and the ASAN shim. The graph does not expose a smaller non-CLI
executable target that constructs `VirtualMachine` or `JSGlobalObject`.

## Decision

Status: standalone Rust VM-construction proof is not viable with the current
Bun repo shape.

A normal `cargo test -p bun_jsc --lib --no-run` is not the correct proof
surface because it bypasses Bun's native C++/WebKit link graph. Forcing it past
the local dead-code lint reaches a Cargo-driven test link that is already
wrong-shaped on macOS: it pulls Windows libraries such as `ntdll` and
`kernel32` into the link line before any VM-construction code can be tested.

Bun's canonical native graph currently exposes:

- `bun-rust` / `libbun_rust.a`: a staticlib rooted at `bun_bin`
- `bun-debug`: the full process-owned CLI executable
- `check`: a smoke test of that full executable

It does not expose a current embeddable VM-construction target below `bun_bin`.
Therefore, Nimbus should not add a selectable Bun/JSC backend yet, and should
not treat `bun_jsc` Cargo test linkage as proof that an in-process Bun runtime
can be embedded safely.

## Next Required Evidence

The follow-up Rust staticlib-root scout is recorded in
`docs/plans/proof/runtime-engine/bun-jsc/gate-2-embed-staticlib-probe.md`.

1. Identify or create an upstream Bun-side embeddable link target that is not
   rooted at `src/bun_bin/lib.rs::main`.
2. That target must link the same C++/WebKit/JSC inputs needed by
   `VirtualMachine::init` and `Zig__GlobalObject__create`, but avoid Bun's
   process entry, global allocator entrypoint, crash/signal setup, stdio setup,
   parent-death watchdog defaulting, `Cli::start()`, and `Global::exit(0)`.
3. Once such a target exists, add an ignored Nimbus proof that constructs and
   tears down a Bun/JSC VM with `InitOptions::is_main_thread = false`.
4. Only after VM construction links and runs should the proof advance to sync
   host functions, guest invocation, promise/event-loop progress, cancellation,
   permission behavior, bundle loading, runtime-extension parity, and teardown
   or reuse semantics.
