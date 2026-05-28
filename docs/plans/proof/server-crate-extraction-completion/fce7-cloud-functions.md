# FCE7: Extract `nimbus-cloud-functions`

Status: completed
Started: 2026-05-28
Completed: 2026-05-28
Requirements: FCE-REQ-001, FCE-REQ-002, FCE-REQ-003, FCE-REQ-004, FCE-REQ-005, FCE-REQ-006, FCE-REQ-007, FCE-REQ-008, FCE-REQ-010

## Extraction Result

Cloud Functions app contract, manifests, target binding, registry, runtime API, host bridge, trigger executor, and neutral HTTP request/response shaping moved to `nimbus-cloud-functions`.

Axum route mounting, active deployment lookup, callable auth/usage, deploy activation, and process-backed codegen fixtures remain in `nimbus-server`.

The extracted crate owns:

- Firebase/functions-framework app discovery and admin import support.
- Cloud Functions artifact manifest, targets manifest, target binding, global/default option validation, and reserved route checks.
- Runtime bundle registry loading, bundle integrity identity, runtime policy/executor handles, trigger registrations, and provenance config values.
- Cloud Functions runtime API extension dispatch, including Firebase Admin Firestore operations through `nimbus-firebase`, `nimbus-bridge`, and admitted runtime capabilities.
- `CloudFunctionsHostBridge`, `CloudFunctionsTriggerExecutor`, `CloudFunctionsRuntimeInvoker`, `CloudFunctionsRuntimeInvocation`, and `CloudFunctionsRuntimeContext`.
- Neutral HTTP/callable request argument construction and `CloudFunctionsHttpResponseParts` response shaping without Axum.

The server shell owns:

- `ServerCloudFunctionsRuntimeInvoker`, the only concrete adapter-to-server runtime invocation implementation.
- Runtime bundle provenance gate execution through existing server artifact verifier effects.
- Axum request extraction, response construction, route fallback, CORS response application, callable auth verification, and authenticated usage recording.
- Deployment activation and active registry lookup.
- Node/codegen process fixtures in tests only.

The generic host-call metering helper moved from `nimbus-server::execution::host_calls` to `nimbus-bridge::host_calls`, so extracted runtime host bridges do not import server execution internals.

## Boundary Proof

- `crates/nimbus-cloud-functions` exists and is in workspace metadata as `nimbus-cloud-functions v0.1.31`.
- `cargo tree -p nimbus-cloud-functions --edges normal | rg "nimbus-server"`: no matches, command exited 1 as expected for an empty search.
- Denied-import scan over `crates/nimbus-cloud-functions` for `nimbus-server`, `AppState`, `RouterBuildConfig`, Axum route/listener types, server-private modules, `_nimbus` persistence, process construction, server execution internals, and artifact verifier effects: no matches, command exited 1 as expected.
- Server wrappers import `nimbus_cloud_functions` APIs and keep only transport/composition/effect code.
- Removed server-owned implementation files:
  - `crates/nimbus-server/src/adapters/cloud_functions/app_contract.rs`
  - `crates/nimbus-server/src/adapters/cloud_functions/registry.rs`
  - `crates/nimbus-server/src/adapters/cloud_functions/host_bridge.rs`
  - `crates/nimbus-server/src/adapters/cloud_functions/runtime_api/`
  - `crates/nimbus-server/src/adapters/cloud_functions/http/request.rs`
- Retained server shells:
  - `crates/nimbus-server/src/adapters/cloud_functions/mod.rs`
  - `crates/nimbus-server/src/adapters/cloud_functions/http.rs`
  - `crates/nimbus-server/src/adapters/cloud_functions/http/callable.rs`
  - `crates/nimbus-server/src/adapters/cloud_functions/http/tenant.rs`
  - `crates/nimbus-server/src/adapters/cloud_functions/http/response.rs`
  - `crates/nimbus-server/src/adapters/cloud_functions/execution.rs`

## Verification

- `cargo check -p nimbus-cloud-functions`: passed.
- `cargo check -p nimbus-server`: passed.
- `cargo test -p nimbus-cloud-functions -- --nocapture`: 20 passed; 0 failed; 0 ignored.
- `cargo test -p nimbus-server cloud_functions -- --nocapture`: 20 passed; 0 failed; 0 ignored; 380 filtered out in unit target; `mongodb_spec` 0 passed; 0 failed; 0 ignored; 23 filtered out; `reactive_loop` 0 passed; 0 failed; 0 ignored; 32 filtered out.
- `cargo fmt --all`: passed.
- `bash scripts/verify-server-crate-extraction-completion.sh`: 15 passed; 0 failed.

## Security Coverage

- `cloud_functions_trigger_executor_fails_closed_when_runtime_bundle_provenance_is_rejected` proves Cloud Functions provenance rejection happens before runtime side effects.
- Existing Cloud Functions focused server tests cover generated Firebase and Functions Framework bundles, callable auth failure, wrong-tenant bearer rejection, App Check rejection, ambiguous tenant rejection, exact HTTP path dispatch, trigger lifecycle, chain-depth limit, no-op update suppression, generated Admin Firestore operations, and missing runtime handler failure.
- Adapter tests cover manifest validation, unsupported global/root API fail-fast behavior, invalid/duplicate target rejection, reserved HTTP path rejection, app root discovery, admin import support matrix, registry loading, and invalid artifact manifest rejection.

## Next Phase

Move to FCE8 `nimbus-convex`; start by reading the Convex AI guidelines, FCE8 plan text, and SSE1D proof before moving code.
