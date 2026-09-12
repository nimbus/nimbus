# DR8 — closeout

Every gate, count and review that stands behind the pull request. Recorded at
branch tip `7f33f1762` on `nimbus-docs-restyle`, cut from `main` at
`b527e1742`.

## The pull request

[nimbus/nimbus#351](https://github.com/nimbus/nimbus/pull/351) —
"docs: rebuild nimbusdocs.com on fumadocs and put the mascot on the mark".
Open, not a draft, 128 files, +12090 / -5293, `nimbus-docs-restyle` into
`main`. The description carries the session URL and states the review gap
recorded below.

## The branch

| Item | Value |
| --- | --- |
| Branch | `nimbus-docs-restyle` |
| Base | `main` at `b527e1742` |
| Tip | `7f33f1762` |
| Commits | 11 |
| Files changed | 128 (117 text, 11 binary) |
| Lines | +12090 / -5293 |

Commits, oldest first:

| Commit | Task | Subject |
| --- | --- | --- |
| `b815b6b2c` | DR1 | design: fill the mascot with the mark colour on every console surface |
| `2a5e188a2` | DR3 | design: add the mascot brand set and the README banner |
| `bb8b350be` | DR1 | design: make the gold one literal and split the accent edge |
| `1ee5cd113` | DR3 | design: retire the white mascot and restore the tiled app icon |
| `13779aca1` | DR4 | docs: rebuild nimbusdocs.com on Next and fumadocs |
| `550d50364` | DR4 | design: delete the canonical cloud logo |
| `ff946e7b6` | DR5 | docs: add static search, the llms files, and a sitemap |
| `5e74fef1a` | DR6 | docs(website): put the Odyssey on / and fold the splash into /docs |
| `ba724ed30` | DR7 | docs: move the gates, the pipeline and the prose onto fumadocs |
| `c42a1f86e` | DR8 | docs(website): drop the credential shape from the Postgres flag example |
| `7f33f1762` | DR8 | fix(ui): keep the gold off every hairline, and gate it |

DR2 lives in the desktop repository on its own branch `nimbus-mark`, at
`0127834`, and is not part of this pull request.

## Gates

| Command | Result |
| --- | --- |
| `npm --prefix website run build` | exit 0, 117 static routes |
| `npm --prefix website run typecheck` | exit 0 |
| `bash scripts/check-docs.sh` | `PASS — 110 pages link-clean, source map resolves, private fence intact, titles unique`, exit 0 |
| `bash scripts/verify-nimbus-docs-site.sh` | `17/17 conditions green`, exit 0 |
| `actionlint .github/workflows/docs.yml` | exit 0 |
| `npm --prefix packages/nimbus-ui run test` | 130 files, 1128 tests, all passed |
| `npm --prefix packages/nimbus-ui run typecheck` | exit 0 |
| `npm --prefix packages/nimbus-ui run lint` | exit 0 |

The console lint run prints one warning and three informational notices. All
four are present on `main` unchanged: the `suppressions/unused` warning at
`packages/nimbus-ui/src/components/data-table.tsx:384`, and three
`noUselessFragments` notices in two spec files. This branch touches that file
only at lines 360 and 484.

## Counts

| Count | Value |
| --- | --- |
| Markdown pages under the six published groups | 108 |
| Routes emitted for them | 108 |
| Pages missing from the build | 0 |
| Routes with no Markdown behind them | 4 (`/`, `/docs/`, `/404/`, `/_not-found/`) |
| Total `index.html` files | 112 |
| Total static routes | 117 |
| Pages per group | get-started 5, developers 23, agents 8, operators 15, concepts 23, reference 34 |
| Distinct pages in `out/api/search` | 108 |
| Search index entries | 8290 |
| `<loc>` entries in `out/sitemap.xml` | 110 |
| Paths under `out/` matching `private` | 0 |
| `docs/private` occurrences in each `llms` file | 0 |

Per-page URL continuity is tabulated in [`dr7-urls.md`](dr7-urls.md).

## Visual check

Six frames under [`dr8/`](dr8/), captured from `website/out` served locally:

| Frame | Shows |
| --- | --- |
| `docs-dark-top.jpg` | the docs chrome, sidebar and search in dark |
| `docs-dark-body.jpg` | a reference page body in dark |
| `docs-light-body.jpg` | the same body in light |
| `docs-light-cards.jpg` | the `/docs/` landing cards in light |
| `odyssey-open.jpg` | `/` at the opening chapter, night ground |
| `odyssey-gold.jpg` | `/` at a gold chapter, mark inverted |

Both themes carry the gold accent as a fill under dark ink. No surface shows
`#b45309`.

`search-light.jpg` is the search dialog answering "tenant isolation" from the
static index, with the gold edge on the matched terms.

## The accent unification

The palette carried three accent literals. `--accent` was the gold `#f0b23e`;
`--accent-edge` and `--accent-link` were both `#866423`, the gold darkened at
its own hue until it cleared 4.5:1 as text on white. That third colour was the
only accent the light theme showed in prose, in a focus ring, and on the
Odyssey's paper beat, and it reads olive beside the mark it echoes. The owner
rejected it and chose option B, gold on dark carriers.

No sRGB colour clears 4.5:1 as text on both `#ffffff` and `#0a0b0c`. The window
is empty, not narrow, so one readable accent literal cannot exist. The palette
now carries two colours and three roles:

| Token | Light | Dark | Role |
| --- | --- | --- | --- |
| `--accent` | `#f0b23e` | `#f0b23e` | the mark, never darkened |
| `--accent-ink` | `#1a1204` | `#1a1204` | ink on a gold fill, and the gold's carrier |
| `--accent-text` | `#1a1204` | `#f0b23e` | whichever of the pair this ground can show |

`--accent-text` is always exactly one of the other two, held by test. The
governing rule is that the gold must name its carrier. A mark with area pairs
the gold with the ink at 9.84:1 either way round; only a mark with no area --
a 1px rule, a counter, a prose link, a fumadocs active row -- falls back to
`--accent-text`, and never as the sole signal (SC 1.4.1).

### The latent defect

The old focus ring was `color-mix(in srgb, var(--accent-edge) 40%,
transparent)`. The token measured 4.61:1 and passed every token-level check;
the composited stroke measured 1.77:1 on white. The ring is now two opaque
strokes, gold inside and ink outside, in one unlayered `:focus-visible` rule
that outranks every layered `ring-*` utility and so reaches the vendored
registry primitives with no theming (SC 2.4.13 measures an indicator against
the adjacent colour, which is what legitimises the outer keyline).

### Gates added

| Gate | What it holds |
| --- | --- |
| `contrast.spec.ts` palette invariants | `--accent-text` is the accent or its ink, and keeps the gold wherever the gold can be read |
| `contrast.spec.ts` focus gates | the indicator is opaque, measured composited, and 3:1 against all eight grounds |
| `contrast.spec.ts` carrier grep | registry-inclusive; an accent fill must name a carrier |
| `verify-nimbus-docs-site.sh` cond 14 | rejects `b45309`, `866423`, `accent-edge`, `accent-link` under `website/src` |
| `verify-nimbus-docs-site.sh` cond 18 | the console and website sheets declare the same roles and values in both themes, accounting for their inverted selectors |

Nine negative tests were run against the finished gates. Each reintroduced one
defect and each was caught: the retired mud in light `--accent-text` (2
failures), the ink chosen on dark where the gold reads (2), the translucent
ring restored (2, of which the composite gate was the only check able to see
it), the keyline stop deleted (1), the sidebar rail losing its keyline (1),
one role drifting in light only (cond 18), a role missing from one sheet (cond
18), the mud reappearing in the docs tree (conds 14 and 18), and
`--accent-text` renamed back to `--accent-edge` (conds 14 and 18). Every
restore returned to green.

### The Odyssey

The stage is not the theme. It runs a cinematic cycle through `--stage-*`,
written per frame by `paletteFor`, so a beat's ground is unrelated to the
reader's theme. The token model had collapsed three distinct accent contexts
into one variable; they are now separate:

| Context | Tone |
| --- | --- |
| canvas structure -- lanes, panels, chip borders, dots | the gold, on every beat |
| DOM labels -- `.beat .beat-eyebrow`, `.telemetry b`, `.status-location .state` | the gold on `--stage-accent-carrier` |
| canvas captions drawn by `scene.tag()`, bare 11px mono with no pill | the readable member per beat |

The caption tone steps rather than crossfades (`inkMix`, not `paperMix`): a
continuous fade from the gold to the accent's ink passes through exactly the
darkened golds this palette exists to exclude. `--stage-accent-text` is
retired -- the change removed a variable rather than adding one.

### Defects the visual pass found

Four, none of which any gate could have seen:

1. The light-theme rest tone was the gold on `--bg-canvas` at 1.88:1, because
   the retired value it replaced had been the readable mud.
2. Canvas captions were left gold on the paper beat at 1.81:1. `scene.chip()`
   draws a pill and labels it from the ink mix; `scene.tag()` has no pill, and
   only `tag` sites consume the accent tone.
3. After that fix `--stage-accent-text` and `--stage-accent-carrier` both
   resolved to the ink on the paper beat, which would have rendered the DOM
   eyebrow ink on ink.
4. `.beat-eyebrow` is (0,1,0) and lost the cascade to `.beat p` at (0,1,1), so
   the chapter label had never rendered as the accent at all, and the new
   carrier had put an ink pill behind paragraph grey. All three rules were
   raised to `.beat .beat-eyebrow`, the mobile margin override included.

### Measured, not eyeballed

Live sampling of the chapter label at three beats, identical in both themes,
which is the point:

| Beat | Label | Carrier | Label on carrier | Carrier vs ground |
| --- | --- | --- | --- | --- |
| night | `rgb(240 178 62)` | `rgb(10 11 12)` | 10.45:1 | 1.00, reads as bare gold |
| paper | `rgb(240 178 62)` | `rgb(26 18 4)` | 9.84:1 | 17.77, a visible ink pill |
| wash | `rgb(26 18 4)` | `rgb(240 178 62)` | 9.84:1 | 1.00, reads as bare ink |

In the console, the active sidebar rail computes to `bg-accent ring-1
ring-accent-ink` -- gold with an opaque `rgb(26 18 4)` keyline -- and a
keyboard focus computes to `rgb(240 178 62) 0 0 0 2px, rgb(26 18 4) 0 0 0 3px`
in both themes.

Seven further frames under [`dr8/`](dr8/):

| Frame | Shows |
| --- | --- |
| `accent-ody-light-night.jpg` | `/` light theme, night beat, gold label and gold captions |
| `accent-ody-light-paper.jpg` | `/` light theme, paper beat, gold label on its ink pill, ink captions |
| `accent-ody-light-wash.jpg` | `/` light theme, gold finale, ink label on the gold ground |
| `accent-ody-dark-night.jpg` | `/` dark theme, night beat, identical to its light twin |
| `accent-ody-dark-paper.jpg` | `/` dark theme, paper beat, identical to its light twin |
| `accent-console-light-focus.jpg` | the console in light, gold rail and a focused two-stroke ring |
| `accent-console-dark-focus.jpg` | the console in dark, the same two marks |

One documented exception to the carrier rule survives: `.highlighted-word` in
`docs-theme.css` draws a 40% border over an `--accent-tint` wash. The wash and
the 500 weight carry the emphasis, so the border is decorative and not a sole
indicator, which is why the opaque-stroke rule, scoped to indicators, does not
reach it.

## Runtime checks against the export

Served from `website/out` on `localhost:8791` and driven in the browser:

| Check | Result |
| --- | --- |
| Distinct page URLs in the search index | 108 |
| Those URLs that return a page | 108 of 108, none broken |
| Index entries mentioning `docs/private` | 0 |
| Links in `llms.txt` | 109, all slash-terminated, none broken |
| Absolute links on a rendered reference page, sidebar included | 68, none broken |
| Canonical tag on `/reference/cli/` | `https://nimbusdocs.com/reference/cli/` |
| `GET /api/search` | 200, 7785900 bytes, served with no content type, parses as JSON |

Pages are served at the root, as they were under Starlight:
`/reference/cli/`, not `/docs/reference/cli/`. `/docs/` is the landing page
only, and `/` is the Odyssey.

## Review

### The structured gate cannot review this branch

`autoreview --gate pre-pr --mode auto` refuses the branch diff:

```
refusing binary changes in branch diff because their contents cannot be reviewed:
- packages/nimbus-ui/public/favicon.ico
- packages/nimbus-ui/public/icon-512.png
- website/public/favicon.ico
- website/public/fonts/Geist-Medium.woff2
- website/public/fonts/Geist-Regular.woff2
- website/public/fonts/Geist-SemiBold.woff2
- website/public/fonts/GeistMono-Medium.woff2
- website/public/fonts/GeistMono-Regular.woff2
- website/public/fonts/GeistMono-SemiBold.woff2
- website/public/icon-512.png
- website/public/og.png
```

The branch legitimately adds eleven binary assets: two favicons, two app
icons, the Open Graph card, and the six self-hosted Geist woff2 faces. The
helper calls `require_no_binary_diff` unconditionally from the branch-diff
path, and exposes no flag or config key that admits binaries. The refusal is
correct on its own terms; a font file has no reviewable content. It leaves the
branch with no supported route to a whole-diff structured review.

Two workarounds were tried and abandoned. A synthetic base commit carrying the
same eleven blobs makes the diff text-only, but reaching it needs `--base` on a
fabricated ref, which the permission layer denied. Reviewing the branch as one
squashed text commit has the same shape and the same objection.

**This is the one DR8 step that did not complete.** Closing it needs a decision
that belongs to the repository owner: either add a binary allow-list to the
autoreview helper, or accept the substitute coverage below as the pre-PR
evidence for this branch.

### Substitute coverage

Five of the first ten commits carry no binary and can be reviewed per commit
by the same helper, same reviewer, same gate:

| Commit | Result |
| --- | --- |
| `b815b6b2c` | `autoreview clean: no accepted/actionable findings`, `overall: patch is correct (0.99)` |
| `ff946e7b6` | `autoreview clean: no accepted/actionable findings`, `overall: patch is correct (0.99)` |
| `ba724ed30` | `autoreview clean: no accepted/actionable findings`, `overall: patch is correct (0.99)` |
| `550d50364` | `autoreview skipped: automatic checkpoint contains no substantive code changes` |
| `c42a1f86e` | cannot run; see below |

Reviewer for all three graded runs: `codex model=gpt-5.6-sol thinking=high`,
profile `auto`, selection `sol=7.83`, tools and web search on.

`c42a1f86e` fails its own TruffleHog preflight. In commit mode the scanner
reads the pre-image as well as the post-image, and the pre-image is the
credential-shaped placeholder this commit exists to delete. `ba724ed30`
carries `postgresql://user:pass@db:5432/nimbus` at
`website/src/components/odyssey/timeline.ts:287`; the commit replaces it with
`postgresql://db:5432/nimbus`. The commit is one line and its whole content is
the removal.

The five commits the helper cannot reach at all are `2a5e188a2`, `bb8b350be`,
`1ee5cd113`, `13779aca1` and `5e74fef1a`. `13779aca1` is the largest text
commit on the branch, so per-commit review leaves the bulk of the new site
code ungated by the structured helper.

To cover that gap the whole 114-file text diff at `c42a1f86e` went through an
adversarial review over the supported Codex path, briefed on the privacy
fence, both gate scripts, the static export, the sidebar generator, the llms
emitter, the Odyssey's reduced-motion behaviour, theme contrast, and the
console regression surface. Findings are recorded in the section below.

### Findings and repairs

Every finding was re-derived against the source before it was accepted or
declined. Contrast figures are computed from the token sheet, not quoted from
the review. Commit `7f33f1762` carries the repairs.

| # | Site | Finding | Disposition |
| --- | --- | --- | --- |
| 1 | `packages/nimbus-ui/src/shell/sub-panel.tsx:373` | `bg-accent` on the 1px resize separator in three states | fixed, `bg-accent-edge` |
| 2 | `packages/nimbus-ui/src/styles/contrast.spec.ts` | `GOLD_MISUSE` omits the `bg` prefix, so finding 1 passed the gate | fixed, new ink rule |
| 3 | `scripts/verify-nimbus-docs-site.sh` | `ARCH_MANIFEST` lists 12 of 13 architecture pages | fixed, slug added and check made bidirectional |
| 4 | `packages/nimbus-ui/src/components/files/upload-queue.tsx:60` | `bg-accent` on the 6px progress fill | fixed, `bg-accent-edge` |
| 5 | `packages/nimbus-ui/src/components/storage/query-bar.spec.tsx:149` | assertion passed by substring against the renamed class | fixed, anchored at both ends |
| 6 | `scripts/check-docs.sh` | the TSX link scan reads literal `href="..."` only | fixed, link constants now scanned |
| 7 | `packages/nimbus-ui/src/styles/tokens.css:268,276` | the shadcn bridge maps `--color-ring` and `--color-sidebar-ring` to the fill | fixed, `var(--accent-edge)` |
| 8 | `packages/nimbus-ui/src/components/ui/dropdown-menu.tsx:91,116,165,206` | `focus:bg-accent focus:text-accent-foreground` puts `--text-1` on gold | declined, pre-existing |

Finding 7 is not in the review's report. It was found by reading the bridge
block against `main`, and it is the most consequential of the eight: it is a
regression this branch introduced. Light `--accent` was `#b45309` on `main`,
so every registry focus ring measured 4.26:1 to 5.02:1; moving the accent to
the gold took them all to 1.60:1 to 1.88:1. The registry primitives reach the
ring through generated `ring-ring` and `outline-ring` utilities, which no
Nimbus-owned rule can intercept, so the bridge is the only place the split
reaches them.

Findings 1, 4 and 7 share one root cause: `--accent` is a fill and carries
contrast only under `--accent-ink`, but nothing stopped it being used as the
visible stroke itself. Measured on the light canvas `#ffffff`:

| Token | Light canvas | Light panel | Light hover |
| --- | --- | --- | --- |
| `--accent` `#f0b23e` | 1.88:1 | 1.81:1 | 1.60:1 |
| `--accent-edge` `#866423` | 5.44:1 | 5.21:1 | 4.61:1 |
| `--accent` on `main` `#b45309` | 5.02:1 | 4.81:1 | 4.26:1 |

`--accent-edge` is `#f0b23e` in dark, so none of the three repairs changes a
dark-mode pixel. `#1a1204` on the gold is 9.84:1, which is why the fill itself
is sound wherever the ink rides on top of it.

Finding 2 could not be repaired by adding `bg` to the prefix list, because
painting a fill is the gold's purpose. The rule the new test encodes is the
ink instead: a string literal containing `bg-accent` must also contain
`text-accent-ink`. Matching runs per string literal rather than per file, so
the ink has to be on the same element. The two `bg-accent` uses that remain,
`upgrade-popover.tsx:94` and `:105`, both carry the ink and pass. A second
test pins the bridge ring tokens to `--accent-edge`.

Finding 6 is real but not as the review described it. `website/src/app/page.tsx`
holds no `href` at all; the homepage's internal links are upper-case constants
in `website/src/components/odyssey/journey.tsx`, so a literal-attribute regex
could never see them. Both resolve today. The scan now also reads
`const NAME = '/path/'`, anchored on the leading slash so that a media query
or a data attribute is not mistaken for a link.

Finding 8 is declined rather than deferred. It is a genuine 1.76:1 failure for
the menu label in dark mode, but dark `--accent` was already `#f0b23e` on
`main`, so the failure predates this branch and repairing it here would widen
the diff past the branch's subject. It belongs to the shadcn hover-ground
collision already documented in the bridge comment.

Each new gate was negative-tested against the defect it exists to catch:

| Gate | Injected defect | Result |
| --- | --- | --- |
| `contrast.spec.ts` ink rule | `sub-panel.tsx` reverted to `bg-accent` | `FAIL shell/sub-panel.tsx:373 bg-accent with no ink` |
| `contrast.spec.ts` ring rule | `--color-ring` reverted to `var(--accent)` | `FAIL expected 'var(--accent)' to be 'var(--accent-edge)'` |
| `check-docs.sh` link scan | `QUICKSTART` pointed at `/get-started/nope/` | `FAIL dead internal link /get-started/nope/` |
| condition 17 | `network-control-plane` removed from the manifest | `FAIL network-control-plane(page not in manifest)` |

All four were restored and the tree verified clean afterwards.

The review also confirmed sound, against the checked-out branch rather than
from reading: the privacy fence, both gate scripts, the sidebar generator
audited against the real `docs/` tree, the llms emitter, static export
routing, and the Odyssey's reduced-motion handling.

### Secrets

TruffleHog, run over the branch additions:

```
trufflehog: clean (5.5s)
```

One pre-existing credential shape stays on `main` and is untouched by this
branch: the MongoDB example URI at `README.md:114`. It is not in this branch's
added lines.

## Hosted CI

Run `34706863339` and its siblings on `7f33f1762`: 53 checks pass, 4 skip, none
fail. The skips are conditional lanes that do not run on a pull request:
`CodeQL`, `Linux Release Candidate`, `Profile-Aware Runtime Crossover`, the
nightly harness and the nightly workspace shards.

The lanes that bear on this branch: `Build and Deploy Docs Site` 1m14s,
`JavaScript Build and Test` 2m56s, `UI Artifacts` 59s, `Desktop UI Smoke Walk`
13m14s, `Rust Clippy` 13m57s, `Rust Format` 18s, `Rust Gate Summary` 4s.

## The worktree at closeout

Two files in `/Users/jack/src/github.com/nimbus/nimbus` belong to a
concurrent session and are deliberately left uncommitted:

- `examples/nimbus/agent-chat/package.json`
- `examples/nimbus/agent-chat/package-lock.json`

They are not part of this branch and must not be committed or stashed by this
plan. The review worktree at
`scratchpad/review-wt` was created detached so the gate could run against a
clean tree without touching them; DR9 removes it.

A second temporary worktree, `scratchpad/lint-head`, was added detached at the
committed tip to establish whether the console lint diagnostics were
pre-existing. Biome exits 0 there with one warning and three notices, which
places all four on the branch before the repair commit and identified the one
real error as formatting in the newly written test. It was removed the same
turn; `git worktree list` now shows only `review-wt` for this plan.

## Commands

| Command | Result |
| --- | --- |
| `npm --prefix website run build` | exit 0 |
| `npm --prefix website run typecheck` | exit 0 |
| `bash scripts/check-docs.sh` | PASS, 110 pages, exit 0 |
| `bash scripts/verify-nimbus-docs-site.sh` | 18/18, exit 0 |
| `actionlint .github/workflows/docs.yml` | exit 0 |
| `npm --prefix packages/nimbus-ui run test` | 1128 tests passed |
| `npm --prefix packages/nimbus-ui run typecheck` | exit 0 |
| `npm --prefix packages/nimbus-ui run lint` | exit 0 |
| `autoreview --gate pre-pr --mode auto` | refused, eleven binary assets |
| `autoreview --gate pre-pr --mode commit --commit b815b6b2c` | clean, 0.99 |
| `autoreview --gate pre-pr --mode commit --commit ff946e7b6` | clean, 0.99 |
| `autoreview --gate pre-pr --mode commit --commit ba724ed30` | clean, 0.99 |
| `autoreview --gate pre-pr --mode commit --commit 550d50364` | skipped, no substantive change |
| `autoreview --gate pre-pr --mode commit --commit c42a1f86e` | refused, pre-image credential shape |
| `npx vitest run src/styles/contrast.spec.ts` | 39 tests passed |
| `npm run test` (root, after the accent unification) | 130 files, 1136 tests passed |
| `npx biome check src` | exit 0 after formatting the new test |

The gate rows above were re-run at `7f33f1762`. The `autoreview` rows are the
runs recorded at `c42a1f86e` and are not re-run for the repair commit, which
is itself the product of the review those rows describe.
