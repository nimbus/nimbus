# UIR19 Tenant logs and log search

Date: 2026-09-08
Worktree: `~/src/github.com/nimbus/nimbus-worktrees/nimbus-ui-rebuild`, branch `codex/nimbus-ui-rebuild-phase4` (stacked on #333)

Commit `89b2544ac`. Run and event rows had no tenant column, so the
console's tenant scope was a client-side promise it could not keep, the
honesty note said so, and the operator tenant selector shipped an inert
"coming soon" branch (finding F2, server half). There was no way to search
a message. The system records now name their tenant, the nimbus-system
crate owns one bounded log query with tenant, run, level, source,
category and text filters, nimbus-server exposes it at
`GET /api/console/logs`, and the Logs tab gains a debounced search facet
with a result count. A run detail page links to the Logs tab narrowed to
that run.

## What changed

| Item | Result |
| --- | --- |
| `crates/nimbus-system/src/schema.rs`, `records/run.rs`, `records/mod.rs` | `runs.tenantId` (required) with `by_tenantId` and `by_tenantId_and_startedAt`; `events.tenantId` (optional) with `by_tenantId` and `by_tenantId_and_createdAt`. `record_system_event_async(engine, SystemEvent { tenant_id, source, level, category, message, data, correlation_id })` replaces the eight-argument writer (Clippy rejected the eighth argument). Runtime and service lines carry the tenant; lifecycle and machine lines carry none. |
| `crates/nimbus-system/src/records/logs.rs` (new) | `LogQuery` and `LogPage`; `query_log_lines_async` builds engine Eq filters for tenant, run (`correlationId`), level, source and category, reads the newest 2,000 lines (`LOG_SCAN_WINDOW`), lowercases the needle over `message`, and returns `lines` (at most `limit`, default 200, clamped to `1..=LOG_PAGE_LIMIT`), `matched`, `scanned`, `exhaustive`. |
| `crates/nimbus-server/src/http/logs.rs` (new), `router.rs` | `GET /api/console/logs?tenant=&run=&level=&source=&category=&q=&limit=` under the console session; a bad tenant id is a 400 through `AppError`. |
| `packages/nimbus-ui/convex/{schema,events,runs}.ts` | The reactive stream reads `by_tenantId_and_createdAt` and `by_tenantId_and_startedAt` with engine-side filters for level, source, category, bundle, function and status; the run path keeps `by_correlationId` and drops rows from another tenant; the null-tenant (operator `all tenants`) paths are unchanged. |
| `src/shell/tenant-scope.ts`, `tenant-selector.tsx`, `-facets.tsx`, `-runs.tsx` | `EVENTS_TABLE_HAS_TENANT_COLUMN`, `TenantScopeNote` and the inert `coming-soon` selector branch are deleted. |
| `-types.ts`, `-logs.tsx` | `?q=` search param; `SearchFacet` commits a trimmed draft after 300 ms (`SEARCH_DEBOUNCE_MS`); `LogSearchResults` reads `logSearchPath(search, tenantId)` through `useApiRead`, groups the page by run, shows `LogSearchCount` ("N lines match “q” in every recorded line / in the newest 2,000 lines. The newest K are shown.") with a **clear search** button, and `LoadFailed` with **Try again** on a failed read; the live stream resumes when `q` clears. |
| `src/components/run-panels.tsx` | The run's events header counts its lines (`<testid>-events-count`) and **search run logs →** opens `/developer/observability?tab=logs&correlationId=<run>`. |
| `src/components/facet-bar.tsx` | `FacetInput` takes `wide` (26 ch) for the search field. |
| `DESIGN.md` | Observability paragraph and the Logs facet list describe the tenant index reads, the search request, the count line and the run link. |

## Spec notes

- `crates/nimbus-system/src/records/logs.rs` tests:
  `log_query_scopes_lines_to_a_tenant_and_searches_text` (tenant scope
  drops another tenant's line and the server-level line; `PAYMENT`
  matches two lines case-insensitively; `run` plus `level` narrows to
  one; `limit: 1` reports `matched 2`, one line shown) and
  `run_rows_record_their_tenant`.
- `crates/nimbus-server/src/tests/local_admin.rs`:
  `console_log_search_scopes_to_the_tenant_and_matches_text` (200 with
  `matched 1, scanned 2, exhaustive true, limit 50` for
  `?tenant=acme&q=PAYMENT&limit=50`; `?tenant=not%20a%20tenant` is 400).
- `crates/nimbus-system/src/tests.rs`: the schema test asserts the four
  new indexes and both `tenantId` fields.
- `logs.spec.tsx` (+6, "LogsTab search"): reads the search page on the
  tenant scope with the count and reach copy; scopes to
  `?correlationId=` with the "search this run" placeholder; commits one
  navigation after the debounce; a zero count shows the filtered empty
  state; a failed read retries through **Try again**; **clear search**
  keeps the level facet.
- `tenant-selector.spec.tsx`: the inert-trigger test is gone with the
  branch.
- `tests/e2e/observability.spec.ts` (+1): `?tab=logs&q=commit` shows "1
  line matches" in every recorded line; the errored run's detail page
  counts "1 line", **search run logs →** lands on the run-scoped Logs
  tab, typing `missing` finds the run's own error line and `health`
  finds nothing.
- Fail-before: `$S/uir19-fail-before.txt` records both nimbus-system
  tests failing before the change (`SchemaValidation("missing required
  field: tenantId")` and the tenant scope returning all four lines);
  `$S/uir19-pass-after.txt` records 7 `records::` tests passing after.
- Net: 121 files, 1053 tests (1048 at UIR18).

## Verification

| Check | Result |
| --- | --- |
| `cargo test -p nimbus-system records:: schema` | `records::` 7 passed; `schema` 5 passed, 2 failed: `projection::reconciliation_tests::*` panic in `nimbus-storage/src/provider_test_fixtures.rs:287` (external-provider fixture env, pre-existing on this host); `system_event` 1 passed |
| `cargo test -p nimbus-server console_log_search thrown_errors machine_lifecycle` | 1, 1 passed; `machine_lifecycle` 4 passed with `--test-threads=1` (two of them fail together under plain `cargo test` with `DuplicateProcessComposition`, the per-process network authority documented in `docs/private/operating/verification.md`) |
| `cargo test -p nimbus-compute` | 514 passed, 1 ignored |
| `cargo fmt --all --check`, `make check`, `make clippy` | clean |
| `npm run lint` | clean, 1 pre-existing warning, 3 infos |
| `npm run typecheck` (nimbus-ui) and root `npm run typecheck` | clean |
| `npm run test` | 121 files, 1053 passed |
| `npm run build`, `npm run storybook:build` | ok |
| `make build` | ok, 1m 19s |
| `npm run test:e2e` | 25 passed, 1 skipped (24 passed at UIR18) |
| `verify.sh` | 0 failing |

Screenshots at 1280×720 from the e2e walk, tenant `obs-e2e`:
`UIR19-search.png` (the Logs tab with `commit` matched and the count
line), `UIR19-run-logs.png` (the run-scoped Logs tab reached from the
run detail page, `missing` matched).

## Open items

- A function's own `console.log` output goes to the server's stdout and is
  not recorded into `events`, so the search reaches the lines the server
  writes (run status, errors, lifecycle) and not the developer's print
  statements. Capture of function console output is a runtime task.
- Server-level lines (lifecycle, machine) name no tenant, so they appear
  under the operator `all tenants` scope alone. The tenant scope shows
  the tenant's own lines only.
- The search reads the newest 2,000 lines once per request. A tenant
  with a deeper history sees `in the newest 2,000 lines` on the count
  line; a time-bounded scan is a follow-up when a tenant's history grows
  past that window.
