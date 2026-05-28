# FCE4: Extract `nimbus-operator`

Status: completed
Started: 2026-05-28
Completed: 2026-05-28
Requirements: FCE-REQ-001, FCE-REQ-002, FCE-REQ-003, FCE-REQ-004, FCE-REQ-006, FCE-REQ-007, FCE-REQ-008, FCE-REQ-010

## Scope

- Files/modules moved:
  - `crates/nimbus-server/src/local_server/access.rs` -> `crates/nimbus-operator/src/access.rs`
  - `crates/nimbus-server/src/local_server/access_policy.rs` -> `crates/nimbus-operator/src/access_policy.rs`
  - `crates/nimbus-server/src/local_server/audit.rs` -> `crates/nimbus-operator/src/audit.rs`
  - `crates/nimbus-server/src/local_server/paths.rs` -> `crates/nimbus-operator/src/paths.rs`
  - `crates/nimbus-server/src/local_server/policy.rs` -> `crates/nimbus-operator/src/policy.rs`
  - `crates/nimbus-server/src/local_server/token.rs` -> `crates/nimbus-operator/src/token.rs`
- Files/modules intentionally left in `nimbus-server`:
  - Axum middleware and route mounting remain in `nimbus-server` through `crates/nimbus-server/src/local_server/middleware.rs` and `crates/nimbus-server/src/router.rs`
  - shutdown and system event effects remain in `crates/nimbus-server/src/http/local_admin.rs`
  - server discovery/listener lease remains in `crates/nimbus-server/src/local_server/discovery.rs`
  - deploy artifact staging remains in server
- Crates created or updated:
  - created `crates/nimbus-operator`
  - updated workspace `Cargo.toml`
  - updated `crates/nimbus-server/Cargo.toml`

## Ownership Decisions

- Authority owner: `nimbus-operator` owns local/deploy operator token, session, launch-ticket, origin, route-family, deploy-bearer, and audit value logic.
- Effect owner: `nimbus-operator` explicitly owns file-backed local token and audit-log persistence; server owns Axum middleware, route mounting, AppState, shutdown, and system-event persistence.
- Server composition shell: server translates Axum/AppState inputs into operator-owned policy checks and records server/system effects.
- Explicit keep decisions:
  - local/deploy operator auth remains separate from tenant application auth and does not depend on `nimbus-auth`.
  - `nimbus-operator` depends on the generic `http` crate for header/method value models, not Axum.
  - server discovery remains in `nimbus-server` because it is listener/process-lifecycle state rather than operator security policy.

## Seam Fix Attempts

- Messy seam found: local admin token/session policy, route-family/origin classification, deploy admin bearer checks, audit records, token-file effects, Axum middleware, shutdown, and server discovery were colocated in `nimbus-server`.
- Right-sized ownership-correct repair attempted: move operator security model and its explicit token/audit file effects into `nimbus-operator`; keep middleware, AppState, shutdown, deploy staging, and listener/discovery effects in server.
- Files changed or spike/proof performed:
  - `crates/nimbus-operator/Cargo.toml`
  - `crates/nimbus-operator/src/lib.rs`
  - `crates/nimbus-operator/src/{access,access_policy,audit,paths,policy,token}.rs`
  - `crates/nimbus-server/src/local_server/{mod,middleware,discovery}.rs`
  - `crates/nimbus-server/src/tests/local_admin.rs`
  - `scripts/verify-server-crate-extraction-completion.sh`
- Result: completed. `nimbus-operator` is server-free and Axum-free; server retains the transport and process-lifecycle shell.
- If blocked, exact architectural reason: n/a.
- Next implementation move: proceed to FCE5 and extract MongoDB protocol/command code while keeping TCP listener lifecycle in server.

## Dependency Evidence

```text
$ cargo tree -p nimbus-operator --edges normal --depth 1
nimbus-operator v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-operator)
├── base64 v0.22.1
├── fs2 v0.4.3
├── http v1.4.0
├── ring v0.17.14
├── serde v1.0.228
├── serde_json v1.0.149
├── tempfile v3.27.0
└── time v0.3.47
```

## Denied-Import Evidence

```text
$ rg -n "nimbus-server|nimbus_server|axum|RouterBuildConfig|AppState|crate::state|crate::router|crate::local_server|nimbus_engine|nimbus-engine|nimbus_auth|nimbus-auth|nimbus_tenant|nimbus-tenant|adapters/|tenant workload" crates/nimbus-operator -g '*.rs' -g 'Cargo.toml'
<no output>
```

```text
$ find crates/nimbus-server/src/local_server -maxdepth 1 -type f -print | sort
crates/nimbus-server/src/local_server/discovery.rs
crates/nimbus-server/src/local_server/middleware.rs
crates/nimbus-server/src/local_server/mod.rs
```

## Tests

```text
$ cargo check -p nimbus-operator
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.71s
```

```text
$ cargo check -p nimbus-server
Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.10s
```

```text
$ cargo test -p nimbus-operator -- --nocapture
running 29 tests
test audit::tests::tenant_id_from_request_extracts_firebase_project_from_metadata_headers ... ok
test access_policy::tests::deploy_admin_bearer_is_separate_from_local_admin_header_gate ... ok
test audit::tests::tenant_id_from_path_extracts_native_debug_and_convex_routes ... ok
test access_policy::tests::validate_origin_rejects_non_loopback_and_pna_preflights ... ok
test access_policy::tests::admin_header_only_ignores_local_session_cookies ... ok
test paths::tests::linux_paths_fall_back_to_home_convention ... ok
test paths::tests::linux_paths_use_xdg_overrides ... ok
test paths::tests::macos_paths_fall_back_to_application_support_run_state ... ok
test paths::tests::macos_paths_prefer_tmpdir_for_run_state ... ok
test policy::tests::parse_origin_supports_ipv4_hostnames_and_ipv6 ... ok
test paths::tests::windows_paths_use_localappdata_with_userprofile_fallback ... ok
test policy::tests::loopback_origin_requires_http_and_matching_port ... ok
test policy::tests::route_family_distinguishes_firebase_grpc_web_and_websocket_requests ... ok
test policy::tests::route_family_classifies_local_surfaces ... ok
test access_policy::tests::extract_server_access_accepts_bearer_or_admin_header ... ok
test token::tests::source_uses_ring_constant_time_compare_for_token_checks ... ok
test token::tests::token_authorization_accepts_only_exact_token_matches ... ok
test access::tests::launch_ticket_is_single_use ... ok
test token::tests::invalid_scope_is_rejected ... ok
test token::tests::corrupt_token_file_errors_clearly ... ok
test token::tests::records_persisted_without_rotated_at_load_with_none ... ok
test access::tests::session_cookie_round_trips_and_revokes_on_rotation ... ok
test audit::tests::append_writes_jsonl_record ... ok
test access::tests::live_rotation_clears_sessions_and_persists_new_generation ... ok
test token::tests::load_or_create_creates_and_reuses_token_file ... ok
test audit::tests::audit_log_is_written_with_user_only_permissions ... ok
test token::tests::offline_rotation_populates_rotated_at_freshness_window ... ok
test token::tests::offline_rotation_bumps_generation ... ok
test token::tests::token_file_is_written_with_user_only_permissions ... ok

test result: ok. 29 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.08s
```

```text
$ cargo test -p nimbus-server local_admin -- --nocapture
running 12 tests
test tests::local_server_security::native_websocket_requires_local_admin_auth ... ok
test tests::local_server_security::bad_origin_returns_forbidden_before_local_admin_auth ... ok
test tests::local_server_security::firebase_routes_remain_application_surfaces_without_local_admin_auth ... ok
test tests::core_http::tenants::local_admin_tenant_api_rejects_and_hides_reserved_system_tenants ... ok
test tests::local_admin::local_admin_rotate_endpoint_rotates_token_and_rejects_previous_bearer ... ok
test tests::local_audit::local_admin_and_origin_failures_are_audited_without_secret_material ... ok
test tests::local_server_security::deploy_admin_requires_local_admin_header_even_with_deploy_bearer ... ok
test tests::local_server_security::native_api_and_debug_routes_require_local_admin_auth ... ok
test tests::local_server_security::convex_routes_keep_application_auth_and_reject_local_admin_bearers ... ok
test tests::local_server_security::system_tenant_convex_routes_require_local_admin_auth_when_configured ... ok
test tests::local_admin::system_shutdown_endpoint_stops_live_server ... ok
test service_manager::tests::local_admin_service_lifecycle_routes_remain_server_owned ... ok

test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 710 filtered out; finished in 0.73s

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 23 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 32 filtered out; finished in 0.00s
```

Ignored tests:

- none.

## Verifier Update

- Conditions added or updated:
  - added FCE4 gate requiring `nimbus-operator` metadata presence, no `nimbus-server` dependency, denied-import absence, operator/server security test counts, direct server imports from `nimbus_operator`, and server-retained middleware/route shell proof.
- Current verifier result:

```text
$ bash scripts/verify-server-crate-extraction-completion.sh
Summary: 12 passed, 0 failed
```

## Residual Risk And Resume Notes

- Remaining risk: FCE5 must keep MongoDB TCP listener lifecycle and any AppState/global composition in server while extracting protocol/command/auth/error ownership.
- Next action: start FCE5 by auditing `crates/nimbus-server/src/adapters/mongodb`.
