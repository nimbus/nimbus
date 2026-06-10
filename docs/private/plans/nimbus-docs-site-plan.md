# Plan: Nimbus Documentation Site (nimbusdocs.com)

## Status

- **Status:** `in_progress` — DOC0..DOC5 complete (DOC3 shipped the IA
  shell + landing + Get started live at nimbusdocs.com on 2026-06-10;
  DOC4 shipped the full Developers + adapter Reference corpus; DOC5
  shipped the Operators group + tenancy concepts + configuration/deploy
  reference seeds — 62 built pages, same day). Next: DOC6 (Concepts +
  Reference source-grounded). Verifier: 12/17 (remaining reds are
  DOC7/8/11/12 conditions 10, 11, 12, 15, 17).
- **Primary goal:** ship the canonical public documentation site for Nimbus at
  **`nimbusdocs.com`**. Astro 6 + Starlight, Markdown authored in-repo in the
  public top-level groups of `docs/` as the source of truth, a hybrid persona ×
  Diátaxis information architecture (**Get started · Developers · Operators ·
  Concepts · Reference**) mirroring [`DESIGN.md`](../../DESIGN.md)'s two
  ratified personas, `llms.txt` emitted as a build artifact, deployed to
  **Cloudflare Workers (Static Assets)** via GitHub Actions with per-PR preview
  deployments. The plan also owns the full `docs/` cleanup (all internal
  working state consolidates under `docs/private/`, the single never-published
  home) and the public, source-verified architecture rewrite
  (`concepts/architecture/`, grounded in a system-by-system codebase review).
  The existing apt repository stays on this repo's single GitHub Pages site,
  untouched.
- **Owner:** jackspirou
- **Verifier:** `bash scripts/verify-nimbus-docs-site.sh` (17 conditions; DOC0
  creates it). `/goal` control plane gates on this script.
- **Activation prerequisites (owner-provided, gate the deploy phases only —
  DOC0..DOC8 can proceed without them):**
  - `nimbusdocs.com` registered and its DNS zone added to a Nimbus Cloudflare
    account. The domain is swappable later (≈2 lines of config) if `nimbus.dev`
    / `nimbus.ai` are acquired.
  - A Cloudflare account id + a scoped API token (**Workers Scripts: Edit** +
    the `nimbusdocs.com` zone for the custom domain) stored as GitHub Actions
    secrets `CLOUDFLARE_ACCOUNT_ID` and `CLOUDFLARE_API_TOKEN`.
  - ~~Confirmation of the `docs/architecture/*` public scope~~ — **resolved
    2026-06-10:** architecture documentation is public, rewritten
    system-by-system in DOC7; generated evidence / validation logs / fork
    ledgers stay in `docs/private/`.

## Context and First Principles

This is the first-principles design captured from the 2026-06-10 design session,
expanded with external IA research and a source-grounded audit.

1. **Two corpora, two audiences.** Today `docs/` holds **642** Markdown files,
   but ~520 of them (`plans/` alone is **498**, plus `private/`, `decisions/`,
   `design-review/`, `code-review/`, `prompts/`, `technical-debt.md`) are
   *agent + contributor working state* and must **never** be published. Worse,
   the audit found that even within the ~123 "public-candidate" files, **~40%
   are actually internal** (CI/build/fork contracts, generated compatibility
   manifests, ADR-style `*-contract.md`, architecture deep-dives self-labeled
   "for contributors"). The fix is structural: all internal working state
   consolidates under **`docs/private/`** — the single never-published home —
   so the rest of `docs/` *is* the public tree, and the publish boundary is one
   directory plus an explicit loader allow-list instead of a scattered
   deny-list.
2. **Markdown is the source of truth; HTML and `llms.txt` are build artifacts.**
   The model edits Markdown (diffs cleanly in PRs, is also the machine-readable
   format); a deterministic build produces HTML + `llms.txt`. We never
   hand-author or LLM-author HTML as source. (Reference architecture: crabbox
   keeps Markdown source + a zero-dep build script + a `source-map.md`; we use
   Starlight for batteries-included search/highlighting/nav since our corpus is
   large.)
3. **Mirror the product personas; structure by Diátaxis.**
   [`DESIGN.md:80-138`](../../DESIGN.md) defines two ratified personas —
   **Developer** (`/developer/*`, tenant-scoped app author) and **Operator**
   (`/operator/*`, server-wide self-hoster/admin) — backed by
   `packages/nimbus-ui/src/routes/{developer,operator}/`. The docs reconcile
   persona with the **Diátaxis** framework (Tutorials, How-to, Reference,
   Explanation) using the hybrid model below.
4. **Cloudflare removes the Pages collision.** `apt-repo.yml` already owns this
   repo's single GitHub Pages site. Deploying docs to Cloudflare (not GitHub
   Pages) lets docs live **in this repo** (GitHub-searchable, same-PR workflow)
   while apt keeps its GitHub Pages — no conflict.
5. **Cloudflare target is Workers Static Assets, not Pages.** As of 2026 both
   Astro's deploy guide and Cloudflare recommend Workers for new projects; Pages
   is in maintenance mode. A static Starlight site needs **no adapter**
   (`output: 'static'`), served via Workers Static Assets.

## Decisions (locked 2026-06-10)

| Decision | Choice |
| --- | --- |
| SSG engine | Astro 6 + Starlight (fits the npm monorepo; Pagefind search, Shiki highlighting, a11y) |
| Host | Cloudflare Workers (Static Assets) via GitHub Actions |
| Domain | `nimbusdocs.com` (apex; `www.` 301s to apex); swappable later |
| Repo location | In `nimbus/nimbus` (docs in-repo, GitHub-searchable); apt untouched on GitHub Pages |
| IA model | **Hybrid persona × Diátaxis (model Z)**: top nav **Get started · Developers · Operators · Concepts · Reference**; Tutorials + How-to are persona-split, Reference + Concepts are shared |
| Persona labels | **Developers / Operators** (mirror the product console). Verb labels "Develop"/"Operate" (CockroachDB/Temporal style) are the documented alternative if action-framing is later preferred over product-mirroring |
| v1 scope | Full port (~120 public-candidate pages) **plus** authoring the missing Tutorials/How-to/Reference the audit identified; curated and editorially passed |
| PR previews | Yes — per-PR preview URL via `wrangler versions upload` |
| Theme | Decided by the DOC2 design-harmonization review (frontend-design skill over Starlight defaults + crabbox + `DESIGN.md`), feeding learnings back into `DESIGN.md` |
| `llms.txt` | `starlight-llms-txt` → `llms.txt` / `llms-full.txt` / `llms-small.txt` |
| Agent files | Keep `CLAUDE.md → AGENTS.md` symlink; migrate `.claude/skills` → tool-agnostic `.agents/skills`; add a `docs` skill |
| Front-door stack | **One sentence, three surfaces** (the PocketBase discipline): repo description, README banner, and docs-landing hero carry the same message. Docs landing = Starlight `template: splash` hero doubling as the marketing front door (Biome/Knip pattern) until a dedicated marketing site exists |
| Repo metadata | Rewrite the GitHub description (compatibility-led, no emoji, no "open source" claim — Nimbus is **source-available** per `LICENSING.md`); set homepage → `nimbusdocs.com` once live (Convex precedent: repo homepage points at docs); add 10-20 topics; custom social-preview image |
| README | Refactor to the PocketBase/Supabase hybrid (~150-250 lines): orientation + 30-second quickstart + status-honest protocol table + bold docs handoff. The misplaced Reference content (Node-compat contract, resource-noun spec prose) moves into the docs site |
| `docs/` layout | Five public groups at `docs/` top level (`get-started/ developers/ operators/ concepts/ reference/`) + **`docs/private/` as the single internal home** (absorbs `plans/` with its security/prompts subtrees, `prompts/`, `decisions/`, code-review + design-review, `technical-debt.md`, generated evidence, agent working docs). Replaces the earlier `docs/public/` carve-out — the tree itself becomes the public corpus |
| Architecture docs | **Public** (decided 2026-06-10, reversing the internal-only default): rewritten system-by-system in DOC7 from a source-grounded codebase review into `concepts/architecture/`; the known `ARCHITECTURE.md` doc-drift gets fixed in the same pass; generated evidence, validation logs, proof harnesses, and fork ledgers stay in `docs/private/` |

## Architecture

```
nimbus/nimbus  (one repo)
├── AGENTS.md                       CLAUDE.md → symlink (unchanged)
├── .agents/skills/                 migrated from .claude/skills; + docs skill
├── docs/                           ← the public tree (Markdown source of truth)
│   ├── get-started/  developers/  operators/  concepts/  reference/
│   │                               concepts/architecture/ = the public,
│   │                               source-verified system-by-system rewrite
│   ├── brand/                      logo + brand assets (imported by the site)
│   ├── source-map.md               NEW — behavior → implementing files
│   ├── README.md                   docs index for the public tree
│   └── private/                    ← the SINGLE internal home, never published:
│                                    plans/ (≈500 files incl. archive/ proof/
│                                    research/ security/ prompts/ stories/),
│                                    prompts/, decisions/, reviews/ (code- +
│                                    design-review), technical-debt.md,
│                                    generated compat evidence, fork ledgers,
│                                    CI/build contracts, agent working docs
├── website/                        Astro 6 + Starlight (renderer only; standalone
│                                   npm project with its own lockfile — NOT a
│                                   workspace member, so the docs toolchain never
│                                   enters the product's --workspaces CI lanes)
│   ├── astro.config.mjs            site=https://nimbusdocs.com; loads ../docs/public
│   │                               via Content Layer glob loader; starlight-llms-txt
│   └── wrangler.jsonc              assets.directory=./dist; no "main"
├── scripts/
│   ├── verify-nimbus-docs-site.sh  NEW — 13-condition control-plane gate
│   └── check-docs.sh               NEW — dead-link + source-map + fence checks
└── .github/workflows/
    ├── apt-repo.yml                → GitHub Pages (apt)        [unchanged]
    └── docs.yml                    → Cloudflare Workers (nimbusdocs.com)  [new]
```

Content authoring stays in `docs/`; `website/` only renders it (Astro Content
Layer `glob()` loaders pointed at exactly the five public groups —
`../docs/{get-started,developers,operators,concepts,reference}` — an explicit
allow-list, so nothing else under `docs/` can publish by accident). This keeps
a single in-repo docs tree under `source-map.md` + `check-docs` coverage and
decouples content from the renderer.

## Information Architecture

Research into two-persona products (CockroachDB, Temporal, GitLab, ArgoCD,
Keycloak) and Diátaxis exemplars (Django — Procida's reference implementation;
Kubernetes; HashiCorp Vault) converges on one model — **hybrid (persona ×
Diátaxis), a.k.a. model Z**:

- **Split the action-oriented modes (Tutorials, How-to) by persona.** A
  developer never wants to wade through machine/networking how-tos; an operator
  never wants function-authoring lessons.
- **Share the cognition-oriented modes (Reference, Explanation/Concepts).** The
  SDK/CLI/HTTP API, config, and the data/tenancy/security models are facts both
  personas consult. Forking them per persona violates Diátaxis's "reference
  mirrors the product" rule and guarantees drift.

This is what every strong two-persona exemplar does in practice (CockroachDB
*Develop*/*Deploy* + shared *Reference*; Temporal *Develop*/*Production
deployment* + shared *References*; GitLab *Use*/*Administer*). Pure
persona-as-filter (one flat tree tagged by persona) has no successful exemplar;
pure Diátaxis-first only works for single-persona products. ArgoCD's explicit
User Guide / Operator Manual / Developer Guide validates the persona split but is
*not* Diátaxis (it mixes modes inside each guide — the anti-pattern we avoid).

### Top-level navigation — five Starlight sidebar groups

| Group | Persona | Diátaxis mode(s) | Mirrors |
| --- | --- | --- | --- |
| **Get started** | shared | Tutorial (entry) | — |
| **Developers** | Developer | Tutorials + How-to | console `/developer` |
| **Operators** | Operator | Tutorials + How-to | console `/operator` |
| **Concepts** | shared | Explanation | — |
| **Reference** | shared | Reference | the code / CLI / API surface |

**Get started** carries two side-by-side quickstarts — a *developer quickstart*
(build an app; framework-selectable; Convex-recognizable, since many users
arrive from Convex) and a *self-host quickstart* (one-command Docker/Compose
bring-up) — plus "What is Nimbus" and a "coming from Convex" on-ramp. Neither
quickstart is buried inside a persona branch (an anti-pattern in Appwrite's IA).
The persona labels stay **Developers / Operators** so each console view can
deep-link into its half of the docs.

### Section contents (mapped from the source surface)

**Developers** (Tutorials + How-to), grouped by Convex-recognizable features:
Functions (queries / mutations / actions / HTTP handlers), Database & schema
(`defineSchema`/`v`), Scheduling & crons, File storage, Auth (wire an IdP),
Realtime/subscriptions (`onUpdate`), Clients & SDK (**control-plane**
`Nimbus.services/sandboxes/sessions` vs **data-plane**
`NimbusClient.query/mutation/action` — kept explicitly distinct; the audit
flagged these as easy to conflate), Adapters (Convex / Firebase / Cloud
Functions / MongoDB / DynamoDB compatibility + migration), Node runtime
(`"use node"`, packages, bundling), Testing & local dev (`nimbus dev`, codegen).

**Operators** (Tutorials + How-to): Install & deploy (`nimbus node`, container
image, Compose/Quadlet export), `nimbus deploy` stage/diff/activate, Tenants
(`/api/tenants`, `nimbus policy` validate/diff/prove), Machines (`nimbus machine`
lifecycle, OS apply/upgrade/rollback), Networking (loopback default,
`--allow-network`, CORS/origin allowlist, systemd socket), Storage backends
(SQLite/Postgres/MySQL/libSQL/redb topology + pools), Encryption at rest
(`nimbus encryption` migrate/export/rotate-kek/rotate-dek), Security &
multi-tenancy (isolation model, hardening checklist), Observability (`/debug/*`
metrics, health), Backup / restore / PITR, Upgrades & migration, Troubleshooting.

**Concepts** (Explanation) — two sub-groups:

- *How Nimbus works* (user concepts, ~8-10 pages): short overview distilled
  from `ARCHITECTURE.md`, data & mutation model, tenancy & isolation model,
  runtime & adapter boundary, runtime permission model, resource model
  (services/sandboxes/sessions), Convex compatibility scope (what matches /
  what differs), how Nimbus scales.
- *Architecture* (`concepts/architecture/` — **public**, rewritten
  system-by-system in DOC7 from a source-grounded codebase review; NOT a copy
  of the 52-file internal tree): server & transport, adapters & listeners,
  engine & the single mutation path, runtime & isolates, storage (backends,
  atomicity, encryption, scheduler), sandbox & machines, auth & trust
  boundary, tenancy, node lifecycle, CLI & codegen, SDK & packages,
  observability. Generated evidence, validation logs, proof harnesses, and
  fork ledgers stay in `docs/private/`.

**Reference** (Reference — mirrors the code, one canonical page per fact): CLI
(full command tree — 16 commands; `start`'s ~50 flags — tracked against
`crates/nimbus-bin/src/main.rs`), Configuration & env-var cross-reference
(flag ↔ `NIMBUS_*` ↔ config key, from `start/config.rs`), SDK API (control-plane
+ data-plane + server builders + validators, from `packages/nimbus/src/*`),
Native HTTP & WebSocket API + error catalog (from
`crates/nimbus-server/src/router.rs`), per-adapter compatibility matrices,
Deploy/admin API, Node compatibility (the supported floor + targets — verify the
actual number against `REQUIRED_NODE_MAJOR_VERSION`, currently 22; do not
restate the test-fixture lanes as user-selectable versions), current-capabilities,
releases/changelog.

### Diátaxis refactor rules

1. **No mode-mixing.** Every current front-door page illegally fuses modes —
   `README.md`, `docs/getting-started.md`, all five adapter READMEs, and
   `operating/storage-backends.md` each blend Tutorial + How-to + Reference.
   Split each into its quadrants; these highest-traffic pages go first.
2. **Reference mirrors the product.** One canonical page per fact, tracking the
   source. Kill the duplication the audit found (Convex quickstart in 3 files;
   the Node-compat contract in 4; `runtimes/nodejs/index.md` duplicates its own
   README; tenant-isolation split across 3 files).
3. **Generalize the one good example.** `docs/runtimes/nodejs/**` already
   separates fundamentals (Explanation) / configuration+packages (How-to) /
   compatibility (Reference). Use it as the template for every feature area.
4. **One internal home: `docs/private/` (fail-closed).** Everything that is
   working state or contributor-only consolidates under `docs/private/`:
   - `plans/` (with its archive/ proof/ research/ security/ prompts/ stories/
     subtrees), top-level `prompts/`, `decisions/`, code-review +
     design-review (as `reviews/`), `technical-debt.md`, and agent working
     documents;
   - the generated `node-lts-compat/**` evidence (~20 manifests / failure
     inventories), krun validation logs, fork ledgers;
   - the CI/build/local-dev contracts (`ci-caching`, `ci-modernization`,
     `ci-pr-wall`, `local-dev`, `deno-fork-workflow`, `fork-health`,
     `node-dbus-binding`, `multi-backend-adapter-hardening`);
   - ADR-style `*-contract.md` files and upstream test catalogs / suites.
   The old architecture deep-dives are **not** moved verbatim to public: DOC7
   *rewrites* them into `concepts/architecture/`, and superseded originals
   retire into `docs/private/` or are deleted. `check-docs.sh` fails if any
   published page links into `docs/private/**` or embeds `make verify-*` /
   `cargo test` plumbing.

### Content gap backlog (author-new, source-grounded)

The corpus is ~0% Tutorial, ~11% How-to, and its Reference is mostly internal —
so most of the user-facing surface is **missing**, not just mis-filed. Author
these (they did not exist before; each is grounded in the source surface):

- **Tutorials** (zero exist today): developer first-app (Convex), "use stock
  MongoDB drivers against Nimbus", operator "deploy Nimbus to a Linux server".
- **DynamoDB front door** — the adapter has 6 reference/internal files but no
  README / quickstart / migration; give it parity with the other adapters.
- **Operator production deploy guide** — end-to-end (bind hardening, admin-token
  rotation *before* public binds, reverse proxy/TLS, systemd); only fragments
  exist in `container-image.md` / `node-lifecycle.md`.
- **Auth / IdP setup how-to** — wiring OIDC/custom-JWT (`auth.config`,
  `ctx.auth.getUserIdentity()`); only a contract exists today.
- **Backup / restore / disaster recovery** — PITR/CDC are mentioned but have no
  operator how-to.
- **Convex → Nimbus migration** — the flagship adapter has no migration guide
  (Firebase and Cloud Functions do).
- **Observability how-to** — scraping / alerting on `/debug/*` metrics.
- **Consolidated configuration & env-var reference** — flags/env are scattered
  across `cli.md` + every adapter README; one canonical cross-reference.
- **Troubleshooting / FAQ** — beyond the native `errors.md` code catalog.
- **Short "How Nimbus works" Concepts entry** — `ARCHITECTURE.md` (1497 lines,
  partly contributor-aimed) is not a new-user on-ramp.

The full file-by-file migration map (≈123 files → keep / move / refactor / split
/ keep-internal, with target paths) is produced in DOC3 and is the checklist
DOC4..DOC6 execute against.

## Repo Front Door and Messaging

Research across PocketBase (the canonical single-binary BaaS peer), Caddy,
Meilisearch, Litestream, NATS, Supabase, Appwrite, Convex, and the
Starlight-splash marketing landings (Biome, Knip, Cloudflare docs) yields one
governing rule and a division of labor:

**One sentence, three surfaces.** PocketBase's repo description, README banner,
and site hero are the same sentence ("Open Source realtime backend in 1 file").
Nimbus's canon today is fragmented: the README banner ("**BaaS in a binary. For
apps and agents.**") is strong, but the GitHub description ("Cloud for modern
apps & agents. 🤖 Baas in a binary. ☁️") miscapitalizes BaaS, leads with the
generic "Cloud" metaphor that contradicts the single-binary identity, and omits
the one claim no peer can make — wire compatibility with four ecosystems. Align
description → README, not the reverse.

- **Canonical description (lead candidate):** `The single-binary backend for
  apps and AI agents. Drop-in compatible with Convex, Firestore, MongoDB, and
  DynamoDB.` Compatibility-as-identity is the MinIO move ("S3 compatible object
  store") with four logos behind it. No emoji (zero of the top-tier peer
  descriptions use any). **No "open source" claim** — Nimbus is source-available
  (`LICENSING.md`); the honest term avoids the credibility backlash that claim
  would invite. "BaaS in a binary" stays as the spoken hook / README banner
  sub-line.
- **Repo metadata (currently free wins):** homepage URL is empty → set to
  `https://nimbusdocs.com` once DOC9 lands (Convex points its repo homepage at
  docs.convex.dev, not the marketing site — the right move when docs are the
  front door). Topics are null → add ≈ `baas, backend, self-hosted,
  single-binary, realtime, firebase-alternative, convex, firestore, mongodb,
  dynamodb, cloud-functions, rust, ai-agents, serverless`
  (alternative-positioning via topics is standard practice — Supabase and
  Appwrite both carry `firebase`). Add a custom social-preview image (logo +
  canonical sentence + the protocol marks).
- **README refactor (PocketBase/Supabase hybrid, ~150-250 lines):**
  1. centered dark/light logo banner → 2. 3-5 badges (CI, release, license,
  community — drop codecov as vanity and the beta badge as duplicate of the
  WARNING callout) → 3. link strip (`Docs · Quickstart · Discussions`) →
  4. one-paragraph definition + bold-keyword capability bullets naming the
  protocols, with the Convex-style agent line ("for apps and AI agents —
  whether the developer is human or LLM") → 5. **bold docs handoff** ("For
  documentation and examples, visit https://nimbusdocs.com" — PocketBase
  verbatim pattern) → 6. the existing `> [!WARNING]` beta callout (keep;
  honesty is a feature pre-launch) → 7. proof-by-code: one block showing two
  different clients (e.g. Convex + MongoDB driver) hitting the *same* running
  binary — the multi-protocol claim proven in ~15 lines, agent-readable →
  8. 30-second quickstart (install → run → first request) → 9. **protocol
  compatibility status table** (Supabase's `[x]` checklist adapted: rows =
  Convex / Firestore / Cloud Functions / MongoDB / DynamoDB / native SDK;
  columns = status + docs link) — the status-honest scope statement →
  10. "built for agents" block (llms.txt, AGENTS.md, MCP-server roadmap line) →
  11. community / security / licensing.
  **Moves out:** the ~85-line "Node compatibility contract" Reference block
  (→ docs Reference; it is one of the four duplicated copies), the
  resource-noun spec paragraph (→ Concepts resource-model page), the deep
  install matrix (→ docs; keep Homebrew + one-liner). The ASCII architecture
  diagram may stay (terminal-aesthetic, agent-readable, on-brand) but gets a
  staleness pass (it currently omits the DynamoDB adapter).
- **Docs landing (Starlight splash as interim marketing front door):**
  `template: splash`; `hero:` with a benefit-led title (Biome's pattern — not
  the product name), the canonical sentence as tagline, dark/light brand image,
  actions `Get started` (primary → developer quickstart) + `View on GitHub`
  (minimal); `banner:` slot reserved for the launch announcement. Below the
  hero: `<Tabs>` code demo (native SDK / Convex / Firebase / MongoDB / DynamoDB
  against one binary — PocketBase's JS/Dart tabs extended to Nimbus's actual
  differentiator), `<CardGrid stagger>` of 4-6 value props, per-adapter doc
  entry-point cards (Cloudflare's product-directory pattern), community footer.
  Logo walls / testimonials wait for real users. OG images configured sitewide.
- **Division of labor (no surface does another's job):** **README** =
  orientation, 30-second run, status-honest scope, handoff. **Docs landing** =
  hero pitch, proof-by-code, routing. **Docs body** = everything else. Every
  surface repeats the same one sentence; none duplicates another's content
  (the Appwrite failure mode vs the PocketBase success mode).
- **Root markdown posture (already healthy):** `CONTRIBUTING.md`, `SECURITY.md`,
  `LICENSING.md`, `COMMERCIAL.md`, `TRADEMARKS.md`, `CHANGELOG.md` exist and
  stay root-level; `ARCHITECTURE.md` remains the contributor deep-dive that
  Concepts distills; `DESIGN.md` gains the docs surface (DOC2) and records the
  canonical sentence in its brand section so all three surfaces have one
  source of truth.

## Execution Order (DOC0..DOC13)

Each phase has a completion gate. DOC0..DOC8 need no Cloudflare access; DOC9+
consume the Activation prerequisites.

- **DOC0 — Control plane.** Create `scripts/verify-nimbus-docs-site.sh` (the 17
  conditions in Verification) and register this plan in
  `docs/plans/README.md`. *Gate:* verifier runs and reports each condition
  (red until satisfied).
- **DOC1 — Scaffold renderer.** Add `website/` as a standalone npm project
  (own lockfile, not a workspace member): Astro 6 + `@astrojs/starlight` +
  `starlight-llms-txt`; `astro.config.mjs` with
  `site: 'https://nimbusdocs.com'` and default static output. *Gate:*
  `npm --prefix website run build` succeeds against a single placeholder page
  and emits `dist/` + `llms.txt` (the five-group glob loaders land in DOC3).
- **DOC2 — Design harmonization (frontend-design skill).** Review Starlight's
  default theming recommendations, crabbox's styling, and `DESIGN.md`'s two-tier
  brand system (`brand-system-plan.md`: Brand tier vs Industrial Precision
  Product tier). Decide which tier (or blend) the docs site adopts — working
  hypothesis: Brand tier for landing/hero, Product-tier discipline for the dense
  doc body. Produce concrete Starlight theme tokens and a `DESIGN.md` update that
  adds the docs surface to the brand system, records the **canonical one-sentence
  message** (see Repo Front Door and Messaging) in the brand section, specifies
  the splash-landing hero direction, and fixes any product↔docs inconsistencies
  surfaced. *Gate:* `DESIGN.md` updated with a "Documentation site" surface entry
  + messaging canon; theme tokens committed in `website/`.
- **DOC3 — Restructure `docs/` + IA shell + landing + migration map.** Make the
  tree its final shape in one atomic wave: create
  `docs/{get-started,developers,operators,concepts,reference}/` with landing
  pages and the five-group Starlight nav; move all internal working state under
  `docs/private/` (`plans/` → `private/plans/`, `prompts/` →
  `private/prompts/`, `decisions/` → `private/decisions/`, `code-review/` +
  `design-review/` → `private/reviews/`, `technical-debt.md` →
  `private/technical-debt.md`); run the repo-wide reference-fixup sweep
  (`AGENTS.md` routing, `scripts/verify-*.sh`, `.github/workflows/*`,
  cross-plan links). Create the `docs/source-map.md` skeleton (populated by
  DOC4..DOC7, validated by DOC8). Build the **splash landing page** per the
  Repo Front Door spec (hero + `<Tabs>` multi-protocol code demo + value-prop
  cards + adapter entry-point cards). Land the **Get started** group (developer
  quickstart + self-host quickstart + "What is Nimbus" + "coming from Convex"),
  de-duplicating the three copies of the Convex quickstart and four copies of
  the Node-compat contract into one home each. Write the file-by-file migration
  map (≈123 public-candidate files → keep / move / refactor / split / private)
  that DOC4..DOC7 execute against. Implementation notes: not-yet-migrated
  public-candidate trees stage under `docs/private/staging/` until DOC4..DOC7
  publish or retire them, and the loader allow-list is implemented as five
  group symlinks under `website/src/content/docs/` resolved through
  Starlight's `docsLoader()` (the only real file there is the splash
  landing). *Gate:* `docs/` top level is exactly the
  five groups + `brand/` + `private/` + `README.md` + `source-map.md`; no file
  outside `docs/private/` references an old internal path; nav renders all five
  groups; the landing uses `template: splash` with the canonical message; both
  quickstarts work end-to-end; the migration map exists.
- **DOC4 — Developers: migrate + author.** Split the mixed adapter READMEs into
  Tutorial / How-to (their Reference content moves to the shared Reference
  group); migrate `runtimes/nodejs` (already Diátaxis-shaped) and generalize its
  pattern. Author the missing developer content: first-app tutorial (Convex),
  MongoDB-drivers tutorial, the DynamoDB front door, the Auth/IdP how-to, the
  Convex→Nimbus migration guide, and the control-plane-vs-data-plane SDK
  guidance. *Gate:* every Developers page is single-mode; tutorials exist and
  are runnable end-to-end; DynamoDB has parity with the other adapters; no
  dead/internal links.
- **DOC5 — Operators: migrate + author.** Migrate the operator runbooks
  (`node-lifecycle`, `container-image`, `storage-backends`, `encryption`,
  `updates`, the `tenant-isolation` runbook, `desktop-install`), stripping
  `make verify-*` / `cargo test` / `docs/plans/**` developer plumbing. Author
  the missing operator content: the end-to-end "deploy to a Linux server"
  tutorial, backup/restore/PITR how-to, observability how-to (`/debug/*`),
  the security hardening checklist, and troubleshooting/FAQ. *Gate:* every
  Operators page is single-mode and free of dev/CI plumbing; the deploy tutorial
  is end-to-end; no dead/internal links.
- **DOC6 — Concepts + Reference (source-grounded).** Curate ~6-10 Concepts
  (Explanation) pages from the architecture tree — How Nimbus works, data &
  mutation model, tenancy & isolation, runtime/adapter boundary, runtime
  permissions, resource model, Convex-compat scope, scaling — NOT a wholesale
  copy. Build the shared Reference: the full CLI command tree (16 commands;
  `start`'s ~50 flags) and a flag ↔ `NIMBUS_*` ↔ config-key cross-reference
  derived from `crates/nimbus-bin/src/main.rs` + `start/config.rs`; the SDK API
  reference (control-plane + data-plane + server builders + validators) from
  `packages/nimbus/src/*`; the native HTTP/WS API + error catalog from
  `crates/nimbus-server/src/router.rs`; per-adapter compatibility matrices; the
  deploy/admin API; Node-compat evidence; current-capabilities. *Gate:* CLI
  reference covers every command in `main.rs`; SDK reference covers every public
  export; Concepts links resolve to public pages only.
- **DOC7 — Architecture rewrite (system-by-system).** Review the codebase
  section by section, system by system, subsystem by subsystem, and rewrite the
  architecture documentation as public, source-verified Explanation under
  `concepts/architecture/`. Fix the known `ARCHITECTURE.md` doc-drift (2026-06
  codebase-review finding) in the same pass. **System manifest** (one page
  each; every load-bearing claim grounded in current source and recorded in
  `docs/source-map.md`): server & HTTP/WS transport (`nimbus-server`),
  adapters & listeners (convex / firebase / cloud-functions / mongodb /
  dynamodb crates), engine & the single mutation path (`nimbus-engine`),
  runtime & isolates (`nimbus-runtime`, V8/deno fork, Node compat, bundle
  integrity), storage (`nimbus-storage`: backends, atomicity invariant,
  encryption-at-rest, scheduler), sandbox & machines (`nimbus-sandbox`,
  `nimbus-machine`, microVM/krun model), auth & trust boundary (`nimbus-auth`,
  capability/grant model), tenancy (`nimbus-tenant`, isolation modes), node
  lifecycle (`nimbus-node`), CLI & codegen (`nimbus-bin`, embedded packages),
  SDK & packages (`packages/nimbus`, compat wrappers, codegen, `nimbus-ui`),
  observability & diagnostics (`/debug/*`, metrics). Root `ARCHITECTURE.md`
  shrinks to a concise contributor map linking into the public pages;
  superseded internal architecture files retire into `docs/private/` or are
  deleted. *Gate:* every manifest system has a page; every page cites real
  source paths with `source-map.md` entries; zero claims contradict current
  source (spot-verified per page).
- **DOC8 — Machine corpus + honesty gates.** Wire `starlight-llms-txt`
  (`projectName: 'Nimbus'`, `exclude` for `llms-small.txt`); finalize
  `scripts/check-docs.sh` (dead-link + source-map claim resolution + the
  `docs/private/` fence guard) over the `source-map.md` entries accumulated
  since DOC3. *Gate:* `llms.txt`, `llms-full.txt`, `llms-small.txt` emitted;
  `check-docs.sh` passes.
- **DOC9 — GitHub Actions pipeline.** Add `.github/workflows/docs.yml`:
  triggers on `push: main` and `pull_request` with paths = the five public
  `docs/` groups + `docs/brand/**` + `website/**` + the workflow itself
  (deliberately NOT `docs/private/**` — plan edits must not trigger site
  builds) + `workflow_dispatch`; Node 22; `npm ci` → `astro build` →
  `check-docs` on PRs; `cloudflare/wrangler-action` uploads a **preview
  version** per PR and a bot comments the URL; merges to `main` run
  `wrangler deploy`. `concurrency` group enforces single-flight. *Gate:*
  workflow valid (`actionlint`); a test PR gets a preview URL comment.
- **DOC10 — Cloudflare custom domain.** `website/wrangler.jsonc` with
  `assets.directory: ./dist` and no `main`; attach `nimbusdocs.com` as a Workers
  Custom Domain (apex), `www.` 301 → apex; Universal SSL. *Gate:* `nimbusdocs.com`
  serves the site over HTTPS; `nimbusdocs.com/llms.txt` resolves.
- **DOC11 — Repo front door.** Execute the Repo Front Door spec now that the
  site is live: refactor `README.md` to the PocketBase/Supabase hybrid outline
  (docs handoff to `nimbusdocs.com`, proof-by-code block, protocol status
  table, agent block; Node-compat contract and resource-noun spec prose moved
  into the site; badge prune; ASCII-diagram staleness pass adding DynamoDB);
  update the GitHub repo description to the canonical sentence, set homepage →
  `https://nimbusdocs.com`, add topics, upload the social-preview image.
  *Gate:* README contains the `nimbusdocs.com` handoff + protocol status table
  and no longer contains the Node-compat contract block; `gh repo view` shows
  the new description, homepage, and topics.
- **DOC12 — Agent workflow.** Migrate `.claude/skills` → `.agents/skills`; add a
  `.agents/skills/docs` skill encoding the hybrid IA, the Diátaxis rules, the
  `docs/private/` fence, the messaging canon, and "Markdown not HTML"; add an
  AGENTS.md routing entry for docs-site work. *Gate:* skill loads; AGENTS.md
  routes a docs task to it.
- **DOC13 — Closeout.** Full editorial pass, Lighthouse/a11y check, link check,
  `current-capabilities` truth pass, Diátaxis-purity review (no mixed-mode
  pages). Flip plan status to `done`; archive on completion. *Gate:* verifier
  17/17; Lighthouse a11y ≥ 95; zero dead links; zero mixed-mode pages.

## Verification

`bash scripts/verify-nimbus-docs-site.sh` checks (each prints PASS/FAIL):

1. `website/package.json` declares `astro`, `@astrojs/starlight`,
   `starlight-llms-txt`.
2. `website/astro.config.mjs` sets `site: 'https://nimbusdocs.com'`, default
   static output, and the `starlight-llms-txt` plugin.
3. Starlight content is loaded only from the five public groups
   `docs/{get-started,developers,operators,concepts,reference}/**` — the
   allow-list is five group symlinks under `website/src/content/docs/`
   (verified by readlink target), with the splash landing index as the only
   real file there.
4. `docs/{get-started,developers,operators,concepts,reference}/` each have a
   landing page.
5. `docs/` top level contains only the five public groups plus `brand/`,
   `private/`, `README.md`, and `source-map.md`; no published page links into
   `docs/private/**` — fail-closed leak guard.
6. `npm --prefix website run build` succeeds and emits `dist/`, `llms.txt`,
   `llms-full.txt`, `llms-small.txt`.
7. `website/wrangler.jsonc` sets `assets.directory: ./dist` and has no `main`.
8. `.github/workflows/docs.yml` exists with the paths filter, PR preview step,
   main deploy step, and a `concurrency` group; `actionlint` clean.
9. `docs/source-map.md` exists.
10. `scripts/check-docs.sh` exists and passes (dead links + source-map claims
    resolve to real files + fence guard).
11. `.agents/skills/docs/` exists and `.claude/skills` is migrated.
12. `AGENTS.md` has a docs-site routing entry.
13. `apt-repo.yml` remains the only GitHub Pages deployer (docs deploys to
    Cloudflare; no Pages collision).
14. The docs landing page uses `template: splash` with a `hero:` carrying the
    canonical message, and `DESIGN.md` contains the "Documentation site"
    surface entry + messaging canon.
15. `README.md` contains the `nimbusdocs.com` docs handoff and the protocol
    compatibility status table, and no longer contains the "Node compatibility
    contract" Reference block (conditions 14-15 go green at DOC2/DOC3 and
    DOC11 respectively).
16. The legacy internal locations are gone: `docs/plans/`, `docs/prompts/`,
    `docs/decisions/`, `docs/code-review/`, `docs/design-review/`, and
    `docs/technical-debt.md` no longer exist at their old paths (consolidated
    under `docs/private/`), and no file outside `docs/private/` still
    references an old `docs/plans/...`-style path.
17. `docs/concepts/architecture/` contains a page for every system in the DOC7
    manifest, and each page's load-bearing claims have `docs/source-map.md`
    entries.

Standard repo gates still apply: `npm run typecheck`, `npm run build`,
`cargo fmt --all --check` (unaffected). The docs lane is independent of the Rust
workspace (pure npm; no sccache/V8 dependency).

## Risks and Open Items

- **Authoring burden is larger than a port.** The audit showed most user-facing
  content (all Tutorials, most How-to, the consolidated Reference) does not exist
  yet. DOC4..DOC6 are the long poles and mix *migrate-and-split* with
  *author-new*; each ships per-section so the site is live and growing
  throughout.
- **Fence discipline.** ~40% of the current tree must be kept internal; the
  `check-docs` fence guard (condition 5/10) is the automated backstop against
  leaking CI/fork/ADR material or linking into it.
- **Content drift.** Mitigated by `source-map.md` + `check-docs.sh` (the crabbox
  honesty pattern) — behavior claims must resolve to real source files; the
  CLI/SDK/API references track `main.rs` / `packages/nimbus/src/*` / `router.rs`.
- **Owner inputs gate DOC9+.** Domain registration, Cloudflare account, and the
  API token are owner-provided; DOC0..DOC8 proceed in parallel.
- **Restructure blast radius.** DOC3 moves ~520 files under `docs/private/` in
  one atomic commit plus a repo-wide reference-fixup sweep (`AGENTS.md`
  routing, `scripts/verify-*.sh`, workflows, cross-plan links). In-flight
  branches and worktrees that touch `docs/plans/**` will need a rebase —
  land the move as a single commit and coordinate with active worktrees
  (storage MVCC, NDS) before merging it.
- **Architecture rewrite is a long pole.** DOC7 is a system-by-system rewrite
  grounded in source review, not a move; the 2026-06 codebase review already
  flagged `ARCHITECTURE.md` doc-drift, so the accuracy work is real. Scope is
  bounded by the DOC7 system manifest.
- **Persona-label framing (minor).** Nouns "Developers/Operators" (product
  mirror) vs verbs "Develop/Operate" (CockroachDB/Temporal IA convention) — the
  plan uses nouns; switchable if action-framing is preferred.
- **Theme tier.** Resolved by DOC2; until then the site builds on Starlight
  defaults + the Nimbus logo.
- **License-claim discipline.** Nimbus is source-available, not open source
  (`LICENSING.md`). No published surface — description, README, landing, docs —
  may claim "open source"; the landing/README say "source-available" or
  "self-hostable". The DOC12 editorial pass enforces this.
- **Marketing-copy honesty.** The splash landing is a marketing surface inside
  a docs site; its claims (protocol compatibility, status) must match the
  README status table and `current-capabilities.md`. Pre-launch, the protocol
  status table is the single source of truth for scope claims.

## References

- [`DESIGN.md`](../../DESIGN.md) — product IA + two-tier brand system (the
  personas we mirror).
- [`docs/plans/archive/brand-system-plan.md`](archive/brand-system-plan.md) —
  Brand tier vs Industrial Precision Product tier.
- **Diátaxis** (`diataxis.fr`) — the four-mode framework. Exemplars: Django
  (reference implementation), Kubernetes, HashiCorp Vault (mode-first).
- **Two-persona exemplars** (hybrid model Z): CockroachDB (Develop/Deploy +
  shared Reference), Temporal (Develop/Production deployment + shared
  References), GitLab (Use/Administer), ArgoCD (User Guide/Operator Manual —
  persona split but non-Diátaxis), Keycloak.
- **Convex docs** (`docs.convex.dev`) — the IA incoming Nimbus users expect.
- crabbox (`github.com/openclaw/crabbox`) — Markdown-source + zero-dep build +
  `source-map.md` + custom-domain reference architecture.
- **Front-door exemplars:** PocketBase (one-sentence-three-surfaces; README →
  docs handoff), Supabase (status-honest feature checklist; community table),
  Convex backend repo ("whether human or LLM" framing; repo homepage → docs),
  MinIO (compatibility-as-identity description), Litestream (minimal README +
  excellent site), Biome / Knip / Cloudflare docs (Starlight `template: splash`
  landing-as-marketing), Caddy (README as routing page).
- llms.txt standard (`llmstxt.org`); `starlight-llms-txt`
  (`github.com/delucis/starlight-llms-txt`).
- Cloudflare acquired Astro (2026-01-16); Astro + Cloudflare Workers Static
  Assets deploy guides.
- Source anchors for Reference: `crates/nimbus-bin/src/main.rs` (CLI tree),
  `crates/nimbus-bin/src/start/config.rs` (operator config),
  `crates/nimbus-server/src/router.rs` (HTTP/admin/deploy API),
  `packages/nimbus/src/{index,browser,server,values}.ts` (SDK).
