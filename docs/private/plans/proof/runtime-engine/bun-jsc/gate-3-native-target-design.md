# Bun/JSC Gate 3: Native Embed Target Design

Date: 2026-05-21

Superseded by:
`docs/plans/proof/runtime-engine/bun-jsc/gate-4-native-vm-run.md`

Nimbus revision: `e2cd715f` (`Record Bun embed staticlib probe`)

Bun worktree: `/Users/jack/src/github.com/oven-sh/bun`

Bun revision: `0b20408b656f95aa347cb6c06eb03c14a20051cb`

Bun worktree status: clean during source review.

## Question

What is the smallest Bun-side build target that could turn the Gate 2
non-CLI Rust staticlib root into a runnable VM-construction proof?

## Build Graph Findings

- `scripts/build.ts` treats `--target=<name>` as a Ninja target selector, not
  a build-mode selector. That is useful for proof work: a new
  non-default `check-bun-embed-probe` Ninja target can be added without making
  it part of normal `bun` or `check` defaults.
- `scripts/build/configure.ts` always calls `emitBun(n, cfg, sources)`, then
  installs default targets only from `output.exe`. A proof target can be
  emitted as an extra phony inside `emitBun` and remain opt-in.
- `scripts/build/profiles.ts` only has `BuildMode` values for `full`,
  `cpp-only`, `rust-only`, and `link-only`. A proof target does not need a new
  mode for the first experiment; building it as an explicit Ninja target
  avoids perturbing CI split modes.
- `scripts/build/bun.ts::emitBun` is the composition point that already has
  every native input the probe needs: resolved dependencies, codegen outputs,
  generated C++ sources, full C/C++ object list, WebKit/JSC libs, system
  libraries, shim link flags, and the generic `link()` helper.
- `scripts/build/rust.ts::emitRust` is still hard-coded to
  `cargo build -p bun_bin --lib`, `rustLibPath(cfg)`, `libbun_rust.a`, and the
  `bun-rust` phony. A non-CLI proof target needs a parameterized Rust archive
  emitter or a sibling `emitRustArchive` helper.
- `scripts/build/compile.ts::link` is already generic enough to link another
  executable once it receives objects, libs, and flags.
- `scripts/glob-sources.ts` tracks `src/**/*.rs`, `src/**/Cargo.toml`,
  `Cargo.toml`, `Cargo.lock`, and `rust-toolchain.toml` as Rust build
  invalidation inputs. A new workspace crate under `src/` would be tracked
  automatically.

## Link-Root Finding

Gate 2 proved that a separate Rust staticlib root can compile against
`bun_core`, `bun_jsc`, and `bun_runtime`, but the final native link is likely
to need a small process-neutral link bridge currently owned by `bun_bin`.

`src/bun_bin/lib.rs` owns forbidden process-entry behavior:

- `#[global_allocator]`
- ASAN/LSAN process-level defaults
- `#[no_mangle] extern "C" fn main(...)`
- `bun_crash_handler::init()`
- `libc::signal(...)`
- Windows libuv allocator/environment setup
- `output::stdio::init()`
- `ParentDeathWatchdog::install()`
- `bun_runtime::cli::Cli::start()`
- `Global::exit(0)`

The same file also force-links `bun_platform`:

```rust
use bun_platform as _;
```

That force-link is not inherently a CLI entry concern; it exists so
`bun_platform`'s `#[no_mangle]` C exports reach the native linker.

`src/bun_bin/phase_c_exports.rs` also still owns seven `#[no_mangle]` C ABI
definitions or placeholders:

```text
Bun__panic
Bun__VM__scriptExecutionStatus
JSC__JSValue__parseJSON
BunString__toErrorInstance
Bun__LifecycleAgentPreventExit
Bun__LifecycleAgentStopPreventingExit
DNSResolver__getConstructor
```

Most other prior bridge symbols have already moved to `bun_jsc`,
`bun_runtime`, `bun_http_jsc`, or `bun_bundler_jsc`, but these remaining
symbols are still tied to the process-owned `bun_bin` crate. A native embed
target that links Bun's normal C++ object graph without `bun_bin` may fail on
these symbols unless they are moved to, or re-owned by, a non-process link
bridge.

## Proposed Bun-Side Shape

1. Add a process-neutral Rust crate under the Bun workspace, for example
   `src/embed_probe`, with `crate-type = ["staticlib"]`.
2. The crate should depend on `bun_core`, `bun_jsc`, `bun_runtime`, and any
   process-neutral link bridge crate needed for the remaining C ABI exports.
3. Move the process-neutral parts of `src/bun_bin/phase_c_exports.rs` and the
   `bun_platform as _` force-link into a shared crate or module that both
   `bun_bin` and the embed probe can include. Keep `main`, allocator
   selection, crash/signal setup, stdio setup, parent-death watchdog process
   install, `Cli::start()`, and `Global::exit(0)` in `bun_bin`.
4. Parameterize `scripts/build/rust.ts::emitRust` into an archive emitter that
   accepts at least:
   - Cargo package name
   - archive base name
   - phony target name
   - build label
   - optional extra Cargo args or environment
5. Keep the existing `bun_bin` call path producing `libbun_rust.a` unchanged.
6. Add a proof-only Rust archive target such as
   `libnimbus_bun_embed_probe.a` that builds the new probe crate with the same
   codegen, vendor, toolchain, target triple, and Rust flags as the normal Bun
   Rust archive.
7. Generate or add a tiny C++ driver that is not part of the normal Bun C++
   source glob:

   ```cpp
   extern "C" int nimbus_bun_embed_probe_construct_and_destroy_vm();

   int main() {
     return nimbus_bun_embed_probe_construct_and_destroy_vm();
   }
   ```

   A generated file under `cfg.buildDir` is safer than a source file under an
   existing globbed `src/jsc/...` directory because the normal `bun-debug`
   executable must not compile a second `main`.
8. In `emitBun`, after `allObjects`, `depLibs`, flags, and shims are known,
   emit an opt-in target that links:
   - the generated probe driver object
   - Bun's existing `allObjects` C/C++ object list
   - the probe Rust staticlib, not `libbun_rust.a`
   - Windows resources only if the platform requires them
   - the same dependency libraries, system libraries, manifest flags,
     shims, and link implicit inputs as the full Bun executable
9. Add a probe-specific smoke test that runs the executable with no arguments
   and checks exit status 0. The existing `emitSmokeTest` always runs
   `<exe> --revision`, so it should either be generalized or left untouched
   while the proof target gets its own `run_probe` rule.
10. Expose phony targets such as:
    - `bun-embed-probe`
    - `check-bun-embed-probe`

## First Verification Command

Use a no-ASAN debug build first to reduce process-level sanitizer variables in
the initial proof:

```sh
bun scripts/build.ts --profile=debug-no-asan \
  --build-dir=/private/tmp/nimbus-bun-embed-native \
  --cache-dir=/private/tmp/nimbus-bun-cache \
  --target=check-bun-embed-probe
```

Expected success criteria:

- Ninja builds the probe Rust archive from the new non-CLI package.
- Ninja does not build or link `libbun_rust.a` as an input to the probe
  executable.
- The probe executable final-links against Bun's C++/WebKit/JSC graph.
- The generated driver calls the exported Rust C ABI function.
- `VirtualMachine::init` and `VirtualMachine::destroy` execute and the process
  exits 0.
- Bun's worktree remains clean except for the intentional proof-target patch.

Expected failure value:

- Missing-symbol failures identify which link roots remain incorrectly owned
  by `bun_bin`.
- Runtime crashes identify which process setup steps are actually required for
  VM construction and therefore must be split into an embed-safe initializer
  rather than inherited from the CLI entry path.

## Decision

Status: target design implemented and verified locally in
`docs/plans/proof/runtime-engine/bun-jsc/gate-4-native-vm-run.md`.

The next proof should be a Bun-side patch, not a Nimbus production backend.
The key design constraint is separating process-neutral native link roots from
`bun_bin` before using the normal Bun C++/WebKit/JSC link graph. Gate 4 proved
that this condition can be satisfied locally. Bun/JSC remains proof-only until
host-call transport, async progress, guest invocation, cancellation,
permissions, teardown/reuse, and artifact routing gates pass.
