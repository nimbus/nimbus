---
name: docs
description: Author and maintain the public Nimbus docs site (nimbusdocs.com) — information architecture, Diátaxis rules, the docs/private fence, messaging canon, and verification gates.
---

# Nimbus docs site (nimbusdocs.com)

The public documentation lives in-repo and is the source of truth. The site
is a renderer; the Markdown is the product. Completed baseline:
`docs/private/plans/archive/nimbus-docs-site-plan.md` (the `nimbus-docs-site`
plan, DOC0..DOC13, closed 2026-06-10).

## Where things live

- **Content:** `docs/{get-started,developers,agents,operators,concepts,reference}/`
  — the six public groups, and the ONLY public directories under `docs/`.
- **Renderer:** `website/` — a Next static export rendered by fumadocs.
  The loader in `website/source.config.ts` reads `../docs` in place. Nothing
  is copied or symlinked into the package, and no content belongs under
  `website/src/`.
- **Private fence:** `PUBLISHED_GROUPS` in `website/source.config.ts`. The
  collection compiles only `<group>/**/*.md`, so a new top-level directory
  under `docs/` publishes nothing until it is named there.
- **Sidebar:** `website/src/lib/sidebar.ts` — folder titles and the three
  hand-ordered sections. Page order inside a folder comes from the
  `sidebar.order` frontmatter the pages already carry. `docs/` holds no meta
  files and must not grow any.
- **Two entrances:** `/` is the Odyssey, the scroll-driven pitch
  (`website/src/app/page.tsx`). `/docs/` is the written landing
  (`website/src/app/(docs)/docs/page.tsx`). Both are hand-authored TSX with
  no Markdown behind them, so a change to the pitch is a code change.
- **Audit trail:** `docs/source-map.md` — every page's source-verification
  rows. Adding or changing a page means updating its rows.
- **Internal docs:** `docs/private/` — plans, proofs, architecture working
  notes, staging. **Never published, never linked from public pages.**

## Information architecture (hybrid persona × Diátaxis)

Six groups, persona-first, Diátaxis-shaped inside each:

| Group | Persona | Dominant mode |
| --- | --- | --- |
| Get started | new arrival | Tutorial |
| Developers | app builder | Tutorial + How-to |
| Agents | agent builder (sandboxes, services, sessions) | Tutorial + How-to |
| Operators | self-hoster | How-to |
| Concepts | understanding-seeker | Explanation |
| Reference | looker-upper | Reference |

**One Diátaxis mode per page.** A page that teaches doesn't enumerate flags;
a reference page doesn't tutor. Split mixed pages instead of blending.
`concepts/architecture/` is the one public place that cites real crate and
module paths inline.

## Hard rules

1. **Never claim "open source."** Nimbus is **source-available**
   (`LICENSING.md`). The check-docs gate rejects the phrase.
2. **`docs/private/` is never published.** No public page, sidebar entry, or
   link may reference it. The build fence and `scripts/check-docs.sh`
   enforce this.
3. **Every behavior claim is source-verified.** Verify against the code
   before writing; record the rows in `docs/source-map.md`. If you can't
   verify a claim, drop it — never fabricate, never copy staged prose on
   trust. State caveats honestly (config-gated, embedding-API-only,
   not-yet-wired).
4. **Markdown, not HTML.** `docs/` is plain Markdown that reads correctly on
   GitHub. No inline HTML, no MDX components, no frontmatter beyond `title`,
   `description` and `sidebar`. Anything richer belongs in the TSX landing.
5. **Strip internal plumbing.** No `make verify-*` commands, plan links,
   test-lane tables, or contributor workflow in public pages.

## Messaging canon

- Canonical sentence (repo description, README banner, docs hero — all
  identical): *The single-binary backend for apps and AI agents. Drop-in
  compatible with Convex, Firestore, MongoDB, and DynamoDB.*
- Spoken hook: *BaaS in a binary.*
- Division of labor: README = orientation + 30-second run + status-honest
  scope + handoff to nimbusdocs.com. Docs landing = hero pitch +
  proof-by-code + routing. Docs body = everything else. No surface
  duplicates another's content.
- Landing leads with `nimbus dev` — the command is the demo. The install
  step goes beneath it, never as the opener.
- No self-referential praise: never call our own docs or tables "honest"
  ("an honest capability table", "tells that story honestly"). Be honest;
  don't say it.
- Short declarative sentences. Headings carry the claim so the page skims
  without a card grid.

## Verification

```bash
npm --prefix website run build   # writes website/out/ with the three llms files
bash scripts/check-docs.sh       # dead links + source-map resolution + private fence
bash scripts/verify-nimbus-docs-site.sh   # full plan gate (17 conditions)
```

CI: `.github/workflows/docs.yml` builds + gates every PR touching the six
groups or `website/`, comments a preview URL on PRs, and deploys `main` to
Cloudflare Workers at nimbusdocs.com. The build writes `website/out/`, which
is what `website/wrangler.jsonc` serves. `llms-small.txt` excludes
(`concepts/architecture/**`, per-protocol reference matrices) are tuned in
`SMALL_EXCLUDES` in `website/src/lib/llms.ts` — everything stays in
`llms-full.txt`.
