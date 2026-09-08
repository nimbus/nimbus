# UIR10 Compute

Date: 2026-09-08
Worktree: `~/src/github.com/nimbus/nimbus-worktrees/nimbus-ui-rebuild`, branch `codex/nimbus-ui-rebuild-phase3` (stacked on #332)

Commit `COMMIT_SHA`. Compute had its own toolbar, filter chips and a
drawer, a function page with a Logs tab that duplicated Observability,
and a runner that asked for a tenant and took only JSON. The sub-panel
now owns the function tree, the page and the function page use
`PageTabs`, every list is a `DataTable`, and the runner reads the
function's validator into a form.

## What changed

| Item | Result |
| --- | --- |
| `src/routes/developer/compute.tsx` (rewritten, 257 lines) | Search `{tab}` with `-compute-tabs.ts` (`functions`, `sandboxes`, `graph`; `parseComputeTab`). The sub-panel is `FunctionSubPanel` with the filter box. `PageHeader` with the bundle count as trailing text, `PageTabs` (`compute-tab-*`). Functions tab: `DataTable` (`compute-functions`) of path, kind pill, adapter pill, last status pill, last run; a row opens the function page. Empty: "No functions deployed" with `nimbus dev --app-dir .` and a copy control. Sandboxes tab keeps the empty state; Graph tab renders `GraphView`. The old toolbar, `FilterChips`, `ComputeDrawer` and `-compute-views.ts` are gone. |
| `src/shell/function-sub-panel.tsx` (new, 29 lines) | Reads the sub-panel search and renders `FunctionTreeView` with `testidPrefix="sub-panel"`. |
| `src/routes/developer/compute_.$function.tsx` (rewritten, 679 lines) | Breadcrumb Compute › path, mono title, kind and adapter pills, bundle `CopyChip`, `PageTabs` Overview / Source / Runs / Graph (Logs removed). Overview: facts and the argument list from the validator (`function-overview-arg-*`). Source: `SourceTab` with the `nimbus dev --app-dir .` empty state (`function-source-missing-snippet` with copy), the error state, and the module with the DEFINES / CALLS / CALLED BY strip. Runs: `RunsTab` as `DataTable` (`function-tab-runs`), `SkeletonRows` while loading, "No runs yet" when empty; a row opens the run page. Graph: `GraphView focus={fn.path}`. The bundle fact matches `bundles.sha256` to `functions.bundleId` (the server keys by sha, the old page compared the document id and always showed "—"). |
| `src/components/function-runner/args-validator.ts` (new, 185 lines) | `parseArgsValidator` reads the SDK shape (`{kind:"object", fields}` with `optional/inner`) and the Convex JSON shape (`{type:"object", value:{fieldType, optional}}`) into `ArgField[]` (text, number, boolean, json). `buildArgs` turns field values into the argument object with per-field errors; `valuesFromArgs` does the reverse for the mode switch. |
| `src/components/function-runner/function-runner.tsx` (rewritten, 588 lines) | Toggle bar: "Runner", kind and adapter `CategoryPill`, `tenant {t}` read-only, last-run `Pill`. Body is a form: Form / JSON `SegmentedControl` when a validator exists, one `Input`/`Checkbox`/`Textarea` per field (`function-runner-field-*`), JSON textarea otherwise; "Run function" with `⌘ ⏎` kbd, ⌘/Ctrl+Enter submits, plain Enter in a text field is swallowed. Result panel with ok / error pills, duration, JSON, and the correlation id as a `CopyChip`. No tenant chooser: `activeTenant` from the store. A stale answer after a newer submit is dropped (`lastSubmitRef`). |
| `src/routes/developer/compute_.runs_.$runId.tsx` | Breadcrumb Compute › Runs (Observability runs tab) › id; kind as `CategoryPill`. `Breadcrumb` segments accept `search`. |
| `src/routes/developer/-graph-view.tsx` | `GraphView({focus})` draws the focused node with the accent stroke. |
| `src/stories/function-runner.stories.tsx` (new) | `Components/FunctionRunner`: WithValidator, WithoutValidator, NotRunnable, NoTenant. |
| `DESIGN.md` | "Compute (Developer)" and "Function Runner" rewritten for the sub-panel tree, the page tabs, the four function tabs, the source empty state, and the runner contract. |

## Spec notes

- `function-runner.spec.tsx` (new, 7 tests): prefilled fields for a
  two-string validator in form mode, ⌘⏎ posts
  `/convex/demo/mutation` with `{name, args}` and shows the correlation
  id, the submit label with two `kbd`, no tenant chooser and the target
  text, JSON-only without a validator, values carried across the mode
  switch both ways, the error envelope with remediation and request id.
  Fail-before on the first run: 5 failed, 2 passed, the first on
  `function-runner-field-text`.
- `args-validator.spec.ts` (new, 7 tests): both validator shapes,
  optional and id fields, number and JSON errors, round trip.
- `compute_.$function.source.spec.tsx` (new, 3 tests): the missing
  state shows `SOURCE_CAPTURE_COMMAND` with a copy button, the error
  state, the present state with code and symbol chips.
- `compute_.$function.runs.spec.tsx` (replaced, 4 tests): skeleton,
  empty, `role="table"` with three rows and pills, row activation.
- `compute-tabs.spec.ts` (new, 3 tests) replaces `compute-views.spec.ts`.
- `tests/e2e/compute.spec.ts` (new, chromium): seeds a tenant, a bundle
  and `agent:send` with a validator through `/convex/_nimbus/mutation`,
  then asserts the page, the tabs, the table row, the sub-panel leaf,
  the Overview arguments, the runner fields in form mode with no tenant
  chooser, the Source empty state with the command and copy button, and
  the leaf navigation to the Source tab.
- Net: 108 files, 836 tests (105 files, 822 at UIR9).

## Verification

| Check | Result |
| --- | --- |
| `npx biome check --write` and `npm run lint` (277 files) | clean |
| `npm run typecheck` (nimbus-ui) and root `npm run typecheck` | clean |
| `npm run test` | 108 files, 836 passed |
| `npm run build` | ok |
| `npm run storybook:build` | ok |
| `cargo fmt --all --check` | clean |
| `make build` (worktree root) | ok |
| `npm run test:e2e` | 13 passed, 1 skipped (chromium and mobile projects; compute spec skipped on mobile) |
| `verify.sh` | 26 ok, 1 FAIL (spec for routes/developer/settings, owned by UIR13) |

Manual run of agent-chat `send`, recorded in
`UIR10-agent-chat-send-dark.jpg`: `nimbus dev --app-dir
examples/nimbus/agent-chat --no-open --skip-codegen --once` with an
isolated `HOME` and data dir, signed in through the launch URL, tenant
`demo`. The deploy registered `agent:list`, `agent:listMemory`,
`agent:send`, `agent:deliverReminder`. The server writes no
`argsSchema`, so the `agent:send` row was patched through
`/convex/_nimbus/mutation` (`type: update`) with the two-string
validator. The runner opened in form mode, `conversationId` `conv-1`
and `text` `hello from the console` were typed, ⌘⏎ ran it: `ok · 91ms`,
result `{"tool": null}`. `agent:list` on `demo` then returned the user
message and the assistant reply, and the Runs tab showed the run
(`ok`, 82ms). The persisted active tenant `acme-prod` from an earlier
session was shown on first load although that server only had `demo`.

Screenshots at 1440×900 from a throwaway Playwright spec against the
rebuilt binary (spec deleted after), tenant `agent-chat` with six
agent-chat functions (validators seeded) and twelve runs:
`UIR10-compute-dark.png` and `UIR10-compute-light.png` (tree in the
sub-panel, tabs, the table), `UIR10-function-runner-dark.png` and
`UIR10-function-runner-light.png` (Overview with the argument list,
runner open in form mode with both fields filled, "Run function ⌘ ⏎"),
`UIR10-function-source-missing-dark.png` (the source empty state with
the command), `UIR10-function-runs-dark.png` (the runs table).

## Open items

- The server never writes `argsSchema` or `adapter` on deploy
  (`crates/nimbus-system/src/records/deployment.rs` writes `bundleId`,
  `path`, `kind`), so a real deploy opens the runner in JSON mode and
  shows "—" for adapter. The console is ready for the fields; the
  deploy record is the owner.
- `lastStatus` and `lastRunAt` on the function row are not derived by
  the server either, so the Overview says "never run" after a run that
  the Runs tab lists.
- Sandboxes is still an empty state until the sandbox runtime exposes
  its live state.
- A persisted active tenant that the server does not have is shown
  until the operator picks another; the tenant selector should fall
  back to the first tenant the server reports.
