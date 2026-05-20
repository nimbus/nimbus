# Plan: UI Persona Route Rename (`/app` → `/developer`, `/admin` → `/operator`)

Canonical plan for aligning the desktop UI's URL prefixes with the
persona names the DU-shell work standardized on. Today the URL prefixes
`/app` and `/admin` carry a mismatch with the canonical persona labels
"Developer" and "Operator" — this plan closes that gap end-to-end.

This is mechanical, pre-launch, no migration shims. Single landing.

---

## Status

- **Status:** `todo` (ready to start, no dependencies)
- **Primary owner:** this plan
- **Activation gate:** none — the DU-redesign CS10 baseline already
  established the canonical persona names; this is the matching URL
  alignment.
- **Rough size:** one focused pass, ~50 file edits + 2 directory moves
  + 1 codegen regeneration. Verification is the larger half.

## Goal

Rename the UI's two persona-home URL prefixes so the URL bar, route
file system layout, persona classifier, and persona labels all use the
same word:

| Persona   | Before          | After                |
| --------- | --------------- | -------------------- |
| Developer | `/app/*`        | `/developer/*`       |
| Operator  | `/admin/*`      | `/operator/*`        |

After landing, the persona classifier `pathname.startsWith("/admin") ?
"operator" : "developer"` collapses to `pathname.split("/")[1]` because
the URL segment **is** the persona name. The translation map in
`view-switcher.tsx` becomes the identity map and can be deleted.

## Non-Goals

These names look related but stay untouched. Each has an explicit
reason it does not move:

- **`/api/admin/deploy`** — server HTTP contract for the `nimbus
  deploy` CLI. Capability-gated wire endpoint, not a UI persona prefix.
  Renaming touches the CLI deploy code, server router, policy
  classifier, Cloud Functions adapter, and four test files for zero
  UI clarity gain.
- **`LOCAL_ADMIN_TOKEN_SCOPE` constant** in `crates/nimbus-server/src/
  local_server/token.rs` — internal scope name carried on the auth
  token record. Never user-facing.
- **`nimbus auth rotate-admin` CLI subcommand** — CLI verb name.
  Describes the capability tier (admin-token rotation), not a persona.
- **`~/Library/Application Support/nimbus/auth/token` file path** — on
  disk credential location; no "admin" in the path today.
- **Field label "Local admin token"** in `crates/nimbus-server/assets/
  auth.html` — see [Phase 7](#phase-7-optional-token-label-tweak)
  for the standalone decision. Default is "leave alone": after the
  prefix rename, "admin" cleanly means capability tier (vs. the
  overloaded persona-+-capability meaning it carries today).
- **Archived plans + proof artifacts** under `docs/plans/archive/` and
  `docs/plans/proof/` — historical record of what shipped at the time.
  Do not retroactively edit.
- **Unrelated `/app` strings**: `crates/nimbus-sandbox/.../oci/
  builder.rs` `/app/server` (OCI image path) and `crates/nimbus-bin/
  src/{deploy,dev}.rs` `<tmp>/app/` (deploy walker test scratch).
  These are filesystem/container paths, not URL prefixes.

## Touch-Point Inventory

Full grounded sweep — every file that needs editing, grouped by phase.

### Phase 1 — Route directory rename (2 moves, 1 regen)

| Action | Path |
| --- | --- |
| `git mv` | `packages/nimbus-ui/src/routes/app` → `routes/developer` |
| `git mv` | `packages/nimbus-ui/src/routes/admin` → `routes/operator` |
| Regenerate | `packages/nimbus-ui/src/route-tree.gen.ts` (via `npm run codegen` in `packages/nimbus-ui`) |

The 32 route files inside the two directories move with the dir
rename; their **contents** are edited in Phase 3.

### Phase 2 — UI shell logic (8 files)

The runtime mapping between persona and prefix:

| File | Concern |
| --- | --- |
| `packages/nimbus-ui/src/shell/view-switcher.tsx` | `developer: "/app"`, `operator: "/admin"` map — becomes identity, collapse the indirection |
| `packages/nimbus-ui/src/shell/nav-entries.ts` | Nav entry `to: "/admin"`, pathname-→-persona classifier (line 200) |
| `packages/nimbus-ui/src/shell/primary-drawer.tsx` | `entry.to === "/app" \|\| entry.to === "/admin"` guard |
| `packages/nimbus-ui/src/shell/top-nav.tsx` | `pathname === "/admin/observability"` check |
| `packages/nimbus-ui/src/shell/tenant-selector.tsx` | Two `navigate({ to: "/admin/..." })` callsites |
| `packages/nimbus-ui/src/shell/function-tree-view.tsx` | `<Link to="/app/compute/$function">` deep link |
| `packages/nimbus-ui/src/shell/status-bar.tsx` | (verify — appears in grep set; confirm at edit time) |
| `packages/nimbus-ui/src/store/ui-store.ts` | Two lines (`100`, `107`) prefix-by-view lookup |

### Phase 3 — Route page components (32 files)

Each file may contain internal `<Link to="/app/...">` /
`<Link to="/admin/...">` / `navigate({ to: ... })` / store-key
prefixes. After Phase 1's `git mv` and Phase 2's shell update, the
typecheck pass surfaces the broken ones — work through the list.

Developer routes (move from `routes/app/` to `routes/developer/`):
`compute.tsx`, `compute_.$function.tsx`, `compute_.runs_.$runId.tsx`,
`files.tsx`, `index.tsx`, `observability.tsx`,
`observability/_filters.tsx`, `observability/logs.tsx`,
`observability/runs.tsx`, `observability/types.ts`, `schedules.tsx`,
`services.tsx`, `services_.$service.tsx`, `settings.tsx`,
`storage.tsx`, `storage_.$table.tsx`.

Operator routes (move from `routes/admin/` to `routes/operator/`):
`index.tsx`, `machines.tsx`, `network.tsx`, `observability.tsx`,
`services.tsx`, `services_.$service.tsx`, `settings.tsx`,
`settings/configuration.tsx`, `settings/danger-zone.tsx`,
`settings/deploys.tsx`, `settings/hooks.ts`,
`settings/integrations.tsx`, `settings/primitives.tsx`,
`settings/server-info.tsx`, `settings/sub-drawer.ts`,
`settings/types.ts`, `tenants.tsx`.

### Phase 4 — Spec / story updates (mechanical find-replace)

Shell specs:
- `nav-entries.spec.ts`
- `view-switcher.spec.tsx`
- `primary-drawer.spec.tsx`
- `sub-drawer.spec.tsx`
- `system-tenant-lens.spec.tsx`
- `top-nav.spec.tsx`
- `tenant-selector.spec.tsx`
- `use-tenant-bootstrap.spec.tsx`
- `keyboard-contract.spec.tsx`
- `status-bar.spec.tsx`

Route specs (move with their owners; update contents):
- `routes/developer/section-nav.spec.ts`
- `routes/developer/storage.spec.tsx`
- `routes/developer/services.spec.tsx`
- `routes/developer/services_.$service.spec.tsx`
- `routes/developer/observability-types.spec.ts`
- `routes/developer/compute_.runs_.$runId.spec.ts`
- `routes/operator/services.spec.tsx`
- `routes/operator/services_.$service.spec.tsx`
- `routes/operator/tenants.spec.tsx`

Stories + component specs:
- `stories/sub-drawer.stories.tsx`
- `stories/empty-state.stories.tsx`
- `components/empty-state.spec.tsx`

E2E:
- `tests/e2e/smoke.spec.ts` — 13 `/ui/app/...` and `/ui/admin/...`
  references in route navigation calls + header comments.

### Phase 5 — Root redirect, gate script, build entry

| File | Edit |
| --- | --- |
| `packages/nimbus-ui/src/routes/index.tsx` | `redirect({ to: "/app" })` → `redirect({ to: "/developer" })` |
| `scripts/verify-desktop-ui-shell-gates.sh` | Allowlist line `packages/nimbus-ui/src/routes/admin/settings/hooks.ts` → `routes/operator/settings/hooks.ts` |

### Phase 6 — DESIGN.md (15 hits)

`DESIGN.md` carries the canonical IA description. Hits cluster in:

- The persona-prefix mapping table (line 92–95: "Developer console —
  `/app/*`", "Operator console — `/admin/*`")
- The two §sidebar IA section headers (lines 104, 125)
- The persona-routing description at line 501 ("`/app/*` → Developer,
  `/admin/*` → Operator")
- The tenant-selector behavior matrix (lines 509–510:
  `/admin/observability`, `/admin/tenants?new=1`)
- Inline cross-references (`/admin/observability`, `/admin/services`,
  `/admin/tenants/_nimbus`, `/app/storage/<table>`, etc.)

Edit each occurrence; one full re-read after to catch any prose that
described the *reason* for picking `/admin` (e.g. "the admin namespace
keeps server-wide concerns…") that no longer reads naturally.

### Phase 7 — (optional) Token label tweak

Standalone decision. After the route rename, `/admin` no longer
overloads "admin" to mean both capability and persona prefix — the
word is freed up to mean exactly what it should: **capability tier**
(this is the admin-privileged token). The current label "Local admin
token" reads cleanly in that frame; the prefix rename doesn't force a
change.

Defaults:
- **Do nothing.** "Local admin token" remains accurate. Skip this
  phase.

Optional polish if we want shorter:
- Drop "Local" (redundant with the footer "Local-only • 127.0.0.1"):
  field label becomes "Admin token", and the disclosure sentence
  "the local admin token" becomes "the admin token". Two edits in
  `crates/nimbus-server/assets/auth.html`; rebuild `nimbus-bin`
  (the file is `include_str!`-baked).

Do **not** rename to "Operator token" — the token grants admin
access to *both* consoles, so a persona-named label would be
misleading.

### Phase 8 — Plan README (active narrative only)

`docs/plans/README.md` has 22 `/app` / `/admin` hits, all in the
post-DU-redesign executive summary describing what shipped. Two
options:

- **Tight edit:** update only the *currently-active* state references
  (the two-view IA description), leave historical "this is what
  landed in CS5" sentences alone since they accurately describe what
  shipped at the time.
- **Full edit:** rewrite all 22 to use new prefixes plus add a
  one-line note: "Routes were renamed from `/app`→`/developer`,
  `/admin`→`/operator` on 2026-05-20."

Recommend tight edit for honesty (the archived plans really did ship
with `/admin`).

### Phase 9 — (related cleanup, do not block on) CLI banner naming

Pre-existing terminology gap surfaced during inventory: `crates/
nimbus-bin/src/{start/boot.rs,dev.rs}` print "operator console:\t{url}"
where url is `/ui/` (the whole UI root, which redirects to the
*developer* console by default). After this rename, the gap becomes
more visible — the CLI literally points "operator console:" at
`/developer/`.

**Out of scope for this plan.** Track as a follow-up: either rename
banner to "Nimbus console:" (generic) or land per-persona launch URLs
(`nimbus auth url --as operator`). Capture in a separate plan if it
matters.

## Verification

Run in this order so each gate catches its own regressions:

1. **TanStack codegen:** in `packages/nimbus-ui`, `npm run codegen`.
   Confirm `route-tree.gen.ts` regenerates with `DeveloperIndexRoute…`
   / `OperatorIndexRoute…` imports replacing the `App…` / `Admin…`
   variants.
2. **Typecheck:** `npm run typecheck` (in `packages/nimbus-ui` or via
   the workspace alias). TanStack's typed route paths surface every
   stale `<Link to="/app/…">` as a type error — work the list to
   zero.
3. **Vitest:** `npm run test` in `packages/nimbus-ui`. Catches spec
   files that still stub `pathnameRef: { current: "/app/..." }`.
4. **Lint:** `npm run lint` (Biome). Style/import drift catch.
5. **UI build:** `npm run build` in `packages/nimbus-ui`. Verifies the
   embedded bundle that `nimbus-server` ships via `include_dir!`.
6. **Workspace gate script:** `scripts/verify-desktop-ui-shell-gates.sh`
   — the allowlist path edit in Phase 5 must hold.
7. **Rust rebuild:** `cargo build -p nimbus-bin --bin nimbus`. The UI
   bundle is embedded into the daemon binary, so a clean rebuild
   bakes the new routes.
8. **Rust gates:** `cargo fmt --all --check`, `make clippy`. Should
   be no-ops but smoke them.
9. **E2E smoke:** if running, point Playwright at the rebuilt daemon
   and assert `/ui/developer/`, `/ui/operator/...` resolve.
10. **Browser proof:** daemon restart on the tmp data dir; capture
    the two console roots (`/ui/developer/`, `/ui/operator/`) in
    both themes via the playwright-cli tooling. Store at
    `docs/plans/proof/ui-persona-route-rename/after/`.

## Risks & Mitigations

| Risk | Mitigation |
| --- | --- |
| TanStack route-tree.gen.ts not picking up renamed dirs | Run `npm run codegen` explicitly; never edit the gen file by hand. The package.json `build` script chains `codegen → tsc → vite build`. |
| Stale `<Link to="/app/...">` strings slip past typecheck | TanStack's typed router catches these. If a string was constructed dynamically, grep `"/app/"` and `"/admin/"` after edits. |
| Spec files use string literals that don't typecheck | Vitest run catches them; no router-typing on raw strings. The shell spec fixtures use `pathnameRef.current = "..."` strings. |
| Embedded UI bundle stale (axum serves old asset) | Rust rebuild step is non-skippable. The `include_dir!` proc-macro re-runs on the rebuilt vite output. |
| Daemon serving cached pages | Restart the daemon on the tmp data dir after rebuild. |
| User has bookmarks to `/admin/...` | Not applicable — pre-launch. No migration redirect needed. |

## Out-of-Tree Sanity

These were spot-checked during inventory and confirmed clean:

- **Makefile**: only one comment mentions the UI (`# Build the embedded
  operator UI bundle…`); no `/admin` or `/app` route strings. The CLI
  banner naming concern from Phase 9 lives in Rust, not Make.
- **package.json scripts**: no hardcoded UI route paths.
- **Vite / Vitest configs**: no path-prefix coupling.
- **`crates/nimbus-server` HTTP layer**: the auth-redirect targets
  `/ui/` and `/ui/auth` (not persona-specific). Launch ticket
  consumption redirects to `/ui/` — unchanged.

## Phase Status Ledger

| Phase | Status |
| --- | --- |
| 1. Route directory rename + codegen | `todo` |
| 2. UI shell logic | `todo` |
| 3. Route page components | `todo` |
| 4. Spec / story updates | `todo` |
| 5. Root redirect + gate script | `todo` |
| 6. DESIGN.md update | `todo` |
| 7. Token label tweak (optional) | `todo` (skip by default) |
| 8. Plan README update | `todo` |
| 9. CLI banner cleanup (out of scope) | `deferred` (capture follow-up only if user wants) |

## Execution Log

(populate as phases land)
