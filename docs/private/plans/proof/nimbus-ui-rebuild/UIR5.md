# UIR5 Sidebar

Date: 2026-09-08
Worktree: `~/src/github.com/nimbus/nimbus-worktrees/nimbus-ui-rebuild`, branch `codex/nimbus-ui-rebuild-phase2` (stacked on #331)

## What changed

| Item | Result |
| --- | --- |
| `src/shell/sidebar/sidebar.tsx` | New. `Sidebar` is the `<aside>` column: 240px expanded, 64px rail, `data-view`, `data-collapsed`, `data-tier`. `SidebarBody` composes the brand row (28px outline mascot and the wordmark, linking to the view home), the scope row, the nav groups and the footer, and is shared with the mobile sheet. `useSidebarCollapse` persists the desktop choice under `nimbus-ui:sidebar-collapsed`; below the desktop tier the sidebar starts as a rail and a toggle is a per-tier session override that is not persisted. |
| `src/shell/sidebar/scope-row.tsx` | New. Expanded: the `ViewSwitcher` segmented control, then the tenant selector (developer) or the server identity chip (operator, host from the client URL). Collapsed: an icon radio group for the view and one scope button whose label names the tenant or the server and whose click expands the sidebar. |
| `src/shell/sidebar/nav-groups.tsx` | New. `<nav aria-label="Primary">` with the groups from `nav-entries.ts`; group labels are the only tracked capitals in the console (`sidebar/` is the allowed path in `token-utilities.spec.ts`). The active row has the amber accent bar and `aria-current="page"`; the home entry matches exactly, every other entry by prefix. Collapsed rows are icon-only with a right-side tooltip and an `aria-label`. |
| `src/shell/sidebar/footer.tsx` | New. Connection status (`role="status"`, `data-state`, "Connected · v0.1.46"), the labelled theme toggle ("Light theme" / "Dark theme", aria-label names the switch), and the collapse button (`aria-expanded`, `aria-controls="sidebar"`). The sheet hides the collapse button. |
| `src/shell/sidebar/rail.tsx` | New. `rowClass` is the one row recipe (44px rows in the sheet, 36px on desktop; 40px square in the rail) and `RailTooltip` wraps the registry tooltip on the right side. No sidebar file binds a private focus ring: the global `:focus-visible` rule paints it (`contrast.spec.ts`). |
| `src/shell/sidebar/mobile-sheet.tsx` | New. Below 640px (`useSmallScreen`) the root renders `MobileTopBar` instead of the sidebar: a 48px bar with the menu button and the brand, and the registry `Sheet` on the left carrying `SidebarBody`. The sheet closes on a link click and on any pathname change (state adjusted during render, so a view switch inside the sheet cannot leave it open over the top bar). |
| `src/shell/nav-entries.ts` | Rewritten as `NavGroup[]` per view: Developer = Overview, Build (Compute, Storage, Files), Run (Services, Schedules), Observe (Observability), Settings; Operator = Nodes, Fleet (Machines, Network, Services), Access (Tenants), Observe (Observability), Settings. Count badges, `countKind` and `tenantScoped` are gone from the entry shape. `navGroupsForView`, `navEntriesForView` and `viewFromPathname` are the selectors. |
| `src/shell/view-switcher.tsx` | `useViewSwitch` exported so the rail radio group and the segmented control share the persist and restore logic; the expanded control is full width at 28px. |
| `src/shell/tenant-selector.tsx` | Trigger and menu are full width inside the scope row. The `operator-filter` mode is kept but not rendered; see Open items. |
| `src/shell/use-viewport-tier.ts` | `SMALL_SCREEN_QUERY` (`max-width: 639px`) and `useSmallScreen`. |
| `src/store/ui-store.ts` | `sidebarCollapsed`, `toggleSidebar`, `setSidebarCollapsed`, `readSidebarCollapsed`, `SIDEBAR_COLLAPSED_KEY`. The `primary-drawer-collapsed` key and its store slice are gone. |
| `src/routes/__root.tsx` | Renders the top bar on small screens and the sidebar otherwise, before the sub-drawer and the main column. |
| Deleted | `top-nav.tsx`, `primary-drawer.tsx`, `appearance-menu.tsx` and their specs. UIR8's deletion list shrinks to the status bar, the static sub-drawers, the "Coming soon" items and the old localStorage keys. |
| `tests/e2e/smoke.spec.ts` | The walk navigates through the sidebar with `openNav`, `closeNav`, `switchView`, `navigateTo` and `openSubDrawer` helpers, so the same eight steps run on the chromium and the new `mobile` (Pixel 7) Playwright projects. `openNav` dismisses an open sub-drawer overlay and opens the sheet on the small layout. |
| `tests/e2e/sidebar-sheet.spec.ts` | New, mobile project only: at 400px the top bar shows, no `<aside>` mounts, the sheet opens with the view switcher, the active row and the theme toggle, and closes on a link tap and on a view switch. |
| `tests/e2e/auth-overview.spec.ts` | Two assertions had been stale since the 2026-05-20 sign-in page rewrite ("Nimbus Sign In" and a "Nimbus" heading no longer exist, and the page carries one inline script). They now assert the brand image, the "Enter auth token" label, "Sign in to Nimbus" and the absence of the SPA `#root` mount. The full e2e suite had two failures on main because of this. |
| `DESIGN.md` | Layout diagram and the Sidebar section replace the top-nav and primary-drawer sections; responsive tiers rewritten (desktop, tablet rail, mobile overlay, small-screen sheet); the operator tenant filter is noted as moving to the Observability filter bar. |

## Spec notes

- `sidebar.spec.tsx` (9 tests): group order and labels per view, the scope
  row swap on a view switch, no count badges, exact and prefix active
  matching, collapse persistence and ARIA, the scope button expanding the
  rail, the tablet default rail without persistence, the theme toggle text
  and `nimbus-ui:theme`, and the footer status text.
- `mobile-sheet.spec.tsx` (2 tests): the sheet opens from the menu button
  and closes on a link click; a pathname change closes it.
- `nav-entries.spec.ts` rewritten (10 tests) for the grouped shape.
- `__root.spec.tsx` mocks the sidebar, the top bar and `useSmallScreen`.
- The first attempt to close the sheet on navigation used a `useEffect` on
  the pathname and biome rejected the dependency list; the state-adjust
  during render pattern is what shipped.
- A biome rule refused `aria-label` on a plain `div`, so the connection row
  carries `role="status"`.

## Verification

| Check | Result |
| --- | --- |
| `npm run lint` (biome, 268 files) | clean |
| `npm run typecheck` (nimbus-ui) and root `npm run typecheck` | clean |
| `npm run test` | 105 files, 812 passed (826 at UIR4; top-nav 8, primary-drawer 7, appearance-menu 4 specs removed; sidebar 9, mobile-sheet 2 added; nav-entries re-cut) |
| `npm run build` | ok |
| `npm run storybook:build` | ok |
| `cargo fmt --all --check` | clean |
| `make build` (worktree root) | ok, 57s incremental |
| `npm run test:e2e:smoke` | 2/2 passed (chromium 8.3s, mobile) |
| `npm run test:e2e` | 11 passed, 1 skipped (the sheet spec on chromium); 9 passed and 2 failed before the sign-in test fix |
| `verify.sh` | UIR5 row `ok`; 4 failing (sub-panel UIR6, status-bar and sub-drawer UIR8, developer settings spec UIR13) |

Screenshots at 1280×800: `UIR5-sidebar-light.png` and `UIR5-sidebar-dark.png`
(developer view, expanded), `UIR5-rail-dark.png` (64px rail with the Compute
tooltip), `UIR5-operator-light.png` (operator view with the server chip),
`UIR5-sheet-400.png` (400px sheet over the Overview page).

## Open items

- The operator tenant filter (`TenantSelector mode="operator-filter"`) has
  no host until UIR8 puts a filter bar on the Observability tabs.
- The status bar still shows the connection state and the server URL that
  the sidebar footer now owns; UIR8 deletes it.
- The sign-in page keeps its blue palette (carried from UIR4).
