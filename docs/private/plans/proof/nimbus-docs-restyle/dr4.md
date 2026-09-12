# DR4 Fumadocs site

Commits `13779aca1` and `550d50364` on `nimbus-docs-restyle`.

Fail-before: `website/` was an Astro Starlight site. It symlinked the six published
groups of `docs/` into `website/src/content/docs/`, carried its own palette in
`src/styles/custom.css`, and had no relation to the console UI beyond the accent
hue. The retired cloud logo still had a canonical source and eighteen generated
variants.

## What the site is now

A Next 16 static export (`output: 'export'`, `trailingSlash: true`) with fumadocs
16. The content root is `../docs`, read in place. No symlink, no copy.

| Concern | Owner |
| --- | --- |
| Compile fence | `source.config.ts`, `files` naming the six published groups |
| Page order | `sidebar.order` frontmatter, already on every page |
| Folder titles, section headings | `src/lib/sidebar.ts` |
| Palette and type | `src/styles/tokens.css` |
| Fumadocs role mapping | `src/styles/docs-theme.css` |

`src/lib/sidebar.ts` generates every meta file through `update(source).files()`.
`docs/` carries no meta files and must not grow any: it is the repository's own
Markdown, checked by the docs gates and read directly on GitHub.

A folder's index page is its clickable folder row. Listing `index` in a `pages`
array would show the same page twice, so no list names it.

## Theme

`--color-fd-primary` maps to `--accent-edge`, not `--accent`. Fumadocs uses the
primary role as a text colour 38 times against 16 uses as a fill, so binding it
to the large-area gold would have put gold text on gold-tinted grounds. The edge
token is the one measured to 4.5:1 on all four grounds.

`--color-fd-accent` maps to `--bg-hover`, which is the console's binding contrast
ground. The sidebar takes `--bg-panel`.

## Verification

Build: `npm run build`, exit 0, TypeScript clean, 112 static pages.

Routes against sources: 108 exported directories, 108 Markdown sources, `diff`
of the two sorted lists is empty apart from `404` and `_not-found`.

Private fence: no path under `out/` contains `private`, `brand` or `assets`.
Only `out/brands/` is present, which is the public adapter-glyph directory in
`website/public/`.

Markdown compile safety: the angle-bracket tokens survive as escaped text, not as
swallowed tags.

| Page | Tokens found |
| --- | --- |
| `operators/troubleshooting` | `<dir>` `<error>` `<file>` `<token>` |
| `operators/hardening` | `<token>` |

Visual: `/concepts/how-nimbus-works/` in dark and light, `/docs/`,
`/get-started/quickstart/` with code blocks and the copy control, and `/`.
The mascot lockup renders gold on both grounds.

## Deleted

`docs/brand/logo/` (18 SVGs), `docs/brand/gen-variants.sh`, and
`packages/nimbus-ui/public/nimbus-logo.svg`. Nothing referenced them except the
generator itself.

## Left for DR7

`bash scripts/check-docs.sh` now fails: it opens `website/src/content/docs`,
which no longer exists. It raises `FileNotFoundError` and **still exits 0**, so
the gate cannot fail. Both the Starlight assumption and the swallowed exit
status belong to the DR7 rewrite. `website/wrangler.jsonc` still points
`assets.directory` at `./dist` instead of `./out`, also DR7.
