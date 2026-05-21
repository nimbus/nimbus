# Bun/JSC Gate 9: Generated Program Bundle Probe

Date: 2026-05-21

Nimbus revision: `c286ff63` (`Record Bun artifact shape proof`)

Bun worktree: `/Users/jack/src/github.com/oven-sh/bun`

Bun prior proof commit: `08b993bce8` (`Add Bun embed program bundle proof`)

Bun proof commit: `db4d1067b9` (`Use generated Nimbus program bundle in embed proof`)

Bun patch status: committed locally on Bun `main`, not upstreamed.

## Question

Can the non-CLI Bun/JSC embed probe execute a real Nimbus-generated program
wrapper, invoke `globalThis.__nimbusInvoke(...)`, materialize generated runtime
bindings, and carry both sync and async host calls through a Nimbus-shaped
context?

## Scope

This is still a program-wrapper proof, not an ESM loader proof. Gate 8 selected
the near-term artifact shape because real Nimbus bundles become a global
`__nimbusInvoke` registry after evaluation, while the bare Bun embed path has
not yet proven module loading.

The proof fixture is generated from Nimbus codegen using the same runtime-only
mutation and generated scheduled-function reference shape covered by the
Nimbus selftest. It is checked into the Bun proof target as:

```text
src/embed_probe/nimbus_generated_program_bundle.js
```

The checked-in fixture is 17,459 bytes including its trailing newline and
contains:

- `messages:sendInternal`
- `messages:sendAndSchedule`
- a generated reference-tree binding for `internalScheduledFunctions`
- the real generated `compileRuntimeHandler(...)`,
  `materializeRuntimeBindings(...)`, and `globalThis.__nimbusInvoke`
  implementation

## Patch Shape

The Bun proof commit replaces the synthetic Gate 7 bundle with the generated
Nimbus program wrapper.

Touched files:

- `src/embed_probe/lib.rs`
- `src/embed_probe/nimbus_generated_program_bundle.js`

`src/embed_probe/lib.rs` now:

1. includes the generated fixture with `include_bytes!(...)`,
2. installs JSC host functions for `__nimbusHostCall` and
   `__nimbusAsyncHostCall`,
3. installs a JavaScript `__nimbusCreateContext` stub shaped like Nimbus ctx,
4. routes `ctx.db.insert(...)` through the async host function,
5. routes `ctx.scheduler.runAfter(...)` through the sync host function,
6. evaluates the generated program bundle through `Bun__REPL__evaluate`,
7. invokes `globalThis.__nimbusInvoke({ kind: "mutation",
   function_name: "messages:sendAndSchedule", args: { body: "hello" } })`,
8. waits for the invocation-level promise under the JSC API lock,
9. verifies the async host promise fulfilled with `42`,
10. verifies the sync host call ran after the async continuation,
11. verifies the generated reference tree produced
    `messages:sendInternal`, `internal`, and `mutation`, and
12. verifies the generated invocation response was `{ status: "ok",
    value: "message-id" }` by mapping it to a numeric sentinel inside JS.

This makes the proof exercise the generated Nimbus dispatch and runtime-handler
compilation path instead of a hand-written `__nimbusBundle` object.

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

Observed upstream warnings remained unchanged:

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
[configured] bun-debug in 830ms (unchanged)
ninja: Entering directory `/private/tmp/nimbus-bun-embed-native'
[0/4] cargo bun_embed_probe -> libbun_embed_probe.a (--target aarch64-apple-darwin)
Compiling bun_embed_probe v0.0.0 (/Users/jack/src/github.com/oven-sh/bun/src/embed_probe)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 20.76s
[1/4] cxx obj/src/jsc/bindings/BunProcess.cpp.o
[2/4] link bun-embed-probe
[3/4] bun-embed-probe
[build] check-bun-embed-probe done
```

Whitespace check:

```sh
git diff --check
```

Result: passed.

Build graph and generated-driver checks:

```sh
rg -n "nimbus_generated_program_bundle|generated-program|nimbus_bun_embed_probe_program_bundle_host_calls|bun-embed-probe|rule regen|command = .*build.ts --config-file" \
  /private/tmp/nimbus-bun-embed-native/build.ninja \
  /private/tmp/nimbus-bun-embed-native/embed-probe/driver.cpp
```

Relevant results:

```text
/private/tmp/nimbus-bun-embed-native/embed-probe/driver.cpp:4:extern "C" int nimbus_bun_embed_probe_program_bundle_host_calls();
/private/tmp/nimbus-bun-embed-native/embed-probe/driver.cpp:13:  return nimbus_bun_embed_probe_program_bundle_host_calls();
/private/tmp/nimbus-bun-embed-native/build.ninja:148:rule regen
/private/tmp/nimbus-bun-embed-native/build.ninja:149:  command = cd /Users/jack/src/github.com/oven-sh/bun && .../bun .../scripts/build.ts --config-file=/private/tmp/nimbus-bun-embed-native/configure.json
/private/tmp/nimbus-bun-embed-native/build.ninja:14939:build bun-embed-probe.smoke-test-passed: embed_probe_smoke_test bun-embed-probe
/private/tmp/nimbus-bun-embed-native/build.ninja:14941:build check-bun-embed-probe: phony bun-embed-probe.smoke-test-passed
```

## Decision

Status: generated program-wrapper load and invocation proof passed.

This gate closes the main representativeness gap left by Gate 7. Bun/JSC can
now execute a real Nimbus-generated program wrapper in the non-CLI embed probe,
invoke through the generated `__nimbusInvoke` contract, compile a generated
runtime handler, materialize generated reference-tree bindings, and settle the
invocation promise after async host work resumes guest execution.

Bun/JSC remains proof-only for Nimbus. This gate still does not prove:

- ESM or Bun module loading in the bare embed path,
- Node builtin or external package resolution,
- timeout and external cancellation,
- memory limit policy,
- permission containment for Bun-exposed builtins,
- safe VM reuse or discard-only production policy, or
- production runtime metadata and server routing for JavaScript evaluation
  format.

The next proof should use this generated-program path for timeout/cancel. That
keeps cancellation evidence representative of real Nimbus dispatch instead of
a hand-written bundle.
