# UIR7 Command palette, keyboard contract, overlays

Date: 2026-09-08
Worktree: `~/src/github.com/nimbus/nimbus-worktrees/nimbus-ui-rebuild`, branch `codex/nimbus-ui-rebuild-phase2` (stacked on #331)

## What changed

| Item | Result |
| --- | --- |
| `src/shell/command-palette.tsx` | Rewritten on the registry `command` primitive: `CommandDialog` (Base UI dialog, `sm:max-w-[640px]`, top at 12vh, accessible name "Command palette") around `Command loop` with `CommandInput`, `CommandList`, `CommandEmpty`, `CommandGroup`, `CommandItem`. The navigate / run / filter mode chips are gone; the palette opens on one list with fixed groups: **Recent** (empty query only, from `nimbus-ui:commands:recent`, limit 5), **Routes** (the current console's nav entries first, then the other console's, each with a `Developer` / `Operator` detail), **Tenants** (from the shared `useTenantList`, active tenant checked; a pick in the developer console sets the active tenant in place, a pick from the operator console sets it and navigates to `/developer`; a fetch error renders one disabled "Tenants unavailable" row), **Actions** (switch console, open system tenant lens in the developer console with the `⌘ \` keys shown, refresh current view through `router.invalidate()`, switch to dark/light theme). Typing adds the resource groups Tables, Functions, Services, Machines, HTTP routes from the existing queries (tables skipped without a tenant, 200 rows each); the tenant resource query is gone because tenants are a fixed group. Selected row: `bg-bg-hover` with a 2px accent inset. Focus returns to the opener through the existing `setPaletteOpen(false, opener)` path. |
| Palette footer | `command-palette-footer` writes the keyboard contract: `↑ ↓ move`, `⏎ open`, `⎋ close` on the left; `⌘ K palette`, `⌘ \ tenant lens` (developer console only), `/ filter page` on the right, all as registry `Kbd` / `KbdGroup`. |
| Filter mode | Dropped. The old `nimbus:filter` CustomEvent had no listener anywhere in `src`, so the mode never filtered a page; `/` still focuses the page filter through `KeyboardContract` and the footer says so. |
| `src/shell/keyboard-contract.tsx` | Unchanged. `⌘K`, `⌘\`, `/`, Escape and the input guard keep their behaviour; the spec passes as it was. |
| Theme toggle label | Already visible since UIR5: `ThemeToggle` in `sidebar/footer.tsx` reads "Dark theme" / "Light theme" and `sidebar.spec.tsx` asserts it. No change. |
| `src/components/empty-state.tsx` | New `mascot?: MascotState` prop renders the solid mascot at 56px (decorative) in the icon's place. For the shell's own screens only; a list's empty state keeps its icon. Story `WithMascot` added. |
| `src/shell/disconnected-overlay.tsx` | Same probe logic; the banner is now the EmptyState idiom laid out as a row: solid error mascot at 32px, "Reconnecting" title, "Stale data shown, mutations disabled." body, `bg-bg-raised` card with `shadow-overlay`, `w-[min(420px,calc(100vw-2rem))]`, centred at the top of `<main>`, `pointer-events-none` so the stale page stays readable. The `StateDot` is gone from it. |
| `src/shell/error-boundary.tsx` | The 480px / 90vw card keeps its width reasoning; inside it `EmptyState mascot="error"` ("The console shell failed to render" / "Moving to another view clears this screen."), the `CopyChip` with the message, and two registry `Button variant="outline" size="sm"` (Retry, Reload console). `ACTION_CLASS` removed. |
| `src/components/route-error.tsx` | `EmptyState mascot="error"` and the same registry buttons; testids unchanged. |
| `tests/e2e/smoke.spec.ts` | Step 8 asserts `palette-group-routes`, `palette-group-tenants`, `palette-group-actions`, the footer text "tenant lens", and that Escape hides the palette (the `palette-mode-*` chip assertion is gone). |
| `DESIGN.md` | "Command Palette (⌘K)" section rewritten to the fixed groups, the resource groups, recents, the footer contract, and the registry implementation. The "Bottom Status Bar" section stays until UIR8 deletes the bar. |

## Spec notes

- `command-palette.spec.tsx` rewritten (21 tests, 10 before): no
  subscriptions and no palette DOM while closed; no resource queries until
  typing; modal dialog named "Command palette" at 640px; the three groups
  with the current console's routes first and the active tenant checked;
  ArrowDown ×2, ArrowUp, Enter opens `/developer/compute` and closes;
  Escape on the dialog closes; footer hints and their `kbd` glyphs; the
  operator console shows no lens hint or action and offers "Switch to
  Developer console"; table by name, resource by id, exactly one selected
  row; pick stores the recent and recents list first and selected; tables
  skipped without a tenant; tenant pick in the developer console sets the
  tenant and closes without navigating; tenant pick from the operator
  console navigates to `/developer`; the input has no `outline-none` and
  only the `--accent` variable (the console's unlayered `:focus-visible`
  rule paints the ring, the registry `outline-hidden` does not fight it);
  refresh calls `router.invalidate()` not `location.reload()`; switch
  console navigates to `/operator`; the theme action flips the mode; a
  tenant fetch error renders the unavailable row before and after typing.
- `keyboard-contract.spec.tsx` passes unchanged (acceptance criterion).
- `empty-state.spec.tsx` +1: the mascot renders `data-mascot="solid"`,
  `data-state="error"`, `aria-hidden`, width 56.
- `disconnected-overlay.spec.tsx`: asserts the body copy and the error
  mascot in the banner. `error-boundary.spec.tsx`: asserts the EmptyState
  title and mascot inside the card. `route-error.spec.tsx` passes unchanged.
- Base UI `Dialog` inside cmdk works in happy-dom: keyboard navigation,
  `data-selected`, Escape and `aria-modal` all assert without shims.

## Verification

| Check | Result |
| --- | --- |
| `npm run lint` (biome, 268 files) | clean |
| `npm run typecheck` (nimbus-ui) and root `npm run typecheck` | clean |
| `npm run test` | 105 files, 831 passed (819 at UIR6; palette 10 → 21, empty-state +1) |
| `npm run build` | ok |
| `npm run storybook:build` | ok |
| `cargo fmt --all --check` | clean |
| `make build` (worktree root) | ok, 69s incremental |
| `npm run test:e2e` | 12 passed, 1 skipped (the sheet spec on chromium); smoke 2/2 with the new step 8 |
| `verify.sh` | 2 failing (status-bar UIR8, developer settings spec UIR13), same as UIR6 |

Screenshots at 1280×800 on `/ui/developer/`, taken with a throwaway
Playwright spec against the rebuilt binary (spec deleted after):
`UIR7-palette-dark.png` and `UIR7-palette-light.png` (open on Routes),
`UIR7-palette-action-light.png` ("theme" typed, one action left; Enter
flipped the theme, which is how the dark shot was taken),
`UIR7-palette-search-dark.png` ("def" typed: Routes, Tenants with
`default` checked, HTTP routes), `UIR7-error-boundary.png` and
`UIR7-error-boundary-360.png` (a `matchMedia` override that throws for the
sidebar's tier queries, then a resize across 767px, so the shell chrome
crashed under the boundary while the root stayed up),
`UIR7-disconnected.png` (after `POST /api/system/shutdown`; the status
bar's own "Reconnecting" is still there until UIR8).

## Open items

- The status bar duplicates the footer's `⌘\` / `⌘K` / `/` hints and the
  connection state; UIR8 deletes it.
- The tenants group needs a fetch on every open (the shared hook fetches
  on mount). It is one small request behind the same-origin session and
  the sidebar already makes it; a cache is not worth a second seam yet.
