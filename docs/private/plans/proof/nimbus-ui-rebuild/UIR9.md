# UIR9 Developer Overview

Date: 2026-09-08
Worktree: `~/src/github.com/nimbus/nimbus-worktrees/nimbus-ui-rebuild`, branch `codex/nimbus-ui-rebuild-phase3` (stacked on #332)

Commit `8ef79ea6c`. The Overview was sixteen equal boxes (machines,
services, tenants, functions, tables, runs, an events feed) with no
thesis. It now answers "is my server fine" in one line, then gives a
developer the three things they come here for: how to connect, how big
the server is, and what ran last.

## What changed

| Item | Result |
| --- | --- |
| `src/routes/developer/index.tsx` (rewritten, 801 lines) | Four queries (`system.status`, `functions.list`, `tables.list` for the active tenant, `runs.recent`, each limit 200) and one `LoadingValue` for the inventory so the page loads as one value. Exports `readHeadline`, `connectSnippets`, `hourlyBuckets`, `readStats` and the `RunRow` type for the spec. |
| Headline (`overview-headline`) | The mascot in the server's state and one sentence. Order: connection dropped ("The connection to the server dropped. Stale data is shown."), status or inventory error (the message), loading ("Reading the server status.", working mascot), health not ok ("The server reports its health as {health}."), first run ("The server is up and waiting for its first app.", empty mascot), failed runs in 24h ("The server is up. N runs failed in the last 24 hours.", error mascot), else "The server is up and every recent run succeeded." (idle). Under it one mono fact line: tenant, server URL, version; a fact the server did not report is left out, never a dash. |
| Connect (`overview-connect`) | The server origin as a `CopyChip` and one snippet per client in registry `Tabs` (line variant): curl against `POST /api/tenants/{t}/query` with a Bearer header, the TypeScript SDK (`new NimbusClient("{origin}/convex/{t}")` and `client.query(api.x.y, {})`), the Convex `ConvexReactClient`. Each names the active tenant and the first function path and table the server has, with `demo`, `messages.list` and `messages` as defaults, so it runs as pasted. The copy button sits in a flex row beside the tab list (an absolute overlay covered the code at 390px). |
| Server URL | `useNimbus().url` is the console's own Convex endpoint (`/convex/_nimbus`), not the server. The page uses `new URL(url).origin`; the first e2e run showed `…/convex/_nimbus/api/tenants/…` in the snippet. |
| Stats (`overview-stats`) | Four `Link` tiles: functions (subline with the top three kinds, to Compute), tables ("in {tenant}", to Storage), runs in 24h and errors in 24h (each an hourly `Sparkline` from `hourlyBuckets`, 24 points oldest first; errors use `CHART.error` when non-zero; both link to the Runs tab of Observability, errors with `status=error`). Runs and errors render only when the server has any run, the others only when the count is above zero. The stat tile lost its `focus-visible:ring-accent` class because `contrast.spec.ts` forbids per-component rings. |
| Recent runs (`overview-runs`) | The five newest as a `DataTable` (function, status pill, duration, relative time), `onRowActivate` navigates to `/developer/compute/runs/$runId`, "View all runs" links to `?tab=runs`. |
| `src/components/onboarding/first-run.tsx` (new, 139 lines) | `FirstRun({progress, testid, className})` and `firstRunComplete(progress)`. Solid empty mascot at 72px, "Nothing here yet", three steps with commands and a `CopyButton` (install the CLI, run `nimbus dev` in an app, call a function); a step is done when the same queries show functions or runs, and the check circle fills. sr-only "N of 3 steps done". Shown on the Overview in place of the stats and the table when the tenant has no tables and no step is done; the connect panel stays. |
| `src/stories/first-run.stories.tsx` (new) | `Components/FirstRun`: NothingYet, FunctionsDeployed, Complete. |
| Removed from the page | Machines, services and tenants tiles and the events feed. They belong to the operator console. |
| `DESIGN.md` | "Overview (Developer)" rewritten: headline, connect, stats, recent runs, first-run panel; no greeting, no marketing copy, no tile for a number the server has not reported. |

## Spec notes

- `index.spec.tsx` (rewritten, 17 tests): mocks the router (`Link` as an
  anchor with `to` plus search), `useQuery` per query key,
  `useNimbusConnectionState`, and `useNimbus` with a `/convex/_nimbus`
  URL so the origin rule is proved. Fake timers at 2026-09-08 12:00 UTC.
  Headline (6): offline beats a healthy status, error message, loading
  sentence and mascot state, unhealthy health text, failed-run count,
  all-good sentence and the fact line. Connect (2): the three tabs and
  their content name the tenant, function and table. Stats (4): counts,
  sublines, hourly buckets, hidden tiles. Recent runs (2): five rows and
  activation navigates to the run. Empty tenant (2): the first-run panel
  replaces the tiles; steps read done from the queries and the panel
  retires on the first run.
- The first version of the mock returned the status document for every
  query, so `functions` was not iterable; the mock now returns status
  only for `api.system.status`.
- `tests/e2e/smoke.spec.ts` step 1 asserts `page-overview`, the
  headline, the connect panel with a snippet that contains
  `${baseURL}/api/tenants/`, the first-run panel visible, and no stats
  on a fresh server.
- Net: 105 files, 822 tests (824 at UIR8; the old Overview spec had 19).

## Verification

| Check | Result |
| --- | --- |
| `npx biome check --write` and `npm run lint` (271 files) | clean |
| `npm run typecheck` (nimbus-ui) and root `npm run typecheck` | clean |
| `npm run test` | 105 files, 822 passed |
| `npm run build` | ok (index chunk 479 kB) |
| `npm run storybook:build` | ok |
| `cargo fmt --all --check` | clean |
| `make build` (worktree root) | ok, 68s incremental |
| `npm run test:e2e` | 13 passed, 1 skipped; smoke walk 2/2 (chromium, mobile) with the new step 1 |
| `verify.sh` | 26 ok, 1 failing (developer settings spec, UIR13), same as UIR8 |

Screenshots at 1440×900, taken with a throwaway Playwright spec against
the rebuilt binary (spec deleted after). The server was seeded through
`POST /convex/_nimbus/mutation` with agent-chat-shaped data in tenant
`default`: six functions (`messages:list`, `messages:send`,
`agent:reply`, `agent:remember`, `agent:recall`,
`agent:deliverReminder`), tables `messages` (42 rows) and `agentMemory`
(7), about 62 runs over 24 hours with 2 errors. The `runs.error` field
must be an object (`{message}`), the server returns 422 for a string.
`UIR9-overview-dark.png` and `UIR9-overview-light.png` (headline "The
server is up. 2 runs failed in the last 24 hours.", curl tab, four
tiles with sparklines, five runs), `UIR9-overview-sdk-dark.png` (SDK
tab), `UIR9-overview-first-run-dark.png` (a fresh server: headline
"waiting for its first app", connect panel, the first-run panel with
no step done), `UIR9-overview-390-light.png` (mobile, full page: tiles
in one column, the copy button beside the tabs, the runs table scrolls
inside its container).

## Open items

- Runs link to `/developer/compute/runs/$runId`; that route exists today
  and UIR10 rebuilds it.
- The snippet defaults (`messages.list`, `messages`) show on a server
  with no functions; the first-run panel above them explains why.
