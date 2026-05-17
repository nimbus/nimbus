# Plan: Brand & Design System — Two-Tier Palette + Logo Rollout

Canonical execution plan for applying the 9-variant brand palette
(Image #12, #13) across `nimbus/nimbus` and `nimbus/desktop` without
breaking the existing Industrial Precision operator console rules.

This plan is the **control plane** for the brand work. It survives
compaction: the Phase Status Ledger and Execution Log are the source of
truth for what is done, what is in flight, and what remains.

---

## Status

- **Status:** `in_progress`
- **Primary owner:** this plan
- **Parent plan:** none (brand identity is a peer plan, not a sub-plan)
- **Repos affected:** `nimbus/nimbus`, `nimbus/desktop`
- **Started:** 2026-05-16

## Mission

Apply the new 9-variant brand palette spec to both repos. Produce a
canonical, verifiable logo SVG and its variants; wire favicon, app icon,
tray icon, sidebar mark, and a brand-palette section in `DESIGN.md`. Do
this without contaminating the operator console's Industrial Precision
rules (one teal accent, cool 240° neutrals, no gradients).

## Resolution: Two-Tier System

A direct merge of the new palette into `DESIGN.md` is incompatible: the
brand spec introduces gradients, blue/purple dominance, and pastels that
the operator console design system explicitly forbids. The conflict is
real and intentional once you read both specs side-by-side.

The fix is to split the design system into two tiers:

### Brand Tier — full 9-variant palette, gradients permitted
Used on:
- Logo mark and variants
- README hero images
- Marketing site
- App icon / favicon
- Desktop setup card (`cli-not-found.html`)
- Print and other external touchpoints

### Product Tier — Industrial Precision, single teal accent, OKLCH neutrals, no gradients
Used on:
- `packages/nimbus-ui/` operator console
- Desktop native chrome (Electron `BrowserWindow`, menus, tray context)
- Every in-app UI surface

### Bridge
The brand "Teal" variant `#67E8F9 → #68B6DA` (gradient form) maps to the
product palette teal accent (`oklch(...)` solid form). Ink `#0F172A` is
shared across tiers for primary text. No other colors cross tiers.

## Control Plan Rules

Source of truth, in order:

1. This plan's **Phase Status Ledger** and **Execution Log**
2. The canonical SVG: `packages/nimbus-ui/public/nimbus-logo.svg`
3. `DESIGN.md` (product tier) and the brand-palette section added by L3
4. The 9 variant SVGs under `docs/brand/logo/`

When this plan disagrees with another document, this plan wins until the
two are reconciled here.

## Phase Status Ledger

| Lane | Description                              | Status      | Owner deliverable                                                   |
|------|------------------------------------------|-------------|---------------------------------------------------------------------|
| L0   | Canonical logo SVG (safe-area form)      | `done`      | `packages/nimbus-ui/public/nimbus-logo.svg`                         |
| L1   | Tight mark variant for icon use          | `done`      | `packages/nimbus-ui/public/nimbus-mark.svg`                         |
| L2   | 9 brand-palette variants                 | `done`      | `docs/brand/logo/nimbus-*.svg` regenerated from L0 by L9             |
| L3   | DESIGN.md brand-palette section          | `done`      | new section in `DESIGN.md`                                          |
| L4   | Favicon assets + HTML wiring             | `pending`   | `packages/nimbus-ui/public/favicon.{svg,ico}` + `index.html` link   |
| L5   | Sidebar logo mark                        | `pending`   | `packages/nimbus-ui/src/shell/sidebar.tsx`                          |
| L6   | Desktop app icon                         | `pending`   | `desktop/buildResources/icon.{icns,ico,png}` + `electron-builder.yml` |
| L7   | Tray icon refresh                        | `pending`   | `desktop/buildResources/trayTemplate.png`                           |
| L8   | `cli-not-found.html` token migration     | `pending`   | `desktop/buildResources/setup/cli-not-found.html`                   |
| L9   | `gen-variants.sh` refresh + run          | `done`      | `docs/brand/gen-variants.sh` regenerates L2 from L0                 |

Status values: `pending`, `in_progress`, `partial`, `done`, `blocked`.

---

## Phase Detail

### L0 — Canonical logo SVG (DONE)

**Outcome.** `packages/nimbus-ui/public/nimbus-logo.svg` exists, traced from
the full cloud (not a clipped source crop), with symmetric 40-unit safe area
on all four sides. Two-layer composition: silhouette (`var(--logo-fill, transparent)`)
behind ink outline + curls + eye spiral (`var(--logo-stroke, currentColor)`).
Consumers parameterize color via CSS variables.

- viewBox: `0 0 382 261`
- content bbox: `(40, 40) → (342, 221)` — `302 × 181` cloud
- transform: `translate(0,261) scale(0.1,-0.1)` (potrace convention,
  baked into a single composition `<g>`)

**Decision: 40-unit safe area.** 10.5% of viewBox width, 15.3% of viewBox
height. On the heavier end of typical mark padding (vs Lucide/Heroicons
at ~8%) but appropriate for a brand mark used in headers and marketing.
Tight-mark derivative for icon-budget contexts lives in L1.

**Decision: vertical padding > horizontal padding.** Intentional —
cloud aspect (302:181 ≈ 1.67) differs from viewBox aspect (382:261 ≈ 1.46).
Equalizing absolute padding (40u all sides) is the right choice; equalizing
*proportional* padding would force the viewBox to match content aspect and
remove the safe area's purpose.

**Verification command:**

```bash
rsvg-convert -w 764 -h 522 -b "#ffffff" \
  packages/nimbus-ui/public/nimbus-logo.svg -o /tmp/logo-check.png
python3 -c "
from PIL import Image
img = Image.open('/tmp/logo-check.png').convert('RGB')
w, h = img.size; xs, ys = [], []
for y in range(h):
    for x in range(w):
        if img.getpixel((x,y))[0] < 128:
            xs.append(x); ys.append(y)
print(f'padding (viewBox units): L={min(xs)/2} R={(w-1-max(xs))/2} T={min(ys)/2} B={(h-1-max(ys))/2}')
"
```

**Expected output:** `L=40 R≈40 T=40 B=40` (within rounding).

**Last verified:** 2026-05-16, output was `L=40.0 R=40.5 T=40.0 B=40.0`.

### L1 — Tight mark variant (DONE)

**Outcome.** `packages/nimbus-ui/public/nimbus-mark.svg` exists with viewBox
`0 0 322 201` and transform `translate(-30, 231) scale(0.1, -0.1)`. Same
two-layer composition + CSS variable contract as L0. Pixel-scan verified
2026-05-16: `L=10.0 R=10.5 T=10.0 B=10.0` — same 0.5-unit sub-pixel asymmetry
as L0 from rsvg-convert 2x rasterization.

#### Original specification (reference)

**Deliverable.** `packages/nimbus-ui/public/nimbus-mark.svg`: same path data
as L0, tighter viewBox for favicon, app icon, tray, and 16/24/32-px
sidebar use where the pixel budget is constrained.

**Specification.**
- viewBox: `0 0 322 201` (10-unit padding all sides on 302×181 content)
- Same two-layer composition as L0
- Same `--logo-fill` / `--logo-stroke` CSS variable contract

**Compute transform.** Content currently positioned via
`translate(0,261) scale(0.1,-0.1)` with content bbox starting at path-coord
`(841, 328)` → screen `(84.1, 228.1)`. Want content top-left at viewBox
`(10, 10)`. Shift: `(10 - 84.1, 10 - (261 - 224.7))` after re-deriving for
new viewBox height. Easier path: shift transform `(−30, +30)` so the
40-unit-padded layout collapses to a 10-unit-padded layout, then drop the
viewBox to `322 × 201`.

Concretely: `transform="translate(-30, 231) scale(0.1, -0.1)"`,
`viewBox="0 0 322 201"`.

**Verification.** Same pixel-bbox harness as L0, expecting
`L≈10 R≈10 T≈10 B≈10`.

### L2 — Brand palette variants (PARTIAL → must be redone from L0)

**Status.** 11 variant files exist at `docs/brand/logo/nimbus-*.svg` but they
were generated from an OLD clipped trace (the source PNG was cropped before
potrace, chopping ~17 rows off the cloud's bottom). They must be regenerated
from L0's path data. **Defer to L9** which owns the regeneration script.

**Variant table** (hex values from the palette spec, Image #13):

| Variant       | `--logo-stroke` | `--logo-fill` | Background    |
|---------------|-----------------|---------------|---------------|
| warm          | `#0F172A`       | `#FFE7B3`     | `#FFFAF2`     |
| cool-blue     | `#3B82F6`       | `#FFFFFF`     | `#F8FAFC`     |
| night-blue    | `#60A5FA`       | `#1E293B`     | `#0B1220`     |
| monochrome    | `#111827`       | `#FFFFFF`     | `#FFFFFF`     |
| reverse-mono  | `#FFFFFF`       | `#111827`     | `#111827`     |
| sunset-red    | `#DC2626`       | `#FFFFFF`     | `#FEF2F2`     |
| soft-purple   | `#9333EA`       | `#FFFFFF`     | `#FAF5FF`     |
| golden-hour   | `#D97706`       | `#FFFFFF`     | `#FFFBEB`     |
| slate         | `#475569`       | `#FFFFFF`     | `#F1F5F9`     |

Spec hex values must be verified against Image #13 during execution —
small differences (`#FFE7B3` vs `#FFE7AE`) matter for brand consistency.

**Verification.**

```bash
# All variants share the canonical path data (sanity check):
for f in docs/brand/logo/nimbus-*.svg; do
  grep -c "M1447 2194" "$f"  # canonical L0 path start; expect 2 per file
done
```

Plus a visual grid render (`docs/brand/all-variants.png`) for side-by-side
comparison against Image #12.

### L3 — DESIGN.md brand-palette section (DONE)

**Outcome.** Renamed the existing `### Palette` section to `### Product
Palette` and added a new `### Brand Palette` section after it. Documented
the two-tier system, the two cross-tier values (teal gradient/solid,
ink), the 9-variant table with hex values, usage guidelines per variant,
and a pointer back to this plan.

#### Original specification (reference)

**Deliverable.** New section after "Product Palette" in `DESIGN.md`:

```
## Brand Palette

The brand palette is distinct from the operator console palette. Use it
for the logo, README hero images, the marketing site, app/favicon
backdrops, the desktop setup card, and any external touchpoint.

NEVER use brand-palette colors inside the operator console. Inside the
console, see "Product Palette" above.

### Two-Tier Bridge
- Brand "Teal" `#67E8F9 → #68B6DA` (gradient) = Product "teal accent"
  (`oklch(...)` solid). They are the same conceptual color in different
  forms.
- Ink `#0F172A` is shared across tiers for primary text.
- No other colors cross tiers.

### Variants
(table from L2)

### Usage Guidelines
- Warm or Golden Hour: marketing, brand-friendly touchpoints
- Cool Blue: product UI light mode (matches operator console light theme)
- Night Blue: product UI dark mode (matches operator console dark theme)
- Monochrome or Reverse Mono: minimal, enterprise, print
```

**Verification.** Render the markdown; `DESIGN.md` continues to validate
against the operator console rules (a passing read-through reveals no
gradient/purple/blue-dominance leaks into the product tier section).

### L4 — Favicon assets (PENDING)

**Deliverables.**
- `packages/nimbus-ui/public/favicon.svg` — cool-blue variant scaled to the
  32×32 grid; **use the L1 tight mark**, not the L0 safe-area canonical
- `packages/nimbus-ui/public/favicon.ico` — generated from PNG renders at
  16, 32, 48 px (use ImageMagick `convert` or `png2ico`)
- `packages/nimbus-ui/index.html` — add:

  ```html
  <link rel="icon" type="image/svg+xml" href="/favicon.svg" />
  <link rel="icon" type="image/x-icon" href="/favicon.ico" />
  ```

**Verification.**
- `curl http://localhost:5173/favicon.svg` returns SVG
- Browser tab shows favicon at multiple zoom levels
- At 16×16, the cloud is still recognizable (this is the test of L1's
  tight mark sizing decision)

### L5 — Sidebar logo mark (PENDING)

**Deliverable.** `packages/nimbus-ui/src/shell/sidebar.tsx`: replace the
text-only `<h1>nimbus</h1>` header with the L1 mark + wordmark.

**Sizing.** Check existing sidebar column width and header height. Likely
24–32 px height for the mark. Wordmark either inline-SVG or text in the
existing font (Inter or whatever DESIGN.md specifies).

**Verification.** Open sidebar in browser at default zoom, then at
`Cmd+−` (smaller) and `Cmd+=` (larger). Mark must stay crisp and
proportional at all sizes.

### L6 — Desktop app icon (PENDING)

**Deliverables.**
- `desktop/buildResources/icon.icns` — macOS, generated from 1024×1024
  source render of L0 (safe-area canonical, not L1 — macOS icon grid has
  its own ~15% safe area expectation)
- `desktop/buildResources/icon.ico` — Windows, multi-resolution
- `desktop/buildResources/icon.png` — Linux, 512×512
- `desktop/electron-builder.yml` — add:

  ```yaml
  mac:
    icon: buildResources/icon.icns
  win:
    icon: buildResources/icon.ico
  linux:
    icon: buildResources/icon.png
  ```

**Variant choice.** Use **warm** for the app icon per Image #13 usage
guidelines ("Use Warm or Golden Hour for marketing, brand and friendly
touchpoints"). The app icon is the most marketing-facing surface in the
product.

**Verification.**
- `npm run electron-builder -- --dir` produces a `.app` (macOS) or
  equivalent
- App icon visible in Finder, dock, Cmd+Tab switcher
- At 16×16 (Finder sidebar), icon is still recognizable

### L7 — Tray icon refresh (PENDING)

**Deliverable.** `desktop/buildResources/trayTemplate.png` — monochrome
cloud only (no wordmark). 16×16 base + 32×32 @2x.

**macOS template-image convention.** Image must be black + alpha only
(no color). macOS renders it white on dark menu bars, black on light menu
bars automatically. Use L1's tight mark, set `--logo-stroke: black` and
`--logo-fill: transparent`, then PNG-export.

**Verification.**
- Tray icon visible on both light and dark macOS wallpapers
- Stays sharp at 16×16 and 32×32

### L8 — `cli-not-found.html` token migration (PENDING)

**Deliverable.** Replace hardcoded colors in
`desktop/buildResources/setup/cli-not-found.html` with brand-tier tokens
or CSS variables that match the brand palette.

This page is brand-tier (it's external-feeling, the user's first contact
with the app when setup is incomplete). Use the warm variant as the
primary brand presentation.

**Verification.** Render page in Chrome, manually toggle prefers-color-scheme,
confirm both modes look correct.

### L9 — `gen-variants.sh` refresh + run (PENDING)

**Deliverable.** `docs/brand/gen-variants.sh`: shell script that reads the
canonical L0 SVG and emits 9 variants with palette substitutions.

**Idempotency requirement.** Running the script twice in a row produces
byte-identical files (no timestamps, no order-dependent output).

**Verification.**

```bash
docs/brand/gen-variants.sh
sha256sum docs/brand/logo/*.svg > /tmp/hashes-1.txt
docs/brand/gen-variants.sh
sha256sum docs/brand/logo/*.svg > /tmp/hashes-2.txt
diff /tmp/hashes-1.txt /tmp/hashes-2.txt  # expect empty diff
```

Plus the L2 verification (all variants share canonical path data).

---

## Execution Log

- **2026-05-16: L0 complete.** Iteration count: 4. Root cause of the
  bottom-clipping issue (resolved this session): source PNG
  `cloud-full.png` was a `384×195+0+120` crop of `monochrome-tile.png`
  which chopped 17 rows off the cloud bottom AND included a tile-border
  vertical line at `x=0` that contaminated the bbox calculation. Fix:
  re-crop with `x>=2` and tight bounds `(58..359, 151..331)` →
  `302 × 181` tight cloud → padded to `382 × 261`. Re-trace with
  potrace. Pixel-scan verifies symmetric 40-unit padding all sides.

- **2026-05-16: Decision logged on padding.** Kept 40-unit safe area on
  L0 canonical. Tight mark derivative deferred to L1 for icon-budget
  contexts.

- **2026-05-16: Path-bbox vs rendered-bbox lesson.** Bezier control points
  sit outside the rendered curve. Parsing path data and taking
  `min/max` over control points overestimates the visible bbox by
  ~5–15 units. The pixel-scan harness is the ground truth — use it in
  every L1–L9 verification.

- **2026-05-16: L1 complete.** `nimbus-mark.svg` written with the predicted
  transform (`translate(-30, 231) scale(0.1, -0.1)`, viewBox `322×201`).
  Pixel-scan verified `L=10.0 R=10.5 T=10.0 B=10.0` on first attempt — the
  shift-by-30 math from L0 held. Unblocks L4 (favicon), L5 (sidebar mark),
  L7 (tray icon).

- **2026-05-16: L3 complete.** `### Palette` in DESIGN.md renamed to
  `### Product Palette`; new `### Brand Palette` section added with the
  two-tier rules, bridge documentation, variant table, usage guidelines,
  and pointer back to this plan. Verified `grep "Palette" DESIGN.md`:
  only `Product Palette`, `Brand Palette`, and `Command Palette` remain
  (no orphan `### Palette` headings).

- **2026-05-16: L9 + L2 complete.** `docs/brand/gen-variants.sh` reads
  canonical L0 and emits 9 variant files by substituting the two
  `var(--logo-*)` references for hex and inserting a background rect.
  Idempotency verified: sha256sum diff between two consecutive runs is
  empty. All 9 variant SVGs contain canonical L0 path data (2 instances
  per file). Stale `nimbus-product-{light,dark}.svg` removed (predate
  brand spec, not in 9-variant table; pre-launch policy forbids
  compat shims). Visual grid rendered to verify cloud/swirl
  composition matches across variants.

---

## Risks

- **electron-builder rebuilds are slow.** Defer L6 verification to a
  single batch run at the end of the lane rather than per-asset.
- **Tray icon (L7) needs real macOS hardware** for verification. CI can
  build the bundle, but the visual test of template-image rendering on
  light vs dark menu bars is manual.
- **Sidebar logo (L5) may conflict with existing layout dimensions.**
  Check `sidebar.tsx` column widths and existing header height before
  resizing.
- **Hex values from Image #13 need careful re-verification.** Brand
  consistency demands the exact codes from the spec, not eyeballed
  approximations. Treat the L2 table as a draft until verified
  pixel-by-pixel against Image #13.

## Out of Scope

- Marketing site (separate repo, separate plan)
- Animated logo variants
- Light/dark split inside the product palette (separate plan if needed;
  the product palette already uses semantic tokens that handle both)
- Wordmark font / typeface decisions (the logo already includes its own
  wordmark; the system font choice is owned by `DESIGN.md` typography
  section)
