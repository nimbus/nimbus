# Gate 64: Operator Diagnostics And DX

Date: 2026-05-25

## Decision

Bun/JSC runtime diagnostics now split execution state from adapter artifact
state.

- `execution_adapter_state` remains the coarse runtime switch: `linked` or
  `not_linked`.
- `execution_adapter_artifact` carries sanitized operator/install diagnostics:
  status, source, reason code, install hint, expected source/ABI contract, and
  verified manifest metadata when available.
- Diagnostics never expose absolute manifest paths, shared-library paths,
  environment variable values, tenant paths, or secrets.

This keeps the existing fail-closed runtime seam stable while giving operators
enough information to distinguish:

- default no-link builds
- missing adapter artifacts
- checksum mismatch or tamper evidence
- unsupported platform artifacts
- invalid manifests
- dynamic-loader or export failures
- successfully linked adapters

## Runtime Contract

The stable lane shape remains:

```json
{
  "lane_name": "bun_jsc",
  "default_lane": false,
  "executor_started": false,
  "execution_adapter_state": "not_linked",
  "execution_adapter_artifact": {
    "status": "not_linked",
    "source": "build_feature_disabled",
    "reason_code": "linked_adapter_feature_disabled",
    "expected": {
      "kind": "nimbus.bun_jsc.adapter",
      "schema_version": 1,
      "source_repository": "https://github.com/nimbus/bun",
      "source_ref": "bun-v1.4.0-nimbus.5",
      "source_revision": "ad0e1d2bbc6690651e04f10eaf1dcdf8a6c0de57",
      "abi_name": "nimbus-bun-jsc-embedder",
      "abi_version": 1,
      "memory_enforcement": "outer_quota_required",
      "lifecycle": "fresh_discard"
    },
    "manifest": null
  }
}
```

Verified manifest diagnostics expose only basename-level artifact metadata plus
expected source/ABI evidence. The verifier tests explicitly serialize
diagnostics and assert local temp paths are absent.

## Code Changes

- `nimbus-runtime` owns the Bun/JSC adapter contract in
  `crates/nimbus-runtime/src/backends/bun_jsc/contract.rs`.
- `nimbus-runtime` exposes typed artifact diagnostics through
  `RuntimeExecutionAdapterArtifactDiagnostics`.
- linked-adapter discovery classifies missing artifact, checksum mismatch,
  unsupported platform, invalid manifest, and load/export failures.
- `nimbus-server` includes `execution_adapter_artifact` in
  `/debug/runtime/metrics` lane diagnostics.
- the operator settings UI renders an artifact column with status, source, and
  source ref.
- API/runtime/install docs explain the state machine and the fail-closed
  default.

## Verification

Passed locally:

```text
cargo check -p nimbus-runtime -p nimbus-server
npm run typecheck --workspace packages/nimbus-ui
cargo test -p nimbus-runtime backends::bun_jsc --lib -- --nocapture
cargo test -p nimbus-runtime --features bun-jsc-linked-adapter --lib backends::bun_jsc -- --nocapture
cargo test -p nimbus-server registry_and_license::runtime_metrics --lib -- --nocapture
npm run test --workspace packages/nimbus-ui -- src/routes/operator/settings/configuration.spec.tsx src/test/msw.spec.ts
make verify-bun-jsc-runtime-contract
cargo fmt --all --check
git diff --check
```

Results:

- default Bun/JSC runtime tests: 10 passed
- linked-adapter Bun/JSC runtime tests: 32 passed
- runtime metrics API tests: 2 passed
- operator UI/MSW tests: 2 files / 5 tests passed
- canonical Bun/JSC runtime contract gate passed after rerunning outside the
  Codex filesystem sandbox for the local listener bind: runtime policy 11
  tests, Bun/JSC pool scaffold 10 tests, Convex runtime lane registry 15 tests,
  runtime diagnostics API 2 tests, tenant admission 1 test, and operator UI 2
  files / 5 tests
