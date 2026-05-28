# FCE2: Extract `nimbus-provenance`

Status: completed
Started: 2026-05-28
Completed: 2026-05-28
Requirements: FCE-REQ-001, FCE-REQ-002, FCE-REQ-003, FCE-REQ-004, FCE-REQ-005, FCE-REQ-006, FCE-REQ-007, FCE-REQ-008, FCE-REQ-010

## Scope

- Files/modules moved:
  - `RuntimeBundleProvenanceConfig` moved from `crates/nimbus-server/src/execution/invocations/provenance.rs` into `crates/nimbus-provenance/src/lib.rs`.
  - Cloud Functions and Convex registry code now consume `nimbus_provenance::RuntimeBundleProvenanceConfig` directly instead of using a server invocation re-export.
- Files/modules intentionally left in `nimbus-server`:
  - runtime invocation plumbing in `crates/nimbus-server/src/execution/invocations/`
  - server runtime-bundle byte-integrity admission wiring in `crates/nimbus-server/src/execution/invocations/provenance.rs`
  - process-backed verifier effects in `crates/nimbus-server/src/artifact_verifier_effects.rs` and children
  - adapter registry loading effects in Cloud Functions and Convex registry modules
- Crates created or updated:
  - created `crates/nimbus-provenance`
  - updated workspace `Cargo.toml`
  - updated `crates/nimbus-server/Cargo.toml`

## Ownership Decisions

- Authority owner: `nimbus-tenant` keeps tenant policy decisions that consume provenance evidence.
- Effect owner: `nimbus-server` keeps runtime invocation plumbing and process-backed verifier effects.
- Server composition shell: server and adapter code translate runtime-owned bundle integrity into artifact/provenance admission inputs.
- Explicit keep decisions:
  - `nimbus-runtime` must not depend on `nimbus-provenance`; runtime byte integrity remains runtime-owned.
  - SLSA/SBOM verifier evidence remains in `nimbus-artifacts` because it is artifact-verification evidence, not a runtime-specific provenance admission configuration.
  - `nimbus-provenance` may hold verifier trait objects as explicit effect capabilities, but it must not construct process runners or execute verification itself.

## Seam Fix Attempts

- Messy seam found: provenance spanned tenant artifact policy/evidence, runtime byte integrity, server runtime admission, adapter registries, and process-backed verifier effects.
- Right-sized ownership-correct repair attempted: extract only the coherent runtime-bundle provenance admission input into `nimbus-provenance`; keep runtime integrity in `nimbus-runtime`, artifact evidence in `nimbus-artifacts`, process execution in server effects, and tenant authority in `nimbus-tenant`.
- Files changed or spike/proof performed:
  - `crates/nimbus-provenance/src/lib.rs`
  - `crates/nimbus-server/src/execution/invocations/mod.rs`
  - `crates/nimbus-server/src/execution/invocations/provenance.rs`
  - `crates/nimbus-server/src/adapters/cloud_functions/registry.rs`
  - `crates/nimbus-server/src/adapters/convex/mod.rs`
  - `scripts/verify-server-crate-extraction-completion.sh`
- Result: completed. The new crate owns the shared runtime-bundle provenance config, server call sites use it directly, and fail-closed runtime admission tests still pass.
- If blocked, exact architectural reason: n/a.
- Next implementation move: proceed to FCE3 and extract `nimbus-services` only after service evidence and lifecycle seams are ownership-correct.

## Dependency Evidence

```text
$ cargo tree -p nimbus-provenance --edges normal --depth 2
nimbus-provenance v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-provenance)
└── nimbus-artifacts v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-artifacts)
    ├── nimbus-core v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-core)
    ├── oci-client v0.16.1
    └── serde v1.0.228
```

```text
$ cargo tree -p nimbus-runtime --edges normal --depth 1
nimbus-runtime v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-runtime)
├── base64 v0.22.1
├── deno_ast v0.53.2
├── deno_core v0.401.0 (https://github.com/nimbus/deno?tag=v2.8.0-nimbus.5#37b6333a)
├── deno_crypto v0.262.0 (https://github.com/nimbus/deno?tag=v2.8.0-nimbus.5#37b6333a)
├── deno_error v0.7.1
├── deno_fetch v0.272.0 (https://github.com/nimbus/deno?tag=v2.8.0-nimbus.5#37b6333a)
├── deno_fs v0.158.0 (https://github.com/nimbus/deno?tag=v2.8.0-nimbus.5#37b6333a)
├── deno_http v0.246.0 (https://github.com/nimbus/deno?tag=v2.8.0-nimbus.5#37b6333a)
├── deno_io v0.158.0 (https://github.com/nimbus/deno?tag=v2.8.0-nimbus.5#37b6333a)
├── deno_napi v0.179.0 (https://github.com/nimbus/deno?tag=v2.8.0-nimbus.5#37b6333a)
├── deno_net v0.240.0 (https://github.com/nimbus/deno?tag=v2.8.0-nimbus.5#37b6333a)
├── deno_node v0.186.0 (https://github.com/nimbus/deno?tag=v2.8.0-nimbus.5#37b6333a)
├── deno_node_crypto v0.18.0 (https://github.com/nimbus/deno?tag=v2.8.0-nimbus.5#37b6333a)
├── deno_node_sqlite v0.18.0 (https://github.com/nimbus/deno?tag=v2.8.0-nimbus.5#37b6333a)
├── deno_os v0.65.0 (https://github.com/nimbus/deno?tag=v2.8.0-nimbus.5#37b6333a)
├── deno_permissions v0.107.0 (https://github.com/nimbus/deno?tag=v2.8.0-nimbus.5#37b6333a)
├── deno_process v0.63.0 (https://github.com/nimbus/deno?tag=v2.8.0-nimbus.5#37b6333a)
├── deno_resolver v0.79.0 (https://github.com/nimbus/deno?tag=v2.8.0-nimbus.5#37b6333a)
├── deno_semver v0.10.0
├── deno_telemetry v0.70.0 (https://github.com/nimbus/deno?tag=v2.8.0-nimbus.5#37b6333a)
├── deno_tls v0.235.0 (https://github.com/nimbus/deno?tag=v2.8.0-nimbus.5#37b6333a)
├── deno_web v0.279.0 (https://github.com/nimbus/deno?tag=v2.8.0-nimbus.5#37b6333a)
├── deno_webidl v0.248.0 (https://github.com/nimbus/deno?tag=v2.8.0-nimbus.5#37b6333a)
├── deno_websocket v0.253.0 (https://github.com/nimbus/deno?tag=v2.8.0-nimbus.5#37b6333a)
├── libc v0.2.183
├── libloading v0.8.9
├── node_resolver v0.86.0 (https://github.com/nimbus/deno?tag=v2.8.0-nimbus.5#37b6333a)
├── serde v1.0.228
├── serde_json v1.0.149
├── sha2 v0.10.9
├── sys_traits v0.1.28
├── tempfile v3.27.0
├── thiserror v2.0.18
├── tokio v1.51.1
├── tracing v0.1.44
├── twox-hash v2.1.0
└── url v2.5.8
```

## Denied-Import Evidence

```text
$ rg -n "nimbus-server|nimbus_server|nimbus_runtime|nimbus-runtime|axum|std::process|Command::new|Stdio|registry/loading|adapters/|nimbus_storage|nimbus-storage|crate::state|crate::router|crate::local_server|crate::system_tenant|AppState|RouterBuildConfig" crates/nimbus-provenance -g '*.rs' -g 'Cargo.toml'
<no output>
```

```text
$ rg -n "nimbus-(artifacts|provenance|server|tenant|system|auth|bridge|node|storage|services)|nimbus_(artifacts|provenance|server|tenant|system|auth|bridge|node|storage|services)" crates/nimbus-runtime/Cargo.toml
<no output>
```

```text
$ rg -n "crate::execution::invocations::RuntimeBundleProvenanceConfig|pub\\(crate\\) use provenance::RuntimeBundleProvenanceConfig|pub\\(crate\\) use nimbus_provenance::RuntimeBundleProvenanceConfig" crates/nimbus-server/src -g '*.rs'
<no output>
```

## Tests

```text
$ cargo check -p nimbus-provenance
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.84s
```

```text
$ cargo check -p nimbus-server
Finished `dev` profile [unoptimized + debuginfo] target(s) in 9.21s
```

```text
$ cargo test -p nimbus-provenance -- --nocapture
running 1 test
test tests::runtime_bundle_provenance_config_exposes_policy_and_context_without_debugging_verifier ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

```text
$ cargo test -p nimbus-server runtime_bundle_provenance -- --nocapture
running 4 tests
test execution::invocations::provenance::tests::runtime_bundle_provenance_gate_rejects_missing_digest_before_verifier ... ok
test execution::invocations::provenance::tests::runtime_bundle_provenance_gate_rejects_checksum_mismatch_before_verifier ... ok
test execution::invocations::provenance::tests::runtime_bundle_provenance_gate_rejects_wrong_attestation_evidence ... ok
test execution::invocations::provenance::tests::runtime_bundle_provenance_gate_admits_matching_bundle_before_executor_entry ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 766 filtered out; finished in 0.00s

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 23 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 32 filtered out; finished in 0.00s
```

```text
$ cargo test -p nimbus-server cloud_functions_registry -- --nocapture
running 2 tests
test adapters::cloud_functions::registry::tests::cloud_functions_registry_rejects_invalid_artifact_manifest ... ok
test adapters::cloud_functions::registry::tests::cloud_functions_registry_loads_bundle_and_trigger_targets ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 768 filtered out; finished in 0.01s

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 23 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 32 filtered out; finished in 0.00s
```

```text
$ cargo test -p nimbus-server bun_jsc_function_fails_closed_when_adapter_is_not_linked -- --nocapture
running 1 test
test adapters::convex::registry::resolution::runtime_access::tests::bun_jsc_function_fails_closed_when_adapter_is_not_linked ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 769 filtered out; finished in 0.07s

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 23 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 32 filtered out; finished in 0.00s
```

Ignored tests:

- none.

## Verifier Update

- Conditions added or updated:
  - added FCE2 gate requiring `nimbus-provenance` metadata presence, no `nimbus-server` dependency, denied-import absence, no runtime workspace dependency, focused test counts, direct server imports from `nimbus_provenance`, and no server re-export of `RuntimeBundleProvenanceConfig`.
- Current verifier result:

```text
$ bash scripts/verify-server-crate-extraction-completion.sh
Summary: 10 passed, 0 failed
```

## Residual Risk And Resume Notes

- Remaining risk: FCE3 must not pull HTTP service lifecycle, `_nimbus` persistence, or AppState into `nimbus-services`; those should remain server-owned or trait-inverted.
- Next action: start FCE3 by auditing `service_registry.rs`, `service_manager.rs`, `service_manager/*`, `service_manager/registry.rs`, and the existing service evidence/system-state seams before moving code.
