# Bun/JSC Gate 5: Sync Host-Call Probe

Date: 2026-05-21

Superseded by:
`docs/plans/proof/runtime-engine/bun-jsc/gate-6-async-host-call.md`

Nimbus revision: `1703db5f` (`Record Bun native embed proof`)

Bun worktree: `/Users/jack/src/github.com/oven-sh/bun`

Bun prior proof commit: `ead332f17f` (`Add Bun JSC embed probe target`)

Bun proof commit: `52788ee03b` (`Add Bun embed sync host-call proof`)

Bun patch status: committed locally on Bun `main`, not upstreamed.

## Question

Can guest JavaScript running inside the non-CLI Bun/JSC embed probe call a
Rust-owned host function synchronously, and can the host verify the operation,
payload, and return value without using Bun's CLI entrypoint?

## Patch Shape

The Bun proof commit extends the Gate 4 embed target instead of adding a
production backend.

Touched files:

- `scripts/build/bun.ts`
- `scripts/build/configure.ts`
- `src/embed_probe/lib.rs`

`src/embed_probe/lib.rs` now exports a second C ABI probe function:

```rust
#[unsafe(no_mangle)]
pub extern "C" fn nimbus_bun_embed_probe_sync_host_call() -> i32
```

The generated C++ driver now runs both probes:

```cpp
int status = nimbus_bun_embed_probe_construct_and_destroy_vm();
if (status != 0) return status;
return nimbus_bun_embed_probe_sync_host_call();
```

The sync host-call probe:

1. creates a fresh non-CLI `VirtualMachine`,
2. takes the JSC API lock,
3. installs `globalThis.__nimbusHostCall` using `JSFunction::create`,
4. evaluates guest JavaScript,
5. verifies no JS exception was raised,
6. verifies the guest result is `42`,
7. verifies the Rust host function observed exactly one call with payload `41`,
8. destroys the VM before returning.

The host function is a Rust `#[bun_jsc::host_fn]`:

```rust
#[bun_jsc::host_fn]
pub fn nimbus_bun_embed_sync_host_call(
    _global: &JSGlobalObject,
    frame: &CallFrame,
) -> JsResult<JSValue>
```

It reads argument 0, records it in atomics for the proof, returns
`payload + 1`, and the probe asserts `41 -> 42`.

## Initial Failure

The first attempt used `JSModuleLoader::evaluate(...)` with:

```javascript
globalThis.__nimbusHostCall(41);
```

That compiled and linked, but the native smoke run aborted:

```text
src/jsc/bindings/bindings.cpp:3136:18: runtime error: member call on null pointer of type 'JSC::JSPromise'
SUMMARY: UndefinedBehaviorSanitizer: undefined-behavior src/jsc/bindings/bindings.cpp:3136:18
Abort trap: 6
FAILED: [code=134] bun-embed-probe.smoke-test-passed
```

The null site was:

```cpp
auto* promise = JSC::importModule(...);
if (scope.exception()) {
    promise->rejectWithCaughtException(globalObject, scope);
}
auto status = promise->status();
```

This is useful negative evidence: the module-loader path still assumes more
module-loader state than the bare non-CLI embed proof has installed. It should
not be the first sync host-call proof path.

The proof switched to the lower-level C++ program evaluator already used by
Bun's REPL:

```cpp
Bun__REPL__evaluate(...)
```

Despite the REPL name, the helper is a small `JSC::evaluate(...)` wrapper over
`SourceProviderSourceType::Program`. It is a better fit for this gate because
the goal is synchronous guest code execution, not module loading.

## Build-System Finding

Changing `scripts/build/bun.ts` while using an out-of-tree build directory
under `/private/tmp` exposed a generator replay issue:

```text
error: Could not find bun repository root
  hint: Run this from within the bun repository
ninja: error: rebuilding 'build.ninja': subcommand failed
```

Ninja replayed the `build.ninja` generator rule from the build directory, but
`findRepoRoot()` walks upward from `process.cwd()`. That only works when the
build directory is inside the Bun worktree.

The proof commit fixes the generated `regen` rule to `cd` to `cfg.cwd` and pass
an absolute `configure.json` path:

```text
command = cd /Users/jack/src/github.com/oven-sh/bun && .../bun .../scripts/build.ts --config-file=/private/tmp/nimbus-bun-embed-native/configure.json
```

This keeps the documented `/private/tmp` proof command reproducible after build
script edits.

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
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.60s
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
[configured] bun-debug in 603ms (unchanged)
ninja: Entering directory `/private/tmp/nimbus-bun-embed-native'
[0/3] cargo bun_embed_probe -> libbun_embed_probe.a (--target aarch64-apple-darwin)
Compiling bun_embed_probe v0.0.0 (/Users/jack/src/github.com/oven-sh/bun/src/embed_probe)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.13s
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
rg -n "rule regen|command = .*build.ts --config-file|bun-embed-probe|nimbus_bun_embed_probe_sync_host_call" \
  /private/tmp/nimbus-bun-embed-native/build.ninja
```

Relevant result:

```text
rule regen
  command = cd /Users/jack/src/github.com/oven-sh/bun && .../bun .../scripts/build.ts --config-file=/private/tmp/nimbus-bun-embed-native/configure.json
build bun-embed-probe-rust: phony rust-target/aarch64-apple-darwin/debug/libbun_embed_probe.a
build bun-embed-probe: link obj/embed-probe/driver.cpp.o ...
build bun-embed-probe.smoke-test-passed: embed_probe_smoke_test bun-embed-probe
build check-bun-embed-probe: phony bun-embed-probe.smoke-test-passed
```

## Decision

Status: sync guest-JS-to-Rust host-call proof passed.

The proof now shows:

- a non-CLI Bun/JSC VM can be created and destroyed in process,
- a Rust `#[bun_jsc::host_fn]` can be installed on the global object,
- guest JavaScript evaluated inside that VM can call the host function,
- the host can read a primitive payload from `CallFrame`,
- the host can return a primitive `JSValue`,
- the embedding side can verify operation count, payload, return value, and
  exception state before teardown.

Bun/JSC still remains proof-only for Nimbus. The next gates must prove async
host calls and event-loop progress, bundle/module loading without CLI module
state assumptions, timeout/cancel behavior, permission containment, and
teardown/reuse semantics.

The immediate next recommended gate was an async host-call proof. Gate 6
completed that proof locally by returning a pending `JSPromise`, resolving it
from a scheduled `ManagedTask`, and driving `wait_for_promise` until a guest
`.then` observer saw the fulfilled value. Bun/JSC remains proof-only until
bundle/module loading, timeout/cancel behavior, permission containment,
teardown/reuse, and artifact/server routing gates pass.
