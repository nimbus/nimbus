# FCE9: Optional `nimbus-adapters` Facade

Status: completed
Started: 2026-05-28
Completed: 2026-05-28
Requirements: FCE-REQ-001, FCE-REQ-002, FCE-REQ-007, FCE-REQ-008, FCE-REQ-009, FCE-REQ-010

## Scope

- Files/modules moved: none. FCE9 intentionally adds no implementation owner.
- Files/modules intentionally left in `nimbus-server`: route mounting, listener lifecycle, AppState construction, shutdown, deployment composition, operator audit, and all transport shells.
- Crates created or updated:
  - Created `crates/nimbus-adapters`.
  - Added it to the workspace.

## Ownership Decisions

- Authority owner: unchanged. Tenant, auth, bridge, provenance, system, and per-adapter crates keep their existing authority boundaries.
- Effect owner: unchanged. The facade owns no process execution, persistence, runtime invocation, listener lifecycle, WebSocket handling, or route mounting.
- Server composition shell: unchanged. Composition remains with the application integration layer and the server crate.
- Explicit keep decisions: per-adapter implementation remains in `nimbus-mongodb`, `nimbus-firebase`, `nimbus-cloud-functions`, and `nimbus-convex`; the facade is a feature-gated re-export-only facade.

## Seam Fix Attempts

- Messy seam found: none in this phase. FCE5-FCE8 already proved the per-adapter crates are clean enough to aggregate.
- Right-sized ownership-correct repair attempted:
  - Added `nimbus-adapters` as a crate with optional dependencies and feature flags for each per-adapter crate.
  - Set `default = []` so consumers must opt into the adapter family they need instead of pulling every adapter by default.
  - Exported one module per adapter crate using only `pub use`.
  - Added verifier checks that deny server/effect/composition imports and any facade-owned implementation logic.
- Files changed or spike/proof performed:
  - `Cargo.toml`
  - `crates/nimbus-adapters/Cargo.toml`
  - `crates/nimbus-adapters/src/lib.rs`
  - `scripts/verify-server-crate-extraction-completion.sh`
- Result: `nimbus-adapters` exists as a facade only; it does not centralize adapter behavior or create a hidden composition layer.
- If blocked, exact architectural reason: not blocked.
- Next implementation move: proceed to FCE10 final closeout.

## Dependency Evidence

```text
`cargo check -p nimbus-adapters`: passed

`cargo tree -p nimbus-adapters --edges normal --depth 1`:
nimbus-adapters v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-adapters)

`cargo tree -p nimbus-adapters --all-features --edges normal --depth 1`:
nimbus-adapters v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-adapters)
├── nimbus-cloud-functions v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-cloud-functions)
├── nimbus-convex v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-convex)
├── nimbus-firebase v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-firebase)
└── nimbus-mongodb v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-mongodb)

Full `crate_has_no_server_dependency nimbus-adapters` verifier check passed.
```

## Denied-Import Evidence

```text
rg -n 'nimbus[-_]server|AppState|RouterBuildConfig|crate::state|crate::router|crate::local_server|crate::system_tenant|crate::application_auth|crate::execution|axum|tower|tonic|WebSocket|WebSocketUpgrade|listener|shutdown|record_deployment_state_async|upsert_system_document|SystemDeploymentRecordInput|_nimbus|std::process|process::Command|Command::new' crates/nimbus-adapters -g '*.rs' -g 'Cargo.toml'

Result: no matches, command exited 1 as expected.

rg -n '(^|[[:space:]])(fn|struct|enum|trait|impl)[[:space:]]|macro_rules!|tokio::|std::process|process::Command|Command::new|axum|AppState|RouterBuildConfig|WebSocket|listener|shutdown|record_deployment_state_async|upsert_system_document|SystemDeploymentRecordInput|_nimbus' crates/nimbus-adapters/src crates/nimbus-adapters/Cargo.toml

Result: no matches, command exited 1 as expected.
```

## Tests

```text
`cargo test -p nimbus-adapters -- --nocapture`: 0 passed; 0 failed; 0 ignored.

Unit target:
running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out.

Doc-tests:
running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out.
```

Ignored tests:

- None.

## Verifier Update

- Conditions added or updated:
  - Step 7 now allows the optional facade only when FCE9 is active or complete and FCE5-FCE8 are already complete.
  - Step 17 enforces completed FCE9 proof, crate metadata, no `nimbus-server` dependency, denied imports, opt-in default features, feature-gated dependencies, adapter re-export modules, no implementation logic, and exact check/test evidence.
- Current verifier result: `bash scripts/verify-server-crate-extraction-completion.sh`: 17 passed; 0 failed while FCE9 was in progress.

## Residual Risk And Resume Notes

- Remaining risk: none for facade dependency bloat; adapter families are now opt-in through explicit Cargo features.
- Next action: mark FCE10 in progress, run the final verifier, focused tests, formatting, workspace check, and record the enterprise-trust closeout.
