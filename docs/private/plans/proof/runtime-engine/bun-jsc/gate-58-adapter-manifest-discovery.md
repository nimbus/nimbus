# Gate 58: Bun/JSC Adapter Manifest Discovery

Date: 2026-05-25

## What Changed

BJD1 added a typed manifest and discovery contract for packaged Bun/JSC adapter
artifacts.

The default no-link state remains intact. A linked-feature build now resolves a
shared adapter in this order:

1. direct development shared-library override:
   `NIMBUS_BUN_EMBED_SHARED_LIBRARY`
2. explicit adapter manifest override:
   `NIMBUS_BUN_JSC_ADAPTER_MANIFEST`
3. packaged Linux manifest:
   `/usr/libexec/nimbus/runtime/bun-jsc/current/nimbus-bun-jsc-adapter.json`
4. packaged Homebrew manifests:
   `/opt/homebrew/opt/nimbus/libexec/runtime/bun-jsc/current/nimbus-bun-jsc-adapter.json`
   and
   `/usr/local/opt/nimbus/libexec/runtime/bun-jsc/current/nimbus-bun-jsc-adapter.json`
5. fail closed as `not_linked`

The manifest is strict JSON with `serde(deny_unknown_fields)`. Nimbus validates
the manifest before opening the shared library:

- schema version
- artifact kind
- adapter and Nimbus version presence
- Bun source repository/ref/revision
- target triple and platform
- single relative library filename beside the manifest
- library SHA-256
- ABI name, ABI version, and exact required export list
- `outer_quota_required`
- `fresh_discard`
- optional provenance file names
- Unix group/other-writable manifest directory/file/library rejection

The source contract is now runtime-owned rather than test-only so the manifest
validator and loader use the same repository/ref/revision/export list.

## Files

- `crates/nimbus-runtime/src/backends/bun_jsc/manifest.rs`
- `crates/nimbus-runtime/src/backends/bun_jsc/linked.rs`
- `crates/nimbus-runtime/src/backends/bun_jsc/mod.rs`
- `crates/nimbus-runtime/build.rs`

## Verification

Commands run:

```sh
cargo fmt --all --check
cargo test -p nimbus-runtime --features bun-jsc-linked-adapter --lib backends::bun_jsc -- --nocapture
cargo test -p nimbus-runtime --lib backends::bun_jsc -- --nocapture
make verify-bun-jsc-runtime-contract
git diff --check
```

Results:

- `cargo fmt --all --check`: passed
- `cargo test -p nimbus-runtime --features bun-jsc-linked-adapter --lib backends::bun_jsc -- --nocapture`: passed, 25 tests
- `cargo test -p nimbus-runtime --lib backends::bun_jsc -- --nocapture`: passed, 9 tests
- `make verify-bun-jsc-runtime-contract`: passed outside the sandbox after the
  sandboxed run failed to bind the local server fixture listener. The passing
  run covered 11 runtime policy tests, 9 Bun/JSC scaffold tests, 15 registry
  tests, 2 runtime metrics tests, 1 tenant admission test, and 2 UI files / 5
  UI tests.
- `git diff --check`: passed

The focused test suite covers:

- valid packaged manifest resolution
- direct development override precedence
- explicit manifest override
- packaged manifest discovery
- missing adapter fail-closed install hint
- wrong Bun revision rejection
- wrong target triple rejection
- schema mismatch rejection
- checksum mismatch rejection
- unsupported memory/lifecycle policy rejection
- library path escape rejection
- unknown field rejection
- group/other-writable packaged directory rejection on Unix
- existing no-link runtime behavior and linked-adapter source contract tests
