# SSE1A MongoDB Adapter Readiness Proof

Date: 2026-05-28
Status: completed

## Scope

Prepare the MongoDB adapter seam for a per-adapter extraction decision by
removing server-private authority imports and proving the remaining boundary.

## Task Checklist

- [x] SSE1A.1 Audit MongoDB listener/protocol split.
- [x] SSE1A.2 Remove or isolate server state assumptions, or record blocker.
- [x] SSE1A.3 Preserve MongoDB auth/protocol behavior.
- [x] SSE1A.4 Record extraction decision for MongoDB.

## File Ownership

| File or subtree | Owner classification | Notes |
| --- | --- | --- |
| `adapters/mongodb/wire.rs` | Protocol candidate | OP_MSG parsing/validation/framing with no server state, router, system, runtime, or auth imports. |
| `adapters/mongodb/bson_bridge.rs` | Protocol/data-shape candidate | Converts between BSON and `nimbus-core` values. No server-private imports. |
| `adapters/mongodb/auth.rs` | Protocol auth candidate | Implements MongoDB SCRAM protocol against `AuthConfig`; separate from Nimbus application auth and local operator auth. |
| `adapters/mongodb/error.rs` | Protocol candidate | Maps `nimbus_core::Error` to Mongo-style command errors. |
| `adapters/mongodb/connection.rs` | Protocol candidate | Per-connection state, cursor/session/change-stream stores. No server composition imports. |
| `adapters/mongodb/commands/*` | Protocol plus engine capability candidate | Uses `nimbus-core`, `nimbus-tenant`, and `nimbus_engine::Service`; tenant routing flows through `TenantIsolationContext` from the canonical tenant crate. |
| `adapters/mongodb/listener.rs` | Effectful adapter/server boundary | Owns `TcpListener` accept loop, per-connection task spawning, and direct `Arc<Service>` injection. It is isolated from `AppState`/router but is not pure protocol code. |
| `tests/mongodb_wire.rs` and `tests/mongodb_spec/*` | Behavior proof | Cover wire roundtrips and spec/corpus behavior through the current server-owned adapter module. |

## Denied Dependency Audit

Command:

```bash
rg -n "crate::|nimbus_(core|engine|tenant|server|system|bridge|auth)|AppState|RouterBuildConfig|local_server|system_tenant|runtime_host|std::process::Command" crates/nimbus-server/src/adapters/mongodb -g '*.rs'
```

Result summary:

- No production MongoDB adapter file imports `AppState`, `DeploymentState`,
  `RouterBuildConfig`, `crate::router`, `crate::local_server`,
  `crate::system_tenant`, `crate::application_auth`, `crate::runtime_host`,
  `crate::tenant`, `nimbus-system`, `nimbus-bridge`, `nimbus-auth`, or
  `std::process::Command`.
- Allowed production hits are `nimbus_core`, `nimbus_tenant`, and
  `nimbus_engine::Service`.
- Test-only hits use `crate::adapters::mongodb::*` helpers.

Cleanup performed:

- Replaced `crate::tenant::TenantIsolationContext` with
  `nimbus_tenant::TenantIsolationContext` in `commands/tenant.rs` and
  `commands/session.rs`.

## Authority And Effect Notes

Tenant separation is enforced before engine access by mapping MongoDB database
names to `TenantIsolationContext`:

- `commands/tenant.rs` resolves tenant ids from `$db`.
- `ensure_database_matches_context` rejects a database/context mismatch before
  collection or document access.
- `ensure_tenant` creates or confirms the admitted tenant through the engine.

The remaining extraction work is mechanical and should be handled in SSE6 if
MongoDB is selected for extraction:

- Commands take concrete `Arc<Service>`. This is an explicit dependency on the
  engine, not a server-private dependency. If SSE6 chooses to extract MongoDB,
  either keep `nimbus-engine` as an allowed adapter dependency or introduce a
  MongoDB command capability trait before moving files.
- `listener.rs` owns TCP accept/spawn lifecycle. For an enterprise-reviewable
  extraction, keep listener startup in `nimbus-server` and extract the protocol
  handler/commands behind a server-owned listener wrapper.

## Decision

MongoDB is ready for a per-adapter extraction decision after SSE1A cleanup.

Required extraction shape:

- Extract protocol/command/BSON/SCRAM code without `nimbus-server`.
- Preserve `nimbus-core`, `nimbus-tenant`, and either `nimbus-engine` as an
  explicit capability dependency or a narrower MongoDB command trait.
- Keep listener startup, process lifecycle, and route/process composition in
  `nimbus-server`.

Do not create an aggregate `nimbus-adapters` crate based on MongoDB alone; this
adapter is cleaner than Firebase, Cloud Functions, and Convex, and aggregating
them would hide those different ownership states.

## Verification Log

- `cargo test -p nimbus-server mongodb -- --nocapture`: passed. Unit-test
  target reported 266 passed, 0 failed, 500 filtered; `mongodb_spec` and
  `reactive_loop` integration targets had 0 matching filtered tests.
- `cargo test -p nimbus-server --test mongodb_spec -- --nocapture`: passed.
  Integration target reported 23 passed, 0 failed.
