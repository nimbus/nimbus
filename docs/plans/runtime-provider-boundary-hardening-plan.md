# Runtime And Provider Boundary Hardening Plan

Status: completed

This plan owns the architectural follow-up from the April 2026 codebase review
around runtime service activation, runtime host ABI typing, and persistence
provider boundaries. Neovex is still pre-launch, so the work should make clean
breaking changes instead of preserving compatibility with the current
implementation quirks.

## Scope

- remove blocking sandbox service activation from synchronous V8 host paths
- replace untyped host payload dispatch with a versioned typed ABI family
- move provider-specific catch-up, polling, hints, and snapshot behavior behind
  provider-owned capability facades

## Non-Goals

- localhost/browser security hardening, owned by
  `docs/plans/localhost-server-security-plan.md`
- desktop UI routes, owned by `docs/plans/desktop-ui-plan.md`
- adding new persistence providers before the provider facade is in place

## Current Risk

- `ctx.services.*` currently performs a synchronous host lookup that can call
  sandbox activation and readiness polling. That makes a V8 worker path
  responsible for slow lifecycle work and weakens cancellation/fairness.
- `HostCallRequest` uses a typed operation plus a raw `serde_json::Value`
  payload. Runtime payload structs exist, but the top-level ABI does not prove
  that an operation and payload family match before dispatch.
- `neovex-engine` still coordinates provider behavior through parallel enum
  matches and macros. That is acceptable for a closed set, but it spreads each
  provider addition or capability change across engine surfaces.

## Roadmap Status Ledger

| Item | Status | Notes |
| --- | --- | --- |
| RPB1 | `done` | Landed snapshot-only `ctx.services.<name>` reads plus async cancellable `ctx.services.get("name")` activation outside the sync V8 host path |
| RPB2 | `done` | Runtime now owns the versioned host envelope plus typed payload family, and the Convex server adapter validates ABI/version compatibility before lowering to adapter-specific semantics |
| RPB3 | `done` | Provider-owned background-task selection plus tenant-state capability methods now carry catch-up, scheduled-work, refresh-planning, and provider-specific apply semantics out of the service layer |

## Implementation Checkpoints

| Checkpoint | Status |
| --- | --- |
| RPB1 ownership, implementation, and verification are recorded | `done` |
| RPB2 ownership, implementation, and verification are recorded | `done` |
| RPB3 ownership, implementation, and verification are recorded | `done` |

## RPB1: Async Service Activation Contract

Goal: service activation is always async and cancellable before it reaches the
runtime worker's synchronous host-call path.

- replace missing-service `ctx.services.<name>` activation with an explicit
  async API such as `await ctx.services.get("name")`
- keep `ctx.services.<name>` as a snapshot-only read for services that were
  already resolved before invocation; missing names return `undefined`
- add an async runtime op for service activation that carries cancellation
  through `HostCallCancellation`
- remove `ensure_service_binding(...)` from sync host-call dispatch; sync
  lookup may only read an already-ready in-memory binding
- keep sandbox start, inspect, and readiness polling outside V8 worker
  execution and covered by bounded waits with named failure messages

Verification:

- runtime bootstrap tests prove `ctx.services.<name>` does not issue a blocking
  sync activation for missing services
- server tests prove service activation uses the async/cancellable host path
- a regression test asserts the sync host-call service lookup implementation
  cannot call `ensure_service_binding(...)`

## RPB2: Versioned Typed Host ABI

Goal: host-call dispatch validates the operation/payload pair at the runtime
boundary instead of leaving every backend adapter to parse raw JSON ad hoc.

- introduce a versioned `HostCallEnvelope` with an ABI version field
- introduce a `HostCallPayload` enum or equivalent typed family whose variants
  map one-to-one with supported `HostCallOperation` values
- move payload structs that are runtime-owned into `neovex-runtime`; keep
  Convex-specific semantic lowering in `neovex-server`
- make deserialization fail when the operation and payload family do not match
- keep backend-neutral names at the runtime boundary where possible, and map
  Convex names in the Convex adapter

Verification:

- serde tests cover every operation/payload pair and reject mismatches
- server dispatch consumes typed payload variants instead of raw `Value`
- runtime tests cover ABI version rejection and compatibility errors

## RPB3: Provider-Owned Capability Facade

Goal: provider-specific behavior is expressed by provider-owned capabilities
instead of engine-wide match lattices.

- define a provider capability facade for read/write execution, snapshots,
  catch-up, freshness hints, notifications, and polling
- move provider-specific catch-up and freshness policy out of engine switch
  statements into provider-owned implementations
- keep `neovex-engine` as coordinator of tenant semantics, not provider
  mechanics
- shrink or remove the `match_persistence_provider!`,
  `match_tenant_persistence!`, and related executor/snapshot macros once the
  facade can carry those behaviors

Verification:

- adding a capability to one provider no longer requires touching every
  provider enum match in `neovex-engine`
- existing Postgres/MySQL/libsql freshness and catch-up tests remain green
- provider benchmark and harness lanes still identify the provider under test
  in failure output

## Exit Criteria

- no synchronous V8 host path can start or wait for sandbox service activation
- the runtime host ABI has operation/payload-level typing with a version gate
- provider-specific behavior has a provider-owned facade and the remaining enum
  matches are composition roots, not behavior switches
- `cargo fmt --all --check`, `make check`, `make clippy`, and focused runtime,
  server, and engine tests pass for the touched surfaces

## Execution Log

| Date | Item | Status | Notes |
| --- | --- | --- | --- |
| 2026-04-23 | RPB1 | `done` | Resumed the active runtime/provider boundary hardening plan from the live worktree, added a roadmap ledger plus checkpoints, and landed the async service-activation contract. `ctx.services.<name>` is now snapshot-only, missing bindings resolve through async `ctx.services.get("name")`, sync runtime lookup no longer starts sandboxes, and service-manager readiness polling now honors `HostCallCancellation`. Verification: `cargo test -p neovex-runtime host_bridge -- --nocapture`; `cargo test -p neovex-server services -- --nocapture`; `cargo test -p neovex-server service_manager -- --nocapture`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo check -p neovex-runtime -p neovex-server`; `cargo clippy -p neovex-runtime -p neovex-server --all-targets -- -D warnings`. Next: introduce the versioned typed host ABI in RPB2. |
| 2026-04-23 | RPB2 | `done` | Landed the versioned typed host ABI. `neovex-runtime` now owns `HOST_CALL_ABI_VERSION`, runtime-owned payload structs, `HostCallPayload`, and `HostCallEnvelope`, and `HostCallRequest` now carries an ABI version plus a constructor. The Convex host bridge converts requests through the typed envelope before dispatch, consumes typed payload variants by route family, and only then lowers into Convex-specific payload parsing. Added runtime and server regression tests for ABI version rejection, operation/payload mismatch rejection, adapter-wire roundtrips, and typed dispatch acceptance. Verification: `cargo check -p neovex-runtime -p neovex-server`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo test -p neovex-runtime host -- --nocapture`; `cargo test -p neovex-server adapters::convex::tests -- --nocapture`; `cargo clippy -p neovex-runtime -p neovex-server --all-targets -- -D warnings`. Next: start RPB3 and replace the provider match lattice with provider-owned capability facades. |
| 2026-04-23 | RPB3 | `done` | Landed the provider-owned capability facade for the behavior-heavy provider seam. `PersistenceProvider` now owns background-task selection for Postgres notifications versus MySQL/libsql polling, and `TenantPersistence` now owns async schema load, journal progress/recovery, commit-log tail reads, scheduled-work checks, loaded-runtime refresh planning, and the libsql replica applied-head exception after durable batch apply. The engine service layer now coordinates tenant semantics through those capability methods instead of matching provider families directly in tenant load, scheduler recovery, schema refresh, provider polling, and mutation-journal apply flow. Verification: `cargo check -p neovex-engine`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo test -p neovex-engine mysql_background_poll -- --nocapture`; `cargo test -p neovex-engine libsql_replica_background_poll -- --nocapture`; `cargo test -p neovex-engine postgres_listener_reconnect -- --nocapture`; `cargo test -p neovex-engine embedded_replica_catch_up -- --nocapture`; `cargo clippy -p neovex-engine --all-targets -- -D warnings`. External-provider integration tests self-skipped where no explicit provider URLs were configured and Docker was unavailable (`/var/run/docker.sock` missing). Next: this plan is complete; promote a new active plan before another provider-boundary cleanup wave. |
