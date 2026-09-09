# UIR12 Observability

Date: 2026-09-08
Worktree: `~/src/github.com/nimbus/nimbus-worktrees/nimbus-ui-rebuild`, branch `codex/nimbus-ui-rebuild-phase3` (stacked on #332)

Commit `310bd5304`. Observability had a flat events table with a filter
row, a hand-rolled runs table, and an operator page whose scope chip
said the tenant filter did not work. The Logs tab is now one stream of
runs and the log lines that belong to them, the Runs tab runs on the
shared `DataTable` with a right-side run sheet, and both pages read
through one facet bar whose every facet lives in the address. The
operator page reads every tenant by default and narrows through the
same tenant facet.

## What changed

| Item | Result |
| --- | --- |
| `convex/events.ts`, `convex/runs.ts` | `recent` takes `tenantId: string \| null`. Rows carry no tenant column yet, so the scope applies to rows that name one and a row without a tenant passes every scope. The check is inline in the handler: the runtime bundle ships the handler body alone, so a module-level helper is `undefined` at run time (found by the first e2e run: `ReferenceError: inTenantScope is not defined (at events:61)`). Comments hold no quote characters because the codegen scanner tracks them inside comments. |
| `src/components/facet-bar.tsx` (new, 149 lines) | `FacetBar` (labelled toolbar that wraps), `FacetInput` (bounded text facet), `FacetToggle` (pressed state), `FacetButton`. Story in `src/stories/facet-bar.stories.tsx`. |
| `src/components/run-panels.tsx` (new, 245 lines) | `RunSummary`, `RunErrorPanel`, `RunCorrelatedEvents` lifted out of the run page so the page and the sheet compose the same panels. `compute_.runs_.$runId.tsx` shrinks by 200 lines. |
| `observability/-types.ts` | `ObservabilitySearch` with `tab`, `tenant`, `level`, `category`, `source`, `correlationId`, `status`, `functionPath`, `run`, `follow`, `pauseOnError`; `parseObservabilitySearch`; `hasLineFilters`. |
| `observability/-log-groups.ts` (new, 97 lines) | `groupLogs(runs, events, {correlationId, lineFiltered})`: one group per run keyed by `_id`, lines attached by `correlationId`, the rest under `server`; a correlation narrows to that run; a line filter drops runs with no matching line. |
| `observability/-facets.tsx` (new, 95 lines) | `TenantFacet` (`ALL_OPTION = "*"` sentinel because Base UI Select shows the placeholder for `""`), `TenantScopeNote`, `SystemLensButton`, `ObservabilityTabProps`. Replaces `-filters.tsx` (deleted). |
| `observability/-navigation.ts` (new, 52 lines) | `useObservabilityNavigation(to)`: `setSearch` (replace) and `setSearchAction` (push) merge a patch into the parsed search. |
| `observability/-logs.tsx` (rewritten, 674 lines) | `LogsTab`: reads events and runs on the tenant scope, perf store kept for `tests/perf/log-stream.spec.ts`; pause-on-error freezes at the newest alarm's `createdAt` and `Resume` releases; `LogFacetBar` (tenant, level, category, source, correlation, follow, pause on error, clear, lens); `LogStream` as a fixed-column table with a sticky head (Time, Level, Source, Message, Run), a `RowContextMenu` (open run, only this run, copy id), group bodies with a `RunHead` (state pill, function path, kind, duration, relative time, line count, open run) or `ServerHead`, an empty-run row, and a jump link per line to the run page. |
| `observability/-runs.tsx` (rewritten, 263 lines) | `RunsTab` on `DataTable` (Started, Function, Status, Kind, Duration, Run id as `CopyChip`, shown at rest after the runs screenshot showed an empty column); status and function facets; settled-empty states that blame a filter only when one is set; row activation opens the sheet through `?run=`. |
| `observability/-run-sheet.tsx` (new, 205 lines) | `RunSheet`: `Sheet` bound to `?run=`, header with function path and status pill, `RunSummary`, `RunErrorPanel`, `RunCorrelatedEvents`, `Show in logs` (→ `?tab=logs&correlationId=`) and `Open run`; a missing run says whether the read is in flight or the run fell outside the page. |
| `src/routes/developer/observability.tsx` | `validateSearch: parseObservabilitySearch`; tenant scope is `?tenant=` then the active tenant; subtitle within the 100-character budget. |
| `src/routes/operator/observability.tsx` | Same tabs and facet bar; scope is `?tenant=` or every tenant; the scope chip is gone. |
| `DESIGN.md` | Observability (Developer), Observability (Operator), Logs / runs shell pattern, Logs And Events, and the two `?tenant=` table cells describe the shipped stream, facet bar, run sheet, and operator tenant facet. |

## Spec notes

- `logs.spec.tsx` (rewritten, 16 tests): the acceptance tests "defaults
  the tenant facet to the active tenant and scopes both reads to it"
  and "makes one log group per run: three runs, three groups"; plus
  `?tenant=` override, the lens button, correlation attachment and the
  server group, the empty-run row, group heads, correlation narrowing,
  line-filter drop, the fixed-column grid, the wrapping facet bar,
  bounded text facets, loading, settled-empty, filter naming, and the
  mounted frame.
- `runs.spec.tsx` (rewritten, 14 tests): status filter, `DataTable`
  skeletons with `aria-busy`, empty states, query args, columns, row
  activation, the adapter note link, and the sheet closed, open, with an
  error, show-logs, the events read, and the missing run.
- `-log-groups.spec.ts` (new, 7 tests): grouping, ordering, orphan
  lines, correlation, and line-filter behaviour of `groupLogs`.
- `facet-bar.spec.tsx` (new, 5 tests): toolbar label, input binding,
  toggle pressed state, button, and the trailing slot.
- `developer/observability.spec.tsx` (4) and `operator/observability.spec.tsx`
  (6): subtitle through `PageHeader`, tab strip placement, "defaults to
  every tenant and reads with a null tenant scope", "honours a tenant
  named in the address".
- Fail-before for the unit acceptance tests was not captured as a file
  (UNVERIFIED as a recorded artifact). The old page had no
  `observability-log-group-*` element, so the e2e assertion of three
  run groups cannot pass on the previous build; the first e2e run
  against the new UI failed on the handler defect noted above and
  passed after the fix.
- `tests/e2e/observability.spec.ts` (new, chromium, 2 tests): seeds
  tenant `obs-e2e`, three runs and two events through the system
  mutation route, reads each inserted id from the response; asserts the
  tenant facet, three run groups plus the server group, the errored
  run's line under its run, the Runs table with three rows, the sheet
  with status and error and one correlated line, the handoff to
  `?tab=logs&correlationId=` with one group, and the operator page with
  `all tenants` then `?tenant=obs-e2e`. Run ids carry a colon, so the
  URL assertions compare the encoded id.
- Net: 110 files, 871 tests (108 files, 842 at UIR11).

## Verification

| Check | Result |
| --- | --- |
| `npm run lint` (285 files) | clean, 1 pre-existing warning |
| `npm run typecheck` (nimbus-ui) and root `npm run typecheck` | clean |
| `npm run test` | 110 files, 871 passed |
| `npm run build` | ok |
| `npm run storybook:build` | ok |
| `cargo fmt --all --check` | clean |
| `make build` (worktree root) | ok, 1m 08s |
| `npm run test:e2e` | 16 passed, 1 skipped (chromium and mobile; observability spec skipped on mobile) |
| `verify.sh` | 26 ok, 1 FAIL (spec for routes/developer/settings, owned by UIR13) |

Screenshots at 1280×720 from the e2e walk against the rebuilt binary,
tenant `obs-e2e` with three runs and two events: `UIR12-logs.png` (the
run-grouped stream with the facet bar), `UIR12-runs.png` (the runs
table), `UIR12-run-sheet.png` (the sheet on the errored run, captured
with animations settled), `UIR12-operator.png` (the operator stream on
`all tenants`).

## Open items

- Run and event rows carry no tenant column, so the tenant facet is a
  scope over rows that name one and the note under the bar says so. The
  server side owns recording `tenantId` on both tables; when it does,
  the queries become index reads and the note goes.
- Events and Errors tabs arrive with UIR20.
- The insert route answers with the document id, which the e2e seed
  reads through a small key probe; the response shape is not typed in
  the fixture.
