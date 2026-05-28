# SBA4 Bridge Extraction Proof

Date: 2026-05-28
Status: completed

## Scope

Extract provider-neutral runtime host bridge code into `crates/nimbus-bridge`
without moving adapter protocols, server state, HTTP/router code, system
evidence persistence, or runtime bundle invocation orchestration.

## Allowed Contents

- Runtime host bootstrap/context types.
- Runtime capability host trait implementation helpers.
- Generic runtime host ABI document-call dispatch.
- Runtime host response encoding.
- Runtime session state and read-set tracking.
- Runtime cancellation checks.
- Runtime admission mapping from tenant policy admission to runtime execution
  availability.

## Denied Contents

- Convex, Firebase, Firestore, Cloud Functions, or MongoDB protocol handlers.
- HTTP/router/listener/server state construction.
- `_nimbus` system persistence.
- `nimbus-server` imports.
- `nimbus-system` imports.
- Concrete storage providers.
- Runtime bundle invocation worker/provenance orchestration.

## Intended Moves

- Create `crates/nimbus-bridge` and add it to the workspace.
- Move classified bridge-owned modules from `crates/nimbus-server/src/runtime_host`
  into `crates/nimbus-bridge/src`.
- Export a crate API that lets server and adapters build bridge state from
  admitted decisions/projections.
- Update server/adapters to use `nimbus_bridge` directly.
- Remove the server `runtime_host` module instead of keeping a compatibility
  shim.

## Forbidden Imports For Extracted Crate

SBA4 is not complete while `crates/nimbus-bridge` contains production imports
or references to:

- `nimbus_server`
- `crate::adapters`
- `crate::router`
- `crate::state`
- `crate::http`
- `crate::system_tenant`
- `crate::local_server`
- `crate::application_auth`
- provider names: `convex`, `firebase`, `firestore`, `cloud_functions`,
  `mongodb`

## Task Checklist

- [x] SBA4.1 Create `crates/nimbus-bridge`.
- [x] SBA4.2 Move provider-neutral bridge code.
- [x] SBA4.3 Enforce bridge dependency rules.
- [x] SBA4.4 Route adapters through bridge APIs.
- [x] SBA4.5 Preserve runtime behavior.

## Verification Log

- `cargo metadata --no-deps --format-version 1` showed
  `crates/nimbus-bridge` in `workspace_members`.
- `cargo tree -p nimbus-bridge --edges normal --depth 1`:
  `nimbus-bridge` depends on `base64`, `nimbus-core`, `nimbus-engine`,
  `nimbus-node`, `nimbus-runtime`, `nimbus-tenant`, `serde`, `serde_json`,
  and `time`; no `nimbus-server`, `nimbus-system`, or adapter crate edge.
- `rg -n "nimbus_server|crate::(adapters|router|state|http|system_tenant|local_server|application_auth)|convex|firebase|firestore|cloud_functions|mongodb" crates/nimbus-bridge -g '*.rs' -g 'Cargo.toml'`
  returned no matches.
- `rg -n "crate::runtime_host|runtime_host::" crates/nimbus-server/src/adapters -g '*.rs'`
  returned no matches.
- `rg -n "mod runtime_host|src/runtime_host|crate::runtime_host" crates/nimbus-server/src -g '*.rs'`
  returned no matches.
- `cargo check -p nimbus-bridge` passed.
- `cargo check -p nimbus-server` passed.
- `cargo test -p nimbus-bridge -- --nocapture` passed: 7 passed, 0 failed.
- `cargo test -p nimbus-server runtime_host -- --nocapture` passed:
  5 passed, 0 failed, 763 filtered; integration filters reported 0/23 and
  0/32.
- `cargo test -p nimbus-server cloud_functions -- --nocapture` passed:
  39 passed, 0 failed, 729 filtered; integration filters reported 0/23 and
  0/32.
- `cargo test -p nimbus-server async_runtime_integration_removes_hot_path_blocking_adapters -- --nocapture`
  passed: 1 passed, 0 failed, 767 filtered; integration filters reported
  0/23 and 0/32.
- `cargo check --workspace` passed.
- `cargo fmt --all --check` passed.

## Extraction Decision

`nimbus-bridge` is extracted. `nimbus-server` now depends on the crate and no
longer owns a `runtime_host` module. Convex and Cloud Functions runtime paths
import bridge APIs through `nimbus_bridge`, while adapter-private protocol
handlers remain in server until the later adapter extraction phases.
