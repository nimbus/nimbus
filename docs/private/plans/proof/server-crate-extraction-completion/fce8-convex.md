# FCE8: Extract `nimbus-convex`

Status: completed
Started: 2026-05-28
Completed: 2026-05-28
Requirements: FCE-REQ-001, FCE-REQ-002, FCE-REQ-003, FCE-REQ-004, FCE-REQ-005, FCE-REQ-006, FCE-REQ-007, FCE-REQ-008, FCE-REQ-010

## Scope

- Files/modules moved:
  - `crates/nimbus-server/src/adapters/convex/auth/**` -> `crates/nimbus-convex/src/auth/**`.
  - `crates/nimbus-server/src/adapters/convex/document_identity.rs` -> `crates/nimbus-convex/src/document_identity.rs`.
  - `crates/nimbus-server/src/adapters/convex/manifest.rs` -> `crates/nimbus-convex/src/manifest.rs`.
  - `crates/nimbus-server/src/adapters/convex/registry/**` -> `crates/nimbus-convex/src/registry/**`.
  - `crates/nimbus-server/src/adapters/convex/requests/**` -> `crates/nimbus-convex/src/requests/**`.
  - `crates/nimbus-server/src/adapters/convex/templates/**` -> `crates/nimbus-convex/src/templates/**`.
  - `crates/nimbus-server/src/adapters/convex/host_bridge/{contract,pagination,responses}.rs` and `host_bridge/payloads/**` -> `crates/nimbus-convex/src/host_bridge/**`.
  - `crates/nimbus-server/src/adapters/convex/subscriptions/types.rs` and pure transform planning/state/builtin/selection modules -> `crates/nimbus-convex/src/subscriptions/**`.
- Files/modules intentionally left in `nimbus-server`:
  - `crates/nimbus-server/src/adapters/convex/handlers/**`: Axum extraction, route handling, local operator route-family authorization.
  - `crates/nimbus-server/src/adapters/convex/subscriptions/socket/**`: WebSocket upgrade/session lifecycle and outbound forwarding.
  - `crates/nimbus-server/src/adapters/convex/execution/**`: concrete runtime invocation, cancellation, scheduler execution, and engine operation dispatch.
  - `crates/nimbus-server/src/adapters/convex/host_bridge/{bridge,async_bridge,db_ops,function_ops,read_tracking}/**`: concrete host effects over `Service`, admitted tenant context, and runtime service registry.
  - `crates/nimbus-server/src/adapters/convex/mod.rs`: server composition shell plus `_nimbus` deployment summary conversion.
- Crates created or updated:
  - Created `crates/nimbus-convex`.
  - Added it to the workspace and to `nimbus-server`.

## Ownership Decisions

- Authority owner: application auth verifier logic moved into `nimbus-convex` and implements `nimbus_auth::ApplicationAuthVerifier`; tenant authority remains in `nimbus-tenant`; runtime capability admission still flows through `nimbus-bridge` and server-owned `ConvexHostBridge` effects.
- Effect owner: adapter owns Convex registry file loading, JWT metadata fetch, runtime bundle value selection, host-call wire contracts, and pure transform planning. Server owns Axum/WebSocket lifecycle, concrete runtime invocation, request cancellation, local operator audit, and `_nimbus` persistence.
- Server composition shell: `nimbus-server` re-exports `ConvexRegistry`, route handlers, and concrete host bridge shells but does not own the moved Convex protocol/auth/registry model internals.
- Explicit keep decisions: `_nimbus` deployment persistence stayed in server via `convex_system_deployment_record_input`; route mounting, AppState construction, active deployment lookup, shutdown, and global composition stayed in server.

## Seam Fix Attempts

- Messy seam found: Convex registry/auth/protocol code was mixed with `AppError`, `axum::http::Method`, server `_nimbus` conversion, and runtime subscription execution.
- Right-sized ownership-correct repair attempted:
  - Replaced server `AppError` in moved auth with `nimbus_auth::ApplicationAuthError`.
  - Added bounded OIDC/JWKS metadata fetches plus cache refresh on key rotation so application auth is reliable without accepting stale signing keys.
  - Replaced Axum method imports in moved templates/registry code with the neutral `http` crate.
  - Split neutral `ConvexHttpRequestContext` and `ConvexSubscriptionEvent` into the adapter while keeping `ConvexHttpRouteRequest` in server because it contains Axum request extraction types.
  - Moved `ConvexRuntimeQueryBuilderState::into_query` into the adapter so server no longer adds inherent impls to adapter-owned types.
  - Added `ConvexRegistry::function_definition` instead of exposing registry internals.
  - Moved `system_deployment_record_input` out of adapter and into server helper `convex_system_deployment_record_input`.
- Files changed or spike/proof performed:
  - `Cargo.toml`, `crates/nimbus-server/Cargo.toml`, new `crates/nimbus-convex/**`, and server Convex shells/imports.
- Result:
  - Convex registry, auth verifier, manifest/schema parsing, request models, document identity, host-call contract/envelopes, neutral HTTP templates, and subscription transform models moved to `nimbus-convex`.
  - Axum handlers, WebSocket session lifecycle, concrete runtime invocation, request cancellation, local operator audit, and `_nimbus` deployment persistence remain in `nimbus-server`.
- If blocked, exact architectural reason: not blocked.
- Next implementation move: proceed to FCE9 optional `nimbus-adapters` facade.

## Dependency Evidence

```text
`cargo check -p nimbus-convex`: passed
`cargo check -p nimbus-server`: passed

`cargo tree -p nimbus-convex --edges normal | rg "nimbus-server"`:
no matches, command exited 1 as expected.

Observed cargo tree root:
nimbus-convex v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-convex)
```

## Denied-Import Evidence

```text
rg -n 'nimbus-server|crate::state|crate::router|crate::local_server|crate::system_tenant|crate::application_auth|crate::execution|AppState|RouterBuildConfig|WebSocket|axum|tower|tonic|listener|shutdown|record_deployment_state_async|upsert_system_document|SystemDeploymentRecordInput|std::process|process::Command|Command::new' crates/nimbus-convex -g '*.rs' -g 'Cargo.toml'

Result: no matches, command exited 1 as expected.
```

## Tests

```text
`cargo test -p nimbus-convex -- --nocapture`: 6 passed; 0 failed; 0 ignored.

Adapter tests:
- auth::tests::custom_jwt_convex_projection_stays_narrower_than_verified_identity
- auth::verifier::metadata::tests::auth_metadata_is_cached_and_refresh_can_replace_stale_jwks
- host_bridge::responses::tests::storage_error_round_trips_through_runtime_encoding
- manifest::tests::convex_schema_manifest_preserves_composite_indexes
- registry::resolution::runtime_access::tests::bun_jsc_function_fails_closed_when_adapter_is_not_linked
- subscriptions::types::tests::runtime_transform_clone_reuses_last_value_arc

`cargo test -p nimbus-server convex -- --nocapture`:
- lib target: 126 passed; 0 failed; 5 ignored; 264 filtered out.
- mongodb_spec target: 0 passed; 0 failed; 0 ignored; 23 filtered out.
- reactive_loop target: 18 passed; 0 failed; 0 ignored; 14 filtered out.
```

Security-relevant tests covered by the focused server lane:

- `adapters::convex::tests::authorization::runtime_host_bridge_rejects_wrong_table_convex_document_ids`
- `adapters::convex::tests::authorization::host_bridge_service_lookup_rejects_service_missing_from_decision_grants`
- `tests::local_server_security::convex_route_rejects_application_bearer_for_different_tenant`
- `tests::local_server_security::convex_http_action_rejects_application_bearer_for_different_tenant`
- `tests::local_server_security::system_tenant_convex_routes_require_local_admin_auth_when_configured`
- `tests::convex_functions::runtime_queries::execution::services::production_convex_node_runtime_rejects_loopback_network_grants_before_invocation`

Ignored tests:

- `tests::convex_functions::runtime_queries::execution::services::convex_runtime_query_starts_real_krun_service_under_manager_and_tears_it_down`: requires a Linux host with KVM, buildah, conmon, and network access.
- `tests::convex_runtime::http_routes::demo_flow::seeded_usage::verification_harness_nightly_generated_history_seed_corpus_matches_model_on_convex_demo_surface`: verification harness nightly corpus runs in dedicated harness lanes.
- `tests::convex_runtime::http_routes::demo_flow::seeded_usage::verification_harness_nightly_generated_history_seed_corpus_matches_model_on_faulted_convex_demo_surface`: verification harness nightly corpus runs in dedicated harness lanes.
- `tests::convex_runtime::http_routes::demo_flow::seeded_usage::verification_harness_required_generated_history_seed_corpus_matches_model_on_convex_demo_surface`: verification harness required corpus runs in dedicated harness lanes.
- `tests::convex_runtime::http_routes::demo_flow::seeded_usage::verification_harness_required_generated_history_seed_corpus_matches_model_on_faulted_convex_demo_surface`: verification harness required corpus runs in dedicated harness lanes.

## Verifier Update

- Conditions added or updated:
  - Added step 16 enforcing FCE8 proof completion, crate metadata, no `nimbus-server` dependency, denied imports, adapter/server/reactive-loop test counts, required moved symbols, retained server shells, required security tests, and old server-owned copy removal.
  - Updated FCE1/FCE2 verifier paths to point at `crates/nimbus-convex/src/registry/loading.rs` and `crates/nimbus-convex/src/lib.rs` after extraction.
- Current verifier result: pending final post-ledger run after FCE9 is opened.

## Residual Risk And Resume Notes

- Remaining risk: the old server directories are empty on disk after file deletion but are no longer module roots; verifier checks the removed source files rather than directory existence because Git does not track empty directories.
- Next action: start FCE9, decide whether to create the optional `nimbus-adapters` thin facade now that all per-adapter crates are clean.
