# DR7 — URL map

The Starlight site published one URL per Markdown page under the six groups.
The Next export publishes the same set. Nothing redirects, because nothing
moved: `trailingSlash: true` plus the slug rule in `website/src/lib/source.ts`
reproduce the old paths exactly.

## Method

```
find out -name index.html | sed 's|^out||; s|/index.html$|/|' | sort
```

compared against every `docs/<group>/**/*.md`, with `<dir>/index.md` folding to
`/<dir>/`.

## Result

| Group | Markdown pages | Routes emitted |
| --- | --- | --- |
| `get-started/` | 5 | 5 |
| `developers/` | 23 | 23 |
| `agents/` | 8 | 8 |
| `operators/` | 15 | 15 |
| `concepts/` | 23 | 23 |
| `reference/` | 34 | 34 |
| **total** | **108** | **108** |

Missing from the build: none. Every one of the 108 pages answers at the URL it
answered before.

## Routes with no Markdown behind them

| URL | What it is |
| --- | --- |
| `/` | the Odyssey, `website/src/app/page.tsx` |
| `/docs/` | the documentation landing, `website/src/app/(docs)/docs/page.tsx` |
| `/404/`, `/_not-found/` | the export's not-found page, `website/src/app/not-found.tsx` |

## Non-page artifacts

| Path | Source |
| --- | --- |
| `/llms.txt`, `/llms-full.txt`, `/llms-small.txt` | `website/src/app/llms*.txt/route.ts` over `website/src/lib/llms.ts` |
| `/sitemap.xml` | `website/src/app/sitemap.ts` |
| `/api/search` | the fumadocs static search index, built at export time |
| `/og.png`, `/favicon.svg`, `/icon-512.png` | rendered by `docs/brand/mascot/render.sh` |
