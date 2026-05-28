# FCE5: Extract `nimbus-mongodb`

Status: completed
Started: 2026-05-28
Completed: 2026-05-28
Requirements: FCE-REQ-001, FCE-REQ-002, FCE-REQ-003, FCE-REQ-004, FCE-REQ-006, FCE-REQ-007, FCE-REQ-008, FCE-REQ-010

## Scope

- Files/modules moved:
  - `crates/nimbus-server/src/adapters/mongodb/auth.rs` -> `crates/nimbus-mongodb/src/auth.rs`
  - `crates/nimbus-server/src/adapters/mongodb/bson_bridge.rs` -> `crates/nimbus-mongodb/src/bson_bridge.rs`
  - `crates/nimbus-server/src/adapters/mongodb/commands/` -> `crates/nimbus-mongodb/src/commands/`
  - `crates/nimbus-server/src/adapters/mongodb/connection.rs` -> `crates/nimbus-mongodb/src/connection.rs`
  - `crates/nimbus-server/src/adapters/mongodb/error.rs` -> `crates/nimbus-mongodb/src/error.rs`
  - `crates/nimbus-server/src/adapters/mongodb/wire.rs` -> `crates/nimbus-mongodb/src/wire.rs`
- Files/modules intentionally left in `nimbus-server`:
  - TCP listener lifecycle remains in `nimbus-server`
  - AppState/router/global composition
  - server-owned shutdown and task supervision
- Crates created or updated:
  - created: `crates/nimbus-mongodb`
  - updated: root `Cargo.toml`
  - updated: `crates/nimbus-server/Cargo.toml`
  - updated: `crates/nimbus-server/src/adapters/mongodb/mod.rs`
  - updated: `crates/nimbus-server/src/adapters/mongodb/listener.rs`

## Ownership Decisions

- Authority owner: MongoDB database names resolve to existing tenant context checks in `nimbus-mongodb::commands::tenant`; server does not re-derive command authority from listener state.
- Effect owner: MongoDB command operation core consumes explicit `Arc<nimbus_engine::Service>` capability; TCP bind/accept/task supervision stays server-owned.
- Server composition shell: server instantiates `TcpListener`, records listener status, spawns/aborts the MongoDB listener task, and passes only `Service` plus `AuthConfig`.
- Explicit keep decisions:
  - `MongoDbConfig` remains in server because it includes bind address/listener configuration.
  - `listener.rs` remains in server because it owns `TcpListener`, accepted sockets, `tokio::spawn`, and wire loop lifecycle.
  - `AuthConfig` moved to the adapter crate because SCRAM behavior and command auth are adapter-domain code.

## Seam Fix Attempts

- Messy seam found: MongoDB protocol, command, auth, BSON bridge, error mapping, and connection state lived under `nimbus-server` even though only `listener.rs` needed server-owned lifecycle.
- Right-sized ownership-correct repair attempted: extracted the protocol/command owner into `nimbus-mongodb` while keeping the TCP listener shell in `nimbus-server`.
- Follow-up hardening after review: SCRAM now binds the asserted username to the configured user, generates salts and server nonces from the operating-system CSPRNG, and avoids content-dependent proof equality.
- Files changed or spike/proof performed:
  - created `crates/nimbus-mongodb`
  - moved MongoDB protocol/core files listed in Scope
  - rewired server listener imports to `nimbus_mongodb`
  - tightened newly exposed internals: `ConnectionState` fields are no longer public API, unused aggregation query helpers were removed, and test-only/dead cursor helpers were removed instead of exported.
- Result: completed.
- If blocked, exact architectural reason: n/a.
- Next implementation move: continue with FCE6 `nimbus-firebase`.

## Dependency Evidence

- `cargo tree -p nimbus-mongodb --edges normal`
  - output root: `nimbus-mongodb v0.1.31`
  - direct Nimbus dependencies: `nimbus-core`, `nimbus-engine`, `nimbus-tenant`
  - no `nimbus-server` dependency present.

- `cargo tree -p nimbus-mongodb --edges normal | rg "nimbus-server"`
  - exit code: 1
  - output: no matches.

## Denied-Import Evidence

- Command:
  - `rg -n "nimbus[-_]server|AppState|RouterBuildConfig|crate::state|crate::router|crate::system_tenant|system_tenant|local_server|axum|TcpListener|TcpStream|tokio::net|route\\(|Router<|State<|Extension<" crates/nimbus-mongodb -g '*.rs' -g 'Cargo.toml'`
- Result:
  - exit code: 1
  - output: no matches.

- Server-retained shell proof:
  - `crates/nimbus-server/src/adapters/mongodb/listener.rs` imports `nimbus_mongodb::{commands, connection, error, wire, AuthConfig}`.
  - `crates/nimbus-server/src/adapters/mongodb/listener.rs` retains `TcpListener`, socket read/write, accept-loop handling, and listener tests.
  - `crates/nimbus-server/src/construction.rs` retains bind address setup, listener status recording, `tokio::spawn`, and listener task abort.

## Tests

- `cargo check -p nimbus-mongodb`
  - passed.
- `cargo check -p nimbus-server`
  - passed.
- `cargo test -p nimbus-mongodb -- --nocapture`
  - 263 passed; 0 failed; 0 ignored.
- `cargo test -p nimbus-server mongodb -- --nocapture`
  - unit target: 5 passed; 0 failed; 0 ignored; 456 filtered out.
  - `mongodb_spec` target under this filter: 0 passed; 0 failed; 0 ignored; 23 filtered out.
  - `reactive_loop` target under this filter: 0 passed; 0 failed; 0 ignored; 32 filtered out.
- `cargo test -p nimbus-server --test mongodb_spec -- --nocapture`
  - 23 passed; 0 failed; 0 ignored.
  - Note: report-style compatibility tests intentionally print BSON/CRUD corpus gaps while asserting the report machinery; the Rust test result is passing.

Ignored tests:

- none.

## Verifier Update

- Conditions added or updated:
  - Step 13 enforces completed FCE5 proof, target crate metadata, no `nimbus-server` dependency, denied imports absent, adapter/server/spec test counts, server imports from `nimbus_mongodb`, and retained server listener shell.
- Current verifier result:
  - `bash scripts/verify-server-crate-extraction-completion.sh`
  - 13 passed; 0 failed.

## Residual Risk And Resume Notes

- Remaining risk: MongoDB command core intentionally depends on `nimbus-engine::Service` as the explicit service capability; this is allowed by the FCE5 plan and keeps command execution separate from server listener lifecycle. A future smaller trait is optional, not required for this phase.
- Resume notes:
  - FCE5 is complete.
  - FCE6 is now active; start by inspecting Firebase REST/gRPC model, operation, auth/usage, and stream boundaries.
