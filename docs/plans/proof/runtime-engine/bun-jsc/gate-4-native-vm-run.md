# Bun/JSC Gate 4: Native VM Run Probe

Date: 2026-05-21

Superseded by:
`docs/plans/proof/runtime-engine/bun-jsc/gate-5-sync-host-call.md`

Nimbus revision: `92e75afb` (`Document Bun embed native target design`)

Bun worktree: `/Users/jack/src/github.com/oven-sh/bun`

Bun base revision: `0b20408b656f95aa347cb6c06eb03c14a20051cb`

Bun proof commit: `ead332f17f` (`Add Bun JSC embed probe target`)

Bun patch status: committed locally on Bun `main`, not upstreamed.

## Question

Can Bun's native build graph link and execute a non-CLI Rust staticlib root
that constructs and destroys a `bun_jsc::VirtualMachine` without depending on
`bun_bin`, Bun's Rust `main`, allocator entrypoint, crash/signal setup,
stdio setup, CLI dispatch, or process exit?

## Patch Shape

The local Bun patch adds a proof-only native target and separates the remaining
process-neutral link roots from `bun_bin`.

Touched files:

- `Cargo.toml`
- `Cargo.lock`
- `scripts/build/bun.ts`
- `scripts/build/rust.ts`
- `src/bun_bin/Cargo.toml`
- `src/bun_bin/lib.rs`
- `src/bun_bin/phase_c_exports.rs`
- `src/link_bridge/Cargo.toml`
- `src/link_bridge/lib.rs`
- `src/embed_probe/Cargo.toml`
- `src/embed_probe/lib.rs`

`src/link_bridge` is a process-neutral Rust crate. It owns the remaining C ABI
bridge symbols that had been in `src/bun_bin/phase_c_exports.rs`, and it
force-links `bun_platform` so Bun's platform C exports reach any Rust staticlib
root that needs the native object graph. `bun_bin` now depends on
`bun_link_bridge` instead of owning those symbols directly.

`src/embed_probe` is a non-CLI Rust staticlib root. Its exported C ABI function
does the minimum pre-VM setup found during the proof:

```rust
bun_core::output::init_test();
bun_runtime::allocators::register_safety_vtables();
bun_jsc::initialize(false);
let _ = bun_runtime::jsc_hooks::runtime_state();
```

It then calls `VirtualMachine::init` with `InitOptions::is_main_thread = false`
and immediately calls `VirtualMachine::destroy` on success.

`scripts/build/rust.ts` now exposes `emitRustArchive(...)`, parameterized by
Cargo package name, archive base name, phony target name, and optional Windows
shim inclusion. The existing `emitRust(...)` path still emits
`cargo build -p bun_bin --lib` and `libbun_rust.a`.

`scripts/build/bun.ts` emits an opt-in `check-bun-embed-probe` target. The
target generates a tiny C++ driver under the build directory, compiles it, links
`bun-embed-probe` against Bun's existing C++/WebKit/JSC object graph plus
`libbun_embed_probe.a`, and runs the executable as a smoke test. It is not part
of the normal `bun` target.

## Initial Failures

The first native target build configured successfully, but failed because the
patch emitted both an executable and a phony target named `bun-embed-probe`:

```text
error: Duplicate build output: bun-embed-probe
```

Removing the redundant phony let Ninja build and link the executable.

The next run final-linked and executed the driver, then crashed during
`VirtualMachine::init`:

```text
ASSERTION FAILED: g_jscConfig.options.allowUnfinalizedAccess || g_jscConfig.options.isFinalized
JavaScriptCore/Options.h(145) : static OptionsStorage::Bool &JSC::Options::forceTrapAwareStackChecks()
1   JSC::VMTraps::VMTraps()
2   JSC::VM::VM(JSC::VM::VMType, JSC::HeapType, WTF::RunLoop*, bool*)
3   JSC::VM::tryCreate(JSC::HeapType, WTF::RunLoop*)
4   Zig__GlobalObject__create
5   bun_jsc::virtual_machine::VirtualMachine::init
6   nimbus_bun_embed_probe_construct_and_destroy_vm
7   main
```

This proved the target was linked and executing the probe function. The blocker
was not a missing symbol; it was that the embed root reached VM creation before
Bun finalized JSC options.

Source review found the existing process-neutral initializer:

- `src/jsc/bindings/ZigGlobalObject.cpp::JSCInitialize(...)`
- `src/jsc/lib.rs::initialize(eval_mode: bool)`
- CLI callers invoke `bun_jsc::initialize(...)` before `VirtualMachine::init`

Adding `bun_jsc::initialize(false)` to the probe matched that contract and
removed the JSC options assertion.

## Verification

Formatting:

```sh
cargo fmt --all
```

Result: passed.

Focused Rust check:

```sh
RUSTUP_TOOLCHAIN=nightly-2026-05-06 \
CARGO_TARGET_DIR=/private/tmp/nimbus-bun-native-proof-target \
BUN_CODEGEN_DIR=/private/tmp/nimbus-bun-rust-only/codegen \
CARGO_ENCODED_RUSTFLAGS= \
cargo check -p bun_link_bridge -p bun_embed_probe --lib
```

Result:

```text
Checking bun_embed_probe v0.0.0 (/Users/jack/src/github.com/oven-sh/bun/src/embed_probe)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.84s
```

Observed upstream warnings:

- `bun_crash_handler`: 3 unnecessary `unsafe` warnings
- `bun_spawn`: 1 unused-label warning
- `bun_install`: 1 unused-label warning
- `bun_runtime`: 2 unnecessary `unsafe` warnings

Native proof target:

```sh
bun scripts/build.ts --profile=debug-no-asan \
  --build-dir=/private/tmp/nimbus-bun-embed-native \
  --cache-dir=/private/tmp/nimbus-bun-cache \
  --target=check-bun-embed-probe
```

Result:

```text
[configured] bun-debug in 760ms (unchanged)
ninja: Entering directory `/private/tmp/nimbus-bun-embed-native'
[0/3] cargo bun_embed_probe -> libbun_embed_probe.a (--target aarch64-apple-darwin)
Compiling bun_embed_probe v0.0.0 (/Users/jack/src/github.com/oven-sh/bun/src/embed_probe)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.45s
[1/3] link bun-embed-probe
[2/3] bun-embed-probe
[build] check-bun-embed-probe done
```

Whitespace check:

```sh
git diff --check
```

Result: passed.

Build graph check:

```sh
rg -n "bun-embed-probe|libbun_embed_probe|libbun_rust" \
  /private/tmp/nimbus-bun-embed-native/build.ninja
```

Relevant result:

```text
build rust-target/aarch64-apple-darwin/debug/libbun_embed_probe.a: rust_build_cross
build bun-embed-probe-rust: phony rust-target/aarch64-apple-darwin/debug/libbun_embed_probe.a
build bun-embed-probe: link obj/embed-probe/driver.cpp.o ... rust-target/aarch64-apple-darwin/debug/libbun_embed_probe.a ...
build bun-embed-probe.smoke-test-passed: embed_probe_smoke_test bun-embed-probe
build check-bun-embed-probe: phony bun-embed-probe.smoke-test-passed
```

The generated graph still has the normal `libbun_rust.a` and `bun` targets,
but the proof executable links `libbun_embed_probe.a`, not `libbun_rust.a`.

## Decision

Status: native non-CLI VM construction/destruction proof passed.

This changes the Bun/JSC feasibility picture. The proof now shows that Bun's
Rust port can expose a non-CLI staticlib root that final-links with Bun's
C++/WebKit/JSC graph and constructs/destroys a JSC VM in process without
linking through `bun_bin`.

This still does not make Bun/JSC production-ready for Nimbus. The next gates
must prove:

1. a stable embed initializer contract rather than a proof-local sequence,
2. sync host functions through a Nimbus-owned C/Rust ABI,
3. async host calls and event-loop progress,
4. guest bundle/module loading without Bun CLI entry assumptions,
5. timeout/cancel semantics,
6. permission and filesystem/network policy containment,
7. teardown and either safe reuse or fresh-VM-per-invocation semantics,
8. artifact metadata and server routing that keep Bun/JSC explicit.

The next recommended gate was a host-call proof built on the same non-CLI
target shape. Gate 5 completed that sync host-call proof locally. Bun/JSC
remains proof-only until async host calls, event-loop progress, guest
bundle/module loading, cancellation, permissions, teardown/reuse, and artifact
routing gates pass.
