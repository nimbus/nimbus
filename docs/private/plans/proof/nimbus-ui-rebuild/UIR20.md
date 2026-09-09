# UIR20 Traces and error groups

Date: 2026-09-08
Worktree: `~/src/github.com/nimbus/nimbus-worktrees/nimbus-ui-rebuild`, branch `codex/nimbus-ui-rebuild-phase4` (stacked on #333)

Commit `8f692a7e7`. A run row recorded its status, duration, and error
message and nothing about what the run did, so the run page drew a
"waterfall" from the run's log lines, and two runs that failed the same
way were two unrelated rows. Every run now carries its spans (the
function's own span and one span per host call under it) and a failed run
carries an error class and a fingerprint, so the console draws a real
waterfall per run and folds failures into groups.

## Seam finding

The plan named `crates/nimbus-engine` as the owner of span recording and
the fingerprint. The engine never sees a run: the four Convex function
routes in `nimbus-server` open the run, the host bridge in the same crate
dispatches every `ctx.*` call, and `nimbus-system` writes the run row.
The records (`RunSpan`, `RunSpanRecorder`, `error_fingerprint`,
`query_error_groups_async`) therefore live in `nimbus-system` beside the
run writer, and the recording hooks live in the server's host bridge. The
engine is untouched.

Scheduled execution writes no run row today (the scheduler invokes the
function through a different path), so a scheduled run has no trace and
no group. "Scheduled action" spans are the `convex.ctx.scheduler.*` host
calls a function makes, recorded under the calling run.

## What changed

| Item | Result |
| --- | --- |
| `crates/nimbus-system/src/records/errors.rs` (new) | `error_class` names the core error variant; `normalize_error_message` strips the stack, ids, and numbers so two occurrences agree; `error_fingerprint(function_path, class, message)` is sixteen hex characters of SHA-256; `query_error_groups_async` reads the newest 2,000 failed runs (`ERROR_SCAN_WINDOW`) with an optional tenant filter and folds them by `fingerprint` into `ErrorGroup { fingerprint, tenant_id, function_path, kind, class, message, location, count, first_seen, last_seen, latest_run_id }`, newest group first, at most `limit` (default `ERROR_GROUP_LIMIT` 200); `ErrorGroupPage { groups, scanned, exhaustive, limit }`. |
| `crates/nimbus-system/src/records/trace.rs` (new) | `RunSpan { name, kind, parent, start_ms, duration_ms, status }`; `span_kind_for_operation` maps `convex.ctx.db.*` to `db`, `convex.ctx.scheduler.*` to `scheduler`, `convex.ctx.storage.*` to `storage`, else `host`; `RunSpanRecorder::start(kind, name) -> OpenSpan`, `finish(span, ok)`, `snapshot() -> (Vec<RunSpan>, dropped)` with a 500-span cap (`RUN_SPAN_LIMIT`). |
| `records/run.rs`, `schema.rs`, `lib.rs` | `RunRecord.spans`; `RunError.class`; the run row writes `error.class`, top-level `fingerprint` (error rows only) and `spans`; runs table gains `fingerprint` and `spans` with `by_fingerprint`. |
| `crates/nimbus-server/src/adapters/convex/handlers/function_routes/mod.rs`, `{queries,mutations,actions}.rs` | `RunTrace` owns an `Arc<RunSpanRecorder>` and the root `function` span; `record` finishes the root, snapshots the spans (warns when any were dropped), and passes them to the run row. Each route hands the recorder to the invocation context. |
| `execution/runtime_backed/invoke/context.rs`, `host_bridge/bridge.rs`, `host_bridge/async_bridge/mod.rs`, `function_ops/nested_runtime/dispatch.rs` | `RuntimeInvocationContext::with_span_recorder` threads the recorder into `ConvexHostBridgeScope`; the bridge wraps `call`, `call_cancellable` and `call_async` in a host-call span keyed by the operation, and a nested `ctx.run*` in a `function` span named after the callee. |
| `crates/nimbus-server/src/http/errors.rs` (new), `router.rs` | `GET /api/console/errors?tenant=&limit=` under the console session; a bad tenant id is a 400. |
| `packages/nimbus-ui/convex/{schema,runs}.ts` | `runs.recent` takes `fingerprint`; the tenant path adds it as an engine-side filter, the null-tenant path reads `by_fingerprint`. |
| `src/components/trace-waterfall.tsx` (new) | One `TraceWaterfall` for the Traces tab, the run sheet and the run page: run bar from the run's own status and duration, each span nested under its parent (`data-depth`), kind label, duration, ✗ on a failed span and no glyph on a successful child, and an "No spans were recorded for this run" line for a row that predates recording. |
| `observability/-traces.tsx` (new) | Traces tab: the same `runs.recent` read as Runs with a span-count column beside the picked run's summary line, **Open run ↗** and its waterfall; `?run=` names the trace; a run outside the current page says so with a link to its page. |
| `observability/-errors.tsx` (new) | Errors tab: `useApiRead<ErrorGroupPage>` on `/api/console/errors`, a `DataTable` of groups (last seen, runs, function, class, message with location, first seen, tenant), a scan note ("N failed runs grouped" or "newest N failed runs grouped; older failures are not shown"), **refresh**, `LoadFailed` with **Try again**; a row drills into `?tab=runs&fingerprint=`. |
| `-runs.tsx`, `-run-sheet.tsx`, `-types.ts`, both `observability.tsx` routes | `?fingerprint=` narrows the run list and shows an **error group ffffffff ×** chip that clears it; the sheet gains the waterfall between the summary and the error; `OBSERVABILITY_TABS` is Logs, Runs, Traces, Errors, and one `ObservabilityTabBody` switch serves both pages; subtitles rewritten. |
| `compute_.runs_.$runId.tsx` | The event-based waterfall is deleted; the page renders `TraceWaterfall` from the run's spans. |
| `DESIGN.md` | Observability tab list describes Traces and Errors on both pages; `?fingerprint=` joins the address facets. |

## Spec notes

- `crates/nimbus-system/src/records/errors.rs` tests: the fingerprint is
  stable across two identical throws, separates function, class and
  message, and ignores ids, numbers and the stack;
  `error_groups_fold_runs_by_fingerprint_and_scope_to_a_tenant`.
- `crates/nimbus-system/src/records/trace.rs` tests: span nesting
  (`parent` index), status from `finish`, the 500-span cap reports the
  dropped count, `span_kind_for_operation`.
- `crates/nimbus-server/src/tests/convex_functions/runtime_writes/thrown_errors.rs`:
  a thrown handler writes `error.class == "function_thrown"`, a 16-char
  `fingerprint`, and `spans[0] == { messages:send, function, parent null,
  status error }`; a second identical throw shares the fingerprint; an ok
  run has no fingerprint, `spans[0].status == "ok"`, and a
  `convex.ctx.db.insert` span with `kind db`, `parent 0`, `status ok`;
  `query_error_groups_async` answers one group with `count 2`,
  `location messages:12`.
- `crates/nimbus-server/src/tests/local_admin.rs`:
  `console_error_groups_fold_failed_runs_and_scope_to_the_tenant` (two
  groups across tenants with `scanned 3, exhaustive true`; `?tenant=acme`
  narrows to one; a bad tenant is 400).
- `compute_.runs_.$runId.spec.tsx` (7): rewritten on spans; the glyph and
  its accessible name still carry the status, plus kind, nesting and the
  no-spans line.
- `traces.spec.tsx` (new, 10): the read scope, the span count column,
  `?run=` on activation, the pick-a-run state, the filtered and unfiltered
  empty states, the waterfall (run bar ✗, ok child unmarked, failed child
  ✗ at depth 0), the summary line and **Open run** link, the no-spans
  line, the run-outside-the-page state.
- `errors.spec.tsx` (new, 9): `errorGroupsPath`, the tenant goes to the
  server, one row per group with its fields, the drill-in search action,
  both scan notes, loading versus empty, the tenant-named empty state, a
  failed read retried through **Try again** (msw
  `*/api/console/errors`).
- `operator/observability.spec.tsx`, `observability.spec.tsx`,
  `runs.spec.tsx`, `observability-types.spec.ts`: four tabs, new
  subtitles, `fingerprint` in the read args and the clear action.
- `tests/e2e/observability.spec.ts` (+1): the seeded errored run carries
  `error.class`, `fingerprint` and three spans in the shape
  `RunSpanRecorder::snapshot` writes; a second run with the same
  fingerprint; the Errors tab shows one group with count, class and
  location and "2 failed runs grouped"; the drill-in lists the two runs
  with the chip, the chip clears to four; the Traces tab draws the
  waterfall with ✗ on the run bar and the insert span and no glyph on the
  read; the run sheet shows the same failed span. `smoke.spec.ts` step 7
  now opens Traces and Errors and asserts no Events tab.
- Fail-before: `$S/uir20-fail-before.txt` records the nimbus-system
  fingerprint test failing to compile before the function existed
  (`cannot find function error_fingerprint`); `$S/uir20-gates.txt`
  records the gates after. The first full e2e run failed the smoke walk
  on both projects because step 7 asserted that no Errors tab existed;
  the assertion was rewritten to open the two new tabs.
- Net: 123 files, 1074 tests (1053 at UIR19).

## Verification

| Check | Result |
| --- | --- |
| `cargo test -p nimbus-system "records::"` | 16 passed |
| `cargo test -p nimbus-server thrown_errors`, `console_` | 1 passed; 2 passed |
| `cargo fmt --all --check`, `make check`, `make clippy` | clean |
| `npm run lint` | clean, 1 pre-existing warning, 3 infos |
| `npm run typecheck` (nimbus-ui) and root `npm run typecheck` | clean |
| `npm run test` | 123 files, 1074 passed |
| `npm run build`, `npm run storybook:build` | ok |
| `make build` | ok |
| `npm run test:e2e` | 26 passed, 1 skipped (25 passed at UIR19) |
| `verify.sh` | 0 failing |

Screenshots at 1280×720 from the e2e walk, tenant `obs-e2e`:
`UIR20-errors.png` (the Errors tab with one group of two thrown runs),
`UIR20-traces.png` (the Traces tab with the errored run picked and its
three-span waterfall).

## Open items

- Scheduled execution writes no run row, so a scheduled invocation has
  no trace and no error group. Recording a run for scheduled execution
  is a scheduler task.
- The groups fold the newest 2,000 failed runs once per request; the
  scan note names the reach. A time-bounded scan is a follow-up when a
  tenant's failure history grows past that window.
- The Traces list hides its duration and span-count columns when the
  page is narrower than the two-column layout needs; the picked run's
  waterfall still shows both.
