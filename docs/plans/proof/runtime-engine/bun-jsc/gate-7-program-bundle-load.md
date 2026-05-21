# Bun/JSC Gate 7: Program Bundle Load Probe

Date: 2026-05-21

Nimbus revision: `597ff7eb` (`Record Bun async host-call proof`)

Bun worktree: `/Users/jack/src/github.com/oven-sh/bun`

Bun prior proof commit: `68d246f19e` (`Add Bun embed async host-call proof`)

Bun proof commit: `08b993bce8` (`Add Bun embed program bundle proof`)

Bun patch status: committed locally on Bun `main`, not upstreamed.

## Question

Can the non-CLI Bun/JSC embed probe load a Nimbus-shaped JavaScript program
bundle into a fresh VM, then invoke exported sync and async functions from that
loaded bundle while reusing the host-call transports proven in Gates 5 and 6?

## Scope

This is deliberately a program-bundle proof, not an ESM loader proof.

Gate 5 already showed that `JSModuleLoader::evaluate(...)` is not ready in the
bare embed context: it aborted on a null module-import promise before the host
call could be exercised. Gate 7 therefore proves the narrower artifact shape
that Nimbus can plausibly generate for a Bun/JSC backend: a self-contained
program that installs an invocation registry on `globalThis`, with explicit
host-call functions supplied by the embedder.

## Patch Shape

The Bun proof commit extends the existing opt-in embed target.

Touched files:

- `scripts/build/bun.ts`
- `src/embed_probe/lib.rs`

`src/embed_probe/lib.rs` now exports a fourth C ABI probe function:

```rust
#[unsafe(no_mangle)]
pub extern "C" fn nimbus_bun_embed_probe_program_bundle_host_calls() -> i32
```

The generated C++ driver now runs all four probes in order:

```cpp
int status = nimbus_bun_embed_probe_construct_and_destroy_vm();
if (status != 0) return status;
status = nimbus_bun_embed_probe_sync_host_call();
if (status != 0) return status;
status = nimbus_bun_embed_probe_async_host_call();
if (status != 0) return status;
return nimbus_bun_embed_probe_program_bundle_host_calls();
```

The program-bundle probe:

1. creates a fresh non-CLI `VirtualMachine`,
2. calls `ensure_waker()` before event-loop progress,
3. installs both `globalThis.__nimbusHostCall` and
   `globalThis.__nimbusAsyncHostCall`,
4. evaluates a program bundle through `Bun__REPL__evaluate`,
5. the bundle installs `globalThis.__nimbusBundle.sync(...)` and
   `globalThis.__nimbusBundle.asyncCall(...)`,
6. invokes the loaded sync function and verifies `41 -> 42`,
7. invokes the loaded async function and stores the bundle-level returned
   promise on `globalThis`,
8. waits on that bundle-level promise under the JSC API lock,
9. verifies the host-created promise fulfilled, the bundle-level promise
   fulfilled with `42`, and guest-observed sync/async state both equal `42`,
10. destroys the VM before returning.

The async invocation waits on the promise returned by the loaded bundle
function's `.then(...)` chain, not only on the host-created promise. That makes
the proof cover host task execution, promise settlement, async function
continuation, and the guest observer callback.

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
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.92s
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
[configured] bun-debug in 828ms (unchanged)
ninja: Entering directory `/private/tmp/nimbus-bun-embed-native'
[0/1] reconfigure
[0/6] cargo bun_embed_probe -> libbun_embed_probe.a (--target aarch64-apple-darwin)
Compiling bun_embed_probe v0.0.0 (/Users/jack/src/github.com/oven-sh/bun/src/embed_probe)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 24.08s
[2/6] cxx obj/src/jsc/bindings/BunProcess.cpp.o
[3/6] link bun-embed-probe
[4/6] bun-embed-probe
[build] check-bun-embed-probe done
```

Whitespace check:

```sh
git diff --check
```

Result: passed.

Build graph and generated-driver checks:

```sh
rg -n "program_bundle|nimbus_bun_embed_probe_program_bundle_host_calls|bun-embed-probe|rule regen|command = .*build.ts --config-file" \
  /private/tmp/nimbus-bun-embed-native/build.ninja

sed -n '1,50p' /private/tmp/nimbus-bun-embed-native/embed-probe/driver.cpp
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
extern "C" int nimbus_bun_embed_probe_program_bundle_host_calls();
```

## Decision

Status: program-bundle load and invoke proof passed.

The proof now shows:

- a non-CLI Bun/JSC VM can load a self-contained program bundle without Bun's
  CLI runner,
- loaded bundle state persists on the global object across later host
  evaluations,
- a loaded sync export can call the Rust sync host transport,
- a loaded async export can call the Rust async host transport,
- the embedder can wait on the bundle-level promise and observe guest-side
  continuation state after microtask progress.

Bun/JSC still remains proof-only for Nimbus. This gate does not prove ESM
module loading, timeout/cancel, permission containment, teardown/reuse, or
artifact/server routing.

The immediate next recommended gate is either an artifact-shape decision gate
or a timeout/cancel proof. The artifact-shape gate should decide whether a Bun
backend requires ESM module loading or whether Nimbus should generate a
program-bundle wrapper for Bun/JSC, then verify that decision against real
Nimbus-generated function artifacts.
