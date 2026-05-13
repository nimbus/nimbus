# Reliability and Modularity Cleanup Plan

Status: active

This plan owns the next reliability, CI clarity, and large-file cleanup wave.
It follows the current testing reliability posture and CI failure investigation
guidance rather than reopening archived historical plans.

## Principles

- Prefer explicit proof lanes over ambient test behavior.
- Keep broad CI buckets named by what they prove: style, lint, dependency
  audit, Rust runtime, Rust workspace, external providers, harness, JavaScript,
  proof helpers, and coverage.
- Keep composition roots thin once a file has been split.
- Treat generated compatibility evidence as the source of truth for generated
  docs; hand-written docs should explain interpretation, not duplicate rows.
- Because Nimbus has not launched, remove legacy code paths directly instead of
  preserving compatibility shims.

## ER1: External Provider Test Lane

Status: in_progress

Own the Postgres, MySQL, and libsql provider suites as an explicit integration
lane.

- Keep default Rust workspace tests deterministic by disabling implicit
  provider fixtures.
- Add a local `make` entrypoint that requires explicit provider fixture env vars
  and runs the storage and engine provider suites.
- Add a named GitHub Actions job with first-class Postgres, MySQL, and libsql
  services.
- Include the lane in the Rust gate summary so failures are visible as provider
  integration failures, not hidden workspace or coverage failures.

Completion gate:

- `make -n test-external-providers`
- `git diff --check`
- Hosted CI shows `External Provider Integration Tests` as a distinct required
  proof lane.

## ER2: Runtime Module Loader Decomposition

Status: complete

Split `crates/nimbus-runtime/src/module_loader.rs` into concept-owned modules.
The root should remain the public composition point while resolution, package
metadata, CommonJS/ESM bridging, permission-root handling, and test support move
behind narrower modules.

Completion gate:

- No runtime module-loader source file is over the repo modularity threshold
  unless the active plan records a specific ownership exception.
- Existing module-loader tests keep their behavioral assertions and names unless
  a rename improves the public failure signal.

## ER3: Large Test File Decomposition

Status: complete

Reduce large test modules by ownership instead of line count alone.

Initial candidates:

- `crates/nimbus-runtime/src/runtime/tests/basic_invocation.rs`
- `crates/nimbus-runtime/src/runtime/bootstrap/ops/test_runtime.rs`
- `crates/nimbus-bin/src/start/tests.rs`
- `crates/nimbus-server/src/adapters/mongodb/commands/crud/tests.rs`

Completion gate:

- Files over 1,500 lines either move below the threshold or have a narrow,
  plan-recorded ownership exception.
- Split modules keep failure names discoverable and do not collapse behavioral
  assertions into compile-only checks.

## ER4: Generated Node Compatibility Docs

Status: complete

Make the large Node compatibility matrix a generated summary from the manifest
and evidence artifacts. Hand-written runtime docs should link to generated
evidence and explain support posture, not manually duplicate test catalog data.

Completion gate:

- The generation path has a check mode suitable for CI.
- The checked-in matrix can be regenerated without unrelated churn.

## ER5: Prelaunch Legacy Cleanup Audit

Status: complete

Audit legacy labels, aliases, and compatibility scaffolding that remain only
because the project previously carried transitional code. Keep compatibility
where it names an external adapter contract; remove it where it only preserves
old Nimbus internals.

Initial audit points:

- `crates/nimbus-core/src/query.rs` legacy planner comments and ownership.
- CLI alias language in `docs/operating/cli.md` and matching implementation.
- Any remaining plan-item labels such as `NLC*` that are not domain concepts.

Completion gate:

- Compatibility code that remains is tied to an external contract or documented
  adapter promise.
- Transitional labels are renamed to domain terms or removed.

## Execution Log

| Date | Slice | Status | Notes |
| --- | --- | --- | --- |
| 2026-05-13 | ER0 | complete | Created active plan from current reliability guidance and current file/CI review. |
| 2026-05-13 | ER1 | in_progress | Starting explicit external-provider CI lane and local `make` entrypoint. |
| 2026-05-13 | ER2 | complete | Split embedded Node builtin sources and bundle code-cache state out of `module_loader.rs`; root loader and all owned source assets are below the modularity threshold. Verified with `cargo check -p nimbus-runtime`, `cargo test -p nimbus-runtime module_loader`, and a focused Node22 fs/promises runtime invocation test. |
| 2026-05-13 | ER3 | in_progress | Split `runtime/tests/basic_invocation.rs` into support, web-standard, Node bootstrap, Node capability, and package-resolution modules; updated canary registry validation to scan the split source set. Verified with `cargo test -p nimbus-runtime basic_invocation -- --list`, the canary registry topology test, and a focused Node capability invocation. |
| 2026-05-13 | ER3 | in_progress | Split `nimbus-bin/src/start/tests.rs` into CLI surface, app-dir/codegen, persistence, krun, encryption, and license modules. Verified with `cargo test -p nimbus-bin start::tests -- --list` and a focused CLI help test. |
| 2026-05-13 | ER3 | in_progress | Split `runtime/bootstrap/ops/test_runtime.rs` into parser, types, invocation, op registration, bundle filesystem, and generated-bundle renderer modules. Verified with `cargo test -p nimbus-runtime basic_invocation -- --list`, `cargo test -p nimbus-runtime node_compat_supplementary_runtime_node22 -- --nocapture`, and a spawn-backed Node22 module-wrapper fixture. |
| 2026-05-13 | ER3 | complete | Split `nimbus-server/src/adapters/mongodb/commands/crud/tests.rs` into Mongo command-family modules for insert, find, update, delete, findAndModify, count, and distinct. Verified with `cargo test -p nimbus-server adapters::mongodb::commands::crud::tests -- --list` and `cargo test -p nimbus-server adapters::mongodb::commands::crud::tests` (88 tests passed). |
| 2026-05-13 | ER4 | complete | Added `--check` support to the generated Node.js evidence publisher, exposed it through `make node-compat-publish-docs CHECK=1`, regenerated the checked-in public evidence page, and turned the architecture surface matrix into a generated-evidence index instead of a hand-maintained duplicate table. Verified with `make node-compat-publish-docs CHECK=1` and the canary registry claim test. |
| 2026-05-13 | ER5 | complete | Removed active-surface transition labels from query comments, CLI docs, compose discovery naming, and CLI tests. Remaining `legacy` and `NLC*` hits are archival history, upstream fixture names, or protocol names rather than active Nimbus compatibility scaffolding. Verified with focused core, compose discovery, parse/help, encryption, and retired-command tests. |
