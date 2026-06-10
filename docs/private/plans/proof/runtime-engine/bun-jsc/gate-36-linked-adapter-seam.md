# Gate 36: Linked Adapter Seam

Date: 2026-05-24

## Purpose

`BJA1` promotes the Bun/JSC optional backend from a pool scaffold into an
explicit execution-adapter seam. The goal is not to link Bun yet. The goal is
to make the future linked adapter a narrow, testable boundary while preserving
the default no-link product contract.

## Implementation

The runtime now owns the canonical adapter state:

```text
nimbus_runtime::RuntimeExecutionAdapterState
  linked
  not_linked
```

The Bun/JSC backend now has a concept-owned adapter module:

```text
crates/nimbus-runtime/src/backends/bun_jsc/adapter.rs
  BunJscExecutionAdapterFactory
  BunJscExecutionAdapter
  BunJscNoLinkExecutionAdapterFactory
  BunJscNoLinkExecutionAdapter
```

The default backend path constructs:

```text
BunJscRuntimeBackendFactory
  -> BunJscRuntimeBackend
      -> BunJscPool
      -> BunJscNoLinkExecutionAdapter
```

That preserves the existing fail-closed behavior:

```text
Bun/JSC runtime backend is admitted only for the proven fresh/discard
lockdown profile, but this Nimbus build does not link a Bun embedder execution
adapter yet
```

The Convex diagnostics path now consumes the runtime-owned
`RuntimeExecutionAdapterState` instead of carrying a Convex-local duplicate
enum. This keeps `/debug/runtime/metrics`, operator UI data, and future linked
adapter state on the same vocabulary.

## Tests Added

`cargo test -p nimbus-runtime backends::bun_jsc --lib` now covers 7 tests:

- pool policy separates trusted retained from untrusted fresh/discard
- pool policy rejects mismatched backend axes
- lifecycle remains ack-driven and ordered
- default Bun/JSC backend uses the `not_linked` adapter
- default Bun/JSC runtime invocation fails closed with the existing contract
  error
- a fake linked adapter receives the invocation through the seam and returns a
  result
- the public Bun/JSC backend envelope has no V8/Deno or Bun embedder symbols
  linked by default

Existing server diagnostics tests continue to prove:

- all lanes remain lazy before invocation
- default V8 and Node lanes report `linked`
- Bun/JSC reports `not_linked`
- Bun/JSC memory semantics remain `outer_quota_required`

## Verification

Passed so far:

```sh
cargo test -p nimbus-runtime backends::bun_jsc --lib
cargo test -p nimbus-server registry_and_license::registry --lib
cargo test -p nimbus-server registry_and_license::runtime_metrics --lib
cargo fmt --all --check
git diff --check
```

Full default contract gate:

```sh
make verify-bun-jsc-runtime-contract
```

The gate passed with:

- 11 runtime policy and memory-semantics tests
- 7 Bun/JSC pool and adapter seam tests
- 13 Convex runtime lane registry tests
- 2 runtime diagnostics API tests
- 1 tenant admission test for the proven Bun/JSC profile
- 2 operator UI runtime diagnostics test files / 5 tests

## Result

`BJA1` is implemented. The next gate, `BJA2`, should keep this seam intact
while proving or productizing the Bun-side embedder execution API behind a
linked adapter.
