# Desktop UI — UX/UI Review Fixes (2026-05-20)

Status: active
Owner: desktop-ui workstream
Predecessor (closed, archived): `docs/plans/archive/desktop-ui-design-review-fixes-plan.md`
Source: in-conversation review captured 2026-05-20, 26 screenshots under
        `docs/plans/proof/desktop-ui-ux-review-fixes/before/`
Promoted: 2026-05-20

Related current references:

- `DESIGN.md` (canonical operator-console design system)
- `docs/plans/archive/desktop-ui-design-review-fixes-plan.md` (prior wave)
- `docs/plans/archive/desktop-ui-shell-overhaul-plan.md`
- `docs/plans/archive/desktop-ui-architecture-residue-plan.md`

## Why this plan exists

A 2026-05-20 end-to-end UX/UI review of the operator console drove every
Developer and Operator route, exercised every interactive flow
(⌘K palette, ⌘\ system tenant lens, three-palette × two-mode theme
switching, drawer collapse, tenant create, view-switcher persistence)
and verified each against `DESIGN.md`. Headline finding:

The system **is strong** on every measured axis except the on-ramp and
three finishing bugs. OKLCH tokens, state-chip vocabulary, CopyChip
ubiquity, reactive subscriptions, persistent shell state, command
palette, tenant URL scoping — all hold up and outclass Convex
Dashboard, Firebase Console, Docker Desktop, and Podman Desktop on
their respective axes.

What hurts is concentrated in ten findings:

1. The system tenant lens **always** renders `system.status` instead of
   the route-specific view — the path-prefix lookup is for a routing
   layout that does not exist (`/storage` instead of `/app/storage`).
2. `nimbus dev --open` and `nimbus ui` both drop users on an unstyled
   token-paste form because neither CLI command mints a launch ticket;
   the server-side `launch_ticket` POST path is dead code in
   production.
3. `/ui/auth` is plain HTML — Times New Roman serif, no design tokens,
   no brand mark. First impression of the product.
4. Toast notifications cover the status-bar kbd hints.
5. Observability `LEVEL` filter is a native browser `<select>` while
   peer filters are styled inputs.
6. Storage page tells users to "Choose a tenant from the top-nav
   selector" when there are no tenants and the selector is replaced by
   a Create CTA.
7. `useRuntimeDiagnostics` fetches `/debug/runtime/metrics` and
   gracefully tolerates the 404, but the noise lands in the network
   tab on every Operator Settings visit.
8. Appearance mode toggle uses three radio buttons; everywhere else in
   the shell binary-ish toggles are segmented controls.
9. Light-mode background is essentially pure white
   (`oklch(98% 0.005 248)`); cards rely entirely on borders to read.
10. Styled auth page (when fixed) should surface a "Or run
    `nimbus dev --open` next time" hint for users who landed here by
    accident.

This plan closes those ten gaps in three waves: P0 ship-blockers, P1
visible bugs, P2 refinements. Cleanup follows.

Pre-launch policy applies: prefer breaking changes; no compat shims;
no feature flags for legacy behavior. The `launch_ticket` path is
either wired up or removed — no dead code retained.

## Outcome

After this plan:

- ⌘\ system tenant lens reflects the **current route**. Opening it on
  `/app/storage` shows the `tables` view; `/admin/machines` shows
  `machines`; `/app/observability` shows `runs`; etc. Routes without a
  lens-eligible view still fall back to `system.status` as today.
- `nimbus dev --open` and `nimbus ui` open the SPA already signed in.
  Users never see the auth page in the happy path.
- The auth page that remains for power users (token paste, manual
  bookmark) is styled with the canonical design tokens, respects
  light/dark mode via `prefers-color-scheme`, and surfaces the
  `nimbus dev --open` shortcut.
- Toast notifications sit above the status bar exactly as DESIGN.md
  §Toast specifies; no kbd-hint collision at any viewport width.
- Every filter input on `/app/observability` and `/admin/observability`
  uses the same styled select shell; no native `<select>` survives in
  the shell.
- Storage empty-state hint reflects whether the tenant selector is
  visible or hidden.
- The `/debug/runtime/metrics` fetch either lands on a real endpoint
  or is removed; no 404 in the default flow.
- Mode toggle on `/admin/settings` is a segmented control matching the
  DEVELOPER/OPERATOR view switcher pattern.
- Light-mode background carries a subtle cool-gray tint so cards read
  without depending solely on borders.

Out of scope (deferred to follow-up plans, noted only):

- Convex-style query editor on Storage and full schema editing on
  Storage Settings (existing roadmap).
- Logs streaming UI redesign (existing roadmap).
- Tenant invite / multi-admin flows (out of pre-launch scope).
- Lighthouse / Web Vitals budget enforcement (separate verification
  plan).

## Phase status ledger

| Phase | Slice | Status |
|-------|-------|--------|
| UX0 | Read-in + baseline screenshot capture | done |
| UX1 | Lens path resolution (F1) | pending |
| UX2 | Launch ticket wiring + styled auth page (F2, F3, F10) | pending |
| UX3 | Toast positioning above status bar (F4) | pending |
| UX4 | Styled Select shell + observability filter consistency (F5) | pending |
| UX5 | Storage empty-state truth (F6) | pending |
| UX6 | Runtime diagnostics endpoint decision (F7) | pending |
| UX7 | SegmentedControl shell + mode toggle (F8) | pending |
| UX8 | Light-bg tint across palettes (F9) | pending |
| UX9 | Cleanup: tests, design-system catalog, plan close | pending |

## Roadmap detail

### UX0 — Read-in + baseline (done)

Goal: lock the before-image so UX9 has something to diff against.

Touch list:

- Move `tmp/ux-review/*.png` →
  `docs/plans/proof/desktop-ui-ux-review-fixes/before/`.
- Read once for orientation (already done in conversation, recorded
  here so a future agent has the same starting set):
  - `packages/nimbus-ui/src/styles/globals.css`
  - `packages/nimbus-ui/src/components/state-chip.tsx`
  - `packages/nimbus-ui/src/shell/system-tenant-lens.tsx`
  - `packages/nimbus-ui/src/shell/keyboard-contract.tsx`
  - `packages/nimbus-ui/src/routes/__root.tsx`
  - `packages/nimbus-ui/src/routes/app/index.tsx`
  - `packages/nimbus-ui/src/routes/admin/settings.tsx`
  - `crates/nimbus-server/src/http/ui.rs`
  - `crates/nimbus-bin/src/dev.rs`
  - `crates/nimbus-bin/src/ui.rs`

Done when:

- 26 PNGs committed under
  `docs/plans/proof/desktop-ui-ux-review-fixes/before/`.
- `tmp/ux-review/` removed.

### UX1 — Lens path resolution (F1) — Critical

Goal: ⌘\ on `/app/storage` shows the `tables` view, not `system.status`.

Root cause: `resolveLensView(pathname)` in
`packages/nimbus-ui/src/shell/system-tenant-lens.tsx:77-89` checks
`pathname.startsWith("/storage")`, `/machines`, `/network`, `/compute`,
`/observability` — but TanStack Router (basepath `/ui`) reports
pathnames like `/app/storage` and `/admin/machines`. None of the
prefix checks ever match; every page falls through to `system.status`.

The existing keyboard-contract test (`keyboard-contract.spec.tsx:43`)
only asserts `lensOpen === true`, not the view kind, so CI did not
catch this.

Fix design:

Rewrite `resolveLensView` as a structured route → view map keyed off
the trailing segment after `/app|/admin`:

```ts
const LENS_VIEW_MAP: Record<string, LensView> = {
  machines: { kind: "machines", label: "machines" },
  network: { kind: "listeners", label: "listeners" },
  storage: { kind: "tables", label: "tables" },
  compute: { kind: "functions", label: "functions" },
  observability: { kind: "runs", label: "runs" },
};

function resolveLensView(pathname: string): LensView {
  const match = pathname.match(/^\/(?:app|admin)\/([^/?#]+)/);
  const view = match ? LENS_VIEW_MAP[match[1]] : undefined;
  return view ?? { kind: "system", label: "system.status" };
}
```

Touch list:

- `packages/nimbus-ui/src/shell/system-tenant-lens.tsx` lines 77-89 —
  rewrite `resolveLensView`.
- `packages/nimbus-ui/src/shell/system-tenant-lens.spec.tsx` (new) —
  unit-test each mapping (`/app/storage` → `tables`,
  `/admin/machines` → `machines`, `/app/observability` → `runs`,
  `/admin/observability` → `runs`, `/app/settings` → `system`,
  `/admin/settings` → `system`, `/admin` → `system`).
- `packages/nimbus-ui/src/shell/keyboard-contract.spec.tsx` line 43 —
  extend the existing "opens the lens on Meta+\ from a developer
  pathname" test to also assert `resolveLensView` returns the expected
  view kind for one developer pathname and one operator pathname.

Done when:

- Both new test files pass.
- Manual verification on `/app/storage`, `/app/compute`,
  `/app/observability`, `/admin/machines`, `/admin/network`,
  `/admin/observability` shows the correct lens header subtitle
  ("_nimbus › tables", "_nimbus › functions", etc.) and that the body
  JSON matches the corresponding system-tenant query.
- Screenshots captured under
  `docs/plans/proof/desktop-ui-ux-review-fixes/after/lens-*.png` for
  each mapped route.

Risk: the existing `resolveLensView` view kinds (`machines`,
`listeners`, `system`, `tables`, `routes`, `runs`, `functions`) bind
to `useLensDocuments` queries. Verify `useQuery(api.routes.list, ...)`
target — there is no `/routes` route in the UI, so the current
`routes` view kind appears unreachable from the path-based lookup.
Decide: drop the `routes` kind, or expose it via a new path entry.
Default: drop the kind to keep the surface narrow; revisit if a
`/routes`-like Operator page lands.

### UX2 — Launch ticket wiring + styled auth page (F2, F3, F10) — Critical

Goal: typing `nimbus dev --open` or `nimbus ui` opens the SPA already
signed in. Users who land on `/ui/auth` directly see a styled page
that respects design tokens, light/dark mode, and surfaces the
`nimbus dev --open` shortcut.

Root cause:

- `crates/nimbus-server/src/http/ui.rs:114-129` already accepts a
  `launch_ticket` in the POST body to `/ui/auth/session` and mints a
  session cookie. The code path is well-tested server-side.
- Nothing on the CLI side mints a launch ticket or threads it through
  to the browser. `crates/nimbus-bin/src/dev.rs:140-175` and
  `crates/nimbus-bin/src/ui.rs:53-67` both open the raw `/ui/` URL.
- `crates/nimbus-server/src/http/ui.rs:80-84` returns inline HTML with
  no styling.

Fix design:

This phase has three coordinated changes.

**UX2a — Launch ticket mint + consume endpoints.**

Add a server-side flow that turns a CLI-presented admin token into a
short-lived single-use launch ticket, and a redirect endpoint that
consumes the ticket and sets the session cookie before redirecting to
`/ui/`. The ticket must:

- Be cryptographically random (≥128 bits of entropy).
- Be single-use (invalidated server-side on first redeem).
- Expire within ≤60 seconds of mint.
- Never be logged in plaintext; audit log records a hash or short
  prefix only.

Endpoints to add (in `crates/nimbus-server/src/http/ui.rs`):

- `POST /ui/auth/launch-ticket` — accepts admin bearer in
  `Authorization: Bearer ...`; returns `{ "ticket": "...", "url": "/ui/launch?lt=..." }`.
- `GET /ui/launch?lt=...` — validates ticket, mints session cookie via
  `create_session_for_launch_ticket`, redirects (302) to `/ui/`.

The existing POST `/ui/auth/session` `launch_ticket` body field stays
as the lower-level primitive; the new redirect endpoint wraps it.

**UX2b — CLI integration.**

In `crates/nimbus-bin/src/dev.rs` and `crates/nimbus-bin/src/ui.rs`:

1. Read the local admin token from
   `LocalServerPaths::resolve_for_current_platform()`.
2. After the server is reachable (health check passes), POST to
   `/ui/auth/launch-ticket` with the admin token.
3. Open `http://<addr>/ui/launch?lt=<ticket>` in the browser instead
   of the bare `/ui/` URL.
4. On failure (mint endpoint returns non-2xx, network error, missing
   token): fall back to opening `/ui/` and log a single-line warning;
   the user still gets a styled paste page from UX2c.

Touch list:

- `crates/nimbus-server/src/http/ui.rs` — add two endpoints; reuse
  `local_server_security.create_session_for_launch_ticket`.
- `crates/nimbus-server/src/http.rs` (or the route registry) — wire
  the new endpoints into the router.
- `crates/nimbus-bin/src/dev.rs:140-175` — replace direct `open::that`
  call with the mint+redirect flow.
- `crates/nimbus-bin/src/ui.rs:53-67` — same.
- `crates/nimbus-bin/src/local_server_client.rs` (or equivalent) —
  add `mint_launch_ticket()` helper.

**UX2c — Styled auth page.**

Replace `ui_auth()` in `crates/nimbus-server/src/http/ui.rs:80-84`
with an embedded HTML asset served from `packages/nimbus-ui/dist/`
(or a co-located `crates/nimbus-server/assets/auth.html`).

Requirements for the new page:

- Inline `<style>` block (covered by existing
  `style-src 'self' 'unsafe-inline'` CSP) using the same OKLCH tokens
  declared in `globals.css`.
- No JavaScript (avoid pinning another SHA-256 in CSP). Light/dark
  detection via `@media (prefers-color-scheme: dark)`.
- Brand mark (small SVG, inline).
- JetBrains Mono on identifiers, system-sans on prose. Self-host fonts
  by reusing the embedded `@fontsource/jetbrains-mono` `.woff2` assets
  already shipped to `dist/`.
- Form fields:
  - Token input (`<input type="password" name="token" autofocus />`)
    with monospace, design-token border, focus ring matching the SPA.
  - Submit button styled to match `.btn` shell.
- Below the form: a small muted paragraph: *"Or run
  `nimbus dev --open` next time — the CLI opens the console already
  signed in."*
- Footer with a `nimbus` wordmark and version (server-rendered).
- `aria-label`s and an `<h1>Sign in</h1>` for screen readers.

The page must validate when the SPA bundle is rebuilt: add a unit
test in `crates/nimbus-server/src/http/ui.rs` tests module asserting
the auth response includes:

- `Content-Security-Policy` header.
- The brand wordmark string.
- The `nimbus dev --open` hint string.
- A reference to the JetBrains Mono `@font-face` declaration.

Done when:

- `nimbus start` + `nimbus ui` (or `nimbus dev --open`) lands the
  user directly on `/ui/app/` with a session cookie. Verified on
  macOS; verify Linux + Windows in a follow-up if Chromium open
  semantics differ.
- The token-paste path remains for power users; the page renders with
  design tokens, brand mark, and the CLI hint.
- Manual screenshot under
  `docs/plans/proof/desktop-ui-ux-review-fixes/after/auth-light.png`
  and `…/auth-dark.png`.
- New server tests pass.

Risks:

- **Launch ticket leakage.** The ticket lands in the URL bar
  momentarily. Mitigation: `/ui/launch` redirects with `302` and
  `history.replaceState`-equivalent behavior so the ticket does not
  appear in browser history after the redirect. Also: 60-second TTL
  and single-use redemption.
- **Admin token replay against the mint endpoint.** Audit log already
  captures `auth_method` per session creation; reuse the same record
  for the mint endpoint. Rate-limit if necessary (existing
  `local_server_security` already gates by source).
- **CSP regression.** The auth page must not introduce inline JS.
  Verify the existing CSP smoke test in `ui.rs` tests covers the auth
  response too.

### UX3 — Toast positioning above status bar (F4) — High

Goal: toasts never overlap status-bar kbd hints (DESIGN.md §Toast).

Root cause: `packages/nimbus-ui/src/routes/__root.tsx:64-75` renders
`<Toaster position="bottom-right" />` with no `offset` prop. Sonner
defaults to a small bottom inset that places the toast on top of the
status bar at the default 32px status-bar height.

Fix:

- Expose status-bar height as a CSS custom property
  (`--statusbar-height`) on `:root` in `globals.css`.
- Pass `offset` to `<Toaster>` derived from that variable. Sonner
  accepts a CSS string offset:
  ```tsx
  <Toaster
    position="bottom-right"
    offset="calc(var(--statusbar-height) + 12px)"
    …
  />
  ```
- Verify `StatusBar` uses the same custom property so a future
  status-bar height change cascades automatically.

Touch list:

- `packages/nimbus-ui/src/styles/globals.css` — add
  `--statusbar-height: 32px;` (verify the actual rendered height
  first).
- `packages/nimbus-ui/src/routes/__root.tsx` — add `offset` prop.
- `packages/nimbus-ui/src/shell/status-bar.tsx` — use the variable in
  the `height`/`min-height` style.

Done when:

- Manual verification: trigger a tenant-create toast at 1280×800 and
  at 800×600; toast sits flush above the status bar at both sizes.
- Screenshot under
  `docs/plans/proof/desktop-ui-ux-review-fixes/after/toast-clearance.png`.

### UX4 — Styled Select shell + observability filter consistency (F5) — High

Goal: every filter dropdown in the shell uses the same styled
component; no native `<select>` survives.

Root cause: `packages/nimbus-ui/src/routes/app/observability.tsx` (and
the operator twin) renders the `LEVEL` filter as a native
`<select>`. CATEGORY / SOURCE / CORRELATION are styled text inputs.
Visual rhythm breaks.

Fix design:

Introduce `packages/nimbus-ui/src/components/select.tsx`:

```ts
type SelectOption<T extends string> = { value: T; label: string };

type SelectProps<T extends string> = {
  label: string;
  value: T;
  options: ReadonlyArray<SelectOption<T>>;
  onChange: (value: T) => void;
  placeholder?: string;
  testid?: string;
};
```

Implementation: a button that opens a cmdk-based popover (consistent
with the existing CommandPalette and tenant selector) with arrow-key
navigation, type-ahead, Enter to select, Escape to close. Restrained
visual: same border / focus-ring tokens as the text-input filters.

Migration:

- Replace the `<select>` on `/app/observability` with `<Select>`.
- Replace any equivalent on `/admin/observability`.
- Grep `packages/nimbus-ui/src` for additional `<select>` callsites;
  migrate or document the exemption.

Catalog story:

- Add `packages/nimbus-ui/src/catalog/select.stories.tsx` with
  default, with-placeholder, with-long-list, disabled states.
- Update `packages/nimbus-ui/CATALOG.md` to list `<Select>` as a
  shell component.

Done when:

- `grep -rn "<select" packages/nimbus-ui/src/routes` returns zero
  hits (or only documented exemptions).
- Keyboard navigation in `<Select>` matches WAI-ARIA listbox pattern:
  Arrow up/down, Home/End, type-ahead, Enter, Escape.
- Vitest for `<Select>` covers: open/close via click + Enter,
  arrow-key navigation, type-ahead selection, controlled value
  propagation.
- Manual screenshot under
  `docs/plans/proof/desktop-ui-ux-review-fixes/after/observability-filters.png`.

### UX5 — Storage empty-state truth (F6) — High

Goal: the storage page empty-state message matches the visible UI.

Root cause: `packages/nimbus-ui/src/routes/app/storage.tsx` renders
*"Choose a tenant from the top-nav selector"* unconditionally when
no tenant is scoped. But when zero tenants exist, the top-nav
selector is replaced by a `+ CREATE TENANT` CTA. The hint references
a control that is not on screen.

Fix:

- Read tenant count (or a derived "has any tenants" flag) from the
  same source the top-nav uses (likely `useQuery(api.tenants.list, …)`
  or a UI store selector).
- Render conditional copy:
  - 0 tenants: *"No tenants yet — click `+ CREATE TENANT` in the top
    nav to create one. Tables and documents scope to a tenant."*
  - ≥1 tenant, none selected: *"Pick a tenant from the top-nav
    selector to see its tables."*

Touch list:

- `packages/nimbus-ui/src/routes/app/storage.tsx` — branch the empty
  copy on the tenants query result.
- If the tenants query is not already wired here, import it.

Done when:

- Vitest covers both branches with mocked tenant lists.
- Manual verification: visit `/app/storage` with zero tenants, observe
  the new copy; create one tenant, observe the message changes.
- Screenshots under
  `docs/plans/proof/desktop-ui-ux-review-fixes/after/storage-empty-{zero,unselected}.png`.

### UX6 — Runtime diagnostics endpoint decision (F7) — High

Goal: no 404 noise in the network tab on the default Operator
Settings flow.

Root cause:
`packages/nimbus-ui/src/routes/admin/settings/hooks.ts:62-80` fetches
`/debug/runtime/metrics`, which currently returns 404 in default
deployments. The hook handles 404 gracefully (sets diagnostics to
`{}`), so the page does not break, but the 404 appears on every
Settings load.

Two acceptable paths; pick one:

**Option A — Ship the endpoint.** Add `/debug/runtime/metrics` to the
server (likely under the existing debug or telemetry surface). Return
a stable shape matching the `RuntimeDiagnostics` type already
imported by the hook. Lowest churn for the UI; this is presumably the
intent.

**Option B — Remove the fetch.** Drop `useRuntimeDiagnostics` and the
panel it feeds until the endpoint is real, then re-add together.

Recommendation: **Option A** if the runtime team has the data
available; this hook is clearly designed against a planned endpoint.
**Option B** as fallback if the endpoint is months out — don't leave
dead-but-tolerated fetches in production.

Investigation step: search `crates/nimbus-server` for any partial
implementation; verify with the runtime owner before choosing.

Done when:

- Either the endpoint returns a 2xx with the expected shape on the
  default `nimbus start`, or the hook + its callers are removed.
- A network-tab capture on `/admin/settings` shows no 4xx/5xx (capture
  under `docs/plans/proof/desktop-ui-ux-review-fixes/after/settings-network.png`).

### UX7 — SegmentedControl shell + mode toggle (F8) — Medium

Goal: the appearance mode toggle on `/admin/settings` is a segmented
control matching the DEVELOPER/OPERATOR view-switcher pattern.

Root cause: three `radio`s (`Always light`, `Always dark`, `Match OS`)
in `packages/nimbus-ui/src/routes/admin/settings.tsx` (or its
appearance sub-component). Functional, but visually inconsistent with
the existing segmented control at the top of the shell.

Fix design:

Introduce `packages/nimbus-ui/src/components/segmented-control.tsx`:

```ts
type SegmentedControlProps<T extends string> = {
  label: string;
  value: T;
  options: ReadonlyArray<{ value: T; label: string; description?: string }>;
  onChange: (value: T) => void;
  testid?: string;
};
```

Render as a row of buttons sharing a single bordered container, with
the active segment carrying `bg-surface-2` + `text-default` and the
inactive segments using `text-muted` hover-revealed `bg-surface-2`.
Keyboard: Arrow Left/Right navigates, Enter/Space selects.
`role="radiogroup"` with each segment `role="radio"` so screen
readers still get exclusive-choice semantics.

Migrate:

- `routes/admin/settings.tsx` mode toggle → `<SegmentedControl>`.
- Refactor the existing top-nav DEVELOPER/OPERATOR switcher to use
  the same component so they cannot drift.

Catalog story + docs:

- `packages/nimbus-ui/src/catalog/segmented-control.stories.tsx` —
  two-option and three-option variants, with descriptions.
- `CATALOG.md` updated.
- `DESIGN.md` §Controls updated to name `SegmentedControl` as the
  canonical exclusive-choice control for ≤4 options.

Done when:

- Mode toggle and view switcher both render via the new component.
- A11y: NVDA / VoiceOver announces "radio group, 3 of 3" semantics.
- Vitest covers controlled value propagation and arrow-key navigation.
- Screenshot under
  `…/after/appearance-mode-segmented.png`.

### UX8 — Light-bg tint across palettes (F9) — Medium

Goal: cards in light mode read against the page background without
relying solely on a 1-px border.

Root cause: `--color-bg` in light mode is essentially pure white
across all three palettes (`oklch(98% 0.005 248)` for blue,
`oklch(98% 0.003 257)` for mono, `oklch(98% 0.012 82)` for warm).
`--color-surface` is literally white (`oklch(100% 0 0)`). The
surface-on-bg contrast is invisible without the border.

Fix:

Lower light-mode `--color-bg` lightness by ~1.5–2 percentage points,
keeping per-palette hue cues:

- blue: `oklch(96.5% 0.008 248)`
- mono: `oklch(96.5% 0.006 257)`
- warm: `oklch(96.5% 0.020 82)`

Dark modes stay untouched.

Touch list:

- `packages/nimbus-ui/src/styles/globals.css` lines 38, 89, 119 — bg
  value per palette.

Visual check:

Walk every route in light mode for each of the three palettes
(9 captures total). Verify:

- Cards still read clearly (no muddy contrast).
- Code blocks / surface-2 areas remain distinguishable from surface
  and bg.
- Focus ring (`--color-accent`) is still visible against bg.

Done when:

- 9 light-mode capture diffs under
  `…/after/light-tint-{blue,mono,warm}-{overview,settings,storage}.png`.
- Lighthouse contrast check on `/app/` overview reports no new AA
  failures (verify in catalog at port 6006).

### UX9 — Cleanup, tests, design-system catalog, plan close

Goal: prevent regressions of the same shape; close the plan.

Cleanup tasks:

1. **Lens view-kind assertion gap.** Audit every keyboard-contract
   / shell behavior test to ensure the test asserts the **observable
   outcome**, not just the trigger. The lens bug slipped past because
   the test asserted state was toggled but not what was rendered.
2. **Catalog completeness.** `Select` and `SegmentedControl` shipped
   with stories; verify other shell components added since the last
   catalog audit are listed in `CATALOG.md`.
3. **DESIGN.md sync.** Update §Controls (segmented control), §Filters
   (select shell), §Toast (clarify the offset variable),
   §System Tenant Lens (drop the `routes` view kind if dropped in
   UX1), §Color (note the bg tint adjustment).
4. **Grep gate.** Add a CI check that grep-fails on `<select`,
   `<input type="radio"` outside the SegmentedControl implementation,
   and `/debug/runtime/metrics` outside the wired-up hook. Catches the
   same shape of regression.
5. **Move `tmp/ux-review/` proof.** Confirm UX0 actually completed;
   the `before/` directory should hold all 26 captures, and `tmp/`
   should be empty of review artifacts.

Verification:

- Full `make ci` clean.
- `npm run test --workspace packages/nimbus-ui` clean (vitest +
  storybook unit harness).
- `cargo test -p nimbus-server http::ui` clean.
- Drive each fixed surface in chrome-devtools-mcp; capture the
  matching `after/` screenshot for the proof folder.
- Diff `before/` vs `after/` and embed the deltas inline in this
  plan's UX9 completion notes.

Plan close:

- Move this plan to `docs/plans/archive/desktop-ui-ux-review-fixes-plan.md`.
- Update `docs/plans/README.md` (if it lists active plans).
- Update memory: add a feedback note that "lens-style path-prefix
  lookups must use the full route layout, not the leaf segment in
  isolation" so this class of bug stays surfaced.

## Completion gate

This plan is **done** when **all** of the following hold simultaneously:

1. **F1 lens fix:** ⌘\ on each of `/app/storage`, `/app/compute`,
   `/app/observability`, `/admin/machines`, `/admin/network`,
   `/admin/observability` shows the matching system-tenant view —
   confirmed by manual capture and by the new lens unit test.
2. **F2 + F3 + F10 first-run:** `nimbus dev --open` opens
   `http://127.0.0.1:<port>/ui/app/` already signed in on a clean
   user data directory (verify by clearing cookies). Direct visit to
   `/ui/auth` shows the styled page with brand mark, JetBrains Mono,
   light/dark via `prefers-color-scheme`, and the `nimbus dev --open`
   hint.
3. **F4 toast:** tenant-create toast at 1280×800 sits flush above
   the status bar with the kbd hints fully visible.
4. **F5 filters:** `grep -rn "<select" packages/nimbus-ui/src/routes`
   returns zero hits; `<Select>` is the canonical filter dropdown,
   listed in `CATALOG.md`, with a passing vitest.
5. **F6 storage hint:** `/app/storage` with zero tenants shows the
   "no tenants yet" message; with ≥1 tenant unselected shows the
   "pick a tenant" message; both branches covered by vitest.
6. **F7 diagnostics:** network tab on `/admin/settings` shows no
   `/debug/runtime/metrics` 404 (endpoint shipped) **or** the hook
   and its consumers are gone.
7. **F8 mode toggle:** appearance mode toggle is a `<SegmentedControl>`;
   the top-nav DEVELOPER/OPERATOR switcher uses the same component;
   `CATALOG.md` lists it; `DESIGN.md` §Controls names it as the
   canonical exclusive-choice control.
8. **F9 light tint:** all three palettes have the new `--color-bg`
   values; 9 light-mode captures committed under `after/`; no new
   contrast-AA failures.
9. **CI clean:** `make ci`, `npm run test`, and
   `cargo test -p nimbus-server http::ui` all clean.
10. **Proof folder complete:** `after/` holds the screenshots named
    in each phase's done-when section; `before/` holds the 26 UX0
    captures; the plan is moved to `archive/`.

Until **all ten** of these hold, the plan stays `active`.
