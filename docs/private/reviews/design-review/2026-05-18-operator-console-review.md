# Operator Console — Design & UX Review

**Date:** 2026-05-18
**Reviewer:** Claude (design + UX pass)
**Scope:** Both consoles end-to-end — every top-level nav entry, sub-drawer,
key tab strip, command palette, and system-tenant lens. Light pass over
detail pages. No mobile / responsive testing.
**Build under review:** `main` at `ecfc3ade` (DU-redesign CS10 archived).
**Reference docs:** `DESIGN.md` (1175 lines, single source of truth).
**Benchmarks:** Convex Dashboard, Firebase Console, Docker Desktop, Podman
Desktop.
**Environment:** fresh server on `http://127.0.0.1:8089`, 3 sample tenants
(`acme`, `playground`, `shop-prod`), no machines / services / rows.
Viewport 1440×900. Screenshots:
`docs/plans/proof/desktop-ui-design-review-fixes/before/` (16 PNGs).

> Severities: **P0** ship-blocker · **P1** correct before launch ·
> **P2** worth fixing next pass · **P3** polish.

---

## Headline

The IA is in good shape. Dual-persona Developer + Operator, the three-pane
shell, sub-drawer modes, brand/accent/link tokens, JetBrains Mono for
identifiers, state chips, ⌘K command palette, ⌘\ system tenant lens — the
bones of the design system are all wired.

What hurts the first impression is **the gap between what the screen says
and what the design system says**. Plan codes are leaking into copy,
sub-drawers and tab strips disagree about how many sections a page has,
the system tenant lens fires in places DESIGN.md says it shouldn't, and
several "lands in DU-shell OX" notes will land in front of an operator
who has no idea what DU-shell is.

None of this is structural — every finding below is a focused edit, no
re-architecture.

## Top three to fix first

1. **Strip plan codes from user-facing copy** — five places leak
   `DU-shell O2/O3/O4` or `Phase 1 · Embedded SPA`. [P0]
2. **Gate the system tenant lens (⌘\) by view** — DESIGN.md §System
   Tenant Lens makes it a Developer-side affordance only; today it opens
   in Operator view too. [P1]
3. **Reconcile sub-drawer ↔ tab-strip mismatches on Observability and
   Schedules** — they disagree about how many sections exist. [P1]

---

## Findings

### F1 — Plan codes leak into user-facing copy · P0

Several pages still embed internal phase labels (`DU-shell O2/O3/O4`,
`Phase 1 · Embedded SPA`) inside copy a developer will read. To anyone
outside this repo these read as version strings or worse, broken
feature flags.

| File | Line | Leaked text |
|------|------|-------------|
| `packages/nimbus-ui/src/shell/primary-drawer.tsx` | 59 | `Phase 1 · Embedded SPA` (bottom of primary drawer, every page) |
| `packages/nimbus-ui/src/routes/admin/index.tsx` | 13 | `System overview cards land in DU-shell O2 once the operator metrics surface is finalised.` |
| `packages/nimbus-ui/src/routes/app/files.tsx` | 35 | `Bucket list + object browser lands in DU-shell O3 once the file storage API surface is wired.` |
| `packages/nimbus-ui/src/routes/app/settings.tsx` | 49 | `Tenant settings sub-drawer lands in DU-shell O4. Operator-wide server settings live under /admin/settings.` |
| `packages/nimbus-ui/src/routes/admin/observability.tsx` | 59 | `Operator observability reuses the developer observability surface in DU-shell O4 with an additional tenant filter.` |

**Recommendation:** delete the plan-code references. Replace with neutral
empty-state copy that names *what* is missing and *why*, never *which
internal phase* will deliver it. Convex's empty states are the right
model: a one-sentence what + a CTA or doc link.

> Example rewrite for `admin/index.tsx`:
> *"Overview surfaces machine health, service density, and incident
> activity. The metrics pipeline isn't wired yet — until it is, jump
> directly to **Machines** or **Services** for live status."*

---

### F2 — System tenant lens (⌘\) opens in Operator view · P1

DESIGN.md is explicit:

> **DESIGN.md:149** — *"The system tenant lens (⌘\\) is a Developer-side
> overlay onto the `_nimbus` tenant…"*

In the current build, pressing ⌘\ from `/admin/services` (or any
`/admin/*` route) opens the lens just fine and renders
`_nimbus · system.status` JSON. (`.review-shots/x-02-lens.png`.)

This isn't a small inconsistency — the lens is described in DESIGN.md as
*the* signature affordance that differentiates the Developer console.
Letting it fire in Operator collapses the conceptual separation the
two-view IA is built on.

**Recommendation:** gate the ⌘\ binding on `viewFromPathname(pathname)
=== "developer"`. Decide whether to (a) suppress the binding entirely in
Operator, or (b) keep the binding but show a one-line tip
*"System lens lives in the Developer console — press ⌘D to switch"* — I'd
go with (a). The shortcut should feel personal to Developer.

---

### F3 — Observability: sub-drawer says 4 sections, tab strip says 2 · P1

`packages/nimbus-ui/src/routes/app/observability.tsx`

- **Sub-drawer** (lines 133–164): static, lists **Logs / Runs / Events /
  Errors** (Events + Errors `disabled: true`).
- **Tab strip** (lines 197–204): renders **Logs / Runs only**.

A user reading the sub-drawer sees 4 sections — two ghosted —
implying a richer page. The body then shows only two tabs. This is the
kind of mismatch reviewers notice in a five-second scan.

**Recommendation:** make the tab strip render the same 4 sections with
the same disabled style as the sub-drawer (a clear `aria-disabled` tab
with a `not-yet-wired` tooltip), or, if Events/Errors aren't close,
strip them from both places and add them when they land. Pick a single
source of truth — I'd put the list in `OBSERVABILITY_SUB_DRAWER` and have
the tab strip render from it. DRY.

---

### F4 — Schedules: sub-drawer is `dynamic` with search, but page has only 2 sections · P1

`packages/nimbus-ui/src/routes/app/schedules.tsx:91-105`

The page is two stable sections (Scheduled / Cron). DESIGN.md is
emphatic that sub-drawers come in **two modes only** — static menu
(fixed item set) or dynamic list (filterable items). A 2-item filterable
list is a static menu wearing a dynamic-list costume; the search input
will mostly be wasted real estate.

The tab strip in the body (lines 128–) also covers Scheduled / Cron,
which means the sub-drawer is doing two jobs: section selector
(redundant with the tab strip) **and** filterable list of underlying
scheduled/cron items.

**Recommendation:** pick one. Convex does this well — the section
selector lives in the sub-drawer (static menu, Scheduled / Cron), and
the body shows the items for the active section. Drop the in-body tab
strip; keep the sub-drawer as a `static` menu. If you need filtering of
the actual schedule rows, that filter belongs in the body's table
header, not at the top of the sub-drawer.

---

### F5 — `/app/services` ScopeChip shows `tenant: all` when no tenant is selected · P2

`packages/nimbus-ui/src/routes/app/services.tsx:89-97`

```tsx
function ScopeChip({ activeTenant }: { activeTenant: string | null }) {
  return (
    <span … data-testid="services-scope">
      tenant: {activeTenant ?? "all"}
    </span>
  );
}
```

DESIGN.md's whole point on the Developer console is that the lens is
**always tenant-scoped** — the cross-tenant view is *the operator
console at `/admin/services`*. Showing `tenant: all` here muddies that
boundary, and it's the page's most prominent secondary chip.

**Recommendation:** when `activeTenant` is null, either (a) auto-default
to the first tenant on mount and never render `all`, or (b) replace the
table with an empty state *"Select a tenant from the top nav to see its
services."* and don't render the table or the chip at all. (a) is the
better UX — the URL `?as=` already supports this.

This same pattern likely repeats on other developer pages that read
`activeTenant` — audit Compute, Storage, Files, Schedules.

---

### F6 — Service detail (admin): 3 of 4 tabs are "not yet exposed" empty states · P2

`packages/nimbus-ui/src/routes/admin/services_.$service.tsx`

- Placement tab (line 216): real content (machine, host, state).
- **Restarts** (line 262): generic empty state.
- **Density** (line 271): generic empty state.
- **Drift** (line 280): generic empty state.

Three of four tabs are placeholders. Two effects:

1. The detail page advertises capability it doesn't have.
2. The empty-state copy itself (*"populate this panel once cluster
   telemetry lands"*) reads like a TODO, not user-facing language.

**Recommendation:** at minimum, **hide** the tabs that don't have data
yet — don't render them disabled. DESIGN.md's posture is *show real
content or show a dignified empty state for the whole page*, not *split
the screen into four lobotomized panels*. Add the tabs back as they
land. (Same critique the predecessor CS-redesign plan made about the old
`/app/compute` four-tab layout — same anti-pattern resurfacing.)

---

### F7 — Storage breadcrumb uses `storage` lowercase as first segment · P2

`packages/nimbus-ui/src/routes/app/storage.tsx:103`

```tsx
{ label: "storage", href: "/app/storage" },
```

DESIGN.md §Resource breadcrumb says the breadcrumb is a *resource* path
separated by `›`, e.g. `tenant ‹ acme › storage ‹ users ›`. The leading
`storage` here is the section name (already in the primary drawer), not
a resource. It reads as duplication.

**Recommendation:** drop the leading `storage` segment. Start the
breadcrumb at the first resource the user *navigated into* —
`tenant: acme › users`. The section is already implied by the primary
drawer's selected state. Convex and Firebase both do this; Docker
Desktop is the counter-example and it always feels noisy.

---

### F8 — Operator Services summary chip · ✅ already fixed

For the record — CS9's chip-fit fix at `routes/admin/services.tsx:87-93`
(`whitespace-nowrap`) holds up at 1440×900 and at 1024 width. Good fix.

---

### F9 — Tab-strip casing is `ALL CAPS` on detail pages, lowercase elsewhere · P3

`routes/admin/services_.$service.tsx:178-186` uses
`uppercase tracking-wide` for tab labels (Placement / Restarts / Density
/ Drift become PLACEMENT / RESTARTS / DENSITY / DRIFT).

The Observability tabs (`routes/app/observability.tsx:202-203`) and
Schedules tabs use sentence-case labels.

**Recommendation:** pick one. DESIGN.md's typography section reserves
all-caps + tracking for *metadata chips and column headers*, not
navigation. Sentence-case for tabs everywhere is the right call —
Convex, Firebase, and Podman all use sentence-case for tab strips.

---

### F10 — `/admin/observability` empty state is two sentences of plan-code prose · P2

(See F1 — also a P0 for the plan-code leak.) The page is otherwise
*nothing* — no header structure, no skeleton, no link to the developer
Observability surface it claims to "reuse." This is the bleakest page in
the operator console today.

**Recommendation:** ship a minimal real shell — header, an "Activity"
summary row pulled from the existing developer observability log stream
filtered to all tenants, even just a 20-line tailing list. Operators
will spend a lot of time on this page once it's wired; first impression
matters.

---

### F11 — `/admin` index ("System") · P2

Same as F10 — the page is plan-code prose only. The redeeming detail is
the sub-drawer for System has good static menu structure. But the body
itself shows nothing.

**Recommendation:** even before the metrics pipeline lands, surface
*something real* on /admin: server version, uptime, listener count from
the existing system tenant, an admin token hint. Convex's
`/instances/<x>/health` shows: version, status, region, last deploy,
backup status — a dozen real fields in a 2-column grid. That's the
template.

---

### F12 — Tenant selector doesn't auto-default · P3

When loading a Developer page (`/app/compute`, `/app/services`, etc.)
with no tenant selected and no `?as=` query, the page renders the
**all-tenants empty state** instead of auto-selecting the first
available tenant. Three tenants are visible in the top selector — none
is the default.

**Recommendation:** on first mount, if `activeTenant` is null and the
tenant list is non-empty, set `activeTenant = tenants[0].id`. Persist in
the same UI store that already tracks `?as=`. Users should land in a
*populated* tenant view, not the cross-tenant empty fallback. (This
fixes a chunk of F5 too.)

---

### F13 — Empty states across the console are inconsistent · P2

I saw at least three different empty-state stylings:

1. **Plan-code hint** (`PlaceholderPage` style) — `/admin/observability`,
   `/admin` index, `/app/files`, `/app/settings`. Plain prose + plan
   reference.
2. **Convex-style "nothing here yet"** — `/app/compute` Functions
   landing. Title + sub + decorative restraint. *Good.*
3. **Service-detail per-tab empty** — `Empty` component at
   `admin/services_.$service.tsx:410`. Centered title + detail. Different
   spacing and typography from #2.

DESIGN.md doesn't mandate one canonical empty-state component but it
should — pick the one that's already best (#2) and route everything
through it.

**Recommendation:** extract an `EmptyState` component with three slots
(icon optional, title, body, CTA optional) and use it everywhere. The
current `PlaceholderPage` should be deleted once #1 is migrated.

---

### F14 — Sub-drawer search affordance is inconsistent across pages · P3

- `/admin/services` sub-drawer: search input on top, tenant-grouped
  items below. ✅
- `/admin/services_/$service` sub-drawer: same, but **without** the
  tenant-group dividers. Flat list. Same data, different shape.
- `/app/services` sub-drawer: tenant-scoped list (no grouping needed).
- `/app/schedules` sub-drawer: `dynamic` with search but only 2
  sections (F4).

The list-shape rule should be: if the sub-drawer spans tenants, group
by tenant; if it's tenant-scoped, don't.

**Recommendation:** apply the tenant-grouping consistently across both
`/admin/services` list and `/admin/services_/$service` sub-drawer. The
detail sub-drawer should preserve the same grouping so the user keeps
their mental model when drilling in.

---

### F15 — Light/Dark/System and palette switching not validated this pass · note

I reviewed in default theme only (system → dark, `Mono` palette). The
plan ledger says light mode and the Blue / Mono / Warm palettes are
supported; I'd want a separate eyeball pass before launch with at least
*every state chip color* re-verified against each palette × mode combo.

**Recommendation:** add a `make verify-themes` smoke that screenshots
each route at each (mode × palette) and diffs the SVG of the state
chips — chips are the single thing most likely to disagree with palette
swaps.

---

### F16 — Console hygiene · ✅ pass

Across the full walk — Developer (8 routes), Operator (7 routes), ⌘K,
⌘\ — the browser console produced **zero** errors and zero warnings
(verified via `mcp__chrome-devtools__list_console_messages`). Holds up
the CS9 verification claim.

---

## Two things to keep doing

These showed up enough in the walk that I want to call them out so they
*don't* drift in future passes:

- **JetBrains Mono on identifiers and digests** is applied consistently
  — sha256 chip on service detail (line 152), tenant IDs in sub-drawer
  group headers, machine IDs. Don't ever let body sans creep into those.
- **State chips as labeled dots, not pills** (`StateChip` component) —
  used uniformly on services, machines, service detail. Resist the urge
  to "improve" these with pill backgrounds; the design system depends on
  the dot being the *only* shape that carries a state token color.

---

## Suggested fix order

| Wave | Effort | Findings |
|------|--------|----------|
| **W1 — copy & gating (1 day)** | trivial | F1, F2 |
| **W2 — section reconciliation (1-2 days)** | small | F3, F4, F12 |
| **W3 — empty-state unification (2-3 days)** | medium | F6, F10, F11, F13 |
| **W4 — polish (1 day)** | trivial | F5, F7, F9, F14 |
| **W5 — theme matrix coverage (separate)** | medium | F15 |

W1 alone removes every visible reference to internal plan codes and
restores the conceptual integrity of the system tenant lens — the two
biggest dings against the "feels finished" impression a new developer
forms in the first 30 seconds.

---

## Verification artifacts

All under `docs/plans/proof/desktop-ui-design-review-fixes/before/`:

- `dev-01-overview.png` — Developer overview
- `dev-02-compute.png` — Compute (Functions-only)
- `dev-03-services.png` — Services (developer)
- `dev-04-schedules.png` — Schedules
- `dev-05-storage.png` — Storage
- `dev-06-files.png` — Files (placeholder)
- `dev-07-observability.png` — Observability
- `dev-08-settings.png` — Tenant settings (placeholder)
- `ops-01-system.png` — Operator System index
- `ops-02-tenants.png` — Operator Tenants
- `ops-03-machines.png` — Operator Machines
- `ops-04-network.png` — Operator Network
- `ops-05-observability.png` — Operator Observability
- `ops-06-settings.png` — Operator Settings
- `x-01-cmdk.png` — ⌘K command palette
- `x-02-lens.png` — ⌘\ system tenant lens (opened from Operator
  view — see F2)
