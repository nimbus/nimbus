# Gate 38: Linked Adapter Build Lane

Date: 2026-05-24

## Purpose

`BJA3` makes the optional linked Bun/JSC lane explicit in Nimbus without
changing the default runtime behavior. The goal is a reproducible build/proof
lane that compiles the Nimbus-side linked-source adapter contract, verifies the
exact Bun proof source, and keeps default product builds fail-closed until
`BJA4` wires real execution.

## Implementation

Nimbus now has an opt-in runtime feature:

```text
nimbus-runtime/bun-jsc-linked-adapter
```

The feature includes:

```text
crates/nimbus-runtime/src/backends/bun_jsc/linked.rs
```

That module records the source contract:

```text
Bun proof revision:
2f09ba33b184a541e2ade24bf6e46bebc971a262

Bun proof target:
check-bun-embed-probe
```

It also declares the nine Bun proof exports that the linked adapter work will
use as the next source boundary:

```text
nimbus_bun_embed_probe_construct_and_destroy_vm
nimbus_bun_embed_probe_sync_host_call
nimbus_bun_embed_probe_async_host_call
nimbus_bun_embed_probe_program_bundle_host_calls
nimbus_bun_embed_probe_timeout_and_cancel
nimbus_bun_embed_probe_permission_surface_inventory
nimbus_bun_embed_probe_memory_behavior
nimbus_bun_embed_probe_package_module_policy
nimbus_bun_embed_probe_lifecycle_reuse_stress
```

The default runtime factory still constructs the no-link adapter. Even when the
feature is compiled, the default backend reports `not_linked` until a future
gate explicitly selects a linked execution factory. The feature-owned adapter
factory returns `linked` only when directly constructed in tests, and its
execution path fails with a BJA4 guard:

```text
BJA4 has not wired Bun/JSC execution yet
```

This avoids the dangerous middle state where an opt-in compile feature makes
operator diagnostics claim product Bun/JSC execution before pure invocation,
HostBridge, cancellation, and teardown tests exist.

## Verification Gate

Added:

```sh
make verify-bun-jsc-linked-adapter
```

The target runs `scripts/verify-bun-jsc-linked-adapter.sh`, which verifies:

- default `make verify-bun-jsc-runtime-contract`
- `cargo test -p nimbus-runtime --features bun-jsc-linked-adapter --lib
  backends::bun_jsc`
- exact Bun source revision through `git rev-parse HEAD`
- clean Bun proof worktree
- all nine required proof exports with `git grep`
- Bun `cargo fmt --all --check`
- Bun native `check-bun-embed-probe`
- Nimbus `git diff --check`
- Bun `git diff --check`

The CI proof-helper job now syntax-checks the linked-adapter script. The heavy
external Bun proof remains opt-in until `BJA8` resolves source ownership through
an upstream release/tag or Nimbus fork/tag.

## Results

Focused no-link runtime tests:

```sh
cargo test -p nimbus-runtime --lib backends::bun_jsc
```

Result: 7 passed.

Focused linked-feature runtime tests:

```sh
cargo test -p nimbus-runtime --features bun-jsc-linked-adapter --lib backends::bun_jsc
```

Result: 9 passed.

Full linked-adapter gate:

```sh
make verify-bun-jsc-linked-adapter
```

Result: passed outside the Codex filesystem sandbox. The first sandboxed run
failed only because runtime diagnostics tests could not bind a localhost test
listener; the outside-sandbox rerun passed:

- default runtime contract: 11 runtime policy tests, 7 Bun/JSC no-link tests,
  13 registry tests, 2 runtime diagnostics tests, 1 tenant-admission test, and
  2 operator UI test files / 5 tests
- linked-feature runtime contract: 9 tests
- exact Bun proof source revision: matched
- required Bun proof exports: 9 found
- Bun format: passed
- Bun native `check-bun-embed-probe`: passed and emitted the expected
  cancellation, permission, memory, package/resolver, and lifecycle evidence
- Nimbus whitespace diff check: passed
- Bun whitespace diff check: passed

Default workspace check:

```sh
cargo check --workspace
```

Result: passed.

## Result

`BJA3` is complete. The next gate, `BJA4`, should replace the BJA4 guard with
the first real pure-function Bun/JSC invocation through the Bun pool while
preserving the no-link adapter error for default builds.
