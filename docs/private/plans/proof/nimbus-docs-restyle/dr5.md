# DR5 Search and llms

Commit `ff946e7b6` on `nimbus-docs-restyle`.

Fail-before: after DR4 the export had no search index, no `llms` files, and no
sitemap. The Starlight site had shipped all four.

## Search

`src/app/api/search/route.ts` exports `staticGET` from `createFromSource`, so
the index is written once at build time. `src/components/search-dialog.tsx`
drives it with `staticClient({ from: '/api/search' })`. The dialog is a lazy
import, so the engine reaches the browser only when the reader opens search.

| Measure | Value |
| --- | --- |
| Index on disk | 7,785,674 bytes |
| Index gzipped | 1,521,514 bytes |
| Served | HTTP 200 at `/api/search` |

The index is extensionless, which DR7 must keep in mind when it points
`wrangler.jsonc` at `out/`.

Behaviour: a search for "tenant isolation" returns headings and body matches
from the concepts and operators groups, grouped by page, with the terms marked.

## llms files

| File | Size | Page headings |
| --- | --- | --- |
| `llms.txt` | 21,022 bytes | index only |
| `llms-full.txt` | 920,021 bytes | 108 |
| `llms-small.txt` | 566,501 bytes | 71 |

`llms-small.txt` drops the thirteen `concepts/architecture` pages and the
twenty-four pages under the ten per-protocol `reference` directories. Those are
deep rather than broad: the architecture series explains how the engine is
built, and the protocol directories are method tables that read better one page
at a time.

Fences: `grep -c "docs/private"` prints 0 for each of the three files.
`grep -c '^# .*(/concepts/architecture'` prints 0 for `llms-small.txt`, and no
heading in it addresses an excluded reference directory.

## Trailing slashes

`page.url` carries no trailing slash. `normalizeUrl` in fumadocs-core strips it,
and Next adds it back only where it renders a `<Link>` or a canonical tag under
`trailingSlash: true`. The llms files and the sitemap are raw text that Next
never touches, so the first build emitted `/agents/agent-chat`, an address that
costs every reader a redirect.

`canonical()` in `src/lib/source.ts` now adds the slash, preserving any fragment
or query. `llmsIndex()` applies it to the link targets that come out of the page
tree. The `url` option on `loader()`, which looked like it did this and was
normalized away, is deleted.

After: every markdown link in `llms.txt` and all 110 `<loc>` values in
`sitemap.xml` end in a slash.

## Build

`npm run build`, exit 0, TypeScript clean, 117 static pages: 108 documentation
pages, the home page, the documentation landing, the 404, the search index, the
three llms files, and the sitemap.
