# Desktop UI — Followup Hardening (before-state)

The predecessor's `after/` directory IS this wave's `before/`. No PNG
copying. Cross-reference each H-phase target to the canonical evidence
in the sealed predecessor bundle.

Predecessor bundle:
`docs/plans/proof/desktop-ui-design-review-fixes/after/` (17 captures
at 1440×900, prefix `dr8-`).

## Cross-reference

| H-phase target | Before evidence (predecessor `after/`) |
|----------------|----------------------------------------|
| H1 — `DESIGN.md` ↔ shipped IA (Services row) | Live source `DESIGN.md` L127, L178, L358 (Operator-only framing); shipped IA visible in `dr8-dev-03-services.png` (Developer Services exists) and `dr8-ops-01-system.png` (operator console mounts Services) |
| H2(a) `TenantScope` discriminated union | `routes/admin/observability.tsx:153` — `tenant ?? "all"`; `routes/admin/observability.tsx:147-156` ScopeChip; visible in `dr8-ops-05-observability.png` |
| H2(b) drop `extractTenantId` | `routes/admin/observability.tsx:219-223` (regex against `event.source`); no surface evidence — code only |
| H2(c) `SubDrawerItem<TId>` generic | `routes/admin/observability.tsx:135` `as AdminObservabilityTab`; `routes/app/observability.tsx:208` `as ObservabilityTab` |
| H3(a) tile `LoadingValue<T>` | `dr8-dev-01-overview.png` (every Developer Overview tile is `—`); `dr8-ops-01-system.png` (every Operator System tile is `—`) |
| H3(b) `/admin/tenants` 404 → diagnostic envelope | No screenshot — surface not in the 17-cap walk. Code reference: `routes/admin/tenants.tsx` (rendered `Request failed: 404` in header during four-reviewer pass) |
| H3(c) status-bar canonicalization | `dr8-dev-01-overview.png` header chip says `TENANT beta`; status bar reads `tenant: _nimbus` (mismatch on same screen) |
| H3(d) disabled-tab affordance | `dr8-dev-07-observability.png`, `dr8-ops-05-observability.png` (Events/Errors render dim with no chip) |
| H4 ScopeChip casing | `dr8-dev-03-services.png` shows `TENANT: BETA`; every other dev screen shows `TENANT beta` (`dr8-dev-01-overview.png`, `dr8-dev-02-compute.png`, `dr8-dev-04-schedules.png`, `dr8-dev-05-storage.png`, `dr8-dev-07-observability.png`) |
| H4 palette mode buttons | `dr8-x-01-cmdk.png` (mode buttons render lowercase non-tracked) |
| H4 chevron-mono breadcrumb | `dr8-dev-05-storage.png` — `routes/app/storage.tsx` already imports `Breadcrumb` and renders it at L101; pre-existing component covers the canonical pattern. Re-audit per phase to confirm all three target sites (storage, compute/$function detail, lens header) consume it |
| H4 EmptyState mono title | Every screenshot with an EmptyState (e.g. `dr8-ops-05-observability.png` Runs empty state title) |
| H4 lens separator `›` | `dr8-x-02-lens.png` — header reads `_nimbus · {view.label}` (system-tenant-lens.tsx:44 uses `·`) |
| H4 `/admin/network` default section | No dedicated screenshot (Network not in the 17-cap set); `routes/admin/network.tsx:161` references `label="all"` ScopeChip for the "no section" state |
| H5 spec backfills | Code-only; no visual evidence |
| H6 live-doc "14 of 16" → "12 in-scope" | `docs/plans/README.md` blurb for `desktop-ui-design-review-fixes-plan.md` (current line ~82) |
| H7 `/admin/services/$service` evidence gap | Predecessor's missing capture — not present in `after/`; closed by this wave under `h7-admin-services-detail.png` |

## Plan accuracy delta found at H0

- Predecessor plan H5 lists `"Follow-up plans will surface"` in two
  route files. Grep across `packages/nimbus-ui/src` returns **one** hit
  only:
  `routes/admin/services_.$service.tsx:234`. The Developer-side
  service-detail route (`routes/app/services_.$service.tsx`) has no
  matching prose. H5 still deletes the one extant string; no scope
  reduction.
- `components/breadcrumb.tsx` already exists with the `›` separator
  and per-segment copy chip. H4 wiring is the work; the component
  itself doesn't need to be created.

## Reads completed at H0

- `DESIGN.md` L100–270 (IA + Developer screens), L340–410 (Network +
  Services + Observability + Settings), L820–970 (function runner,
  palette, status bar, breadcrumb, copy chip, toasts, empty states,
  diff, keyboard hints, system tenant lens).
- `packages/nimbus-ui/src/routes/admin/observability.tsx` (full, 363 lines).
- `packages/nimbus-ui/src/routes/app/observability.tsx` L180–240.
- `packages/nimbus-ui/src/shell/sub-drawer.tsx` (full, 195 lines).
- `packages/nimbus-ui/src/shell/use-tenant-bootstrap.ts` (full, 75 lines).
- `packages/nimbus-ui/src/shell/keyboard-contract.tsx` (full, 78 lines).
- `packages/nimbus-ui/src/shell/status-bar.tsx` (full, 204 lines).
- `packages/nimbus-ui/src/components/empty-state.tsx` (full, 81 lines).
- `packages/nimbus-ui/src/components/breadcrumb.tsx` (full, 69 lines —
  already exists, `›` separator already in place).

Zero edits this phase. Scope confirmed; H1 ready to start.
