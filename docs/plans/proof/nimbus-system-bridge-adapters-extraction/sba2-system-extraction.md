# SBA2 System Extraction Proof

Date: 2026-05-27
Status: completed

## Scope

Extract the true `_nimbus` system control-plane boundary into
`crates/nimbus-system` after SBA1 removed adapter-private deployment inputs.

## Allowed Dependencies

`nimbus-system` may depend on:

- `nimbus-core`
- `nimbus-engine`
- `nimbus-machine`
- `nimbus-node`
- `nimbus-sandbox`
- `nimbus-tenant`

## Denied Dependencies

`nimbus-system` must not depend on:

- `nimbus-server`
- future `nimbus-adapters`
- HTTP/router/state modules
- adapter-private deployment summaries or protocol types
- storage-provider-specific implementations
- runtime bridge internals
- host lifecycle backends

## Intended Moves

- Create `crates/nimbus-system` and wire it into the workspace.
- Move `system_tenant` identity, keys, schema, inventory, projections, record
  inputs, and record writers into the new crate.
- Keep server composition call sites in `nimbus-server`, using the extracted
  crate API.
- Remove the server-local `local_enforcement` and `tenant` shims from moved
  code by importing `nimbus-node` and `nimbus-tenant` directly.
- Preserve existing system-tenant tests with the moved crate and keep
  cross-server integration tests in `nimbus-server`.

## Forbidden Imports

SBA2 is not complete while production files under `crates/nimbus-system`
contain any of:

- `nimbus_server`
- `crate::adapters`
- `crate::router`
- `crate::state`
- `crate::http`
- `crate::runtime_host`
- `crate::application_auth`
- `ConvexRegistryDeploySummary`

## Task Checklist

- [x] SBA2.1 Create `crates/nimbus-system`.
- [x] SBA2.2 Move system-owned modules.
- [x] SBA2.3 Enforce system dependency rules.
- [x] SBA2.4 Preserve behavior.

## Verification Log

1. Workspace membership:

   ```bash
   cargo metadata --no-deps --format-version 1
   ```

   Result summary: `workspace_members` includes
   `path+file:///Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-system#0.1.31`.

2. `nimbus-system` dependency graph:

   ```bash
   cargo tree -p nimbus-system --edges normal --depth 1
   ```

   Result summary:

   - workspace dependencies: `nimbus-core`, `nimbus-engine`,
     `nimbus-machine`, `nimbus-node`, `nimbus-sandbox`, `nimbus-tenant`;
   - third-party dependencies: `serde_json`, `sha2`, `tokio`, `tracing`;
   - no `nimbus-server`;
   - no adapter crate or server-private module dependency.

3. Forbidden import audit:

   ```bash
   rg -n "nimbus_server|crate::adapters|crate::router|crate::state|crate::http|crate::runtime_host|crate::application_auth|ConvexRegistryDeploySummary" crates/nimbus-system -g '*.rs'
   ```

   Result: no matches, exit code 1.

4. New crate check:

   ```bash
   cargo check -p nimbus-system
   ```

   Result: passed, finished dev profile in 2.51s after adding explicit
   `tokio` and `tracing` dependencies for the projection observer.

5. Server composition check:

   ```bash
   cargo check -p nimbus-server
   ```

   Result: passed, finished dev profile in 23.22s.

6. Moved system tests:

   ```bash
   cargo test -p nimbus-system -- --nocapture
   ```

   Result: passed. Unit tests reported 8 passed, 0 failed, 0 ignored,
   0 filtered out.

7. Server `_nimbus` integration tests:

   ```bash
   cargo test -p nimbus-server system_tenant -- --nocapture
   ```

   Result: passed. Unit tests reported 7 passed, 0 failed, 768 filtered out;
   integration filters reported 0 passed, 0 failed, 23 and 32 filtered out.

8. Deploy call-site tests:

   ```bash
   cargo test -p nimbus-server deploy -- --nocapture
   ```

   Result: passed. Unit tests reported 9 passed, 0 failed, 766 filtered out;
   integration filters reported 0 passed, 0 failed, 23 and 32 filtered out.

9. Workspace check:

   ```bash
   cargo check --workspace
   ```

   Result: passed, finished dev profile in 31.25s.

10. Formatting:

   ```bash
   cargo fmt --all --check
   ```

   Result: passed.

## Closeout

SBA2 is complete. `crates/nimbus-system` now owns system tenant identity, keys,
schema, inventory, projections, record inputs, record writers, and the
system-specific tests. `nimbus-server` retains only a thin internal
`crate::system_tenant` compatibility shim plus composition call sites. The
extracted crate imports `nimbus-node` and `nimbus-tenant` directly instead of
server-local shims, and the dependency graph proves there is no server or
adapter back-edge.
