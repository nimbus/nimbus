# MIK4 Cross-cutting verification and pull request

Date: 2026-09-09. Branch `codex/mysql-index-keyspace` at d43e4b2f1 (code) plus
the plan commits.

## Verification

All commands ran with the fixture environment (`NIMBUS_REQUIRE_EXTERNAL_PROVIDER_FIXTURES=1`,
MySQL on 3306, Postgres on 55432 because 5432 was in use, libSQL on
18080/18081). Logs: `mik4-*.txt` in this directory.

| Command | Result |
| --- | --- |
| `cargo test -p nimbus-storage --features mysql` | 466 passed, 0 failed, 3 ignored |
| `cargo test -p nimbus-storage --features "mysql postgres libsql" provider_contract_matrix` | 2 passed (`provider_contract_matrix_is_complete`, `provider_contract_matrix_reports_unavailable_lanes_as_unverified`) |
| `cargo test -p nimbus-engine --features mysql mysql` | 13 passed, 0 failed |
| `cargo test -p nimbus-system projection_mysql` | 1 passed, 0 failed |
| `cargo test -p nimbus-storage` (default features) | 401 passed, 0 failed, 3 ignored |
| `cargo fmt --all --check` | clean |
| `make clippy` | clean (first run failed in the UI codegen prerequisite because the worktree had no `node_modules`; `npm ci`, then exit 0) |

Throwaway merge: worktree `mik4-phase4-merge` on branch `mik4/phase4-merge`
from `codex/nimbus-ui-rebuild-phase4` (3c91066a2) merged
`codex/mysql-index-keyspace` cleanly (60 files, no conflicts).
`cargo test -p nimbus-system projection_mysql` there:
`projection_mysql_two_engine_takeover_rejects_late_old_document_schema_and_delete`
1 passed (`mik4-phase4-projection.txt`). The worktree and branch were removed.

## Autoreview

`nimbus-autoreview --gate pre-pr --mode branch --base origin/main`
(`mik4-autoreview.txt`): reviewer `gpt-5.6-sol` at `xhigh`, trufflehog
clean, one review pass over a 280164-byte bundle. Verdict: "autoreview
clean: no accepted/actionable findings reported; overall: patch is correct
(0.95)". No P0 defects; the reviewer noted that the keyspace stays
transactionally coupled to document writes, index history, schema changes,
and commit-log application, and that the bundle covers write maintenance,
schema backfill and rollback, range ordering, query planning, and the prior
64-key failure. Clean attestation stored.

## Pull request

PR_PLACEHOLDER
