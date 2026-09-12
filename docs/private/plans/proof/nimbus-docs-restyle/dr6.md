# DR6 Odyssey on `/`, splash folded into `/docs`

Fail-before: after DR5 the home page was the fumadocs default and the retired
Starlight splash had been deleted with the Astro site, so the install command,
the seven adapter tabs, the production verb, the agents pitch, and the tenancy
paragraph had no page. The marketing prototype held the Odyssey but carried its
own cobalt palette.

## The Odyssey moved to `/`

`marketing-odyssey/app/page.tsx` became
`website/src/components/odyssey/journey.tsx`, a client component, with
`src/app/page.tsx` left as a thin server component so the route keeps its
metadata. `scene.ts`, `timeline.ts`, `world.ts`, and `styles/odyssey.css` came
across whole. Geometry did not change; only tones did.

`world.ts` is 2,657 lines. It is one concept — the world the journey draws —
and decomposition is out of scope for a port. Its header records the exception.

### Retheming

`scene.ts` carried two palette tables and a `setPalette` switch. Both tables,
the switch, and the `let stops` indirection are deleted. One `STOPS` table
remains, whose ten values are the role tokens from `src/styles/tokens.css`
written as literals, because canvas cannot read a CSS custom property.

| Stop | Value | Token |
| --- | --- | --- |
| `night` | `#0a0b0c` | `--bg-canvas` dark |
| `paper` | `#fafafa` | `--bg-panel` light |
| `wash` | `#f0b23e` | `--accent` |
| `washInk` | `#1a1204` | `--accent-ink` |
| `inkNight` | `#f6f7f8` | `--text-1` dark |
| `inkPaper` | `#18181b` | `--text-1` light |
| `accentNight` | `#f0b23e` | `--accent-edge` dark |
| `accentPaper` | `#866423` | `--accent-edge` light |

`blueMixFor` is renamed `washMixFor` throughout.

### Two contrast defects found and fixed

**Daylight frames washed out.** The first pass gave `accentPaper` the literal
`#f0b23e`, which on the `#fafafa` ground is about 1.9:1. Reading `scene.chip()`
and `scene.panel()` showed why that is wrong: a chip is a 1px stroke over a
12–30% tint with its label in the frame's ink, and a panel is a stroke, so the
world draws no large accent fill anywhere across the roughly 163 call sites.
Every accent in the frame is a thin one and has to clear the non-text floor on
its own. `accentPaper` therefore takes `--accent-edge` light, `#866423`, the
same value the console uses for the same job.

**Gold under white in the finale.** `paletteFor` mixed ink and both accent
roles toward `WHITE` by the wash amount, which on a gold ground is again about
1.9:1. The `washInk` stop was added and the mix now goes there instead. At full
wash the stage reads `--stage-background: rgb(240 178 62)` over
`--stage-ink: rgb(26 18 4)`, 10.9:1.

`WHITE` is kept: `world.ts:2334` uses it for terminal output on the night
laptop screen, which is a screen and not a ground.

### The mark

A logo is exempt from SC 1.4.11, so the nav mascot keeps the literal
`--accent` / `--accent-ink` pair on both the night and the daylight grounds
rather than darkening with the frame's other accents. Through the finale the
ground *is* that gold, so `.brand .brand-mark` interpolates the body and the
face against each other on `--wash-mix` alone: the mark inverts instead of
disappearing. See `odyssey-19-mark-inverted.png`.

The five `doorColors` stay as drawn. Five protocol doors need five hues a
reader can tell apart, and the role tokens carry no categorical scale. A
comment in `world.ts` says so.

## The splash folded into `/docs`

`src/app/(docs)/docs/page.tsx` now carries every section of the retired
`index.mdx` except the hero, which the Odyssey replaces: the install command,
the seven adapter tabs, the canonical sentence, the production verb, the agents
pitch with its two cards, the tenancy paragraph, and both card grids.

Rendered `out/docs/index.html` holds:

| Check | Result |
| --- | --- |
| Tab labels | Convex, Firebase, Cloud Functions, MongoDB, DynamoDB, HTTP API, Sandboxes |
| Canonical sentence | present |
| `source-available` | 1 |
| `open source` | 0 |

The page is TSX, not MDX, so two things it would otherwise inherit are
supplied by hand. `src/components/code.tsx` wraps fumadocs' `ServerCodeBlock`
with `themes: { light: 'github-light', dark: 'github-dark' }` and
`defaultColor: false`, the same pair the MDX pipeline uses, so the blocks emit
the `--shiki-light` / `--shiki-dark` properties `docs-theme.css` selects
between. A `TOC` constant feeds `DocsPage` the seven headings that an MDX page
would have extracted.

## Social card

`docs/brand/mascot/og.svg` is the 1200x630 card: the gold mascot beside the
wordmark on `--bg-canvas` dark, with a short `--accent-edge` rule and the
domain in mono. The two strings are Geist outlines extracted from the woff2
files in `website/public/fonts/`, not font references, so the render needs no
installed font and the wordmark is the site's own face. `render.sh` rasterizes
it to `website/public/og.png` alongside the existing icon set.

`src/lib/og.ts` holds the descriptor. The root layout declares it for every
page and `src/app/page.tsx` repeats it, because Next replaces a parent
`openGraph` block whole rather than merging it field by field. Verified in the
export: `/` and `/get-started/quickstart/` both carry
`og:image = https://nimbusdocs.com/og.png` with width, height, and alt, and `/`
adds `twitter:card = summary_large_image`.

## Browser verification

Served from `out/` on port 8791 at 1440x950.

| Shot | What it shows |
| --- | --- |
| `odyssey-01-open.jpg` | chapter 00 on the night ground |
| `odyssey-12-daylight.jpg` | chapter 12, the daylight ground, strokes and labels in `#866423` |
| `odyssey-19-wash.jpg` | chapter 19, the gold ground under `#1a1204` |
| `odyssey-19-mark-inverted.png` | the nav mark at full wash, body and face traded |
| `docs-dark-top.jpg` / `docs-light-top.jpg` | the landing above the fold in both schemes |
| `docs-dark-adapters.jpg` / `docs-light-adapters.jpg` | the seven tabs and a highlighted block in both schemes |

## Reduced motion

At 1280x600 the media query
`(prefers-reduced-motion: reduce), (max-height: 620px), (max-width: 759px) and (max-height: 779px)`
matches and `mountStoryboard` runs: 20 stills plus the stage canvas, 21 in all,
one `<canvas>` per chapter with no scroll driver. The per-beat
`--beat-background` / `--beat-ink` pair carries the same tonal arc, night
through daylight to the gold finale, and the chrome bar tracks it.
`storyboard-19-wash.jpg` is the last beat.

## Build

`npm run build`, exit 0, TypeScript clean, 117 static pages, unchanged from
DR5. `npx tsc --noEmit` clean.

Acceptance greps, run over `src` and `out`:

- `grep -rEn "5577ff|3047bd|3f57e8|070b11|f6f4ee"` prints nothing.
- `out/index.html` carries `<title>nimbus — your cloud, one binary</title>`.
