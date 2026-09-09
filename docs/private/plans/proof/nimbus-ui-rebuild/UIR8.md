# UIR8 Delete the status bar, static sub-panels, Events/Errors nav, old keys

Date: 2026-09-08
Worktree: `~/src/github.com/nimbus/nimbus-worktrees/nimbus-ui-rebuild`, branch `codex/nimbus-ui-rebuild-phase2` (stacked on #331)

UIR5 already deleted `top-nav.tsx`, `primary-drawer.tsx` and
`appearance-menu.tsx`, and UIR6 renamed `sub-drawer.tsx` to
`sub-panel.tsx`. UIR8 removes what was left of the old shell.

## What changed

| Item | Result |
| --- | --- |
| `src/shell/status-bar.tsx`, `status-bar.spec.tsx` | Deleted (271 + 202 lines). The bar carried the connection state, the server version, the upgrade trigger, the server URL and the `⌘\` / `⌘K` / `/` hints. The connection state and version were already in the sidebar footer since UIR5, the server URL is on Settings and in the operator scope row, and the palette footer owns the keyboard hints since UIR7. Only the upgrade trigger had no other home. |
| `src/shell/sidebar/upgrade-row.tsx` (new) | The upgrade trigger's new home, one 32px row in the sidebar footer between the connection line and the theme toggle. It renders nothing while `StalenessApi.state` is `hidden` or there is no version info. `available` / `confirming` render `UpgradePopover` with a `rowClass` trigger ("Update to 0.2.0", `aria-haspopup="dialog"`); the popover opens upward and left-aligned from the row. `upgrading` and `upgraded` are `role="status"` rows ("Updating to 0.2.0…", "Updated to 0.2.0"). The dot takes its colour from `statePalette` through `UPGRADE_TONES` (`available` → `pending`, `upgrading` → `starting`, `upgraded` → `ready`), so it never invents a colour and never animates. Collapsed to the rail, the row keeps its `aria-label` and gets a `RailTooltip`. |
| `src/components/upgrade-popover.tsx` | The trigger button takes `triggerClassName` and `triggerLabel`; the default class is the old status-bar look, so the Settings `UpdatesValue` trigger is unchanged. Testid `status-version-trigger` renamed to `upgrade-popover-trigger`. |
| `src/shell/sidebar/footer.tsx` | Renders `<UpgradeRow collapsed={collapsed} />`. The footer now needs `StalenessProvider` above it; `sidebar.spec.tsx` and `mobile-sheet.spec.tsx` mock `useStalenessContext` with the `hidden` state. |
| `src/routes/__root.tsx`, `src/styles/tokens.css` | `<StatusBar />` and its import gone; the Toaster loses its `offset="calc(var(--statusbar-height) + 12px)"` and sits at the default bottom-right offset; `--statusbar-height` removed from the tokens. |
| `src/shell/sub-panel.tsx` | The overlay popup (below 1024px) is `bottom-0` instead of `bottom-[var(--statusbar-height)]`. `SubPanelItem.disabled` and the `aria-disabled` "coming soon" branch of `SubPanelStaticList` removed: a static panel lists only pages that exist. Story and spec updated (the `indexes` disabled item and the "disabled items" describe block are gone). |
| `src/components/page-tabs.tsx` (new) | `PageTabs<TId>` renders a `nav` of `Link to="."` chips that update only the `tab` search param (`aria-current="page"` on the active one). Shared by both observability routes; Settings keeps its sub-panel for categories per UIR6. |
| `src/routes/developer/observability.tsx`, `observability/-types.ts` | The Observability sub-panel (Logs, Runs plus disabled Events and Errors) is replaced by `PageTabs` under the page header. `OBSERVABILITY_TABS` is the single `[{id, label}]` source; `ObservabilityTab` is `"logs" \| "runs"`. `ACTIVE_/DISABLED_OBSERVABILITY_TABS`, `DisabledTab`, `ActiveTabLink`, `CategoryPill`, `useContributeSubPanel` and `OBSERVABILITY_SUB_PANEL` removed. |
| `src/routes/operator/observability.tsx` | Same shape: `ADMIN_OBSERVABILITY_TABS`, `PageTabs` under the header, no sub-panel, no `useNavigate`. |
| Events and Errors | No longer in any nav, sub-panel or tab strip; `verify.sh` asserts no "Coming soon" text. UIR20 adds their pages and re-adds the tabs. |
| Old localStorage keys | `nimbus-ui:palette`, `nimbus-ui:primary-drawer-collapsed` and `nimbus-ui:sub-drawer-open` were already absent from `src` and `tests` after UIR5 and UIR6 (`grep` returns nothing). No delete-on-boot code, per the no-shims rule. |
| Prose | Comments in `pill.tsx`, `error-boundary.tsx`, `main.tsx`, `not-found.tsx` and a `state-dot.spec.tsx` test name no longer name the status bar. |
| `tests/e2e/smoke.spec.ts` | Step 7 asserts the `observability-tabs` strip, Logs current by default, a click on Runs writes `?tab=runs` and moves `aria-current`, and no Events or Errors chips exist. |
| `DESIGN.md` | Layout diagram loses the status bar row and gains the update row; "Status bar" section replaced by "Page tabs"; "Bottom Status Bar" replaced by "Sidebar Footer" (connection line, update row states and tones, theme toggle, collapse); Observability sections list Logs, Runs, Events (UIR20), Errors (UIR20) and say the page uses tabs, not a sub-panel; toast section says nothing fixed sits under the toast stack. |

## Spec notes

- `upgrade-row.spec.tsx` (new, 14 tests): nothing while hidden; the
  available row is a `dialog` trigger with the label as `aria-label` and
  text, and no popover in the DOM; a click calls `openPopover` once;
  `confirming` renders `upgrade-popover`; `upgrading` is a `role="status"`
  row with no button; `upgraded` reads "Updated to 0.2.0"; collapsed keeps
  the `aria-label` with empty text. The `UpgradeDot` colour, no-animation
  and drift-lock tests are ported from the deleted `status-bar.spec.tsx`
  (three tones, colour from `statePalette`, glyph is never a question
  mark).
- `sidebar.spec.tsx` and `mobile-sheet.spec.tsx` mock `use-staleness`
  because `useStalenessContext` throws without its provider.
- `operator/observability.spec.tsx`: the router mock gains `Link`; the
  sub-panel mock and `navigateMock` are gone. Tests: Logs default, the
  strip is under the header (not inside it) with `aria-current` on the
  active chip, and only `logs` and `runs` exist.
- `developer/section-nav.spec.ts` drops its Observability describe (the
  Schedules tests stay); `observability-types.spec.ts` asserts the tab
  union derives from `OBSERVABILITY_TABS`.
- `sub-panel.spec.tsx` loses the "disabled items" block (5 tests).
- Net: 105 files, 824 tests (831 at UIR7; −22 status-bar, −5 sub-panel
  disabled, −2 section-nav, +14 upgrade-row, others reshaped).

## Verification

| Check | Result |
| --- | --- |
| `npx biome check --write src tests` and `npm run lint` (269 files) | clean |
| `npm run typecheck` (nimbus-ui) and root `npm run typecheck` | clean |
| `npm run test` | 105 files, 824 passed |
| `npm run build` | ok |
| `npm run storybook:build` | ok |
| `cargo fmt --all --check` | clean |
| `make build` (worktree root) | ok, 61s incremental |
| `npm run test:e2e` | 12 passed, 1 skipped (the sheet spec on chromium); smoke 2/2 with the new step 7 |
| `verify.sh` | 1 failing (developer settings spec, UIR13); all six UIR8 shell assertions pass |
| `grep` for `status-bar`, the three old keys, "Coming soon" in `src` and `tests` | no matches |

Screenshots at 1440×900, taken with a throwaway Playwright spec against
the rebuilt binary (spec deleted after). The upgrade row is driven by a
`page.route` on `/api/system/version-info` that reports 0.2.0 available
over 0.1.46 with a `brew upgrade nimbus` command:
`UIR8-observability-dark.png` and `UIR8-observability-light.png`
(`/ui/developer/observability`, Logs tab current, no sub-panel),
`UIR8-observability-runs-dark.png` (`?tab=runs`),
`UIR8-operator-observability-dark.png` (operator page tabs, no
`contentinfo` landmark anywhere on the page),
`UIR8-upgrade-row-dark.png` (footer: Connected · v0.1.46, "Update to
0.2.0" with the amber dot, Dark theme, Collapse; the existing toast at
bottom-right), `UIR8-upgrade-popover-dark.png` (the popover opened upward
from the row with the command and Copy command), and
`UIR8-upgrade-row-rail-dark.png` (the 64px rail: connection dot, update
dot, theme, expand tooltip).

## Open items

- Events and Errors return as `PageTabs` entries when UIR20 builds their
  pages; `OBSERVABILITY_TABS` is the one place to add them.
- `PageTabs` casts `tab.id as typeof prev.tab` inside the search updater
  because TanStack's `Link` collapses the updater type to `never` when
  `to` is generic; `to="."` keeps the route's own search type. A route
  whose `tab` union does not include the ids passed would fail at the
  call site's `active` prop, not silently.
- The staleness toast ("Nimbus 0.2.0 available") and the sidebar row show
  the same fact at the same time on first sight; the toast is dismissable
  and the row stays, which is the intent, but the copy could be shortened
  once the update row has been in use.
