# UIR17 Onboarding and empty states

Date: 2026-09-08
Worktree: `~/src/github.com/nimbus/nimbus-worktrees/nimbus-ui-rebuild`, branch `codex/nimbus-ui-rebuild-phase4` (stacked on #333)

Commit `07b4847d8`. A new tenant saw empty states that sent the reader
to another page: Storage said "create a tenant from the top nav", the
Observability tabs offered "Open Compute", Schedules and Files named
the API without showing a call. The first-run panel on Overview shipped
with UIR9 (three steps, live completion, solid mascot in the empty
state) but had no spec of its own. This task gives every empty state
the next action on the page it is on, as a button that does the thing
or as one command the reader can copy, from one seam that builds the
commands for the active server and tenant. The first-run panel gets its
transition spec.

## What changed

| Item | Result |
| --- | --- |
| `src/components/onboarding/next-action.ts` (new, 128 lines) | The seam. `NextActionContext` is the server origin and the active tenant. `createTenantCommand`, `insertDocumentCommand`, `scheduleJobCommand`, `createCronCommand`, `uploadObjectCommand` build `curl` calls against the native REST routes with `$NIMBUS_TOKEN` in the bearer header; `deployFunctionsCommand` is `nimbus dev --app-dir .`; `callFunctionCommand` is `nimbus run <server> functions <path> '{}' --tenant <t>`. A missing tenant or table falls back to the quick-start names (`demo`, `messages`, `messages:send`). |
| `src/hooks/use-server-url.ts` (new) | `useServerUrl` moved out of `routes/developer/index.tsx`: the origin of `useNimbus().url`, else `window.location.origin`; an unparsable value is returned as given. Overview and every rewritten empty state read through it. |
| `src/components/empty-state.tsx` | The link variant of `EmptyStateCta` takes `search`, so a CTA can carry `?create=1`. A snippet that contains a newline renders as a `pre` block in a flex row with one `CopyButton` beside it (the first cut positioned the button over the block and it covered a long first line); a one-line snippet keeps the chip. |
| `src/routes/developer/storage.tsx` | "No tenants yet" explains that tables live in a tenant, offers Create tenant (`/operator/tenants?create=1`, which UIR14 taught the page to open) and the create-tenant call. "No tables" says a table appears with its first document and shows the insert call. |
| `src/routes/developer/storage_.$table.tsx` | "No documents" offers Insert document, which opens the insert drawer on this page, plus the insert call for this table. |
| `src/routes/developer/observability/-logs.tsx`, `-runs.tsx` | "Open Compute" removed. Both empty states name `nimbus dev` and show the `nimbus run` call for the tenant. `LogStream`, `LogEmptyState` and `RunsEmptyState` take `tenantId`. |
| `src/routes/developer/schedules.tsx` | The scheduled and cron empty states name `ctx.scheduler.runAfter` and the API and show the schedule call and the cron call for the tenant. Both tables take `tenant`. |
| `src/routes/developer/compute_.$function.tsx` | The runs tab of a function shows the `nimbus run` call for that function's path. |
| `src/routes/operator/tenants.tsx` | The empty state keeps Create tenant and adds the create call. |
| `src/routes/developer/files.tsx` | The empty account shows the `PUT .../objects/assets/hello.txt` call beside the New bucket button. |
| `src/stories/empty-state.stories.tsx` | `WithSnippet` uses the deploy command; `WithMultiLineSnippet` shows the block form. |
| `DESIGN.md` Empty States | Two paragraphs: the next action is on the page, the seam file, the `$NIMBUS_TOKEN` convention, the chip-or-block rule, and the rule that a filtered-empty result carries no command. |

## Spec notes

- `first-run.spec.tsx` (new, 5 tests): each step flips `data-done`
  when its query settles, the progress text counts done steps, the copy
  controls go from two to zero as the steps complete, and the
  `firstRunComplete` truth table.
- `next-action.spec.ts` (new, 8 tests): each command names the server,
  the tenant, the table or function it was given, and the fallbacks.
- `use-server-url.spec.tsx` (new, 3 tests): provider origin, window
  origin, unparsable value.
- `empty-state.spec.tsx` (+1): a multi-line snippet renders as one
  block with one copy control.
- Route specs assert the snippet text of their page: `storage.spec.tsx`
  (create-tenant call, Create tenant CTA, no "top nav"; the pick-a-tenant
  state and the filtered states carry no snippet; the insert call under
  "No tables"), `storage_.$table.spec.tsx` (new describe: the insert
  call and the CTA that opens `documents-insert-drawer`), `schedules`,
  `logs`, `runs` (the `nimbus run` call and no CTA),
  `compute_.$function.runs` (`functions messages:list`), `tenants`,
  `files`.
- Fail-before: the new and patched specs against the previous build
  give 10 failed files (2 missing modules, 8 assertion failures on the
  old copy), saved as `uir17-fail-before.txt` in the session
  scratchpad. `first-run.spec.tsx` passed on the first run because UIR9
  shipped the panel; the plan's fail-before named the Overview spec,
  which UIR9 already closed.
- `tests/e2e/onboarding.spec.ts` (new, chromium, desktop-only): tenant
  `onboard-e2e`; Overview shows the first-run panel at "0 of 3 steps
  done" and no stats; Storage shows the insert call against
  `/api/tenants/onboard-e2e/documents`; Schedules shows the schedule
  call and, on `?section=cron`, the cron call; Observability Logs shows
  the `nimbus run` call with `--tenant onboard-e2e` and no CTA; Files
  shows the `PUT .../objects/assets/hello.txt` call.
- Three specs that render a page through the new hook needed the
  `useNimbus` entry in their `@nimbus/nimbus/react` mock
  (`documents-page`, `developer/observability`,
  `operator/observability`).
- Net: 121 files, 1045 tests (118 files, 1026 at UIR16).

## Verification

| Check | Result |
| --- | --- |
| `npm run lint` | clean, 1 pre-existing warning |
| `npm run typecheck` (nimbus-ui) and root `npm run typecheck` | clean |
| `npm run test` | 121 files, 1045 passed |
| `npm run build`, `npm run storybook:build` | ok |
| `make build` (worktree root) | ok |
| `npm run test:e2e` | 24 passed, 1 skipped (14 files; onboarding spec skipped on mobile) |
| `verify.sh` | 27 ok, 0 failing |
| `cargo fmt`, `make clippy` | not run: no Rust changed |

Screenshots at 1280×720 from the e2e walk against the rebuilt binary,
tenant `onboard-e2e` with nothing in it: `UIR17-storage-empty.png`
(Tables with the insert call), `UIR17-schedules-empty.png` (Scheduled
with the schedule call), `UIR17-logs-empty.png` (Logs with the
`nimbus run` call).

## Open items

- The Observability empty states name the quick-start function
  (`messages:send`) because the tabs do not read the functions list;
  the function page's runs tab names the real path.
- The commands assume `$NIMBUS_TOKEN` in the shell. The Settings page
  shows the server URL but does not hand out a token; the reader takes
  it from the CLI login.
- The Services empty state still points at the CLI without a command.
  Services are created through deploys, which UIR23 owns.
