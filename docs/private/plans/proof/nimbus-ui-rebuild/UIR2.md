# UIR2 Tokens, palette, type

Date: 2026-09-08
Worktree: `~/src/github.com/nimbus/nimbus-worktrees/nimbus-ui-rebuild`, branch `codex/nimbus-ui-rebuild`

## What changed (`packages/nimbus-ui`)

| Item | Result |
| --- | --- |
| `src/styles/tokens.css` | New. Six `@font-face` (Geist, Geist Mono at 400/500/600), the Starport role tokens (dark on `:root`, light under `[data-theme="light"]`), `@theme inline` colour bridge (role keys plus the shadcn names), and a non-inline `@theme` for fonts, the type scale, the radius scale, `--ease-standard`, `--shadow-overlay`, `--statusbar-height`. |
| `src/styles/globals.css` | Rewritten. Imports tailwind, tokens, `shadcn/tailwind.css`, `tw-animate-css`; base rules (html/body, mono at `0.93em`, inline code chip, `.tabular`, reduced motion, `.nimbus-checkbox`, `::selection`); one unlayered `:focus-visible` ring (2px accent at 40%, 4px halo at 20%); `.link-inline`; shiki overrides. `.label` deleted. |
| `public/fonts/` | Geist and Geist Mono woff2 (Regular, Medium, SemiBold) plus the OFL licence, copied from Starport. Vite rewrites the URLs to `/ui/fonts/` (checked in `dist/assets/index-*.css`). |
| `@fontsource/jetbrains-mono` | Uninstalled. |
| Utility vocabulary | 105 files rewritten by script from `bg-surface`/`text-muted`/`border-app`/`text-brand`/… to `bg-bg-panel`/`text-text-3`/`border-border-2`/`text-accent`/…; `--nimbus-*` reads to the role tokens; `text-lg` to `text-md`. |
| Labels | Every `uppercase` + `tracking-*` + `font-mono` label is now `text-xs font-medium` sentence case (the sidebar keeps its group heading). `-graph-view.tsx` SVG headers use `fontSize=12 fontWeight=500`. |
| Bare `rounded` | 121 call sites moved to `rounded-xs` (4px). With `--radius-*: initial` the bare utility compiles to nothing. |
| Palette removal | `ui-store.ts` loses `Palette`, `PALETTES`, `setPalette`, the storage key and hydration. `theme-controller.tsx` stamps `data-theme` only. `appearance-menu.tsx` and `appearance-section.tsx` offer Mode only. The command-palette recent key is `nimbus-ui:commands:recent`. |
| `state-chip.tsx` | Running → `--info`; Starting family → `--warning` half; Draining, Queued, Stopped, Stale, Unknown → `--text-3`; Failed → `--error`. |
| Input focus | Seven text fields moved from `focus-visible:border-border-3` to `focus-visible:border-accent` so the edge and the ring agree. |
| `.storybook/preview.tsx` | Backgrounds `#0a0b0c` / `#ffffff`. |
| `DESIGN.md` | §Product Palette (one axis, token tables, six rules), §Two-Tier Bridge, §Usage Guidelines, §Documentation Site body mapping, §Typography (Geist, the seven-step scale, sentence-case labels), §Spacing And Shape (radius scale), §Badges (state table), §Diff Viewer, §Implementation Rules. |

## Light-mode deviations from Starport

Four light tokens differ from the exemplar so the contrast gates pass on
every ground (`--bg-hover` `#ececee` is the hardest):

| Token | Starport | Nimbus | Reason |
| --- | --- | --- | --- |
| `--text-3` | `#71717a` | `#66666e` | 4.5:1 on `--bg-hover` |
| `--success` | `#16a34a` | `#15753a` | 4.5:1 on every ground |
| `--warning` | `#ea580c` | `#b53b0a` | 4.5:1 on every ground |
| `--accent-link` | `#b45309` | `#975c06` | 4.5:1 on `--bg-hover` |

## Gates rewritten

- `contrast.spec.ts`: text tokens ≥ 4.5:1 on all four grounds in both
  modes; `--accent` ≥ 3:1 on all four grounds; `--accent-ink` on
  `--accent` and `--error-ink` on `--error` ≥ 4.5:1; `--text-4` below
  `--text-3`; six distinct literals for states + accent + text-3; grounds
  monotonic; the unlayered `:focus-visible` rule names `--accent` with the
  2px and 4px rings; no `--tw-ring-color`, no `--focus`; no Nimbus-owned
  component binds a ring/outline colour under focus, and a focus border is
  allowed on `border-accent` only.
- `token-utilities.spec.ts`: retired keys and tail-only keys fail; no
  `--nimbus-*`; bare `rounded` and `3xl`+ steps fail; `uppercase` and
  `tracking-*` fail outside `shell/sidebar/`.
- `appearance-section.spec.tsx`: exactly three mode radios; no palette
  control; no `data-palette`; the theme key is the only localStorage key.

## Verification

| Check | Result |
| --- | --- |
| `npm run lint` (biome, 238 files) | clean |
| `npm run typecheck` (nimbus-ui) | clean |
| `npm run test` | 98 files, 827 passed (852 at UIR1; 29 palette-era tests removed, 24 added) |
| `npm run build` | ok; dist CSS has `/ui/fonts/*.woff2`, `.rounded-xs`…`.rounded-full`, no unresolved `--radius`/`--text` vars |
| `make build` (worktree root) | ok, 1m 23s |
| `npm run test:e2e:smoke` | 1/1 passed |
| `verify.sh` | UIR2 rows all `ok` (jetbrains removed, Geist self-hosted, no `nimbus-ui:palette`, no uppercase outside sidebar); 11 failing overall, all UIR3+ |
| Runtime probe (Playwright) | dark: `data-theme=dark`, body `Geist, system-ui, "Segoe UI", sans-serif`, canvas `rgb(10, 11, 12)`, Geist and Geist Mono loaded, no `data-palette`; light: canvas `rgb(255, 255, 255)`, same fonts |

Screenshots: `UIR2-storage-dark.png`, `UIR2-storage-light.png` (1440×900,
`/ui/developer/storage`, empty tenant). The empty-state heading still renders
in mono; that component is replaced in UIR3.
