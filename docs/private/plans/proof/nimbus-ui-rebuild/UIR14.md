# UIR14 Operator pages

Date: 2026-09-08
Worktree: `~/src/github.com/nimbus/nimbus-worktrees/nimbus-ui-rebuild`, branch `codex/nimbus-ui-rebuild-phase3` (stacked on #332)

Commit `4ce10c6a1`. The operator overview was a card grid with the same
counts as the developer page; Tenants had an inline Delete with the same
weight as Create and a create handoff that never fired; Machines showed
a bare total and a pill per row. The overview now reads as one headline
with a facts line, a node card, labelled hosted tiles, and recent
events. Tenants has one primary Create action with a dialog and a
row-menu Delete behind `ConfirmDialog`. Machines shows a `StateDot` per
row and a header that says what the fleet is and what it adds up to.

## What changed

| Item | Result |
| --- | --- |
| `src/routes/operator/index.tsx` (rewritten, 723 lines) | `readNodeHeadline(status, hosted)` orders offline, status error, inventory error, reading, unhealthy, failing services, no services, all running, R of N. `readFacts` reads `details.listenAddress` (or `address`), version, `startedAt`, `details.dataDir`. Node card with health pill, role subline, started, updated, build hash. Hosted tiles link to Tenants, Machines, Services, Network with a large count and a subline under it (`stateSummary` for machines and services, `adapterSummary` for listeners), which fixes F3: the listeners tile no longer runs the count into the adapter list. Recent events: `api.events.recent` with `limit: 5` on `DataTable`, row activation opens Observability on the Logs tab narrowed to the correlation id. |
| `src/lib/inventory-summary.ts` (new, 71 lines) | `stateSummary`, `adapterSummary`, `capacityOf`, `capacitySummary`. Shared by the overview and Machines. |
| `src/routes/operator/tenants.tsx` (rewritten, 468 lines) and `tenants/-create-tenant-dialog.tsx` (new, 148 lines) | `DataTable` with id copy chip, table count, row menu (open, copy id, delete) on right-click and the `⋯` button. `Create tenant` is the one header action; the dialog holds the id input, the pending label `Creating…`, and the server refusal beside the input. Success toasts `Created tenant <id>` and invalidates the loader. `?create=1` opens the dialog on arrival and clears on close. Delete runs through `ConfirmDialog` with the table count in the description and typed proof when the count is above zero. |
| `src/routes/operator/machines.tsx` | State cell is `StateDot` plus the state word. Header trailing is `MachinesSummary`: `N machines · <state summary>` and the fleet capacity on a second line, `loading…` in flight. Sub-panel items are buttons with a `StateDot` that select the machine in place and carry `aria-current`. The nine-column table geometry is unchanged. |
| `src/routes/operator/network.tsx`, `-network-inventory.tsx` | No change: every count already reads `N of M routes` or `N <noun>`. |
| `tests/e2e/smoke.spec.ts` | The tenants step asserts `page-tenants` and the table, empty, or error envelope. |
| `DESIGN.md` | Nodes, Tenants, Machines, and Network describe the shipped headline, facts, tiles, events, create dialog, row-menu delete, `StateDot` rows, header summary, and labelled counts. |

## Spec notes

- Root cause of the silent create failure: the Developer console handed
  off with `?create=1` and the operator page never read the param, so
  nothing opened. The API call itself worked. The fix is the search
  param bound to the dialog, with `replace: true` navigation on close.
- `tenants.spec.tsx` (rewritten, 19 tests): loader (non-OK, throws,
  sorted, abort signal), `tenantRows`, envelope and Retry, empty CTA
  opens the dialog, Create is the one header action and no inline
  delete, sub-panel href, row navigation and inner controls, the
  acceptance tests for pending `Creating…` then toast then row, the
  refusal kept in the dialog with `role=alert` and `aria-invalid`,
  Enter submits, `?create=1` opens and cancel clears, context menu to
  `tenants-row-menu-delete` to the dialog to `Working…` to toast and
  the row gone, the typed phrase when tables exist, and a server
  refusal kept in the dialog.
- `index.spec.tsx` (rewritten, 20 tests): `readNodeHeadline` (7),
  `readFacts` (2), headline and facts, the working face while status is
  in flight, null status as idle, the node card, labelled hosted counts
  and sublines with the listeners href, recent events with the error
  dot and navigation, empty events, and the four tenant-count cases.
- `machines.spec.tsx` (16 tests, 5 new): `StateDot` per state, the
  labelled header with capacity, the singular and the missing capacity
  line, `loading…`, and selection from the sub-panel with
  `aria-current` on the republished item.
- `inventory-summary.spec.ts` (new, 5 tests).
- `tests/e2e/tenants.spec.ts` (new, chromium, 2 tests): creates
  `tenants-e2e` through the dialog and reads it back from the API,
  refuses a duplicate with the server message in the dialog, opens and
  closes through `?create=1`, deletes from the row menu and reads the
  API list without it; then reads the overview headline, facts, the
  tenant count against the API, the listeners subline, node health, and
  the machines header summary.
- Net: 112 files, 918 tests (110 files, 884 at UIR13).

## Verification

| Check | Result |
| --- | --- |
| `npm run lint` (289 files) | clean, 1 pre-existing warning |
| `npm run typecheck` (nimbus-ui) and root `npm run typecheck` | clean |
| `npm run test` | 112 files, 918 passed |
| `npm run build` | ok |
| `npm run storybook:build` | ok |
| `cargo fmt --all --check` | clean |
| `make build` (worktree root) | ok, 1m 06s |
| `npm run test:e2e` | 19 passed, 1 skipped (chromium and mobile; tenants spec skipped on mobile) |
| `verify.sh` | 27 ok, 0 failing |

Screenshots at 1280×720 from the e2e walk against the rebuilt binary:
`UIR14-nodes.png` (the overview on a fresh node), `UIR14-create-dialog.png`
(the create dialog with the id typed), `UIR14-tenants.png` (the table
after the create toast), `UIR14-delete-dialog.png` (the row-menu delete
on a tenant with no tables), `UIR14-machines.png` (the empty machines
page with the labelled header).

## Open items

- The node card's build hash reads `—`: the status row carries no build
  hash yet. The server owns writing it into `details`.
- The tenants table shows a Rows column at zero: the tenant list route
  returns ids only and the table count comes from the per-tenant read;
  a row count needs a server field.
- Upgrade state and recent admin actions on the overview wait on
  Settings (UIR13 shipped the update pill) and an admin-action event
  category.
