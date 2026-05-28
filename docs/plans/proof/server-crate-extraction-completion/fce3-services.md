# FCE3: Extract `nimbus-services`

Status: completed
Started: 2026-05-28
Completed: 2026-05-28
Requirements: FCE-REQ-001, FCE-REQ-002, FCE-REQ-003, FCE-REQ-004, FCE-REQ-006, FCE-REQ-007, FCE-REQ-008, FCE-REQ-010

## Scope

- Files/modules moved:
  - `crates/nimbus-server/src/sandbox.rs` -> `crates/nimbus-services/src/catalog.rs`
  - `crates/nimbus-server/src/service_registry.rs` -> `crates/nimbus-services/src/registry.rs`
  - `crates/nimbus-server/src/service_manager.rs` production service manager core -> `crates/nimbus-services/src/manager.rs`
  - `crates/nimbus-server/src/service_manager/*` -> `crates/nimbus-services/src/manager/*`
- Files/modules intentionally left in `nimbus-server`:
  - HTTP service lifecycle routes in `crates/nimbus-server/src/http/services.rs`
  - AppState construction, route mounting, and runtime service source composition in `crates/nimbus-server/src/router.rs` and `crates/nimbus-server/src/state.rs`
  - direct `_nimbus` service evidence persistence adapter in `crates/nimbus-server/src/service_manager.rs`
- Crates created or updated:
  - created `crates/nimbus-services`
  - updated workspace `Cargo.toml`
  - updated `crates/nimbus-server/Cargo.toml`

## Ownership Decisions

- Authority owner: `nimbus-tenant` still owns tenant service grants; `nimbus-services` consumes admitted decisions and `nimbus-node::LocalEnforcementBinding` projections.
- Effect owner: `nimbus-services` owns sandbox backend lifecycle calls through `nimbus-sandbox` traits; server owns HTTP lifecycle routes and concrete `_nimbus` evidence persistence.
- Server composition shell: server keeps AppState construction, route mounting, system event recording, and the `SystemTenantServiceEvidenceWriter` adapter.
- Explicit keep decisions:
  - HTTP service lifecycle routes remain in `nimbus-server`.
  - `_nimbus` writes remain server/system-owned through `ServiceEvidenceWriter`.
  - `nimbus-services` may depend on `nimbus-runtime` service-binding value types, but `nimbus-runtime` does not depend on `nimbus-services`.

## Seam Fix Attempts

- Messy seam found: service registry, sandbox catalog traits, service manager activation, runtime lookup, sandbox lifecycle, evidence writes, and HTTP lifecycle routes were colocated in `nimbus-server`.
- Right-sized ownership-correct repair attempted: move the service core and its narrow evidence trait into `nimbus-services`; leave concrete system evidence persistence and HTTP route composition in server.
- Files changed or spike/proof performed:
  - `crates/nimbus-services/Cargo.toml`
  - `crates/nimbus-services/src/lib.rs`
  - `crates/nimbus-services/src/catalog.rs`
  - `crates/nimbus-services/src/registry.rs`
  - `crates/nimbus-services/src/manager.rs`
  - `crates/nimbus-services/src/manager/*`
  - `crates/nimbus-server/src/service_manager.rs`
  - server imports in router, state, construction, Cloud Functions, Convex runtime paths, and tests
  - `scripts/verify-server-crate-extraction-completion.sh`
- Result: completed. `nimbus-services` owns service registry/manager core and is server-free; server owns `_nimbus` evidence persistence and HTTP route shell.
- If blocked, exact architectural reason: n/a.
- Next implementation move: proceed to FCE4 and extract `nimbus-operator` without moving Axum middleware, route mounting, deploy staging, or tenant application auth into the operator crate.

## Dependency Evidence

```text
$ cargo tree -p nimbus-services --edges normal --depth 1
nimbus-services v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-services)
├── futures v0.3.32
├── nimbus-core v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-core)
├── nimbus-node v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-node)
├── nimbus-runtime v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-runtime)
├── nimbus-sandbox v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-sandbox)
├── nimbus-tenant v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-tenant)
└── tokio v1.51.1
```

## Denied-Import Evidence

```text
$ rg -n "nimbus-server|nimbus_server|axum|RouterBuildConfig|AppState|crate::state|crate::router|crate::system_tenant|nimbus_engine|nimbus-system|nimbus_system|std::process|Command::new|Stdio" crates/nimbus-services -g '*.rs' -g 'Cargo.toml'
<no output>
```

```text
$ rg -n "mod sandbox|mod service_registry|crate::sandbox|crate::service_registry|crate::service_manager::SandboxServiceManager" crates/nimbus-server/src -g '*.rs'
<no output>
```

```text
$ rg -n "record_service_handle_async|SystemTenantServiceEvidenceWriter|attach_system_state_service" crates/nimbus-server/src/service_manager.rs
crates/nimbus-server/src/service_manager.rs:19:struct SystemTenantServiceEvidenceWriter {
crates/nimbus-server/src/service_manager.rs:23:impl SystemTenantServiceEvidenceWriter {
crates/nimbus-server/src/service_manager.rs:31:impl ServiceEvidenceWriter for SystemTenantServiceEvidenceWriter {
crates/nimbus-server/src/service_manager.rs:38:            nimbus_system::record_service_handle_async(&self.service, tenant_id, handle).await
crates/nimbus-server/src/service_manager.rs:43:pub(crate) fn attach_system_state_service(
crates/nimbus-server/src/service_manager.rs:48:        Arc::new(SystemTenantServiceEvidenceWriter::new(service)),
```

## Tests

```text
$ cargo check -p nimbus-services
Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.34s
```

```text
$ cargo check -p nimbus-server
Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.79s
```

```text
$ cargo test -p nimbus-services -- --nocapture
running 22 tests
test catalog::tests::empty_catalog_returns_no_tenant_sandboxes ... ok
test catalog::tests::empty_service_catalog_returns_none_for_unknown_service ... ok
test catalog::tests::empty_catalog_returns_none_for_unknown_service ... ok
test manager::tests::ensure_service_binding_sync_lookup_stays_snapshot_only_for_missing_service ... ok
test manager::tests::start_service_for_decision_rejects_unadmitted_sandbox_egress_before_launch ... ok
test manager::tests::start_service_for_decision_rejects_unadmitted_service_before_launch ... ok
test manager::tests::ensure_service_binding_async_uses_build_launch_for_build_backed_service ... ok
test registry::tests::resolve_service_binding_rejects_handle_for_a_different_tenant ... ok
test registry::tests::resolve_service_binding_returns_binding_for_named_service ... ok
test registry::tests::snapshot_selects_tcp_as_primary_endpoint ... ok
test registry::tests::snapshot_skips_sandboxes_for_a_different_tenant ... ok
test registry::tests::snapshot_skips_sandboxes_without_ready_endpoints ... ok
test manager::tests::start_service_for_decision_accepts_matching_sandbox_egress_policy ... ok
test manager::tests::ensure_service_binding_async_can_be_cancelled_while_waiting_for_readiness ... ok
test manager::tests::ensure_service_binding_async_rejects_backend_handle_for_wrong_tenant ... ok
test manager::tests::teardown_tenant_stops_tracked_sandboxes_and_clears_snapshot ... ok
test manager::tests::stop_service_for_context_async_stops_active_handle_and_clears_snapshot ... ok
test manager::tests::reload_service_egress_for_decision_updates_active_backend_policy ... ok
test manager::tests::restart_service_for_context_async_stops_then_starts_service ... ok
test manager::tests::ensure_service_binding_async_starts_declared_image_service_once ... ok
test manager::tests::start_service_for_decision_rejects_unverified_image_before_materialization ... ok
test manager::tests::start_service_for_decision_admits_verified_image_before_materialization ... ok

test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.21s
```

```text
$ cargo test -p nimbus-server service_evidence_writer_records_observed_state_to_system_tenant -- --nocapture
running 1 test
test service_manager::tests::service_evidence_writer_records_observed_state_to_system_tenant ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 749 filtered out; finished in 0.50s

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 23 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 32 filtered out; finished in 0.00s
```

```text
$ cargo test -p nimbus-server local_admin_service_lifecycle_routes_remain_server_owned -- --nocapture
running 1 test
test service_manager::tests::local_admin_service_lifecycle_routes_remain_server_owned ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 749 filtered out; finished in 0.51s

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 23 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 32 filtered out; finished in 0.00s
```

Ignored tests:

- none.

## Verifier Update

- Conditions added or updated:
  - added FCE3 gate requiring `nimbus-services` metadata presence, no `nimbus-server` dependency, denied-import absence, focused service lifecycle/security test counts, direct server imports from `nimbus_services`, and server-owned system evidence writer proof.
- Current verifier result:

```text
$ bash scripts/verify-server-crate-extraction-completion.sh
Summary: 11 passed, 0 failed
```

## Residual Risk And Resume Notes

- Remaining risk: FCE4 must keep operator/security policy separate from tenant application auth and from Axum route mounting.
- Next action: start FCE4 by auditing local admin token/session files, deploy admin bearer checks, route-family/origin policy, and shutdown/audit effects before moving code.
