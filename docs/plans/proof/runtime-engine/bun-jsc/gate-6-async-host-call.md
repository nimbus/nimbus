# Bun/JSC Gate 6: Async Host-Call And Event-Loop Probe

Date: 2026-05-21

Nimbus revision: `a168cb77` (`Record Bun sync host-call proof`)

Bun worktree: `/Users/jack/src/github.com/oven-sh/bun`

Bun prior proof commit: `52788ee03b` (`Add Bun embed sync host-call proof`)

Bun proof commit: `68d246f19e` (`Add Bun embed async host-call proof`)

Bun patch status: committed locally on Bun `main`, not upstreamed.

## Question

Can guest JavaScript running inside the non-CLI Bun/JSC embed probe call a
Rust-owned host function that returns a pending promise, have Rust settle that
promise from a scheduled host task, and observe the fulfilled value in guest JS
after driving Bun's event loop?

## Patch Shape

The Bun proof commit extends the Gate 4/5 embed target instead of adding a
production backend.

Touched files:

- `Cargo.lock`
- `scripts/build/bun.ts`
- `src/embed_probe/Cargo.toml`
- `src/embed_probe/lib.rs`

`src/embed_probe/lib.rs` now exports a third C ABI probe function:

```rust
#[unsafe(no_mangle)]
pub extern "C" fn nimbus_bun_embed_probe_async_host_call() -> i32
```

The generated C++ driver now runs all three probes in order:

```cpp
int status = nimbus_bun_embed_probe_construct_and_destroy_vm();
if (status != 0) return status;
status = nimbus_bun_embed_probe_sync_host_call();
if (status != 0) return status;
return nimbus_bun_embed_probe_async_host_call();
```

The async host-call probe:

1. creates a fresh non-CLI `VirtualMachine`,
2. calls `vm.event_loop_mut().ensure_waker()` before driving the event loop,
3. installs `globalThis.__nimbusAsyncHostCall` using `JSFunction::create`,
4. evaluates guest JavaScript through `Bun__REPL__evaluate`,
5. verifies the host function ran synchronously but the queued task did not,
6. drives `vm.wait_for_promise(AnyPromise::Normal(promise))`,
7. verifies the scheduled `ManagedTask` ran exactly once,
8. verifies the promise fulfilled with `42`,
9. evaluates `globalThis.__nimbusAsyncObserved` and verifies the JS `.then`
   observer also saw `42`,
10. destroys the VM before returning.

The host function is a Rust `#[bun_jsc::host_fn]`:

```rust
#[bun_jsc::host_fn]
pub fn nimbus_bun_embed_async_host_call(
    global: &JSGlobalObject,
    frame: &CallFrame,
) -> JsResult<JSValue>
```

It reads argument 0, creates a pending `JSPromise`, stores the promise pointer
for proof assertions, enqueues a `bun_jsc::ManagedTask::ManagedTask`, and
returns the promise to JavaScript.

The scheduled task reclaims its heap context, resolves the promise with
`payload + 1`, and records that it ran. The guest source attaches a `.then`
observer that writes the fulfilled value back to `globalThis` for host-side
verification.

## Initial Failure

The first final-linked async run compiled and linked, but aborted while
`wait_for_promise` drained microtasks:

```text
ASSERTION FAILED: currentThreadIsHoldingAPILock()
.../JavaScriptCore/VM.h(1110) : void JSC::VM::finalizeSynchronousJSExecution()
...
JSC__VM__releaseWeakRefs
EventLoop::drain_microtasks_with_global
EventLoop::tick
EventLoop::wait_for_promise
VirtualMachine::wait_for_promise
run_async_host_call_probe
```

This is useful embed-contract evidence: the bare non-CLI host must hold the JSC
API lock while driving `wait_for_promise`, because that path drains microtasks
and finalizes synchronous JS execution.

The normal `vm.jsc_vm().get_api_lock()` guard borrows `vm` immutably for the
guard lifetime, which prevents calling the mutable `vm.wait_for_promise(...)`
from the probe. The proof commit therefore adds a tiny local raw-pointer guard
around the existing C ABI lock/release functions. That keeps the JSC runtime
contract intact without changing Bun's production API.

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
cargo check -p bun_embed_probe --lib
```

Result:

```text
Checking bun_embed_probe v0.0.0 (/Users/jack/src/github.com/oven-sh/bun/src/embed_probe)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.80s
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
[configured] bun-debug in 938ms (unchanged)
ninja: Entering directory `/private/tmp/nimbus-bun-embed-native'
[0/3] cargo bun_embed_probe -> libbun_embed_probe.a (--target aarch64-apple-darwin)
Compiling bun_embed_probe v0.0.0 (/Users/jack/src/github.com/oven-sh/bun/src/embed_probe)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.77s
[1/3] link bun-embed-probe
[2/3] bun-embed-probe
[build] check-bun-embed-probe done
```

Whitespace check:

```sh
git diff --check
```

Result: passed.

Build graph and generated-driver checks:

```sh
rg -n "nimbus_bun_embed_probe_async_host_call|bun-embed-probe|rule regen|command = .*build.ts --config-file" \
  /private/tmp/nimbus-bun-embed-native/build.ninja

sed -n '1,40p' /private/tmp/nimbus-bun-embed-native/embed-probe/driver.cpp
```

Relevant results:

```text
rule regen
  command = cd /Users/jack/src/github.com/oven-sh/bun && .../bun .../scripts/build.ts --config-file=/private/tmp/nimbus-bun-embed-native/configure.json
build bun-embed-probe-rust: phony rust-target/aarch64-apple-darwin/debug/libbun_embed_probe.a
build bun-embed-probe: link obj/embed-probe/driver.cpp.o ...
build bun-embed-probe.smoke-test-passed: embed_probe_smoke_test bun-embed-probe
build check-bun-embed-probe: phony bun-embed-probe.smoke-test-passed
```

The generated driver declares and calls:

```cpp
extern "C" int nimbus_bun_embed_probe_async_host_call();
```

## Decision

Status: async guest-JS-to-Rust host-call and event-loop proof passed.

The proof now shows:

- a non-CLI Bun/JSC VM can create a pending `JSPromise` in a Rust host
  function,
- guest JavaScript can receive that promise and attach a `.then` observer,
- the host can enqueue a Rust-owned `ManagedTask` onto Bun's event loop,
- `vm.wait_for_promise(...)` can drive the queued task and microtask drain,
- the promise can be fulfilled from the scheduled host task,
- guest JavaScript observes the fulfilled value after event-loop progress,
- the embedder must call `ensure_waker()` before driving the bare event loop,
- the embedder must hold the JSC API lock across `wait_for_promise`.

Bun/JSC still remains proof-only for Nimbus. The next gates must prove
bundle/module loading without CLI module state assumptions, timeout/cancel
behavior, permission containment, teardown/reuse semantics, and artifact/server
routing integration.

The immediate next recommended gate is a bundle/module-loading proof. It should
load a Nimbus-shaped JavaScript bundle without relying on Bun's CLI runner or
the `JSModuleLoader::evaluate` path that failed in Gate 5, then prove the
loaded function can use both sync and async host-call transport.
