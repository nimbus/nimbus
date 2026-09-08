# UIR15 Services and Schedules

Date: 2026-09-08
Worktree: `~/src/github.com/nimbus/nimbus-worktrees/nimbus-ui-rebuild`, branch `codex/nimbus-ui-rebuild-phase3` (stacked on #332)

Commit `9f5d98f54`. Services listed on a hand-rolled table with no
actions, the service detail was one long page, and Schedules was two
read-only lists. Services now runs on `DataTable` with a lifecycle
`StatePill` per row and a row menu that offers the lifecycle actions the
state allows; the detail has tabs with the same lifecycle buttons in the
header; Schedules runs on `DataTable` with a sheet, Run now, cancel, and
delete cron. The e2e walk found a table overflow at 1280 and a menu race
on the scroll that Playwright dispatches; both are fixed with specs.

## What changed

| Item | Result |
| --- | --- |
| `src/lib/api-mutations.ts` | `services.start`, `services.stop`, `services.restart` (`{sourceGeneration, requestId}`, 202), `schedules.runNow` (`POST /schedule` with `run_after_ms: 0`), `schedules.cancel`, `schedules.listCrons`, `schedules.removeCron`. 20 tests. |
| `developer/services/-service-lifecycle.ts` (new, 145 lines) | `OPTIMISTIC_STATES` (`start` → `starting`, `stop` → `stopping`, `restart` → `restarting`), `BRANCHED_SERVICE_STATES`, `actionsForState` (in flight → none; ready or running → stop, restart; stopped or not ready → start; failed → start, restart; unknown → all three), `sourceGenerationOf`, `useServiceActions` (optimistic state while the request is out, invalidate and toast on success, refusal text kept beside the row on failure). |
| `developer/services/-lifecycle-buttons.tsx` (new, 69 lines) | `LifecycleButtons` for the detail header, `shownStateOf`. |
| `developer/services/-service-logs.tsx` (new, 120 lines) | `ServiceLogs`: the live `source=service` event stream for the tenant, newest first, with a link to the Observability Logs tab and an empty state that says what writes a line. |
| `src/routes/developer/services.tsx` (448 lines) | `ServicesTable` on `DataTable`: Name, State pill, Kind, Tenant (operator only), Machine, Endpoints, Updated, actions button. Row activation opens the detail; right-click or the actions button opens a `RowContextMenu` with open, the allowed lifecycle actions, and show logs. Column sizes sum to 640 (developer) and 736 (operator) so the table fits the 748 px the page has at 1280 beside the sidebar and sub-panel. |
| `src/routes/developer/services_.$service.tsx` (456 lines) | Header with kind, state pill, bundle chip, lifecycle buttons; tabs Overview, Logs, Config through `?tab=`. |
| `src/routes/operator/services.tsx`, `services_.$service.tsx` (318 lines) | Same table with the tenant column; detail with tabs Placement and Logs. |
| `developer/schedules/-types.ts`, `-job-ids.ts` (new) | `ScheduledJobDoc`, `CronJobDoc`, `decodeKeySegment` (the `~xx` encoding of `stable_key_segment`), `jobIdFromDocumentId`, `formatSchedule` (`interval:30s` → `every 30s`). |
| `developer/schedules/-use-schedule-actions.ts` (new, 163 lines) | `useScheduleActions`: run now (a cron reads its mutation from the crons route first because the system record does not hold it), cancel, remove cron; toasts name the function path or cron. |
| `developer/schedules/-schedule-sheet.tsx` (new, 309 lines) | `ScheduleSheet` bound to `?job=` and `?cron=`: facts, the recorded mutation, the error, footer actions; a missing row says so. |
| `src/routes/developer/schedules.tsx` (486 lines) | Scheduled jobs and cron jobs on `DataTable` with skeletons, empty states, row menus (open, run now, cancel on pending; open, run now, delete cron behind `ConfirmDialog`), and the sheet. Static sub-panel Scheduled / Cron. |
| `src/components/storage/row-context-menu.tsx` | The `scroll` listener that dismisses the menu is installed on the next animation frame. A scroll pending when the menu opens (the scroll-into-view for a press on an actions button at the table edge) dispatches before that frame and no longer closes the menu. |
| `DESIGN.md` | Services (Developer), Schedules (Developer), and Services (Operator) describe the shipped tables, row menus, optimistic states, detail tabs, sheet, and Run now. |

## Spec notes

- Fail-before: `UIR15-fail-before.txt` in this directory holds the run of
  the original `schedules-run-now.spec.tsx` against the old page (1 test,
  1 failed: no `schedules-row-menu`). That spec is folded into
  `schedules.spec.tsx` as the Run now tests.
- `services.spec.tsx` (13 tests): the acceptance tests for the row
  actions: menu contents per state, stop shows `stopping` until the
  route settles then invalidates and toasts, restart sends
  `sourceGeneration` and a request id, a refusal keeps the real state
  and shows the error under the pill, row activation and show logs
  navigate.
- `-service-lifecycle.spec.tsx` (8 tests): every optimistic and branched
  state renders a `StatePill` with a known glyph (the acceptance test for
  one pill per lifecycle state), `actionsForState`, `sourceGenerationOf`.
- `schedules.spec.tsx` (17 tests): skeleton geometry and header parity,
  empty titles, finished-job outcome and error, cron interval, `?job=`
  and `?cron=`, Run now from the row menu and the sheet (the acceptance
  test for the Run now mutation), cron run through the crons route,
  refusal toast, cancel on pending only, delete cron through the dialog,
  sheet facts and args, missing row.
- `-job-ids.spec.ts` (9), `api-mutations.spec.ts` (20),
  `operator/services_.$service.spec.tsx` (5), `row-context-menu.spec.tsx`
  (11, one new: a scroll pending at open does not close the menu).
- `tests/e2e/services-schedules.spec.ts` (new, chromium, 2 tests): seeds
  tenant `svc-e2e` with four services (ready, starting, stopped, failed)
  and two scheduled jobs plus one cron. Asserts one pill per seeded
  state with a known glyph, the row menu per state, a refused start on
  the seeded row (no service definition), the detail with the Logs and
  Config tabs; the job sheet on `?job=`, Run now adds a row, the row
  settles on `completed` after the history route is read, the cron
  section through the sub-panel and the cron sheet on `?cron=`.
- Net: 114 files, 977 tests (114 files, 918 at UIR14).

## Verification

| Check | Result |
| --- | --- |
| `npm run lint` (298 files) | clean, 1 pre-existing warning |
| `npm run typecheck` (nimbus-ui) and root `npm run typecheck` | clean |
| `npm run test` | 114 files, 977 passed |
| `npm run build` | ok |
| `npm run storybook:build` | ok |
| `cargo fmt --all --check` | clean |
| `make build` (worktree root) | ok, 1m 07s |
| `npm run test:e2e` | 22 passed, 1 skipped (chromium and mobile) |
| `verify.sh` | 27 ok, 0 failing |

Screenshots at 1280×720 from the e2e walk against the rebuilt binary,
tenant `svc-e2e`: `UIR15-services.png` (four lifecycle pills and the row
menu on the ready service), `UIR15-service-detail.png` (the Logs tab with
the lifecycle buttons), `UIR15-schedules.png` (two pending jobs),
`UIR15-schedule-sheet.png` (the job sheet with Cancel job and Run now).

## Open items

- The scheduler writes a job's outcome into its `scheduled_jobs` system
  record only when `GET /api/tenants/{t}/schedule/history/{job_id}` is
  read (`record_scheduled_job_result_state_async` has that one caller).
  A job that ran stays `pending` in the console until something reads
  its history. The e2e reads the route to settle the row. Recording the
  outcome when the job finishes is the scheduler's, in
  `crates/nimbus-compute/src/scheduling.rs`.
- `GET .../schedule` lists pending jobs only, so the console reads job
  history from the system table, not the route.
- Lifecycle actions need a service definition; a row seeded straight into
  `_nimbus.services` is refused with `service_not_found`. The e2e relies
  on that refusal; a definition-backed walk waits on a compose fixture.
