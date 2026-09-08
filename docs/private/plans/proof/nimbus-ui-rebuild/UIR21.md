# UIR21 Storage editors

Date: 2026-09-08
Worktree: `~/src/github.com/nimbus/nimbus-worktrees/nimbus-ui-rebuild`, branch `codex/nimbus-ui-rebuild-phase4` (stacked on #333)

Commit `4fae5536d`. The console could read a table's schema and indexes but
could not build a query, apply a schema safely, or change an index. A
schema PUT stored whatever it was given, so a draft that the table already
violated became enforced on top of documents that broke it. The server
now has a checked apply route that scans every document before it stores
the schema and reports the violations instead; the Schema tab applies and
dry-runs through it, the Indexes tab creates and drops through it with a
polled status pill, and a new Query tab builds the same query the
Documents tab runs and shows it as code.

## Seam finding

The plan named `crates/nimbus-storage` as the owner of schema apply and
index build status. Storage already rebuilds every index inside the same
transaction as the schema commit, so there is no asynchronous backfill and
no build status to own: an index reads `enabled` on the first read after
the commit. The missing piece was the check before the commit, which is a
document scan against the draft and therefore a server-handler concern.
It lives in `crates/nimbus-server/src/http/schema.rs` beside the existing
schema handlers; storage is untouched. The Indexes tab says this plainly
("the index is built inside the same commit") and keeps the poll so a
future asynchronous build shows its transitions without a console change.

The plan's file names (`query-bar.tsx`, `schema-panel.tsx`,
`index-panel.tsx`) are the existing `schema-tab.tsx` and `indexes-tab.tsx`
from UIR13 plus a new `query-tab.tsx`; the tab components own the editors.

## What changed

| Item | Result |
| --- | --- |
| `crates/nimbus-server/src/http/schema.rs`, `http/mod.rs`, `router.rs`, `crates/nimbus-system/src/inventory.rs` | `POST /api/tenants/{tenant}/schema/{table}/apply[?dry_run=true]` (`apply_table_schema`). The handler validates the draft, scans the table 500 documents per page (`APPLY_SCAN_PAGE_SIZE`) against it, and answers 200 `{ applied, dry_run, scanned, violation_count, violations: [{ id, message }] }` with at most 50 violations listed (`APPLY_VIOLATION_LIMIT`). It stores the schema only when the scan is clean and `dry_run` is off, so enforcement never lands on a table that already breaks it. An invalid draft is 422. |
| `crates/nimbus-testing/src/http_api_fixture/schema.rs` | `apply_table_schema(tenant, table, schema, dry_run)` on the HTTP fixture. |
| `src/lib/types/table.ts`, `src/lib/api-mutations.ts` | `INDEX_STATES`, `IndexState`, `TableSchemaIndex { id?, name, fields, state? }`; `schema.apply(tenant, table, draft, { dryRun })` returns `SchemaApplyReport`; `schema.get`, `schema.drop`. |
| `src/components/storage/schema-draft.ts` (new) | `draftFromSchema` prints the committed schema as the editable JSON (`table`, `fields`, `indexes` as `{ name, fields }`, no server-owned `id` or `state`); `parseSchemaDraft` names the field at fault. |
| `src/components/storage/schema-tab.tsx` | **Check** posts the draft with `dry_run=true`, **Apply** without; the report block (`documents-schema-report`, `data-outcome` `applied`, `checked`, `refused`) says "Not applied. N documents of M scanned violate the draft", lists each violating document id as a copy chip with its message, and a trailing "and K more" when the server capped the list. **Drop** sits behind a confirmation and is disabled without a schema. A draft that does not parse never reaches the server. |
| `src/components/storage/use-index-status.ts` (new), `indexes-tab.tsx` | `useIndexStatus` polls the schema route (1.5 s) until every index reads `enabled` and stops. The tab lists name, fields and a `StatePill` per index; **New index** takes a name and a comma list of fields, refuses a duplicate name or an empty list, and creates by re-applying the schema with the index appended; drop re-applies without it behind a confirmation. A refused apply says "Index not created: N of M documents violate the table schema. Fix them on the Schema tab, then try again." A schemaless table gets `{ table, fields: [], indexes }`. |
| `src/components/storage/table-query.ts` | `compileDocumentQuery(table, filters, order, limit)` and `paginatedRequestBody(query, pageSize, after)` are the one compiler the grid, the pager (`use-document-page.ts`) and the builder share; `DOCUMENT_PAGE_SIZE` 200. |
| `src/components/storage/query-tab.tsx` (new), `onboarding/next-action.ts` | Query tab: field, operator, value rows seeded from the URL filters, a sort field and direction, each field labelled `(indexed)` or `(scan)`. An unindexed sort disables **Run** behind a "Scan anyway" checkbox. **Show as code** prints the request body and a curl command (`paginatedQueryCommand`) for `POST /api/tenants/{tenant}/query/paginated`. **Run** hands the filters and sort to the Documents tab through the URL and clears the cursor stack. |
| `src/routes/developer/storage_.$table.tsx` | `?tab=query`; `runQuery` patches the search; the Indexes tab gets `onChanged`. |
| `src/components/state-dot.tsx` | `StateKind` gains `enabled` (success) and `backfilling` (transition). |
| `DESIGN.md` | Storage section describes the checked apply route, the Check dry run, the polled index pill, and the Query tab's marks, gate and code view. |

## Spec notes

- `crates/nimbus-server/src/tests/core_http/schema.rs`
  `schema_apply_reports_violations_and_never_applies_partially`: three
  documents, one violating; apply answers `applied false`, `scanned 3`,
  `violation_count 1` with the offending id and the schema route still
  404s; after the document is deleted a dry run answers `dry_run true`,
  `scanned 2`, no violations, and still stores nothing; apply then lands,
  the schema route reads the index back as `enabled`, a violating insert
  is refused, and a draft whose table does not match the path is 422.
  Route inventory, connectivity and REST parity tests cover the new row.
- `api-mutations.spec.ts` (24): `schema.apply` posts the raw draft, reads
  the report back, and puts `dry_run=true` in the query.
- `schema-tab.spec.tsx` (7): apply POSTs the draft and refetches on
  `applied`; a refused report lists 50 documents and "and 2 more" without
  a refetch; Check sends `dry_run=true` and reports `checked`; invalid
  JSON and a bad field type name the fault and never call the server;
  drop behind the confirmation (DELETE 204); drop disabled without a
  schema; the draft strips `id` and `state`.
- `indexes-tab.spec.tsx` (8, `pollMs=20`): empty state; list with a
  status pill; the pill polls `pending` → `backfilling` → `enabled` under
  an msw server the test releases, then no further reads; create posts
  the exact schema with the index appended; a schemaless table posts an
  index-only schema; duplicate name and empty field list refused client
  side; a refused apply names "3 of 40 documents" and the Schema tab;
  drop behind the confirmation posts the schema without the index.
- `query-tab.spec.tsx` (5): the body under **Show as code** equals, whole,
  the body `documents.queryPaginated` sends for the same compiled query
  under msw, and the curl names the tenant route; Run calls `onRun` with
  the compiled filters; rows seed from the URL; an unindexed sort gates
  Run until "scan anyway", and an indexed sort removes the gate; the
  pickers mark indexed and scan.
- `table-query.spec.ts` (11): `compileDocumentQuery` shape and
  `paginatedRequestBody` defaults and overrides.
- `storage_.$table.spec.tsx`: the Query tab is present; the schema mock
  answers 404.
- `tests/e2e/storage.spec.ts` (+3 steps, 8 total): step 2 asserts the
  seeded index reads `enabled`; step 6 builds `author = grace` on the
  Query tab, asserts the body and the curl under Show as code, runs it
  and lands on Documents with `filters=` in the URL, the chip and "115
  rows"; step 7 creates `by_seq`, sees its pill `enabled` and the row
  count 3, then drops it back to 2; step 8 applies a draft that types
  `seq` as a string and gets "230 documents of 230 scanned", 50 rows and
  "and 180 more", reloads, sees the draft unchanged, and Check reports
  `checked` with "230 documents".
- Fail-before: `$S/uir21-fail-before.txt` records the server test failing
  against the missing route (`left: 404, right: 200`). The first full
  unit run failed `token-utilities.spec.ts` because the Query tab legends
  used `uppercase`; the class is gone. The first storage e2e run failed
  at the final Check click because the toast-hiding style tag does not
  survive `page.reload()`; the walk re-applies it after the reload.
- Net: 124 files, 1088 tests (1074 at UIR20).

## Verification

| Check | Result |
| --- | --- |
| `cargo test -p nimbus-server schema_apply` | 1 passed |
| `cargo test -p nimbus-server route_inventory`, `connectivity`, `rest_route_parity` | 1 passed each |
| `cargo fmt --all --check`, `make check`, `make clippy` | clean |
| `npm run lint` | clean, 1 pre-existing warning, 3 infos |
| `npm run typecheck` (nimbus-ui) and root `npm run typecheck` | clean |
| `npm run test` | 124 files, 1088 passed |
| `npm run build`, `npm run storybook:build` | ok |
| `make build` | ok |
| `npm run test:e2e` | 26 passed, 1 skipped (storage walk 5 → 8 steps) |
| `verify.sh` | 0 failing |

Screenshots at 1280×720 from the e2e walk, tenant `storage-e2e`:
`UIR21-query.png` (the Query tab with one filter and the request body
under Show as code), `UIR21-indexes.png` (two indexes with `enabled`
pills after `by_seq` was created), `UIR21-schema-violations.png` (a
refused apply listing the first violating documents).

## Open items

- Index state is `enabled` on the first read because storage rebuilds the
  index inside the schema commit. The poll and the `backfilling` pill are
  in place for an asynchronous build when one exists.
- The builder does not expose `limit`; the Documents pager owns the page
  size.
- The apply scan reads the whole table on every apply and check (500 per
  page) with no progress signal; a very large table takes time before the
  report arrives.
- The report lists at most 50 violating documents; the count names the
  rest.
