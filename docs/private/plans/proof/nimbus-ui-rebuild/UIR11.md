# UIR11 Storage

Date: 2026-09-08
Worktree: `~/src/github.com/nimbus/nimbus-worktrees/nimbus-ui-rebuild`, branch `codex/nimbus-ui-rebuild-phase3` (stacked on #332)

Commit `29d055a37`. Storage had a hand-rolled documents table with no
virtualization, its own skeleton and context-menu code, and Schema and
Indexes as side inspectors beside the grid. The Tables index and the
documents grid now run on the shared `DataTable`, which owns loading
skeletons, virtualization past 100 rows, the row-menu anchor and roving
row focus. Schema and Indexes are full-width `PageTabs`.

## What changed

| Item | Result |
| --- | --- |
| `src/components/data-table.tsx` (+160 lines) | New props `loading` and `skeletonRows` (skeleton rows under the real header, `aria-busy`), `manualSorting`, `onRowContextMenu(row, anchor)` with a `RowAnchor` `{x, y, element}`, `rowTestid`, `rowClassName`, `maxHeight`. Rows carry `role="row"`, `aria-selected` when selected, a roving `tabIndex`, and answer ArrowUp/ArrowDown, Enter/Space, Shift+F10 and ContextMenu. Past 100 rows the body virtualizes with TanStack Virtual (`data-virtual="true"`, `aria-rowcount` for the full count). |
| `src/components/storage/documents-table.tsx` (rewritten, 553 → shorter) | `DocumentsTable` on `DataTable` with `manualSorting`; TanStack's asc → desc → off cycle maps to the page's flip semantics so the active column is always reported. Selection is `onSelectionChange(ids)`; select-all is ignored while loading. Sort headers keep `documents-sort-<field>` with `data-active` and the index-backed / scans-the-table title. Row menu: edit, copy id, copy JSON, delete. Pager text "page N · loading…" or "N rows · N selected". |
| `src/components/storage/use-document-page.ts` | `PAGE_SIZE` 200 so a full page virtualizes. |
| `src/components/storage/tables-list-table.tsx` (rewritten) | Tables index as `DataTable` (`tenant-tables-table`): name link with `CopyChip`, schema defined/any, rows and last write sortable, hover-visible Schema and Open buttons, row menu open / schema / indexes / copy name. Skeleton rows while the query is undefined. |
| `src/components/storage/schema-tab.tsx` (new, 134 lines) | Replaces `schema-panel.tsx`. Full-width editor with save (PUT), drop behind `ConfirmDialog`, inline parse and request errors, toasts. |
| `src/components/storage/indexes-tab.tsx` (new, 66 lines) | Replaces `index-panel.tsx`. Read-only `DataTable` of name, fields, unique; empty state "No indexes defined." |
| `src/components/storage/column-chooser.tsx` (rewritten) | shadcn `Popover`; trigger "Columns n/m" with the hidden count in warning color; per-field checkbox and move left/right; reset. |
| `src/routes/developer/storage_.$table.tsx` | Search `tab` (`schema`, `indexes`) replaces `panel`. `PageTabs` Documents / Schema / Indexes (`documents-tab-*`); Query arrives in UIR21 and is not shown. The Schema tab renders `LoadingState` (`documents-schema-loading`) until the table row has loaded, so a deep link never seeds an empty draft over a real schema (found by the e2e walk). Toolbar is the Insert button only. |
| `src/routes/developer/storage.tsx` | Tenant states unchanged; tables `undefined` renders `TablesListTable` skeletons, an empty array renders the "No tables" empty state. `TablesTableHead` deleted. |
| `DESIGN.md` | Storage: Schema and Indexes tabs by `?tab=`. Tables: `DataTable` loading, virtualization and row-menu contracts. Data Browser: 200-document page, server-side sort. |

## Spec notes

- `documents-table.spec.tsx` (rewritten, 19 tests): row click opens,
  interactive cells stay theirs, select-all and uncheck report id lists,
  right-click menu with edit and delete, Shift+F10 and ContextMenu raise
  it, Enter activates and ArrowDown moves focus, one row in the tab
  order, `aria-selected`, sort control per header, the flip on the
  active column, `aria-sort`, index-backed titles, skeletons with
  `aria-busy` and the pager frozen, and the acceptance test: a
  1,000-document page renders `data-virtual="true"`, `aria-rowcount`
  1001, and at most 60 row elements (rendered count > 0). Fail-before
  on the old table, recorded in `uir11-fail-before.txt`:
  `expect(element).toHaveAttribute("data-virtual", "true")` received
  `null`, 1 failed.
- `schema-tab.spec.tsx` (new, 5 tests, msw): PUT on save with the
  parsed body, parse error with no request, drop only after the
  confirmation, drop disabled without a schema, the draft seeded from
  the schema.
- `indexes-tab.spec.tsx` (new, 3 tests): empty state, rows with fields
  and unique yes/no, read-only note.
- `storage_.$table.spec.tsx`: the three "document table column floor"
  tests are gone with the side inspectors; new "table views" tests
  cover the default Documents tab with `aria-current`, the Schema and
  Indexes tabs, the loading gate before the editor, and the seeded
  draft.
- `storage.spec.tsx`: the loading tests assert `tenant-tables-table`
  with `aria-busy="true"`, 8 skeleton rows, and the same five column
  headers before and after the load.
- `documents-page.spec.tsx`, `column-chooser.spec.tsx`: `panel` → `tab`,
  the DataTable skeleton testid, "Columns 3/3".
- `tests/e2e/storage.spec.ts` (new, chromium): seeds tenant
  `storage-e2e`, a schema with index `by_author`, and 230 documents
  through the REST routes; asserts the tables row (defined, 230), the
  row menu into `?tab=schema`, the seeded draft, the Indexes tab, the
  documents grid with `data-virtual`, `aria-rowcount` 201, "200 rows",
  at most 60 rendered rows before and after a scroll to the end, hiding
  `body` through the column chooser with the preference in
  `localStorage` and after a reload, and the pager to page 2 (30 rows)
  and back. The release toast sits over the pager, so the walk hides
  the toaster before the pager step.
- Net: 108 files, 842 tests (108 files, 836 at UIR10).

## Verification

| Check | Result |
| --- | --- |
| `npx biome check --write` and `npm run lint` (277 files) | clean, 1 pre-existing warning |
| `npm run typecheck` (nimbus-ui) and root `npm run typecheck` | clean |
| `npm run test` | 108 files, 842 passed |
| `npm run build` | ok |
| `npm run storybook:build` | ok |
| `cargo fmt --all --check` | clean |
| `make build` (worktree root) | ok |
| `npm run test:e2e` | 14 passed, 1 skipped (chromium and mobile; storage spec skipped on mobile) |
| `verify.sh` | 26 ok, 1 FAIL (spec for routes/developer/settings, owned by UIR13) |

Screenshots at 1280×720 from the e2e walk against the rebuilt binary,
tenant `storage-e2e` with 230 `messages` documents and a schema:
`UIR11-tables.png` (the tables index with the row count),
`UIR11-schema-tab.png` (the seeded editor, full width),
`UIR11-indexes-tab.png` (the read-only index table),
`UIR11-documents.png` (the virtual grid, 200 rows, the pager).

## Open items

- The index API has no create or drop endpoint, so the Indexes tab is
  read-only and says so.
- The Query tab is absent until UIR21 lands the query console.
- The release toast anchors bottom-right over every page's bottom
  controls; the pager is the first control it has covered. The staleness
  hook owns the placement.
- Sorting by the Tables index columns is client-side over the loaded
  list (200 tables max), which is the list endpoint's bound.
