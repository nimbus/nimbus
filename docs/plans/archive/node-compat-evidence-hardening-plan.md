# Node Compatibility Evidence Hardening Plan

Status: `done`

Owner: Node-compatible runtime / test infrastructure

This plan owns the post-NLC trust hardening wave for Node compatibility test
infrastructure. The NLC implementation baseline is useful, but the final audit
found remaining evidence gaps that should not be hidden behind completed plan
language: suite-wide status, machine-readable expectations, sync tooling,
harness modularity, supplementary breadth, and durable dashboard evidence.

## Scope

- Keep the current Node22 primary / Node20 validation / Node24 preview lane
  model.
- Preserve the NLC runtime support claim: fixture-backed Node22 compatibility
  with profile-scoped exclusions, not blanket full Node parity.
- Turn pass-rate language into measured output from checked-in tooling.
- Move status and expectation data out of prose and `#[ignore]` annotations
  into machine-readable artifacts.
- Reduce `node_compat.rs` as the current de facto inventory owner by extracting
  data and behavior into narrower modules or manifests.

## Non-Goals

- Do not weaken existing fixtures or change expected values to make reports
  greener.
- Do not claim full Node suite parity until the suite denominator, expected
  failures, skips, and unclassified files are all machine-readable.
- Do not promote Node24 from preview visibility to a public runtime contract in
  this plan.

## Checkpoints

| ID | Status | Outcome |
| --- | --- | --- |
| NCH1 | `done` | Add `make node-compat-status`, a suite-wide denominator report that counts vendored lane-local `test-*` fixtures and compares them with documented manifested green counts. Dashboard output consumes the status report when present. |
| NCH2 | `done` | Added `tests/node-compat/expectations/rust-watchpoints.json`, `make node-compat-expectations-validate`, schema-backed catalog validation against Rust `#[ignore]` watchpoints, and observed-results unexpected-pass detection. |
| NCH3 | `done` | Accepted the current `node_compat.rs` oversized ownership exception for the embedded Node fixture harness, moved supplementary batch inventory into `node_compat/supplementary_batches.rs`, and moved evidence/status/sync behavior into concept-owned Python tools, manifests, and docs instead of adding more inline harness logic. |
| NCH4 | `done` | Added reproducible fixture sync tooling (`make node-compat-sync LANE=<lane> DRY_RUN=1`) with lane provenance, local denominator counts, command plans, schema-validated reports, and compare/apply modes guarded behind explicit flags. |
| NCH5 | `done` | Added the `runtime-supplementary` and `runtime-supplementary-signal-lifecycle` manifest families. Resource-safety and framework-loader probes are green on Node20/22/24 lanes; signal listener lifecycle is a measured expected-failure watchpoint for missing `Deno.addSignalListener` in the embedded host path. |
| NCH6 | `done` | Added `make node-compat-publish-evidence`, checked-in latest evidence snapshots under `docs/architecture/runtime/node-compat-evidence/latest/`, trend snapshots, and nightly artifact publishing for the curated evidence snapshot. |

## Current Evidence Baseline

`make node-compat-status` currently reports the following checked-in
denominator:

| Lane | Upstream | Vendored `test-*` files | Documented manifested green | Unmanifested / unclassified |
| --- | --- | ---: | ---: | ---: |
| `node20` | `v20.20.2` | 1308 | 913 | 395 |
| `node22` | `v22.15.0` | 1283 | 994 | 289 |
| `node24` | `v24.15.0` | 1495 | 925 | 570 |

These are not full-suite pass rates. They are the current truthful denominator
needed to drive the rest of this plan.

## Harness Modularity Decision

`crates/nimbus-runtime/src/runtime/tests/node_compat.rs` remains above the
repo-wide line threshold as an explicit Node-compat harness ownership exception.
The file is the embedded Rust entrypoint for a large checked-in upstream corpus:
most of its size is fixture batch inventory, materialization wiring, and
lane-specific test entrypoints that must stay close to the runtime harness until
the next safe decomposition window.

This plan still reduces the growth pressure on that file:

- expectation data now lives in `tests/node-compat/expectations/`
- JSON artifact schemas now live in `tests/node-compat/schemas/`
- suite status, dashboard publishing, and sync behavior live in
  `scripts/node_compat/`
- the new supplementary fixture batch inventory lives in
  `node_compat/supplementary_batches.rs`
- supplementary evidence is cataloged through manifest JSON and docs

New Node-compat evidence work should prefer manifests, fixtures, and
concept-owned scripts over adding new inline status logic to `node_compat.rs`.

## Verification

- `python3 -m py_compile scripts/node_compat/schema.py scripts/node_compat/expectations.py scripts/node_compat/status.py scripts/node_compat/sync.py scripts/node_compat/refresh.py scripts/node_compat/trends.py scripts/node_compat/publish_evidence.py scripts/node_compat/dashboard.py scripts/node_compat/canary_registry.py`
- `python3 scripts/node_compat/schema.py validate --schema rust-watchpoints.schema.json --instance tests/node-compat/expectations/rust-watchpoints.json`
- `python3 scripts/node_compat/schema.py validate --schema fixture-sync-report.schema.json --instance target/node-compat/sync/node22-sync.json`
- `python3 scripts/node_compat/schema.py validate --schema refresh-report.schema.json --instance target/node-compat/refresh/node22-refresh.json`
- `python3 scripts/node_compat/schema.py validate --schema trend-snapshot.schema.json --instance target/node-compat/trends/trend-summary.json`
- `ruby -e 'require "yaml"; YAML.load_file(".github/workflows/node-compat-nightly.yml"); puts "yaml-ok"'`
- `make node-compat-expectations-validate`
- `make node-compat-status`
- `make node-compat-sync LANE=node20 DRY_RUN=1`
- `make node-compat-sync LANE=node22 DRY_RUN=1`
- `make node-compat-sync LANE=node24 DRY_RUN=1`
- `make node-compat-sync LANE=node22 TAG=v22.15.0 DRY_RUN=1`
- `make node-compat-refresh LANE=node22 TAG=v22.15.0 DRY_RUN=1`
- `make node-compat-dashboard`
- `make node-compat-trends`
- `make node-compat-publish-evidence`
- `make node-compat-validate-claims`
- `python3 scripts/node_compat/expectations.py validate --observed-results /private/tmp/node-compat-unexpected-pass.json`
  intentionally returns nonzero for a cataloged unexpected pass
- `python3 scripts/node_compat/status.py --output-root /private/tmp/node-compat-status-unexpected-pass --observed-results /private/tmp/node-compat-unexpected-pass.json`
  intentionally returns nonzero for the same unexpected pass
- `node crates/nimbus-runtime/src/runtime/tests/node_compat_fixtures/supplementary/resource-safety.mjs`
- `node crates/nimbus-runtime/src/runtime/tests/node_compat_fixtures/supplementary/framework-loader-patterns.mjs`
- `node crates/nimbus-runtime/src/runtime/tests/node_compat_fixtures/supplementary/signal-listener-lifecycle.mjs`
- `cargo test -p nimbus-runtime node_compat_runtime_supplementary -- --nocapture`
- `cargo test -p nimbus-runtime node_compat_manifest_topology_loader_composes_deterministically_from_disk -- --nocapture`
- `cargo test -p nimbus-runtime node_compat_supplementary_runtime_ -- --nocapture --test-threads=1`
- `cargo test -p nimbus-runtime node_compat_supplementary_signal_lifecycle_watchpoint_ -- --nocapture --test-threads=1`
- `make node-compat-report FAMILY=runtime-supplementary SLICE=supplementary-resource-safety CAPTURE_LIVE=1`
- `make node-compat-report FAMILY=runtime-supplementary SLICE=supplementary-framework-loader-patterns CAPTURE_LIVE=1`
- `make node-compat-report FAMILY=runtime-supplementary-signal-lifecycle SLICE=supplementary-signal-listener-lifecycle CAPTURE_LIVE=1`
- `cargo fmt --all --check`

## Progress Log

| Date | Checkpoint | Status | Notes |
| --- | --- | --- | --- |
| 2026-05-11 | NCH1 | `done` | Added `scripts/node_compat/status.py`, `make node-compat-status`, dashboard integration for the status artifact, `tests/node-compat/README.md` docs, and corrected the public Node22 green count from `995+` to the generated `994` manifested denominator. |
| 2026-05-11 | NCH2 | `done` | Promoted the Rust `#[ignore]` watchpoint inventory into `tests/node-compat/expectations/rust-watchpoints.json`; validation now fails on catalog drift and observed unexpected passes. |
| 2026-05-11 | NCH3 | `done` | Documented the `node_compat.rs` harness-size exception and moved new evidence behavior into scripts/manifests/docs rather than adding inline status logic. |
| 2026-05-11 | NCH4 | `done` | Added `scripts/node_compat/sync.py` and `make node-compat-sync LANE=<lane> DRY_RUN=1`; dry-runs now emit per-lane sync reports with upstream tag/subtree provenance. |
| 2026-05-12 | NCH5 | `done` | Re-ran runtime verification after `rusty_v8 v147.4.0-locker.2` published. Resource-safety and framework-loader probes are green across Node20/22/24, and signal listener lifecycle is recorded as a measured expected-failure watchpoint. |
| 2026-05-11 | NCH6 | `done` | Added durable evidence publishing via `make node-compat-publish-evidence`, checked-in latest snapshots, and nightly artifact publication for the curated evidence bundle. |
| 2026-05-12 | Follow-up | `done` | Added `make node-compat-refresh` for coordinated lane refresh dry-runs, converted dashboard markdown sections to table-first evidence views, added schema validation for catalogs/sync/refresh/trends, added `make node-compat-trends` to the nightly workflow, and extracted supplementary batch inventory out of the harness root. |
