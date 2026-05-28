# FCE10: Final Closeout

Status: completed
Started: 2026-05-28
Completed: 2026-05-28
Requirements: FCE-REQ-001, FCE-REQ-002, FCE-REQ-003, FCE-REQ-004, FCE-REQ-005, FCE-REQ-006, FCE-REQ-007, FCE-REQ-008, FCE-REQ-009, FCE-REQ-010

## Scope

- Files/modules moved: none in FCE10. This phase verifies and closes the extraction wave.
- Files/modules intentionally left in `nimbus-server`:
  - Route mounting, listener lifecycle, AppState construction, shutdown, deployment composition, request cancellation, concrete runtime invocation, process-backed verifier execution, system evidence persistence, and transport shells.
- Crates created or updated across the full wave:
  - `nimbus-artifacts`
  - `nimbus-provenance`
  - `nimbus-services`
  - `nimbus-operator`
  - `nimbus-mongodb`
  - `nimbus-firebase`
  - `nimbus-cloud-functions`
  - `nimbus-convex`
  - `nimbus-adapters`

## Ownership Decisions

- Authority owner: `nimbus-tenant`, `nimbus-auth`, `nimbus-artifacts`, and `nimbus-provenance` own admitted policy/value decisions; lower layers consume decisions or narrow projections rather than deriving authority from raw IDs, paths, claims, or request metadata.
- Effect owner: host effects remain concrete at the server/operator boundary. Artifact process execution, route mounting, WebSocket/session lifecycle, listener startup, AppState wiring, deployment persistence, and system evidence writes are not hidden inside extracted pure crates.
- Server composition shell: `nimbus-server` is now primarily transport, adapter wiring, persistence composition, runtime invocation composition, and operational lifecycle.
- Explicit keep decisions: `nimbus-runtime` remains zero workspace dependencies; `nimbus-core` remains zero I/O; `nimbus-adapters` is a feature-gated facade only.

## Seam Fix Attempts

- Messy seam found: the closeout audit found no new ownership seam requiring code movement. The final verification sweep did find enterprise-readiness cleanups in extracted and shared crates: deploy-admin credential isolation, Mongo SCRAM identity/entropy checks, Convex OIDC/JWKS fetch bounds and cache refresh, facade dependency discipline, and workspace clippy findings after the crate split.
- Right-sized ownership-correct repair attempted:
  - Hardened the deploy-admin header-only path so local session cookies cannot satisfy deploy-admin requests.
  - Hardened Mongo SCRAM with configured-username enforcement, CSPRNG salt/nonce generation, and constant-time proof comparison.
  - Added bounded Convex auth metadata fetching, JWKS/OIDC cache refresh, and retry-after-refresh for key rotation.
  - Kept `nimbus-adapters` default features empty so the facade stays opt-in and does not pull every adapter by default.
  - Fixed clippy-gated shape issues exposed by the extracted crates: boxed large schema events, derived lifecycle defaults, storage iterator/return cleanup, Mongo structural lints, Firebase internal gRPC lowering errors, and Cloud Functions invocation argument bundling.
- Files changed or spike/proof performed:
  - `scripts/verify-server-crate-extraction-completion.sh`
  - `docs/plans/server-crate-extraction-completion-plan.md`
  - `docs/plans/proof/server-crate-extraction-completion/fce10-closeout.md`
- Result:
  - No required phase is blocked.
  - Every FCE0-FCE10 phase is completed.
  - Final verifier result: 18 passed; 0 failed.
- If blocked, exact architectural reason: not blocked.
- Next implementation move: none for this plan.

## Dependency Evidence

```text
`cargo tree -p nimbus-server --edges normal --depth 1`:
nimbus-server v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-server)
├── nimbus-artifacts v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-artifacts)
├── nimbus-auth v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-auth)
├── nimbus-bridge v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-bridge)
├── nimbus-cloud-functions v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-cloud-functions)
├── nimbus-convex v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-convex)
├── nimbus-core v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-core)
├── nimbus-engine v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-engine)
├── nimbus-firebase v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-firebase)
├── nimbus-license v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-license)
├── nimbus-machine v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-machine)
├── nimbus-mongodb v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-mongodb)
├── nimbus-node v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-node)
├── nimbus-operator v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-operator)
├── nimbus-provenance v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-provenance)
├── nimbus-runtime v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-runtime)
├── nimbus-sandbox v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-sandbox)
├── nimbus-services v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-services)
├── nimbus-system v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-system)
└── nimbus-tenant v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-tenant)

`cargo tree -p nimbus-adapters --edges normal --depth 1`:
nimbus-adapters v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-adapters)

`cargo tree -p nimbus-adapters --all-features --edges normal --depth 1`:
nimbus-adapters v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-adapters)
├── nimbus-cloud-functions v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-cloud-functions)
├── nimbus-convex v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-convex)
├── nimbus-firebase v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-firebase)
└── nimbus-mongodb v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-mongodb)
```

## Denied-Import Evidence

```text
`bash scripts/verify-server-crate-extraction-completion.sh`: 18 passed; 0 failed.

The verifier enforces, per completed phase:
- extracted crates exist in Cargo metadata;
- extracted crates do not depend on `nimbus-server`;
- denied imports for AppState/router/listener/system-persistence/process/server-private seams are absent;
- required moved symbols exist in owner crates;
- required server-owned shells remain in server;
- old server-owned copies are removed where extraction moved source files.
```

## Tests

```text
`CARGO_INCREMENTAL=0 cargo test -p nimbus-artifacts -p nimbus-provenance -p nimbus-services -p nimbus-operator -p nimbus-mongodb -p nimbus-firebase -p nimbus-cloud-functions -p nimbus-convex -p nimbus-system -p nimbus-adapters`: passed

nimbus-artifacts: 6 passed; 0 failed; 0 ignored
nimbus-provenance: 1 passed; 0 failed; 0 ignored
nimbus-services: 22 passed; 0 failed; 0 ignored
nimbus-operator: 29 passed; 0 failed; 0 ignored
nimbus-mongodb: 263 passed; 0 failed; 0 ignored
nimbus-firebase: 42 passed; 0 failed; 0 ignored
nimbus-cloud-functions: 20 passed; 0 failed; 0 ignored
nimbus-convex: 6 passed; 0 failed; 0 ignored
nimbus-system: 8 passed; 0 failed; 0 ignored
nimbus-adapters: 0 passed; 0 failed; 0 ignored

`cargo fmt --all --check`: passed
`CARGO_INCREMENTAL=0 cargo check --workspace`: passed
`make clippy`: passed

`CARGO_INCREMENTAL=0 cargo test -p nimbus-core -p nimbus-storage -p nimbus-artifacts -p nimbus-provenance -p nimbus-services -p nimbus-operator -p nimbus-mongodb -p nimbus-firebase -p nimbus-cloud-functions -p nimbus-convex -p nimbus-system -p nimbus-adapters -- --nocapture`: passed after rerun with local-port permission for libsql provider tests

nimbus-core: 95 passed; 0 failed; 0 ignored
nimbus-storage: 245 passed; 0 failed; 2 ignored
```

Ignored tests:

- The expanded affected-crate sweep had two intentionally ignored `nimbus-storage` generated-history harness tests:
  - `tests::generated_history::verification_harness_nightly_generated_history_seed_corpus_matches_model`: verification harness nightly corpus runs in dedicated harness lanes.
  - `tests::generated_history::verification_harness_required_generated_history_seed_corpus_matches_model`: verification harness required corpus runs in dedicated harness lanes.
- Prior focused Convex server lane still reported these intentionally ignored tests:
  - `tests::convex_functions::runtime_queries::execution::services::convex_runtime_query_starts_real_krun_service_under_manager_and_tears_it_down`: requires a Linux host with KVM, buildah, conmon, and network access.
  - `tests::convex_runtime::http_routes::demo_flow::seeded_usage::verification_harness_nightly_generated_history_seed_corpus_matches_model_on_convex_demo_surface`: verification harness nightly corpus runs in dedicated harness lanes.
  - `tests::convex_runtime::http_routes::demo_flow::seeded_usage::verification_harness_nightly_generated_history_seed_corpus_matches_model_on_faulted_convex_demo_surface`: verification harness nightly corpus runs in dedicated harness lanes.
  - `tests::convex_runtime::http_routes::demo_flow::seeded_usage::verification_harness_required_generated_history_seed_corpus_matches_model_on_convex_demo_surface`: verification harness required corpus runs in dedicated harness lanes.
  - `tests::convex_runtime::http_routes::demo_flow::seeded_usage::verification_harness_required_generated_history_seed_corpus_matches_model_on_faulted_convex_demo_surface`: verification harness required corpus runs in dedicated harness lanes.

## Enterprise-Trust Review

- Authority flow: tenant and application authority is explicit in `nimbus-tenant` and `nimbus-auth`; runtime and adapter code consumes admitted decisions, verified auth projections, or narrow capability inputs.
- Side-effect ownership: pure artifact/provenance/adapter contracts do not launch host verifier processes or own server persistence; concrete effects remain in server/operator wiring.
- Dependency direction: extracted crates point toward core/runtime/engine/tenant primitives as needed; none point back to `nimbus-server`.
- Fail-closed coverage: verifier-backed tests cover bad/missing artifact and provenance evidence, wrong-tenant Firestore/Mongo/Convex access, denied service grants, local/deploy operator auth, invalid Convex auth metadata/JWT shapes, and runtime bundle provenance admission.
- Maintainability posture: server now composes named architecture crates rather than owning protocol/auth/provenance/service/operator internals inline; `nimbus-adapters` gives consumers a discoverable facade without hiding implementation logic.

## Verifier Update

- Conditions added or updated:
  - Step 3 now accepts zero active phases only when FCE0-FCE10 are all completed and FCE10 proof is complete.
  - Step 18 enforces FCE10 final verifier result, moved-crate focused test counts, formatting, workspace check, ignored-test reasons, dependency evidence, and enterprise-trust review.
- Current verifier result: Final verifier result: 18 passed; 0 failed.

## Residual Risk And Resume Notes

- Remaining risk: full `make test` and `make deny` were not run as part of this plan closeout; the closeout did run the extraction verifier, formatter, workspace check, workspace clippy, and affected crate tests.
- Environment note: the first expanded affected-crate test sweep failed in the sandbox because libsql provider tests could not bind a temporary local port (`PermissionDenied`). The same command passed when rerun with local-port permission.
- Next action: archive this active plan from `docs/plans/README.md` only if the repo's plan lifecycle expects completed active plans to move out of the active index in a separate commit.
