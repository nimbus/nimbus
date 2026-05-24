# Gate 39: Linked Pure Invocation

Date: 2026-05-24

## Purpose

`BJA4` replaces the linked-adapter guard with the first real in-process
Bun/JSC execution path. The goal is intentionally narrow: a self-contained
program wrapper receives JSON invocation input, executes inside the proven
locked-down Bun embedder VM, returns JSON output, and records the Bun/JSC pool
lifecycle. HostBridge operations, forged-context rejection, and cancellation
depth remain owned by later gates.

## Bun Source

The Bun proof baseline advanced to:

```text
a409f596e8e1394d8860e2cd8b2bb558ff1afcac
```

That commit adds:

- `nimbus_bun_embed_invoke_program_wrapper_json`, a C ABI for pure JSON
  program-wrapper invocation.
- native permission-deny setup before tenant bundle evaluation.
- resolver/package denial through the existing embedder resolver guard.
- JSON stringify output copying into a caller-owned buffer with explicit status
  codes for ABI, evaluation, promise, stringify, and buffer failures.
- release-profile link-manifest generation at
  `nimbus-bun-embed-link-args.txt`.

## Nimbus Implementation

`nimbus-runtime` now consumes the Bun link manifest only when
`NIMBUS_BUN_EMBED_LINK_ARGS` is set. The build script:

- enables the internal `nimbus_bun_jsc_linked_ffi` cfg only for manifest-backed
  builds
- emits the Bun/JSC object and archive link arguments for test binaries
- marks `nimbus_bun_embed_invoke_program_wrapper_json` as an explicitly
  required static-archive symbol
- adds the platform C++ runtime library needed because Rust links with
  `-nodefaultlibs`

The linked execution adapter now:

- rejects pre-cancelled invocations
- requires the fresh/discard outer-quota Bun pool policy
- verifies runtime bundle integrity and policy content kind
- reads the self-contained program-wrapper bundle
- serializes `InvocationRequest` to JSON
- calls the Bun embedder ABI
- maps embedder status codes into `NimbusRuntimeError::Contract`
- parses the JSON response back into `serde_json::Value`

`BunJscRuntimeBackend` records the linked pool lifecycle around the adapter
call: bootstrap ready, guest entered, terminated, reset/discarded, and teardown
complete.

## Verification

Bun source checks:

```sh
cargo fmt --all --check
git diff --check
CARGO_TARGET_DIR=/private/tmp/nimbus-bun-proof-target-release \
  bun scripts/build.ts --profile=release \
  --build-dir=/private/tmp/nimbus-bun-linked-adapter-release \
  --cache-dir=/private/tmp/nimbus-bun-cache \
  --target=check-bun-embed-probe
```

Result: passed. The native probe emitted cancellation, permission, memory,
package/resolver, lifecycle, and JSON invocation evidence.

Focused Nimbus linked invocation:

```sh
NIMBUS_BUN_EMBED_LINK_ARGS=/private/tmp/nimbus-bun-linked-adapter-release/nimbus-bun-embed-link-args.txt \
CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=/opt/homebrew/opt/llvm@21/bin/clang++ \
cargo test -p nimbus-runtime --features bun-jsc-linked-adapter --lib \
  bun_jsc_linked_adapter_executes_pure_program_wrapper_json -- --nocapture
```

Result: 1 passed.

Full local linked-adapter gate:

```sh
bash scripts/verify-bun-jsc-linked-adapter.sh
```

Result: passed outside the Codex filesystem sandbox.

The gate verified:

- default no-link runtime contract: 11 runtime policy tests, 7 Bun/JSC
  no-link/pool tests, 13 registry tests, 2 runtime diagnostics tests, 1
  tenant-admission test, and 2 operator UI test files / 5 tests
- no-manifest linked feature contract: 10 runtime tests
- exact clean Bun source revision: `a409f596e8e1394d8860e2cd8b2bb558ff1afcac`
- required Bun proof exports: 10 found
- Bun `cargo fmt --all --check`
- release-profile native `check-bun-embed-probe`
- linked pure invocation through Bun/JSC FFI: 1 runtime test
- Nimbus `git diff --check`
- Bun `git diff --check`

## Result

`BJA4` is complete locally on macOS. The old adapter-not-linked error remains
for no-manifest builds, while manifest-backed linked builds can execute a pure
self-contained Bun/JSC program wrapper through the Bun pool.

Debian 13 `minicloud` found a platform blocker before BJA5: static co-linking
the current Deno/V8 stack and Bun/WebKit stack in one Linux binary collides on
native `simdutf` symbols. A diagnostic `--allow-multiple-definition` retry
linked but crashed with `SIGSEGV`, so that workaround is rejected. Evidence and
the updated decision are recorded in
`docs/plans/proof/runtime-engine/bun-jsc/gate-40-linux-static-colink-symbol-collision.md`.

Next: add a BJA4 symbol-isolation subgate, then continue `BJA5` only after the
linked Linux proof passes without unsafe duplicate-symbol linker policy.
