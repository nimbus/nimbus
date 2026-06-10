# Desktop UI — Followup Hardening (proof bundle)

Proof artifacts for `docs/plans/archive/desktop-ui-followup-hardening-plan.md`
(promoted 2026-05-18 to close the post-design-review correctness wave;
covers H0–H7 against the predecessor's `after/` baseline).

Captures are at 1440×900 against the Vite dev server
(`http://localhost:5173/ui/`) running the production source tree as of
plan closure. `before/` is the predecessor's sealed `after/` bundle
(`docs/plans/proof/desktop-ui-design-review-fixes/after/`, 17 captures,
`dr8-` prefix) — see `before.md` for the cross-reference table. `after/`
images are post-H1–H6 captures with an `h7-` prefix.

## Verifications recorded at H7

- vitest: `cd packages/nimbus-ui && npx vitest run` → 32 files,
  222 tests passing (predecessor closed at 190; this wave adds 32 specs
  spread across breadcrumb, status-bar, EmptyState, /admin/tenants 404,
  observability, schedules, and the AbortController abort path).
- typecheck: `npm run typecheck` clean (no errors); codegen runs as part
  of the gate and `[nimbus-ui] route tree generated` lands first.
- build: `cd packages/nimbus-ui && npm run build` clean (only the
  informational `chunkSizeWarningLimit` note from rolldown — unchanged
  from the predecessor).
- Plan-code grep:
  `grep -rn 'as ObservabilityTab\|as AdminObservabilityTab\|extractTenantId\|"Follow-up plans will surface"\|Operator-only.*Services' packages/nimbus-ui/src DESIGN.md`
  returns zero matches (exit 1) — every magic-string cast, the regex
  helper `extractTenantId`, the residual "Follow-up plans will surface"
  prose, and the `Operator-only` framing in `DESIGN.md` are gone.
- H6 doc grep: `grep -rn '14 of 16\|14 findings' docs/plans/README.md`
  returns zero matches (exit 1) — the archived-baseline blurb now reads
  "Closed 12 of 14 in-scope findings (16 total)".
- Console hygiene during the walk: zero React or runtime `error`/`warn`
  on any /app/* or /admin/* surface. The single console entry observed
  (`Failed to load resource: 404`) is the dev-server fetch returning 404
  for the unmounted `/api/tenants` endpoint in this auth-less capture
  context — it drives the new `/admin/tenants` diagnostic envelope
  (`h7-admin-tenants-404-envelope.png`), not a UI defect.
- ⌘\ lens gating regression: verified live. ⌘\ on `/ui/admin/machines`
  is still a no-op — the page snapshot after the keypress contains no
  `System tenant lens` region (predecessor invariant from
  `dr8-x-03-lens-blocked-on-admin.png` holds).

## H-phase → after-evidence mapping

| H-phase | Summary | After-image / spec |
|---------|---------|--------------------|
| **H0** | Read-in + scope confirmation | `before.md` cross-reference table; plan-accuracy delta recorded (only one "Follow-up plans will surface" site, not two) |
| **H1** | `DESIGN.md` ↔ shipped IA reconciliation (BLOCKER): ratify dual-persona Services row, drop `Operator-only` framing | Diff against `DESIGN.md` Services row (visual capture omitted — DESIGN.md not rendered in the SPA). Grep gate above confirms `Operator-only.*Services` removed. |
| **H2(a)** | `TenantScope` discriminated union replaces `tenant ?? "all"` | `routes/admin/observability.tsx` ScopeChip now reads `TENANT ACME · FILTER UNAVAILABLE` for a specific tenant; visible in `after/h7-status-bar-tenant.png` |
| **H2(b)** | Drop `extractTenantId` regex helper | Code-only; grep gate confirms zero hits |
| **H2(c)** | `SubDrawerItem<TId>` generic; no magic-string casts | Code-only; grep gate confirms zero `as ObservabilityTab`/`as AdminObservabilityTab` casts |
| **H3(a)** | Tile `LoadingValue<T>` envelopes (loading / ok / offline / error) | Developer overview: `after/h7-app-overview-tile-states.png`. Operator system: `after/h7-admin-overview-tile-states.png`. Both show the new tile rendering for offline data. |
| **H3(b)** | `/admin/tenants` 404 → diagnostic envelope with RETRY CTA | `after/h7-admin-tenants-404-envelope.png` — `Tenants endpoint unavailable` heading, `/api/tenants` error context, `RETRY` button. |
| **H3(c)** | Status-bar tenant canonicalization (per-view) | `after/h7-status-bar-tenant.png` — on `/admin/observability?tenant=acme` status bar reads `tenant: acme` matching header chip. Other captures show `tenant: beta` on `/app/*` and `tenant: _nimbus` on `/admin/*` without a tenant param. |
| **H3(d)** | Disabled-tab affordance (coming-soon) | `after/h7-observability-disabled-chip.png` — Observability tab strip with `EVENTS`/`ERRORS` rendered with the `coming-soon` chip and `aria-disabled` semantics. |
| **H4 ScopeChip casing** | `TENANT beta` (label mono lowercase, value mono lowercase) on every chip | `after/h7-app-services-scope-chip.png` — Services page header chip reads `TENANT beta`. |
| **H4 palette mode buttons** | NAVIGATE / RUN / FILTER rendered `font-mono text-[10px] uppercase tracking-wide` | `after/h7-cmdk-modes-and-scroll.png` — command palette open showing the new mode-button typography; listbox scroll container has `min-h-0 flex-1 max-h-[60vh] overflow-y-auto`. |
| **H4 chevron-mono breadcrumb** | Storage / lens use the canonical `›`-separated mono breadcrumb with per-segment copy chip | `after/h7-app-storage-breadcrumb.png` (storage). Lens captured in `after/h7-lens-separator.png` — header reads `_nimbus › system.status` (chevron is `aria-hidden`). |
| **H4 EmptyState mono title** | Title rendered in mono | Asserted by `components/empty-state.spec.tsx` and visible in `after/h7-admin-tenants-404-envelope.png`. |
| **H4 lens separator `›`** | System Tenant Lens header uses chevron | `after/h7-lens-separator.png` — `_nimbus › system.status`. |
| **H4 `/admin/network` default section** | Bare `/admin/network` redirects to `?section=routes` | `after/h7-admin-network-default-section.png` — URL bar shows `?section=routes` after navigating to bare `/admin/network`, Routes sub-drawer item selected, body tab strip on `ALL`. |
| **H5 spec backfills + cleanup** | Shared `fetchTenants` helper; `keyboard-contract` preventDefault hoist; `section-nav.spec.ts` narrowing throws; `use-tenant-bootstrap` abort-controller test (11th); shared `route-ignore-pattern.mjs` | Code-only. 32 files / 222 tests passing. Grep gates above confirm no residual "Follow-up plans will surface" prose. |
| **H6 live-doc corrections** | `docs/plans/README.md` archived-baseline blurb fixed: `Closed 14 of 16 findings` → `Closed 12 of 14 in-scope findings (16 total; F8 already-fixed and F16 folded into F1; F15 theme-matrix smoke and F6-restore of Restarts/Density/Drift deferred to owning plans)` | Doc grep gate above (exit 1). |
| **H7 predecessor evidence-gap** | `/admin/services/$service` capture missing from `dr8-*` set | `after/h7-admin-services-detail.png` — single `Placement` tab in `nav: Admin service detail sections` with sentence-case label; breadcrumb starts at `Services` then the short service id; tenant-grouped sub-drawer present in the column. |

## Out of scope (recorded for traceability, not bundled as proof here)

- `h7-design-services-row.png` was listed as optional ("if rendering
  `DESIGN.md` visually; otherwise omit and rely on the diff"). The SPA
  doesn't render markdown, so the diff against `DESIGN.md` is the
  evidence — grep gate `Operator-only.*Services` returns zero hits.
- F15 theme-matrix smoke (Light/Dark/System × Blue/Mono/Warm) remains
  deferred to a separate verification-tooling plan.
- F6 restoration of the Restarts/Density/Drift tabs returns when their
  owning plans land (`docs/plans/desktop-ui-services-redesign-plan.md`
  is closed; the next plan that wires placement-controller state will
  re-add them).

## Screenshot inventory

`before/` — not duplicated here. The predecessor's sealed `after/`
bundle (`docs/plans/proof/desktop-ui-design-review-fixes/after/`,
17 captures with `dr8-` prefix) is this wave's `before/`. See
`before.md` for the cross-reference table.

`after/` (11 PNGs, `h7-` prefix):

- `h7-app-overview-tile-states.png` — Developer Overview with
  `LoadingValue<T>` tiles.
- `h7-app-services-scope-chip.png` — `/app/services` ScopeChip
  `TENANT beta`.
- `h7-app-storage-breadcrumb.png` — `/app/storage` chevron-mono
  breadcrumb.
- `h7-admin-overview-tile-states.png` — Operator System with
  `LoadingValue<T>` tiles.
- `h7-admin-tenants-404-envelope.png` — diagnostic envelope on
  `/admin/tenants` 404.
- `h7-admin-network-default-section.png` — `/admin/network`
  defaulting to `?section=routes`.
- `h7-admin-services-detail.png` — `/admin/services/$service`
  rendered with the single Placement tab (closes predecessor
  evidence gap).
- `h7-observability-disabled-chip.png` — Observability tab strip
  with disabled `EVENTS`/`ERRORS`.
- `h7-cmdk-modes-and-scroll.png` — command palette with
  uppercase-tracked mode buttons.
- `h7-lens-separator.png` — System Tenant Lens header
  `_nimbus › system.status`.
- `h7-status-bar-tenant.png` — `/admin/observability?tenant=acme`
  with status-bar tenant slot reading `acme` (header chip matches).
