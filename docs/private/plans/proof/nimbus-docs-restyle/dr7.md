# DR7 — the gates, the pipeline and the prose follow the renderer

The site changed from Astro Starlight to a Next static export drawn by
fumadocs in DR4. DR7 moves everything that described the old renderer: the two
docs gates, the deploy pipeline, the wrangler target, the docs skill, the
DESIGN.md surface entry, and the two Markdown files that name the renderer in
prose. No page content changed.

## The red baseline

`scripts/verify-nimbus-docs-site.sh` was written against Starlight. Run against
the fumadocs tree before this task it reported **11/17**, failing:

| # | Condition | Why it failed |
| --- | --- | --- |
| 1 | astro + starlight + starlight-llms-txt deps | none of the three are dependencies any more |
| 2 | `astro.config.mjs` site + static output + llms plugin | the file does not exist |
| 3 | symlink allow-list under `website/src/content/docs/` | no symlinks; the fence is a loader glob |
| 5 | `docs/` top level allow-list | `docs/assets/` was not in the allowed set |
| 6 | `website/dist` + llms artifacts | the export writes `website/out` |
| 14 | `template: splash` landing | the landing is TSX, and `/` is the Odyssey |

`scripts/check-docs.sh` failed the same way, but with a traceback rather than a
gate message: it walked `website/src/content/docs`, which no longer exists, and
raised `FileNotFoundError` at exit 1.

## check-docs.sh

Rewritten for the fumadocs tree:

- The landing is `website/src/app/(docs)/docs/page.tsx`. It is TSX, so its
  links are scanned as `href="..."` rather than as Markdown link syntax, and a
  relative link in it fails the gate — a TSX page has no directory to resolve
  against.
- The page set gained `/` (the Odyssey) and `/docs/` (the landing).
- The symlink fence became a config check on `website/source.config.ts`:
  `PUBLISHED_GROUPS` must be exactly the six groups, the collection must read
  `dir: '../docs'`, and `files` must be built from `PUBLISHED_GROUPS.map`.
- `website/dist` became `website/out`, which also fails on `out/brand` or
  `out/assets`.
- The sitemap entry moved from `/sitemap-index.xml` to `/sitemap.xml`.

Both new checks were proved to fail loudly. Adding `'private'` to
`PUBLISHED_GROUPS` and pointing a landing link at `/developers/nope/` produced
three gate failures and exit 1; restoring both returned the gate to green.

Green:

```
check-docs: PASS — 109 pages link-clean, source map resolves, private fence intact, titles unique
```

## verify-nimbus-docs-site.sh

Still 17 conditions, still the same 17 outcomes. Eight moved:

| # | Now checks |
| --- | --- |
| 1 | `next` + `fumadocs-core` + `fumadocs-mdx` + `fumadocs-ui`, and no leftover `@astrojs/starlight` |
| 2 | `output: 'export'` and `trailingSlash: true` in `next.config.mjs`; `metadataBase` of `https://nimbusdocs.com` in the root layout |
| 3 | the `source.config.ts` allow-list is exactly the six groups, reads `../docs` through it, and nothing was copied into `website/src/content` |
| 5 | `docs/assets/` added to the allowed top-level set |
| 6 | `website/out` holds `index.html`, `docs/index.html`, the three llms files, `sitemap.xml`, and `api/search` |
| 7 | wrangler `assets.directory` is `./out` |
| 8 | `docs.yml` runs `npm ci` and `npm run build`, and watches `docs/assets/**` |
| 14 | `/` renders the Odyssey, `/docs/` is a `DocsPage`, `tokens.css` carries the gold accent, no `#b45309` survives anywhere in `website/src`, DESIGN.md documents the surface |

The condition-3 and condition-14 fences were proved to fail loudly the same
way: adding `'private'` to `PUBLISHED_GROUPS` and setting the light
`--accent-edge` back to `#b45309` dropped the run to 14/17 with both named in
the failing list. Restoring both returned 17/17.

Green:

```
17/17 conditions green
```

## The pipeline

`.github/workflows/docs.yml` already built the export with `npm ci` and
`npm run build`; two things were stale. `docs/assets/**` was missing from both
path filters, so a change to an embedded image would not have rebuilt the site.
The "Check docs" step still carried a skip branch for a gate that has existed
since DOC8; it now runs the gate unconditionally.

`actionlint .github/workflows/docs.yml` exits 0.

`website/wrangler.jsonc` now serves `./out`. Its header comment named a
Starlight site; it names the export. A comment records that `out/api/search` is
an extensionless file served as-is, so `not_found_handling` does not reach it.

## The prose

- `.agents/skills/docs/SKILL.md` — the renderer, the fence, the sidebar and the
  two entrances are described as they are built. The Markdown rule now says
  what `docs/` may contain rather than which Starlight components are allowed.
  The `llms-small.txt` exclusions point at `SMALL_EXCLUDES` in
  `website/src/lib/llms.ts`. No occurrence of "Starlight" remains.
- `DESIGN.md` §Documentation Site — the governing rule now reads "the doc body
  is product-tier; the home page is the site's single brand-tier moment",
  because the splash hero is gone and the Odyssey took its place. The doc-body
  bullet explains the two fumadocs vocabulary collisions the theme resolves:
  `--color-fd-primary` takes `--accent-edge` rather than `--accent`, and
  `--color-fd-accent` is the hover ground rather than the brand colour. The
  logo bullet states the mark rule as shipped: gold body, dark face, no theme
  swap. Typography corrected from system UI + JetBrains Mono to self-hosted
  Geist and Geist Mono.
- `docs/README.md` — names the Next and fumadocs project, says it reads the
  tree in place, and documents `assets/` next to `brand/`.
- `docs/source-map.md` — the seven landing rows were labelled `index.mdx`, a
  file that no longer exists. They are now `/docs/` landing rows, under a note
  saying the landing is hand-authored TSX making the same claims.

## URL continuity

Every one of the 108 Markdown pages answers at the URL it answered under
Starlight. Full table in [`dr7-urls.md`](dr7-urls.md).

| Check | Result |
| --- | --- |
| Markdown pages under the six groups | 108 |
| Routes emitted for them | 108 |
| Pages missing from the build | 0 |
| Routes with no Markdown behind them | 4 (`/`, `/docs/`, `/404/`, `/_not-found/`) |

## Commands

| Command | Result |
| --- | --- |
| `bash scripts/check-docs.sh` | PASS, 109 pages, exit 0 |
| `bash scripts/verify-nimbus-docs-site.sh` | 17/17, exit 0 |
| `actionlint .github/workflows/docs.yml` | exit 0 |
| `bash -n scripts/verify-nimbus-docs-site.sh` | exit 0 |
| `grep -rIn Starlight` over the tracked tree | three intentional historical comments in `website/`, the two gate strings, and one CHANGELOG line |
