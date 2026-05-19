# Desktop UI — Followup Hardening

Status: pending
Owner: desktop-ui workstream
Predecessor (closed, archived): `docs/plans/archive/desktop-ui-design-review-fixes-plan.md`
Source: 2026-05-18 post-closure four-reviewer pass (design vs DESIGN.md + code review of DR1–DR7 + proof-bundle audit + benchmark comparison against Convex / Firebase / Docker Desktop / Podman Desktop).
Promoted: 2026-05-18

Related current references:

- `DESIGN.md` (canonical operator-console design system — this plan
  amends it under H1).
- `docs/plans/archive/desktop-ui-design-review-fixes-plan.md`
  (immediate predecessor; closed cleanly on its scoped 14 findings).
- `docs/plans/archive/desktop-ui-compute-services-redesign-plan.md`
  (the IA decision that moved Services to dual-persona; was never
  mirrored into `DESIGN.md` — the BLOCKER this plan corrects).
- `docs/plans/proof/desktop-ui-design-review-fixes/` (predecessor
  proof bundle; left sealed by this plan, with one evidence gap
  backfilled in this wave's own bundle).

## Why this plan exists

The predecessor closed 14 design-review findings cleanly. A
four-reviewer post-closure pass against the after-state surfaced
~30 follow-up items grouped into four buckets:

1. **Source-of-truth drift (BLOCKER).** `DESIGN.md` (L127, L178,
   L358) still specs Services as Operator-only, but the shipped
   IA — set deliberately by `desktop-ui-compute-services-redesign-plan`
   and visible across every Developer screenshot — runs Services as
   dual-persona. The canonical contract disagrees with the canonical
   UI. Either edge of the contradiction can be the truth; this plan
   ratifies the shipped IA into `DESIGN.md`.

2. **Brittle patterns + magic strings (3 MAJORs).** `extractTenantId`
   regex-scrapes a free-text `source` field to drive the tenant filter
   on `/admin/observability`; `tenant ?? "all"` re-introduced the
   exact ambiguous-fallback pattern DR4/F12 removed from `/app/`;
   `id={item.id as ObservabilityTab}` casts hide that
   `SubDrawerItem.id` is still `string`. All three are visible-on-grep
   today and brittle on first source-format or schema change.

3. **Offline + error envelopes (4 MAJORs).** Operator System
   (`/admin/`) and Developer Overview (`/app/`) render every tile as
   `—` or `Loading…` indefinitely — no offline path, no error path.
   `/admin/tenants` shows raw `Request failed: 404` in page-header
   text. Disabled tabs (`EVENTS`/`ERRORS`) dim but offer no visible
   "why" affordance. Header ScopeChip says `TENANT beta`; status bar
   says `_nimbus` — DESIGN.md §Local Trust treats the status bar as
   canonical, so this is two truths on one screen.

4. **Polish + cleanup (the long tail).** Casing inconsistencies
   across ScopeChip / palette / tab strips; storage breadcrumb is
   plain text where DESIGN.md §Resource Breadcrumb specs chevron-
   separated copyable mono; EmptyState titles render sans-serif where
   DESIGN.md §Empty States specs mono; lens header separator uses `·`
   not `›`; `/admin/network` has no default-selected section. Plus
   code-review minor/nit findings: two parallel `*.spec.tsx` ignore
   regexes, `use-tenant-bootstrap` re-fires on every search-object
   identity change, dead `EmptyState.cta.onClick` branch, DR3 spec
   early-outs that silently pass on union change, `keyboard-contract`
   missing `preventDefault`, `useTenantCount` duplicates a tenant
   fetch already in `use-tenant-bootstrap`, "Follow-up plans will
   surface these" prose left in two route files, redundant
   `as AdminObservabilitySearch` cast. Plus proof-bundle audit
   findings: missing `/admin/services/$service` after-screenshot,
   "14 of 16" phrasing in `docs/plans/README.md` that contradicts the
   plan body's "12 in-scope".

Pre-launch policy applies: prefer breaking changes; no compat shims;
no feature flags for legacy behavior. Tenant scope, sub-drawer item
typing, and tile loading-state all get rewritten end-to-end rather
than transitioned.

## Outcome

After this plan:

- `DESIGN.md` reflects the shipped IA: Services is spec'd as
  dual-persona (Operator + Developer) the same way Observability is.
  Every `Services` reference in `DESIGN.md` is consistent. Grep
  `Services.*Operator-only|Operator-only.*Services` returns zero
  hits.
- Tenant scope is expressed as a discriminated union end-to-end:
  `type TenantScope = { kind: "all" } | { kind: "specific"; tenantId: string }`.
  The string literal `"all"` no longer appears in render logic as a
  fallback marker.
- `SubDrawerItem` is generic over its `id` type. Tab strip mappings
  derive their union from the spec array via
  `OBSERVABILITY_SUB_DRAWER.items[number]["id"]`. No `as ObservabilityTab`
  or `as AdminObservabilityTab` casts remain in the touched files.
- `extractTenantId` (and the regex-against-`event.source` pattern) is
  gone. Tenant-scoped filtering on `/admin/observability` either reads
  a real `EventDoc.tenantId` field or — if the backend doesn't
  surface it yet — the chip renders `tenant filter unavailable` and
  the filter is gated off.
- Operator System (`/admin`), Developer Overview (`/app`), and
  `/admin/tenants` each render `loading | ok | offline | error`
  distinctly. No tile shows `—` indefinitely. No raw error string
  lands in a page header.
- Header ScopeChip and status-bar tenant slot agree on every screen.
  Both reflect the effective tenant scope of the current view:
  `beta` on `/app/beta/*`, `all tenants` on `/admin/observability`
  with no `?tenant=`, `_nimbus` only when the user is explicitly
  viewing system-tenant data (the lens, or `?tenant=_nimbus`).
- Disabled tabs ship with a visible `coming soon` chip beside the
  label, not just `aria-disabled` + hover-only `title`.
- Tab strips, ScopeChips, palette mode buttons, and lens-header
  separators share one canonical convention, documented in
  `DESIGN.md` §Typography.
- `/app/storage` and any other resource breadcrumb renders the
  `DESIGN.md` §Resource Breadcrumb pattern: chevron-separated,
  copyable, mono.
- `EmptyState` titles render in mono per `DESIGN.md` §Empty States.
- Every code-review MINOR/NIT from the four-reviewer pass either
  landed or is explicitly deferred in this plan's execution log
  with the rationale.
- `/admin/services/$service` after-screenshot exists in this wave's
  proof bundle, closing the predecessor's visual-evidence gap for
  F6/F9/F14 without mutating the sealed predecessor bundle.

## Out of scope

- **Backend-side `EventDoc.tenantId` field surfacing.** If exposing
  this requires schema changes in `crates/nimbus-engine/` or the
  event-ingestion path, that lives in a separate backend plan. This
  plan handles the UI side either way — if the field exists, use it;
  if not, gate the filter.
- **`id: serviceId as never` on `api.services.byId` calls.** Symptom
  of a codegen-vs-runtime type mismatch. Belongs in a codegen-typing
  plan. Flagged here only.
- **Theme matrix smoke (Light/Dark/System × Blue/Mono/Warm).**
  Already deferred by the predecessor (F15) to verification-tooling
  work.
- **Re-adding Restarts/Density/Drift tabs on admin service detail.**
  Continues to wait on the placement controller, restart audit log,
  and bundle comparator plans.
- **Mutation of the predecessor proof bundle or archived plan.**
  Sealed paperwork stays sealed. Live docs (`docs/plans/README.md`)
  are editable; closed plans are not. The missing service-detail
  screenshot lands in *this* plan's proof bundle with a cross-
  reference, not in the predecessor's.

## Phase status ledger

| Phase | Slice | Status |
|-------|-------|--------|
| H0 | Read-in + before-state confirmation | pending |
| H1 | `DESIGN.md` ↔ shipped IA reconciliation (BLOCKER) | pending |
| H2 | Type safety + magic-string removal (3 MAJORs) | pending |
| H3 | Offline + error envelopes (4 MAJORs) | pending |
| H4 | Polish: casing + breadcrumb + EmptyState (5 MINORs + 3 NITs) | pending |
| H5 | Code cleanup + test gaps (8 MINORs + 4 NITs) | pending |
| H6 | Live-doc corrections from proof audit (3 MINORs) | pending |
| H7 | Verification + close + archive | pending |

## Roadmap detail

### H0 — Read-in + before-state confirmation

Goal: orient against the predecessor's after-state — that becomes
this wave's *before* — and confirm scope.

Touch list (reads only):

- `DESIGN.md` end-to-end, with focus on L100–130 (IA contract),
  L170–195 (Compute), L350–410 (Services), L850–890 (status bar),
  L900–960 (System Tenant Lens), L905–916 (Empty States), §Resource
  Breadcrumb (find via grep), §Typography (find via grep).
- `docs/plans/archive/desktop-ui-design-review-fixes-plan.md` (entire).
- `docs/plans/proof/desktop-ui-design-review-fixes/README.md`.
- `docs/plans/proof/desktop-ui-design-review-fixes/after/` (visual
  walk-through).
- `packages/nimbus-ui/src/routes/admin/observability.tsx` (lines
  130–225 for `extractTenantId`, ScopeChip, tab cast).
- `packages/nimbus-ui/src/routes/app/observability.tsx` (mirror).
- `packages/nimbus-ui/src/shell/use-tenant-bootstrap.ts`.
- `packages/nimbus-ui/src/components/empty-state.tsx`.
- `packages/nimbus-ui/src/shell/keyboard-contract.tsx`.

Done when:

- This plan's `docs/plans/proof/desktop-ui-followup-hardening/`
  proof directory exists with a `before.md` that cross-references
  the predecessor's `after/` directory by path (no PNG copying —
  the predecessor's after-state IS this wave's before-state).
- Scope confirmed: zero edits this phase.

### H1 — `DESIGN.md` ↔ shipped IA reconciliation (BLOCKER)

Goal: ratify the shipped dual-persona Services IA into `DESIGN.md`
so the canonical contract no longer contradicts the canonical UI.

Background: the archived
`desktop-ui-compute-services-redesign-plan.md` (closed 2026-05-18)
deliberately moved Services from Operator-only to dual-persona
("parallel to Observability"). That plan's closure edited the IA but
not `DESIGN.md`. The predecessor design-review plan inherited the
gap and closed without catching it.

Touch list:

- `DESIGN.md` IA table row for Services (currently L127): change
  scope column to `Operator + Developer (dual-persona)`. Mirror the
  pattern of the Observability row.
- `DESIGN.md` Compute section (currently ~L178): replace
  `Service lifecycle moved out to the Operator console (Services).`
  with prose that reflects the dual-persona split — Compute owns
  request-scoped function execution; Services (in both consoles)
  owns long-running placement.
- `DESIGN.md` §Services (Operator) heading (currently ~L358): rename
  to §Services. Restructure as: shared body (state, endpoints,
  restart policy, sub-drawer) → Operator-specific subsection (cross-
  tenant grouping, system services) → Developer-specific subsection
  (`compose.yaml`-declared services scoped to active tenant).
- Add a one-line back-reference: "See
  `docs/plans/archive/desktop-ui-compute-services-redesign-plan.md`
  for the IA decision rationale."
- Any other `Service.*Operator-only` mentions: rewrite.

Done when:

- `grep -ni 'service' DESIGN.md` shows consistent dual-persona
  framing wherever Services is named.
- `grep -ni 'Operator-only' DESIGN.md` returns zero hits matching
  Services context.
- Diff committed as one focused edit; commit message names the
  predecessor compute-services-redesign plan.

### H2 — Type safety + magic-string removal

Goal: kill the three brittle patterns the code review surfaced.

(a) `TenantScope` discriminated union.

Introduce at `packages/nimbus-ui/src/shell/tenant-scope.ts`:

```ts
export type TenantScope =
  | { kind: "all" }
  | { kind: "specific"; tenantId: string };

export function parseTenantScope(raw: string | undefined): TenantScope {
  return raw ? { kind: "specific", tenantId: raw } : { kind: "all" };
}

export function serializeTenantScope(scope: TenantScope): string | undefined {
  return scope.kind === "specific" ? scope.tenantId : undefined;
}

export function describeTenantScope(scope: TenantScope): string {
  return scope.kind === "all" ? "all tenants" : scope.tenantId;
}
```

Refactor every site that currently uses `string | undefined` for
tenant scope: `routes/admin/observability.tsx` (ScopeChip, LogsTab,
RunsTab, tab strip wire-up), header `TenantSelector` in
`routes/__root.tsx` (or wherever it lives), `routes/app/*` consumers,
`shell/use-tenant-bootstrap.ts`. The literal `"all"` should not
appear in JSX or in URL-search parsers — only `kind: "all"` in code.

(b) Drop `extractTenantId`.

`routes/admin/observability.tsx:219-223` regex-scrapes
`event.source` for `tenant=…`. Two paths, pick based on backend
check at H0:

- **If `EventDoc.tenantId` is exposed by the backend**: import the
  generated `EventDoc` type, read `event.tenantId` directly, drop
  the regex.
- **If the backend doesn't surface it yet**: delete `extractTenantId`
  and the `tenant`-filter wiring; render the ScopeChip as
  `tenant filter unavailable` with a tooltip explaining the gating;
  add an entry under "Out of scope" pointing at the backend plan
  needed to restore filtering.

Either path must land with a test. Zero regex against `source`.

(c) `SubDrawerItem` generic over id.

Edit the shell types (likely `shell/sub-drawer.tsx` or
`shell/sub-drawer-types.ts`):

```ts
export type SubDrawerItem<TId extends string = string> = {
  readonly id: TId;
  readonly label: string;
  readonly disabled?: boolean;
};

export type StaticSubDrawerSpec<TId extends string = string> = {
  readonly kind: "static";
  readonly items: ReadonlyArray<SubDrawerItem<TId>>;
};
```

Then in `routes/app/observability.tsx`:

```ts
export const OBSERVABILITY_SUB_DRAWER = {
  kind: "static",
  items: [
    { id: "logs", label: "Logs" },
    { id: "runs", label: "Runs" },
    { id: "events", label: "Events", disabled: true },
    { id: "errors", label: "Errors", disabled: true },
  ],
} as const satisfies StaticSubDrawerSpec<"logs" | "runs" | "events" | "errors">;

export type ObservabilityTab = (typeof OBSERVABILITY_SUB_DRAWER.items)[number]["id"];
```

Delete the `as ObservabilityTab` cast at the tab-strip map. Mirror
for `ADMIN_OBSERVABILITY_SUB_DRAWER`, `SCHEDULES_SUB_DRAWER`, and
any other static specs.

Specs:

- `tenant-scope.spec.ts`: parse, serialize, describe — five tests
  covering both kinds and round-trip.
- `routes/app/observability.spec.ts` extension: a type-level test
  using an `Equal<A, B>` helper asserting
  `ObservabilityTab` ≡ `"logs" | "runs" | "events" | "errors"`.
  If the helper isn't already in `nimbus-ui/test-utils`, inline it.
- Existing tests adapt; expect a few snapshot/match updates.

Done when:

- `grep -rn 'as ObservabilityTab\|as AdminObservabilityTab' packages/nimbus-ui/src`
  returns zero hits.
- `grep -rn 'tenant ?? "all"\|tenant === "all"' packages/nimbus-ui/src`
  returns zero hits.
- `grep -rn 'extractTenantId\|event\.source\.match' packages/nimbus-ui/src`
  returns zero hits.
- vitest + typecheck + build clean.

### H3 — Offline + error envelopes

Goal: every "loading forever" tile and every raw 404 path renders a
proper state. Benchmark target: Convex Dashboard's reconnect banner.

(a) Tile loading-state.

Introduce a typed helper (no new component yet; just a type):

```ts
export type LoadingValue<T> =
  | { kind: "loading" }
  | { kind: "ok"; value: T }
  | { kind: "offline" }
  | { kind: "error"; message: string };
```

Wire `useQuery` consumers in `routes/admin/index.tsx` and
`routes/app/index.tsx` to produce `LoadingValue<T>` for each tile by
combining the query's `undefined`/`null`/result with the existing
`isConnected` / `connectionState` signal (see `shell/connection-state.ts`
or wherever the WS status lives — find in H0). Render: loading →
animated dots, ok → value, offline → muted "offline" with reconnect
hint, error → red short message.

(b) `/admin/tenants` 404 → actionable diagnostic envelope.

`routes/admin/tenants.tsx` currently renders raw
`Request failed: 404` as page-header text. Replace with a §Actionable
Diagnostics envelope inside the page body (not header): title
("Tenants endpoint unavailable"), body ("This deployment can't
reach `/api/tenants`. The server may be offline or this build doesn't
ship the tenants endpoint."), action button ("Retry"). Use existing
`EmptyState` with `cta`.

(c) Status-bar canonicalization.

`shell/status-bar.tsx` (or equivalent) currently reads
`_nimbus` always. Change to: subscribe to the same effective scope
the header ScopeChip reads. On `/app/*`: show the active dev tenant.
On `/admin/observability` with no `?tenant=`: show
`all tenants`. On `/admin/*` views that genuinely target system data
(if any): show `_nimbus`. The Copy-tenant button (`Copy tenant: …`)
also follows.

(d) Disabled-tab visible affordance.

Extend `TabLink` in `routes/app/observability.tsx` and
`routes/admin/observability.tsx`: when `disabled`, render a small
`<span class="ml-2 rounded bg-surface-2 px-1 text-[10px] uppercase">coming soon</span>`
beside the label. Keep `aria-disabled` and `title` for a11y. Same
treatment if other surfaces have disabled tabs.

Specs:

- `loading-value.spec.ts` covering all four kinds.
- `status-bar.spec.tsx` covering four scenarios: `/app/beta`,
  `/admin/observability` no tenant, `/admin/observability?tenant=beta`,
  system-tenant lens.
- `admin/tenants.spec.tsx` covering the 404 path renders the
  diagnostic envelope.
- `observability.spec.tsx` extension: disabled-tab renders the
  `coming soon` chip.

Done when:

- vitest passing; chrome-devtools walk of `/admin`, `/app`,
  `/admin/tenants` shows the four states distinctly.
- No `—` or `Loading…` tile renders for >2s in the offline path.

### H4 — Polish: casing + breadcrumb + EmptyState

Goal: surface-level consistency. Everything visible-without-running-server.

Touch list:

- `routes/app/services.tsx` ScopeChip: change `TENANT: BETA` body
  chip to `TENANT beta` to match every other dev screen. Likely a
  className + JSX edit at the ScopeChip render.
- `shell/command-palette.tsx`: mode buttons (`Navigate` / `Run` /
  `Filter`) switch to uppercase-tracked-wide.
- `shell/command-palette.tsx`: dialog gets `max-h-[80vh]`; listbox
  gets `max-h-[60vh] overflow-y-auto`; footer hint row pinned outside
  the scrollable region.
- `routes/admin/settings.tsx` Encryption row: add small state-dot
  alongside `unavailable` text so the signal isn't color-only.
  Reuse whatever StateDot component exists (find via grep).
- `routes/app/storage.tsx` breadcrumb: implement chevron-separated
  copyable mono per `DESIGN.md` §Resource Breadcrumb. If a shared
  `<Breadcrumb>` component exists, use it; if not, introduce one at
  `components/breadcrumb.tsx` only because the same pattern needs
  to land here, in `routes/app/compute_/$function.tsx` later, and
  the System Tenant Lens header (per item below). Three call sites
  = real abstraction, not premature.
- `components/empty-state.tsx`: title className `font-sans` →
  `font-mono`.
- `shell/system-tenant-lens.tsx` header: separator `·` → `›`.
- `routes/admin/network.tsx` index: when no `?section=` is set,
  redirect to `?section=routes` so the body doesn't render the
  "no section selected" state.

Specs:

- `breadcrumb.spec.tsx` if the shared component lands: render N
  segments, copy-per-segment, chevron separators, mono.
- `empty-state.spec.tsx`: assert title has `font-mono` class.
- `command-palette.spec.tsx` (or `shell/command-palette.spec.tsx`):
  long item list scrolls, footer remains visible.

Done when:

- Re-walk screenshots for `/app/services`, `/app/storage`, command
  palette open, system tenant lens, `/admin/settings`,
  `/admin/network`.
- vitest + typecheck + build clean.

### H5 — Code cleanup + test gaps

Goal: deletes, consolidations, spec backfills. No behavior change
except where noted.

Touch list:

- `packages/nimbus-ui/scripts/route-ignore-pattern.mjs` (new): export
  the single `*.spec.(ts|tsx)` ignore regex. Import from
  `scripts/generate-routes.mjs` and `vite.config.ts`.
- `shell/use-tenant-bootstrap.ts:73`: add a `useRef` (`didBootstrap`)
  guard or, simpler, depend only on stable `viewFromPathname`
  output instead of the full `search` object. Comment explaining the
  re-fire trap.
- `components/empty-state.tsx`: delete the `onClick` branch of
  `cta`; the `EmptyStateProps['cta']` type collapses to the
  `{ label: string; to: string }` shape. No callers use `onClick` as
  of H4. If a future caller needs an `onClick` cta, restore then.
- `routes/app/section-nav.spec.ts`: replace
  `if (kind !== "static") return;` early-outs with
  `expect(spec.kind).toBe("static")` + non-null narrowing via
  `if (spec.kind !== "static") throw new Error(...)`. The spec fails
  loudly if the union flips.
- `shell/keyboard-contract.tsx`: move `event.preventDefault()` up to
  before the developer-view gate on the `Cmd+\\` branch. Defensive;
  no observed bug.
- `shell/use-tenant-bootstrap.spec.tsx`: add an 11th test covering
  the `AbortController.signal.aborted` path — mount, navigate away
  before the fetch resolves, assert no `setActiveTenant` call lands.
- `shell/tenants-fetch.ts` (new): `async function fetchTenants(signal: AbortSignal): Promise<TenantSummary[]>` —
  the shared `/api/tenants` GET. Use from `routes/admin/index.tsx`
  (`useTenantCount`) and `shell/use-tenant-bootstrap.ts`. Mirror the
  shape the bootstrap already parses; export a `TenantSummary` type.
- Delete the "Follow-up plans will surface these" prose at
  `routes/admin/services_.$service.tsx:233-235` and
  `routes/app/services_.$service.tsx:225-227`. The component renders
  nothing in its place — disabled-tab content per CLAUDE.md.
- Drop redundant `as AdminObservabilitySearch` cast at the
  `validateSearch` return in
  `routes/admin/observability.tsx`. Same pattern audit across other
  `validateSearch` callsites.
- `groupByTenant` cross-route import (`routes/admin/services.tsx` →
  `routes/admin/services_.$service.tsx`): **no action**. Two
  consumers, not three; per CLAUDE.md threshold. Leave as-is.
- `command-palette.tsx` `runActions` rebuilt every render:
  **no action**. Per CLAUDE.md "don't add abstractions beyond what
  the task requires" — `useMemo` on a tiny array isn't a clear win.
- `routes/admin/services_.$service.tsx:68` `id: serviceId as never`:
  **no action this plan**. Codegen-vs-runtime mismatch; flagged for
  separate codegen-typing plan.

Done when:

- `grep -rn '"Follow-up plans will surface"' packages/nimbus-ui/src`
  returns zero hits.
- `grep -rn 'as AdminObservabilitySearch\|as ObservabilitySearch' packages/nimbus-ui/src`
  returns zero hits.
- Spec count grows by at least 2 (controller-abort test +
  breadcrumb specs at minimum).
- vitest + typecheck + build clean.

### H6 — Live-doc corrections from proof audit

Goal: fix the live-doc inaccuracies the proof audit caught. Touches
only editable docs. Sealed paperwork (the archived predecessor plan
and its proof README) stays sealed.

Touch list:

- `docs/plans/README.md` archived-baseline blurb for
  `desktop-ui-design-review-fixes-plan.md`: replace
  "Closed 14 of 16 findings" with
  "Closed 12 of 14 in-scope findings (16 total; F8 already-fixed,
  F13 folded into F1)" — matches the predecessor plan body's own
  scope statement (predecessor line 21).
- Any other live doc that cites the wrong count: grep first, decide
  per-site.

Done when:

- `grep -rn '14 of 16\|14 findings' docs/plans/README.md` returns
  zero hits in this entry's blurb.
- Phrasing consistent across live docs.

### H7 — Verification + close + archive

Goal: prove every gate, capture proof, archive.

Steps:

1. Restart Vite dev server (or use embedded build via
   `nimbus start`); confirm UI loads. Use the predecessor's
   capture rig (chrome-devtools-mcp at 1440×900).
2. Capture this wave's after-shots with prefix `h7-`:
   - `h7-design-services-row.png` — `DESIGN.md` rendered with the
     dual-persona row (if rendering DESIGN.md visually; otherwise
     omit and rely on the diff)
   - `h7-app-services-scope-chip.png` — `/app/services` showing
     `TENANT beta`
   - `h7-app-storage-breadcrumb.png` — chevron-separated copyable
     mono breadcrumb
   - `h7-app-overview-tile-states.png` — Developer Overview showing
     loading + offline + ok tiles distinguishably
   - `h7-admin-overview-tile-states.png` — same for Operator System
   - `h7-admin-tenants-404-envelope.png` — diagnostic envelope on
     `/admin/tenants` 404
   - `h7-cmdk-modes-and-scroll.png` — palette open with
     uppercase-tracked-wide modes and the listbox scrolled
   - `h7-observability-disabled-chip.png` — Observability tab strip
     showing `coming soon` chip next to `EVENTS`/`ERRORS`
   - `h7-status-bar-tenant.png` — status bar showing the effective
     tenant (matches header ScopeChip)
   - `h7-lens-separator.png` — System Tenant Lens header with `›`
   - `h7-admin-network-default-section.png` — `/admin/network` with
     `?section=routes` selected by default
   - `h7-admin-services-detail.png` — **the predecessor evidence
     gap closed** — `/admin/services/$service` rendered with the
     single Placement tab, sentence-case label, tenant-grouped
     sub-drawer
3. Record verifications:
   - `cd packages/nimbus-ui && npx vitest run` — all pass; record
     the count.
   - `npm run typecheck` — clean.
   - `cd packages/nimbus-ui && npm run build` — clean.
   - `grep -rn 'as ObservabilityTab\|as AdminObservabilityTab\|extractTenantId\|"Follow-up plans will surface"\|Operator-only.*Services' packages/nimbus-ui/src DESIGN.md`
     — zero hits.
   - Browser console hygiene: zero `error`/`warn` across the walk
     (allow the same `404` on missing endpoints, with the endpoint
     named in the proof README).
   - ⌘\ on `/admin/*` still a no-op (regression check on the
     predecessor's DR2 win).
4. Write `docs/plans/proof/desktop-ui-followup-hardening/README.md`
   mirroring the predecessor's structure: before/after sections, a
   mapping table from each H-phase outcome to the after-screenshot
   (or spec file) that proves it.
5. Flip plan status to `done`; append execution log entry below;
   `git mv` to `docs/plans/archive/`; update `docs/plans/README.md`
   (remove from active; add archived-baseline entry citing this
   plan as the closure of the post-DR-fixes correctness wave).

Done when:

- Ledger H0–H7 all `done`.
- Plan lives under `docs/plans/archive/`.
- Proof bundle exists with the mapping table covering every Outcome
  bullet.
- `docs/plans/README.md` lists this plan as an archived baseline and
  no longer lists it as active.

## Verification approach

- Use chrome-devtools-mcp (workspace-allowed dirs only) for live
  page inspection and screenshots. No Playwright runner. (Inherits
  the predecessor's choice.)
- For every phase with a unit-testable gate, the test lands in the
  same commit as the implementation.
- Console hygiene is verified on every phase's after-state — zero
  `error`/`warn` is a hard gate. The known dev-server 404 is
  acceptable only if named in the proof README.
- Pre-launch policy applies: when a type or API shape changes
  (`TenantScope`, `SubDrawerItem<TId>`, `LoadingValue<T>`,
  `EmptyState.cta`), refactor every consumer in the same wave. No
  shims, no transitional types.

## Execution log

(a) 2026-05-18 — Plan promoted from a 2026-05-18 post-closure
four-reviewer pass against the predecessor's after-state. Reviewer
pass cataloged ~30 follow-up items: 1 BLOCKER (DESIGN.md ↔ shipped
IA on Services dual-persona), 8 MAJORs (3 code: regex tenant scrape,
admin `?? "all"` fallback, `id as ObservabilityTab` cast; 5 design:
offline-forever tiles on Overview/System, raw 404 on
`/admin/tenants`, disabled-tab affordance, header/status-bar tenant
disagreement, missing service-detail after-screenshot), 13 MINORs
(8 code + 5 design + 3 proof-bundle), 7 NITs. Eight phases
prepared (H0–H7). Pre-launch policy continues to apply. Predecessor
proof bundle stays sealed; this wave's bundle closes the F6/F9/F14
visual-evidence gap.
