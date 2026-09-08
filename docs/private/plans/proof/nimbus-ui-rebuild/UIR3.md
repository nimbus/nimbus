# UIR3 Primitives

Date: 2026-09-08
Worktree: `~/src/github.com/nimbus/nimbus-worktrees/nimbus-ui-rebuild`, branch `codex/nimbus-ui-rebuild`

## What changed (`packages/nimbus-ui`)

| Item | Result |
| --- | --- |
| Registry set (`src/components/ui/`) | Added by the shadcn CLI (`base-nova`): checkbox, command, dialog, dropdown-menu, input, input-group, kbd, popover, scroll-area, select, separator, sheet, skeleton, sonner, tabs, textarea, tooltip, plus the `chart.tsx` seam. Registry hover classes that resolve to the amber `bg-accent` are overridden with `bg-bg-hover` in `select.tsx`. `button.tsx` gains `data-variant` and `data-size`. `input-group.tsx` carries two file-level a11y suppressions for registry markup. command, dropdown-menu, popover, scroll-area, separator, tabs, input and textarea have no app importer yet; the shell phases take them. |
| `state-dot.tsx` | Rewritten on the semantic tokens. Five glyphs (solid, outline, pulse, half, none); the outline glyph draws with an inset box-shadow so the token survives `var()` in the DOM. |
| `pill.tsx` | New. `Pill` (neutral, accent, semantic tints) and `StatePill` (dot + label, `data-state` and `data-glyph`). Replaces `state-chip.tsx` and `category-chip.tsx`. |
| `data-table.tsx` | New. TanStack Table v9 (`useTable`, `tableFeatures`, `createColumnHelper`) with sorting, resizing, row selection, and TanStack Virtual rows. ARIA `table`/`row`/`columnheader`/`cell` on divs (two file-level suppressions explain why the rows are not native `<tr>`); `sortDescFirst: false`; the resize grip is a pointer-only `aria-hidden` affordance. |
| `table-cells.tsx` | Renamed from the old `data-table.tsx`. `Th`, `Td`, `Tr` primitives keep the hand-rolled tables until their page phases move them to `DataTable`. |
| `empty-state.tsx` | Rewritten: sans title, `text-text-3` body, optional action slot and mascot slot (UIR4 fills the mascot). |
| `copy-button.tsx` | New. Icon button with a two-second checked state and a toast on clipboard failure. `copy-chip.tsx` builds on it. |
| `load-failed.tsx`, `skeleton.tsx` | New. Route-level failure panel and a skeleton wrapper over the registry primitive. `loading-state.tsx` keeps `LoadingState` and `SkeletonRows`. |
| `confirm-dialog.tsx`, `slideover.tsx` | Rebuilt on the registry Dialog and Sheet. `hooks/use-modal-focus.ts` deleted: Base UI owns focus and inertness. |
| `segmented-control.tsx`, `select.tsx` | Rebuilt on Base UI RadioGroup and the registry Select. |
| `components/kbd.tsx`, `components/checkbox.tsx` | Deleted in favour of `ui/kbd.tsx` and `ui/checkbox.tsx`. |
| `time.tsx`, `lib/format.ts` | Tooltip moved to the registry Tooltip. |
| Call sites | 30 route and shell files moved from the chips, the old checkbox, kbd, and modal-focus hook to the new primitives. `storage/documents-table.tsx` treats `[role='checkbox']` as interactive so a Base UI checkbox click does not open the row. |
| `ui/sonner.tsx` | The registry Toaster imports `next-themes`; the Nimbus copy reads the theme from `useUiStore` instead, so `next-themes` is not a dependency. |
| Stories | `kbd.stories.tsx` and `state-chip.stories.tsx` deleted with their components. New stories for Pill, DataTable, StateDot glyphs, EmptyState and Mascot land in UIR4 with the Storybook trim. |

## Spec notes

- Base UI `Dialog.Popup` does not set `aria-modal`. Modality is inert
  siblings (`aria-hidden="true"` outside the popup), so the ConfirmDialog
  and Slideover specs assert on an outside element. Initial focus resolves
  after a microtask and a frame, so focus assertions use `waitFor`.
- `userEvent.setup()` replaces `navigator.clipboard`. The CopyButton spec
  installs its `writeText` mock after setup, and the two-second reset test
  drives a synthetic click under fake timers.
- TanStack Virtual reads `offsetHeight`, which happy-dom reports as 0. The
  DataTable virtual test mocks `HTMLElement.prototype.offsetHeight`.
- Base UI RadioGroup handles arrow keys only (no Home/End); the
  SegmentedControl and ViewSwitcher specs assert arrow wrap.

## Verification

| Check | Result |
| --- | --- |
| `npm run lint` (biome, 260 files) | clean |
| `npm run typecheck` (nimbus-ui) and root `npm run typecheck` | clean |
| `npm run test` | 104 files, 814 passed (827 at UIR2; chip, kbd, and modal-focus specs removed; primitive specs plus new specs for JsonEditorForm, LoadingCell, the service loader errors, and the table cells added) |
| `npm run build` | ok (997ms) |
| `make build` (worktree root) | ok, 1m 13s |
| `npm run test:e2e:smoke` | 1/1 passed |
| `verify.sh` | UIR3 row `ok` (chart seam is the only `@tanstack/charts` importer); 10 failing overall, all UIR4+ |
| `grep -rln "@tanstack/charts" src` | `src/components/ui/chart.tsx` only |
| Importers of deleted files | none (`state-chip`, `category-chip`, `components/kbd`, `components/checkbox`, `use-modal-focus`); no `next-themes` import |
| Nimbus-owned component specs | every file in `src/components/*.tsx` has a sibling spec (four added in this phase: `json-editor-form`, `loading-cell`, `service-loader-errors`, `table-cells`) |

Screenshots: `UIR3-storage-dark.png`, `UIR3-storage-light.png` (1440×900,
`/ui/developer/storage`, empty tenant). The empty state now renders the sans
title and the `text-text-3` body from the rebuilt `EmptyState`. The light
shot includes the live update toast from the registry Sonner.
