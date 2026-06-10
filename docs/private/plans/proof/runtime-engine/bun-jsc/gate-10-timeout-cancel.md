# Bun/JSC Gate 10: Timeout And Cancel Probe

Date: 2026-05-21

Nimbus prior proof revision: `1b4a6cae` (`Record Bun generated program proof`)

Bun worktree: `/Users/jack/src/github.com/oven-sh/bun`

Bun prior proof commit: `db4d1067b9` (`Use generated Nimbus program bundle in embed proof`)

Bun proof commit: `ea677357e3` (`Add Bun embed timeout cancel proof`)

Bun patch status: committed locally on Bun `main`, not upstreamed.

## Question

Can the non-CLI Bun/JSC embed probe interrupt a generated Nimbus invocation
that is executing inside guest JavaScript, clean up the termination state, run
more JavaScript in the same VM, and then do the same for an explicit external
cancel signal?

## Scope

This gate proves recoverable host-owned interruption for the generated program
wrapper selected in Gate 8 and loaded in Gate 9. It does not add a production
Bun backend to Nimbus.

The passing proof uses a Nimbus-owned deadline thread and a Nimbus-owned cancel
thread that both call JSC `VM::notifyNeedTermination()` through Bun's Rust
binding. The proof does not use JSC `setExecutionTimeLimit()` as the success
path.

## Patch Shape

The Bun proof commit extends the checked-in generated program fixture with:

- `messages:spinForever`
- a generated runtime handler that enters `while (true) {}`
- the same generated `globalThis.__nimbusInvoke` dispatch path used by Gate 9

The generated native driver now calls a fifth exported C ABI probe:

```text
nimbus_bun_embed_probe_timeout_and_cancel()
```

`src/embed_probe/lib.rs` now:

1. runs the prior VM, sync host-call, async host-call, and generated-program
   probes before the timeout/cancel probe,
2. registers Bun safety vtables only once across the process because the proof
   creates multiple VMs in one native smoke-test process,
3. primes JSC's termination exception on the owning thread with
   `global.request_termination()` followed by
   `global.clear_termination_exception()`,
4. installs the minimal generated-program `__nimbusCreateContext`,
5. evaluates the generated program wrapper through `Bun__REPL__evaluate`,
6. invokes `messages:spinForever` through generated `globalThis.__nimbusInvoke`,
7. uses a probe `body.trim()` marker to prove the generated handler reached the
   loop before accepting an unclassified interruption as success,
8. interrupts the loop from a host deadline thread,
9. clears the termination exception and verifies the VM has no remaining
   termination request or execution time limit,
10. evaluates `40 + 2` in the same VM to prove recovery,
11. repeats the generated-loop invocation with a separate external cancel
    thread, and
12. evaluates `40 + 2` again to prove the VM is still usable after explicit
    cancellation.

## Watchdog Finding

An earlier variant used `VM::setExecutionTimeLimit(0.001)` for the timeout
half of the proof. That path did interrupt the generated loop, but the next
evaluation in Bun's `debug-no-asan` native target hit a JSC watchdog assertion
after `clearExecutionTimeLimit()`:

```text
ASSERTION FAILED: hasTimeLimit()
vendor/WebKit/Source/JavaScriptCore/runtime/Watchdog.cpp(133)
```

The proof also found that calling `VM::notifyNeedTermination()` from another
thread before materializing JSC's termination exception on the owning thread
hits:

```text
ASSERTION FAILED: m_terminationException
JavaScriptCore/VM.h(333)
```

The passing proof therefore records this requirement for a future Bun runtime
backend: initialize the termination exception on the owner thread, then model
Nimbus timeouts as host-owned deadlines that call JSC termination. Bun's
internal watchdog is not promotion evidence until a recoverable embedding
sequence is proven.

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
Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.04s
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
[configured] bun-debug in 793ms (unchanged)
ninja: Entering directory `/private/tmp/nimbus-bun-embed-native'
[0/3] cargo bun_embed_probe -> libbun_embed_probe.a (--target aarch64-apple-darwin)
Compiling bun_embed_probe v0.0.0 (/Users/jack/src/github.com/oven-sh/bun/src/embed_probe)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.46s
[1/3] link bun-embed-probe
[2/3] bun-embed-probe
[build] check-bun-embed-probe done
```

Whitespace check:

```sh
git diff --check
```

Result: passed.

## Decision

Status: generated-program timeout/cancel proof passed for recoverable
host-owned termination.

Bun/JSC can now run a generated Nimbus program wrapper, enter a generated
runtime handler, interrupt guest JavaScript from a host deadline, clear the
termination state, continue evaluating JavaScript in the same VM, and repeat
the interruption with an explicit cancel signal.

Promotion remains blocked. This gate still does not prove:

- JSC watchdog-based deadlines with recoverable `clearExecutionTimeLimit()`,
- memory limits,
- permission containment for Bun-exposed builtins,
- Node builtin or external package resolution,
- ESM module loading in the bare embed path,
- production-safe VM pooling or discard-only lifecycle policy, or
- production runtime metadata and server routing for a Bun/JSC backend.

The next proof should focus on memory/permission containment or VM reuse versus
fresh-VM discard policy. For Nimbus seam design, timeout should be represented
as an engine-neutral cancellation/deadline handle, not as a requirement to use
an engine's built-in watchdog when that watchdog is not recoverable in the
embedding path.
