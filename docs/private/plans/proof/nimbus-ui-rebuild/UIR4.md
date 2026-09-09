# UIR4 Mascot and Storybook

Date: 2026-09-08
Worktree: `~/src/github.com/nimbus/nimbus-worktrees/nimbus-ui-rebuild`, branch `codex/nimbus-ui-rebuild`

## What changed

| Item | Result |
| --- | --- |
| `src/components/mascot.tsx` | New. Concept A ("Dot") as a React component: three circles and a rounded base unioned in a `0 0 120 92` box, eyes at (48,52) and (72,52), mouth at y 64. Props `size` (width; height follows 120:92), `state` (idle, working, error, empty, celebrate), `variant` (`outline` = `currentColor` stroke on `--bg-panel`; `solid` = `--accent` body, `--accent-ink` face), `label`, `decorative`. Every state carries one accessory (`thinking`, `drop`, `sleep`, `sparks`). Below 40px the outline goes from 5 to 7 units and the face from 4 to 5.5 so the mark keeps the weight of the semibold wordmark beside it. |
| `src/hooks/use-reduced-motion.ts` | New. `useSyncExternalStore` over `prefers-reduced-motion: reduce`. The mascot reads it and leaves the blink class and `data-blink` out of the DOM when motion is off, instead of relying on the `globals.css` backstop alone. |
| `src/styles/tokens.css` | `--animate-blink` and `@keyframes blink` next to shimmer in the `@theme inline` block: one blink every six seconds, `scaleY` about the eye line. |
| `src/shell/logo-mark.tsx` | Deleted. `top-nav.tsx` renders `<Mascot size={30} />`; the `aria-label` stays `Nimbus`, so `top-nav.spec.tsx` is unchanged. |
| `src/shell/theme-controller.tsx` | The favicon swap is gone. The favicon is one fixed-colour drawing, so the theme no longer touches the `<link rel="icon">`. |
| `public/` | `nimbus-logo.svg` is the wisp-free body with the idle face and keeps the `--logo-stroke` / `--logo-fill` contract. `favicon.svg` is concept C ("Solid"): `#f0b23e` body, `#1a1204` face, transparent, square viewBox. `favicon.ico` carries 16, 32 and 48; the 16px entry uses larger eyes and a heavier mouth so the face survives one pixel per unit. `icon-512.png` is the solid face on a `#0a0b0c` rounded tile, linked as `apple-touch-icon` in `index.html`. `favicon-night.svg`, `favicon-warm.svg` and `nimbus-mark.svg` deleted. |
| Sign-in page (`crates/nimbus-assets/embedded/ui-auth/auth.html`) | The wisp mark is replaced with the solid mascot sticker at 56px, fixed colours in both schemes. Added to UIR4 because the sign-in page is the first console surface an operator sees and no other task owns it. The page's own blue palette and wordmark are untouched; see Open items. |
| Sign-in page fonts | Pre-existing break from UIR1 found by the server test: the page still asked the bundle for `jetbrains-mono-latin-*` files that stopped shipping when the console moved to Geist. The `@font-face` blocks now point at `/ui/fonts/GeistMono-Regular.woff2` and `-Medium.woff2` (static names, no placeholder), the nine mono `font-family` stacks name Geist Mono, and `find_embedded_font` plus the two `{{ JETBRAINS_MONO_* }}` substitutions are removed from `crates/nimbus-server/src/http/ui.rs`. `tests/local_ui.rs` asserts the new files and the 120×92 mascot. |
| Stories | New `mascot.stories.tsx` (five states, solid, outline and solid galleries at 16/24/32/48, nav lockup), `pill.stories.tsx` (tones, StatePill states, CategoryPill), `data-table.stories.tsx` (basic, empty, controlled sorting, selection, row activate, virtualized past `VIRTUAL_THRESHOLD`, footer). `state-dot.stories.tsx` now renders every `statePalette` token grouped by glyph; `empty-state.stories.tsx` gains icon and snippet stories. Existing Nimbus-owned stories kept; `sub-drawer.stories.tsx` stays until UIR8. No story covers a registry primitive. `addon-a11y` kept. |
| `DESIGN.md` | Brand Palette gains a Mascot subsection (variants, states, motion, sizes, static exports, accent rule). The marketing variants under `docs/brand/logo/` are named as marketing-tier and unchanged. The docs-site paragraph no longer claims the console swaps warm/night favicons. |

## Spec notes

- `mascot.spec.tsx` (10 tests) renders all five states, asserts one
  accessory per state, the blink class and `data-blink` on open dot eyes
  when motion is allowed, no blink markup under
  `prefers-reduced-motion: reduce` for every state, no blink on closed or
  crossed eyes, the solid fills, the outline stroke, the small-size weights,
  and the decorative form. `matchMedia` is stubbed per test because the
  happy-dom setup stub always reports `matches: false`.
- `use-reduced-motion.spec.ts` (2 tests) covers the mount read and the
  change event.
- biome's `noSvgWithoutTitle` cannot see a spread `aria-label`, so the
  mascot sets `role`, `aria-label` and `aria-hidden` inline.
- `token-utilities.spec.ts` scans comments too: the body comment says
  "soft base" rather than "rounded base", and the StateDot gallery heading
  is plain mono, not tracked capitals.

## Verification

| Check | Result |
| --- | --- |
| `npm run lint` (biome, 266 files) | clean |
| `npm run typecheck` (nimbus-ui) and root `npm run typecheck` | clean |
| `npm run test` | 106 files, 826 passed (814 at UIR3; mascot 10 and reduced-motion 2 added) |
| `npm run build` | ok (937ms) |
| `npm run storybook:build` | ok; stories are Nimbus-owned only |
| `cargo test -p nimbus-server --lib local_ui` | 14 passed (the auth page test failed before the font fix on the JetBrains Mono path assertion, then on nothing) |
| `cargo fmt --all --check` | clean |
| `make build` (worktree root) | ok, 64s |
| `npm run test:e2e:smoke` | 1/1 passed |
| `verify.sh` | both UIR4 rows `ok`; 8 failing overall, all UIR5+ |
| Favicon at 32px | `UIR4-favicon-32-16.png` (32px left, 16px right, point-scaled): the solid face reads at both |

Screenshots: `UIR4-topnav-dark.png`, `UIR4-topnav-light.png` (the header
at 1280 wide with the 30px outline mark beside the wordmark),
`UIR4-shell-dark.png` (full shell at 1280×720), `UIR4-auth-sticker.png`
(the sign-in card with the solid sticker), `UIR4-favicon-32-16.png`.

## Open items

- The sign-in page keeps its own blue oklch palette, blue wordmark and blue
  button. It is the only console surface outside the neutral + amber
  palette. No task in the plan owns it; the shell task (UIR8) or a new task
  should take it.
- The marketing logo variants under `docs/brand/logo/` and the docs site
  favicons still carry the wisp cloud. They are marketing tier and out of
  this plan's scope.
