# FCE6: Extract `nimbus-firebase`

Status: completed
Started: 2026-05-28
Completed: 2026-05-28
Requirements: FCE-REQ-001, FCE-REQ-002, FCE-REQ-003, FCE-REQ-004, FCE-REQ-006, FCE-REQ-007, FCE-REQ-008, FCE-REQ-010

## Scope

- Created `crates/nimbus-firebase`.
- Moved pure Firestore REST/model/operation ownership out of `nimbus-server`:
  - request parsers: `batch_get_request`, `batch_write_request`, `commit_request`, `list_collection_ids_request`, `run_aggregation_query_request`, `run_query_request`, `transaction_request`
  - provider model: `firestore_model`, `resource_names`, `serializer`, `response`, `errors`, `operations`
- Moved Firestore proto tree and generated tonic types:
  - `crates/nimbus-server/proto/...` -> `crates/nimbus-firebase/proto/...`
  - server `build.rs` no longer runs Firestore codegen
  - adapter `build.rs` owns `protoc_bin_vendored`, `tonic_build`, `include_file("firebase_grpc.rs")`, and `compile_protos`
- Firestore proto tree and generated tonic types moved to `nimbus-firebase`.
- Moved gRPC operation core into `nimbus-firebase`:
  - `crates/nimbus-firebase/src/grpc/unary.rs`
  - `crates/nimbus-firebase/src/grpc/write_stream.rs`
  - `crates/nimbus-firebase/src/grpc/listen_stream.rs`
- Retained server transport shells:
  - REST Axum route extraction and response wrapping remain in `crates/nimbus-server/src/adapters/firebase/mod.rs`
  - tonic service construction, WebSocket upgrade, auth resolution, and usage recording remain in `nimbus-server`
  - server gRPC `unary.rs`, `write_stream.rs`, and `listen_stream.rs` are thin wrappers over explicit adapter APIs

## Ownership Decisions

- Authority owner: `nimbus-firebase` consumes `TenantIsolationContext`, `PrincipalContext`, and explicit `Arc<nimbus_engine::Service>`; it does not accept `AppState`.
- Application-auth owner: `nimbus-server` resolves bearer tokens through server auth helpers, records authenticated usage, and passes only `PrincipalContext` into the adapter core.
- Effect owner: Firestore operation execution remains through `nimbus-engine::Service`; the adapter owns protocol lowering/raising and stream state, not route mounting or listener lifecycle.
- Protocol owner: Firestore `.proto` files and generated tonic/prost types are now owned by `nimbus-firebase`.
- Server composition shell: Axum REST handlers, tonic `FirestoreServer::new`, WebSocket upgrade, route registration, Firebase enablement checks, and `AppState` access remain in `nimbus-server`.

## Seam Fixes Performed

- Messy seam found: REST error mapping used Axum `Json`/`StatusCode` and `AppError` inside provider semantic code.
  - Repair: adapter error responses now return `(http::StatusCode, serde_json::Value)`; server wraps that into `Json`/`AppError`.
- Messy seam found: Firestore generated protobuf ownership was anchored in `nimbus-server/build.rs`.
  - Repair: moved proto tree and codegen into `nimbus-firebase`; server re-exports generated types only for existing tests and transport glue.
- Messy seam found: gRPC unary/write/listen modules mixed protocol lowering, stream state, auth resolution, usage recording, and `AppState`.
  - Repair: moved unary protobuf lowering/raising plus write/listen stream cores into `nimbus-firebase`; server wrappers synchronously extract request metadata, resolve auth, record usage, then pass `Arc<Service>`, `PrincipalContext`, and adapter-owned registries.
- Messy seam found: a generic async auth helper borrowed tonic streaming requests across an await, making the tonic service futures non-`Send`.
  - Repair: split synchronous metadata extraction from async bearer resolution so streaming requests are moved only after auth is complete.

## Dependency Evidence

- `cargo check -p nimbus-firebase`
  - passed after Firestore proto/gRPC core extraction.
- `cargo check -p nimbus-server`
  - passed after server wrapper rewrite.
- `cargo tree -p nimbus-firebase --edges normal`
  - output includes `nimbus-firebase v0.1.31`.
- `cargo tree -p nimbus-firebase --edges normal | rg "nimbus-server"`
  - exit code: 1
  - output: no matches.

## Denied-Import Evidence

- Command:
  - `rg -n "nimbus[-_]server|AppState|RouterBuildConfig|crate::state|crate::router|crate::system_tenant|system_tenant|local_server|axum|WebSocket|WebSocketUpgrade|Router<|route\\(|State<|Extension<|crate::application_auth|resolve_application_auth|record_authenticated_usage|nimbus[-_]auth|nimbus_auth" crates/nimbus-firebase -g "*.rs" -g "Cargo.toml"`
- Result:
  - exit code: 1
  - output: no matches.
- Server gRPC core deny check:
  - `rg -n "tenant_context_for_database|lower_write_batch|ActiveWriteRequestStream|ActiveListenRequestStream|RetainedListenTargetKey|decode_nimbus_value_from_grpc|proto_document|lower_structured_query" crates/nimbus-server/src/adapters/firebase/grpc/unary.rs crates/nimbus-server/src/adapters/firebase/grpc/write_stream.rs crates/nimbus-server/src/adapters/firebase/grpc/listen_stream.rs`
  - exit code: 1
  - output: no matches.

## Tests

- `cargo test -p nimbus-firebase -- --nocapture`
  - 42 passed; 0 failed; 0 ignored.
- `cargo test -p nimbus-server firebase -- --nocapture`
  - unit target: 98 passed; 0 failed; 0 ignored; 321 filtered out.
  - `mongodb_spec` target under this filter: 0 passed; 0 failed; 0 ignored; 23 filtered out.
  - `reactive_loop` target under this filter: 0 passed; 0 failed; 0 ignored; 32 filtered out.

Ignored tests:

- none.

## Verifier Update

- Added FCE6 verifier condition.
- `bash scripts/verify-server-crate-extraction-completion.sh`
  - 14 passed; 0 failed.
- The verifier now enforces:
  - completed FCE6 proof
  - `nimbus-firebase` metadata presence
  - no `nimbus-server` dependency
  - denied server/auth/AppState/Axum/WebSocket/import patterns absent from the adapter crate
  - exact adapter and server Firebase test counts
  - proto generation ownership in `nimbus-firebase`
  - no Firestore proto generation in `nimbus-server`
  - gRPC core symbols in the adapter crate
  - gRPC core symbols absent from server wrapper files
  - retained server transport/auth shell evidence

## Residual Risk And Resume Notes

- FCE6 is complete.
- The next phase is FCE7, `nimbus-cloud-functions`.
- Resume by reading `docs/plans/proof/server-crate-extraction-completion/fce7-cloud-functions.md`, then classify Cloud Functions ownership across app contracts, runtime invocation, Firebase Admin compatibility, deploy state, provenance, and service-registry access before editing.
