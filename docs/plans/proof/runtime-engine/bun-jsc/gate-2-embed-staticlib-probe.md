# Bun/JSC Gate 2: Embed Staticlib Root Probe

Date: 2026-05-21

Nimbus revision: `fe9b4c03` (`Record Bun JSC VM link probe`)

Bun worktree: `/Users/jack/src/github.com/oven-sh/bun`

Bun revision: `0b20408b656f95aa347cb6c06eb03c14a20051cb`

Bun worktree status: clean at the start and end of the proof.

## Question

Can a non-CLI Rust crate root depend on Bun's Rust libraries and compile a
staticlib that reaches `VirtualMachine::init` and `VirtualMachine::destroy`
without rooting the Rust archive at `src/bun_bin/lib.rs`?

## Temporary Probe

The probe was created outside both repositories:

- manifest: `/private/tmp/nimbus-bun-embed-probe/Cargo.toml`
- source: `/private/tmp/nimbus-bun-embed-probe/src/lib.rs`
- target dir: `/private/tmp/nimbus-bun-embed-probe-target`

Manifest shape:

```toml
[package]
name = "nimbus_bun_embed_probe"
version = "0.0.0"
edition = "2024"

[lib]
path = "src/lib.rs"
crate-type = ["staticlib", "rlib"]

[dependencies]
bun_core = { path = "/Users/jack/src/github.com/oven-sh/bun/src/bun_core" }
bun_jsc = { path = "/Users/jack/src/github.com/oven-sh/bun/src/jsc" }
bun_runtime = { path = "/Users/jack/src/github.com/oven-sh/bun/src/runtime", default-features = false }

[profile.dev]
panic = "abort"
```

Probe source:

```rust
use bun_jsc::virtual_machine::{InitOptions, VirtualMachine};

#[unsafe(no_mangle)]
pub extern "C" fn nimbus_bun_embed_probe_construct_and_destroy_vm() -> i32 {
    bun_core::output::init_test();

    // Touch the high-tier runtime crate so __BUN_RUNTIME_HOOKS is owned by this
    // staticlib root instead of depending on Bun's process-owned bun_bin crate.
    let _ = bun_runtime::jsc_hooks::runtime_state();

    let opts = InitOptions {
        is_main_thread: false,
        ..Default::default()
    };

    match VirtualMachine::init(opts) {
        Ok(vm) => {
            unsafe { (&mut *vm).destroy() };
            0
        }
        Err(_) => 1,
    }
}
```

The probe intentionally avoids `bun_bin`, Bun's `main`, `Cli::start()`,
`Global::exit(0)`, process stdio setup, crash/signal setup, and the parent
death watchdog default.

## Toolchain Finding

The first direct run was launched outside the Bun workspace, so rustup did not
apply Bun's `rust-toolchain.toml`. Cargo selected the host stable toolchain
and failed before the probe could test the embedding shape:

```text
error[E0554]: `#![feature]` may not be used on the stable release channel
  --> /Users/jack/src/github.com/oven-sh/bun/src/wyhash/lib.rs:16:1
   |
16 | #![feature(hasher_prefixfree_extras)]
```

Any out-of-worktree Bun embed proof must set `RUSTUP_TOOLCHAIN` explicitly or
place the proof under the Bun workspace so the pinned toolchain is applied.
The current pinned channel is `nightly-2026-05-06`.

## Check Command

```sh
RUSTUP_TOOLCHAIN=nightly-2026-05-06 \
CARGO_TARGET_DIR=/private/tmp/nimbus-bun-embed-probe-target \
BUN_CODEGEN_DIR=/private/tmp/nimbus-bun-rust-only/codegen \
CARGO_ENCODED_RUSTFLAGS= \
cargo check --manifest-path /private/tmp/nimbus-bun-embed-probe/Cargo.toml --lib
```

Result:

```text
Checking nimbus_bun_embed_probe v0.0.0 (/private/tmp/nimbus-bun-embed-probe)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 33.90s
```

Observed upstream warnings:

- `bun_crash_handler`: 3 unnecessary `unsafe` warnings
- `bun_spawn`: 1 unused-label warning
- `bun_install`: 1 unused-label warning
- `bun_runtime`: 2 unnecessary `unsafe` warnings

## Archive Build Command

```sh
RUSTUP_TOOLCHAIN=nightly-2026-05-06 \
CARGO_TARGET_DIR=/private/tmp/nimbus-bun-embed-probe-target \
BUN_CODEGEN_DIR=/private/tmp/nimbus-bun-rust-only/codegen \
CARGO_ENCODED_RUSTFLAGS= \
cargo build --manifest-path /private/tmp/nimbus-bun-embed-probe/Cargo.toml --lib
```

Result:

```text
Compiling nimbus_bun_embed_probe v0.0.0 (/private/tmp/nimbus-bun-embed-probe)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 50.11s
```

Artifacts:

```text
-rw-r--r--  643M /private/tmp/nimbus-bun-embed-probe-target/debug/libnimbus_bun_embed_probe.a
-rw-r--r--  854K /private/tmp/nimbus-bun-embed-probe-target/debug/libnimbus_bun_embed_probe.rlib
```

Symbol inspection found the exported C ABI probe function:

```sh
nm -gU /private/tmp/nimbus-bun-embed-probe-target/debug/libnimbus_bun_embed_probe.a \
  | rg "_nimbus_bun_embed_probe_construct_and_destroy_vm"
```

Result:

```text
0000000000000000 T _nimbus_bun_embed_probe_construct_and_destroy_vm
```

Apple's `nm` also emitted `Unknown attribute kind` warnings while reading some
Rust nightly object files produced by LLVM 22.1.4. The symbol was still
reported, and this warning did not affect the Cargo build result.

## Decision

Status: Rust staticlib root probe passed; runnable VM construction is still
blocked.

This proof establishes that `bun_bin` is not the only possible Rust archive
root. A separate crate can depend on `bun_core`, `bun_jsc`, and `bun_runtime`,
own `__BUN_RUNTIME_HOOKS` through `bun_runtime::jsc_hooks::runtime_state()`,
call `VirtualMachine::init` with `InitOptions::is_main_thread = false`, call
`VirtualMachine::destroy`, and build as a staticlib without importing Bun's
process-owned CLI entry path.

This proof does not establish that Nimbus can embed Bun/JSC yet. A Rust
staticlib archive does not final-link or execute Bun's C++/WebKit/JSC graph,
and it does not prove that `Zig__GlobalObject__create`,
`WebWorker__createVM`, timers, module loading, or event-loop progress can run
inside Nimbus. The current Bun build graph still exposes the full CLI native
link target, but not an embeddable native target that links this kind of Rust
root against the same C++/WebKit/JSC inputs.

## Next Required Evidence

1. Add or identify a Bun-side native build target that links a non-CLI Rust
   staticlib root like this probe with Bun's required C++/WebKit/JSC inputs.
2. The target should produce a runnable VM-construction proof binary or an
   embeddable dynamic/static artifact with a small exported C ABI.
3. The proof must call the exported function and show that
   `VirtualMachine::init` plus `VirtualMachine::destroy` execute successfully,
   not merely compile.
4. Only after that passes should Nimbus advance to sync host functions, async
   host calls, guest invocation, timeout/cancel, permission behavior, bundle
   loading, runtime-extension parity, and teardown/reuse semantics.
