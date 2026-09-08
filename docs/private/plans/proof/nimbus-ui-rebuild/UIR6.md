# UIR6 Resizable sub-panel

Date: 2026-09-08
Worktree: `~/src/github.com/nimbus/nimbus-worktrees/nimbus-ui-rebuild`, branch `codex/nimbus-ui-rebuild-phase2` (stacked on #331)

## What changed

| Item | Result |
| --- | --- |
| `src/shell/sub-panel.tsx` | Replaces `sub-drawer.tsx` (git rename, then rewritten). `SubPanelLayout` wraps `<main>` in one `react-resizable-panels` `Group` keyed by section; the sub-panel `Panel` and its `Separator` are the only conditional children, so a spec arriving or leaving never remounts the page. Panel limits 180 / 240 / 400px, `collapsible` to a 32px rail, `groupResizeBehavior="preserve-pixel-size"`. The separator is a 1px accent-on-hover/focus/drag handle with `aria-label="Resize sub-panel"`; the library binds ArrowLeft/Right, Home/End and Enter to it. Width and collapsed state persist per section under `nimbus-ui:panel:<section>` (second path segment: `storage`, `compute`, `tenants`, `settings`, ...); stored widths clamp on read. Expand restores the stored width through the imperative `resize`. `SubPanelProvider`, `useContributeSubPanel`, `useSubPanelSearch`, `showsSubPanelSearch` (search field once `rows > 20`), `SheetSubPanel` (tablet and mobile overlay), `SubPanelRail`, `SubPanelBody` and `SubPanelStaticList` are the parts. |
| Ids | Panel ids are `sub-panel-column` and `main-column`, the separator id `sub-panel-separator`: the library stamps each id on its element as `data-testid`, so `sub-panel` as a panel id collided with the shell's own `sub-panel` testid. |
| Layout callbacks | `onLayoutChange` (collapsed state, live during drag) and `onLayoutChanged` (persist) both ignore a layout without the sub-panel column. The group first registers with the main column alone before a spec arrives, and reading the collapsed state then reported "expanded" and overwrote a stored `collapsed: true`. |
| `src/store/ui-store.ts` | `subPanelOpen` slice, `nimbus-ui:sub-drawer-open` key, and the toggle removed; the panel owns its own preference. |
| `src/routes/__root.tsx` | `SubPanelProvider` and `SubPanelLayout` around the main column. |
| Consumers | `tables-sub-panel.tsx`, `compute_.$function.tsx`, developer and operator `services*.tsx`, `operator/tenants.tsx`, `operator/machines.tsx` pass `search.rows` so the search field follows the twenty-row rule. `NoTenantHelp` copy points at the sidebar scope row. Every `sub-drawer` / `SubDrawer` / `subDrawer` name renamed across `src`, `tests`, and `CATALOG.md`; testids are `sub-panel`, `sub-panel-toggle`, `sub-panel-search`, `sub-panel-overlay`, `sub-panel-scrim`, `sub-panel-item-*`, `sub-panel-rail-item-*`. |
| `src/stories/sub-panel.stories.tsx` | Story-only `FakeSubPanelHost` and `FakeSubPanelRail` at fixed widths, restyled to the new rows (`rounded-sm`, `bg-bg-hover` active, `aria-disabled` span with the coming-soon chip); `Widths` story shows 180 / 240 / 400. `CATALOG.md` note updated. |
| `tests/e2e/sub-panel.spec.ts` | New, chromium only: default 240 and stored prefs, an 80px drag to 320, reload keeps 320 on Settings while Storage opens at 240, ArrowLeft shrinks, `End` reaches 400, `Enter` collapses to the 32px rail and stores `{400, true}`, reload keeps the rail, the toggle expands back to 400 and collapses again. |
| `tests/e2e/smoke.spec.ts` | `openSubPanel` helper (variable rename). |
| `DESIGN.md` | "Sub-drawer" section rewritten as "Sub-panel": limits, separator keys, rail, per-section keys, tiers; the layout diagram, responsive tiers, secondary-navigation rules and do-not list renamed. |

## Spec notes

- `sub-panel.spec.tsx` rewritten (23 tests, 8 before): page stays mounted
  when a spec arrives (same input element keeps its value); static and
  dynamic bodies; search field only past twenty rows; default width and
  stored prefs; ArrowRight/ArrowLeft on the separator (300, then 180);
  a step past the minimum collapses to the rail and keeps the last width;
  width survives unmount and remount; one key per section; collapse and
  expand restore the chosen width; stored `{300, true}` hydrates as the
  rail and expands to 300; stored 9000 clamps to 400; rail items switch
  without expanding; tablet rail without a sheet, overlay and scrim on
  expand, no desktop prefs written at tablet width; hit targets; disabled
  items.
- happy-dom has no layout, so the spec installs `offsetWidth` /
  `offsetLeft` getters that derive widths from the inline `flex-basis` and
  `flex-grow` the library writes (group 1200px, separator 1px), and an
  `ariaDisabled` getter, which happy-dom lacks and the library reads to
  decide whether a separator is disabled. Without the `offsetLeft` shim the
  library sorted the separator first and threw "Matching panels not found".
- Plan step "a keyboard step stops at the minimum" does not hold: the
  library collapses a collapsible panel when an arrow step or `Home` goes
  past `minSize`. The spec and the e2e spec assert that behaviour instead,
  and the stored width is kept so expand restores it.
- The double-click-on-background toggle from the old sub-drawer is gone;
  the header button, the separator `Enter` key and the rail button are the
  three collapse paths.

## Verification

| Check | Result |
| --- | --- |
| `npm run lint` (biome, 268 files) | clean after 4 formatter fixes |
| `npm run typecheck` (nimbus-ui) and root `npm run typecheck` | clean |
| `npm run test` | 105 files, 819 passed (812 at UIR5; sub-panel 8 → 23) |
| `npm run build` | ok |
| `npm run storybook:build` | ok |
| `cargo fmt --all --check` | clean |
| `make build` (worktree root) | ok, 65s incremental |
| `npm run test:e2e` | 12 passed, 1 skipped (the sheet spec on chromium); includes smoke 2/2 and the new sub-panel spec. The first run failed because the debug binary still embedded the UIR5 bundle; rebuilt UI and binary first. |
| `verify.sh` | UIR6 row `ok`; 2 failing (status-bar UIR8, developer settings spec UIR13) |

Screenshots at 1280×800 on `/ui/developer/settings`:
`UIR6-sub-panel-default.png` (240px), `UIR6-sub-panel-dragged.png` (320px
after the drag), `UIR6-sub-panel-collapsed.png` (32px rail).

## Open items

- Observability and Settings still contribute static specs; UIR8 moves
  Observability to page-header tabs and keeps Settings on the panel.
- The status bar still exists; UIR8 deletes it.
