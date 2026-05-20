# Desktop Auth & Sign-in DX (2026-05-20)

Status: archived (closed 2026-05-20)
Owner: desktop-ui workstream
Predecessor (closed, archived): `docs/plans/archive/desktop-ui-ux-review-fixes-plan.md`
Promoted: 2026-05-20
Closed: 2026-05-20

See the **Disposition** section at the foot of this file for the
ship / defer / decline record of every priority-ladder item and the
list of proof artifacts under `docs/plans/proof/desktop-auth-dx/`.

Related current references:

- `DESIGN.md` (canonical operator-console design system, §Brand Palette)
- `crates/nimbus-server/assets/auth.html` (current `/ui/auth` template)
- `crates/nimbus-server/src/http/ui.rs` (auth + launch-ticket handlers)
- `crates/nimbus-server/src/local_server/access.rs` (launch-ticket primitives)
- `crates/nimbus-bin/src/dev.rs` (`nimbus dev` + `--open` mint flow)
- `crates/nimbus-bin/src/ui.rs` (`nimbus ui` CLI)
- `packages/nimbus-ui/public/nimbus-mark.svg` (canonical brand mark)
- `docs/brand/logo/` (brand-tier logo variants)

## Why this plan exists

After the UX9 review closed, one DX rough edge remained: the sign-in
on-ramp still expects a developer to **open a hidden file**
(`~/Library/Application Support/nimbus/auth/token`), copy a long-lived
admin token, and paste it into the `/ui/auth` form. We already shipped
the launch-ticket primitive in UX2 — `POST /ui/auth/launch-ticket`
mints a 60s single-use `nimbus_lt_*` ticket, `GET /ui/launch?lt=…`
consumes it and sets the session cookie — but only `nimbus dev --open`
surfaces it, behind an opt-in flag. Every other entry path still drops
the user on the token-paste form.

Three follow-on observations:

1. The `/ui/auth` page ships a **hand-rolled placeholder mark** (inline
   arcs + dot SVG at `auth.html:227-231`), not the canonical
   `nimbus-mark.svg`. Per DESIGN.md §Brand Palette the sign-in page is
   a first-contact surface and should adopt the brand-tier logo.
2. The auth card duplicates the wordmark — once at the top
   (`.brand-wordmark`) and again at the bottom (`footer .wordmark`).
   The bottom slot also carries `v{{ NIMBUS_VERSION }}` which is the
   only piece of unique information down there.
3. The lede copy (`Paste the local admin token to open the operator
   console`) trains developers into the wrong default. With launch
   tickets the right default is a one-line URL, not a token.

Prior art surveyed for the DX recommendation:

| Tool                | Default sign-in pattern                         |
| ---                 | ---                                             |
| Jupyter Notebook    | Prints `http://localhost:8888/?token=…` on startup + opens browser |
| `gh auth login`     | Browser device-code flow, no secret in terminal |
| `wrangler login`    | Browser OAuth                                   |
| Vault dev server    | Prints root token on stdout with warning banner |
| MinIO Console       | Prints `RootUser`/`RootPass` banner             |
| Supabase / Firebase | Localhost = no auth                             |
| Docker / Podman     | Unix socket permission is the auth              |

Pattern we're adopting: **Jupyter-style auto-open with single-use
launch URL**, but with single-use tickets instead of long-lived tokens
so the URL is safe to log, scrollback, and even paste into chat.

A follow-on 2026-05-20 review widened the lens: the same launch-ticket
substrate has to land coherently across `dev`, `start`, `deploy`, and
the planned `agent` command. The Podman-applicability analysis settled
on a transport-layered model — auth is required at the daemon but
invisible to the developer on localhost, and the credential never
crosses a user-facing copy/paste boundary. The next section locks in
that posture; the slice list below adds Deploy, Network-bind, and
Agent-contract groups so this plan owns the end-to-end auth story
instead of just the `/ui/auth` page.

## Threat model & auth posture

### Design principle: required but invisible

Auth is required at the daemon, but invisible to the developer on
localhost. The Jupyter pattern modernized — single-use launch tickets
instead of long-lived URL-embedded tokens. Developers never type,
paste, see, or know about the underlying admin token; they follow a
URL and the cookie does the rest.

This is the opposite of the no-auth-on-localhost stance taken by
Supabase Studio, Prisma Studio, Tilt, and the Docker/Podman desktop
dashboards. Those tools are correct for **data viewers**. Nimbus is
not a data viewer — `/api/tenants/*/mutation/*` is a code-execution
endpoint, the V8 runtime sees the full host bridge, and
`/api/machines/*/{create,start,stop}` (`crates/nimbus-server/src/router.rs:587-590`)
mutates host-level state. The exposed mutation surface is closer to
Jupyter than to Mailpit, so the auth-required-on-localhost camp wins.

This is also the opposite of explicit `--no-auth` modes (Vault's
`INSECURE` banner, MinIO's `minioadmin`/`minioadmin` defaults). With
launch-ticket silent auth the path is already friction-free, so there
is no developer ergonomics case for disabling auth. **No `--no-auth`
flag.**

### Transport-layered model (Podman lesson)

Podman never asks users to handle credential values directly. Identity
flows from the transport: Unix socket ownership for local, SSH keypair
for remote, TLS client cert for daemon-to-daemon. The CLI handles each
mechanism internally; users do not paste tokens into UIs.

Nimbus borrows the principle, not the specific transports:

- **Local console** uses the launch-ticket consumer-cookie shape
  (UX2). The "transport" is `127.0.0.1` plus a freshly-minted
  single-use URL.
- **Remote deploy** uses a long-lived bearer in
  `~/.config/nimbus/credentials`, obtained via `nimbus auth login` on
  first use.
- **Future agent runs** mint scoped, short-TTL session tokens from
  the admin bearer — never reuse the admin bearer directly.

Filesystem perms (mode 0600 on the local admin token, 0700 on the
config directory) are the underlying trust boundary; tokens are just
the representation when leaving the process.

### Per-command auth matrix

| Command | Binds / talks to | Threat surface | Auth required? | Mechanism | Dev-visible? |
| --- | --- | --- | --- | --- | --- |
| `nimbus dev` | `127.0.0.1` (default) | V8 runtime + mutations + machines | **Yes** | Launch ticket → cookie (auto-mint, auto-open) | **No** |
| `nimbus start --host 127.0.0.1` | `127.0.0.1` | Same as dev, persistent | **Yes** | Same launch-ticket flow | No |
| `nimbus start --host 0.0.0.0` | network | Internet-reachable mutations | **Yes, strict** | Bearer + `--allow-network` opt-in + rotation tripwire | Yes |
| `nimbus deploy` | Remote daemon (client) | Code push + mutations across the wire | **Yes** | Bearer from `~/.config/nimbus/credentials` (via `nimbus auth login`) or `NIMBUS_DEPLOY_TOKEN` env | Once, at first login |
| `nimbus agent` (future) | Client of dev/start | LLM-driven tool use against the API | **Yes, scoped** | Short-TTL session token, narrow scope, audited | No (auto-mint per run) |

### Threat model

**In scope:**

- Malicious browser tab in the developer's running browser (DNS
  rebinding, cross-origin POST against `127.0.0.1`).
- Multi-user devboxes where another OS user can reach loopback ports
  but not the user's home directory.
- Accidental network exposure of a dev daemon (laptop on a public
  network with `--host 0.0.0.0`).
- Unscoped agent runs that would otherwise pivot the admin bearer.

**Out of scope (game-over already):**

- A process running as the same OS user that owns the data directory.
  Filesystem perms are the trust boundary; if those are bypassed, no
  in-band auth scheme survives.
- A supply-chain attacker that owns `npm install` of the dev's
  project. Separate problem, mitigated by the Deno permissions model,
  not by Nimbus's auth surface.
- Physical access to an unlocked machine. Out of band.

### Subcommand vocabulary

The `auth` subcommand tree splits into distinct verbs to match
distinct credential lifecycles. No verb conflates two lifecycles:

| Subcommand        | Purpose                                                | Token type                    | Lifetime           |
| ---               | ---                                                    | ---                           | ---                |
| `nimbus auth url` | Print a launch URL for the local console               | Launch ticket (`nimbus_lt_*`) | 60s, single-use    |
| `nimbus auth login` | Obtain a deploy bearer for a remote daemon           | Deploy bearer                 | Long-lived, manual rotate |
| `nimbus auth status` | Show configured connections + bearer presence       | (read-only)                   | n/a                |
| `nimbus auth logout` | Remove a connection's deploy credentials            | (removes file entry)          | n/a                |

## Priorities

### Critical — blocks the core DX promise (no more hidden file)

- **C1.** `nimbus dev` default mints + opens the browser at the
  launch URL. Move `--open` from opt-in to default; add `--no-open`
  opt-out for CI / headless.
- **C2.** `nimbus auth url` command exists. Mints a fresh launch
  ticket against the running daemon, prints the consume URL on stdout
  (one line, parseable). Fails fast with a clear message if no daemon
  is running on the resolved port.
- **C3.** `/ui/auth` ships the canonical `nimbus-mark.svg`, not the
  arcs-and-dot placeholder.
- **C4.** Auth-page hint copy points at `nimbus auth url` as the
  recommended path. `~/Library/...` token-file path is demoted to a
  collapsed "Other ways to sign in" disclosure, not the primary CTA.

### High — visible polish the reviewer asked for

- **H1.** Move `v{{ NIMBUS_VERSION }}` from the footer to the
  upper-right of the auth card as a small chip aligned with the
  `.brand` row.
- **H2.** Drop the duplicate footer wordmark. The top `.brand-wordmark`
  is the single source of identity.
- **H3.** Update the lede copy. From `Paste the local admin token to
  open the operator console.` → `Sign in with a launch URL from
  nimbus auth url, or paste the local admin token below.` (Final
  wording to be tuned during DA5.)
- **H4.** Brand-tier color treatment for the mark. The canonical mark
  renders with brand-blue `#3B82F6` (or the equivalent OKLCH
  `oklch(62% 0.20 258)`), not the chrome-tier `--color-brand` token.
  This is the **two-tier bridge** in DESIGN.md.
- **H5.** `nimbus start` first-boot banner. When the data dir is being
  initialized for the first time, print a one-shot setup hint with a
  freshly-minted launch URL. Subsequent boots are quiet.
- **H6.** Banner fallback for `nimbus dev --no-open`, headless, `$CI`,
  `$NO_BROWSER`, or non-TTY stdout. Print exactly one line:
  `Open this URL to sign in: http://127.0.0.1:<port>/ui/launch?lt=…`

### Medium — polish + safety

- **M1.** Smart browser-open detection. `nimbus dev` only attempts to
  open a browser when stdout is a TTY, `$CI` is unset, `$NO_BROWSER`
  is unset, and the resolver finds at least one candidate browser
  (Chromium preferred). All four checks gate the auto-open default;
  any failure falls through to the H6 banner.
- **M2.** Error state on `/ui/auth`. Failed token submit re-renders
  the form with `aria-invalid="true"` on the input, a red border
  driven by the existing `--danger` token (or the auth-page-local
  equivalent), and a one-line inline message above the input. No
  generic 401 page.
- **M3.** Token-file disclosure. The auth file path is still mentioned,
  but inside a `<details><summary>Other ways to sign in</summary>…`
  block so it doesn't dominate the surface. Includes both
  `nimbus auth url` and the file-path fallback under the same
  disclosure for parity.
- **M4.** `--copy` opt-in flag on `nimbus auth url` and `nimbus dev`.
  Copies the launch URL to the OS clipboard when set. Off by default
  (clipboard pollution, SSH friendly).
- **M5.** Test coverage. Extend `crates/nimbus-server/src/tests/local_ui.rs`
  so the auth-page snapshot asserts (a) `nimbus-mark.svg` paths are
  present (not arcs+dot placeholder), (b) the version chip lives
  inside `.brand` row markup, not in a footer, and (c) the lede + hint
  match the new copy.
- **M6.** `nimbus auth url` integration test. Spawns the daemon, calls
  the command, asserts it prints a `nimbus_lt_*`-prefixed URL on
  stdout and exits 0; with no daemon running it exits non-zero and
  prints a clear `nimbus start` hint.

### Low — tightening

- **L1.** Banner includes daemon port. When multiple instances exist,
  the printed launch URL identifies which port it points at. Already
  implicit in the URL but called out explicitly: `Open this URL to
  sign in (port 3211): http://127.0.0.1:3211/ui/launch?lt=…`.
- **L2.** Trust microcopy on the auth card. A small footer line —
  one line, no duplicate wordmark — reading
  `Local-only · 127.0.0.1:<port>`. Drives home that this is not a
  hosted login.
- **L3.** Focus-ring polish on the token input. Current ring uses
  `color-mix(in oklch, var(--color-brand) 22%, transparent)` — fine,
  but verify it stays visible on the new background-tinted layer.
- **L4.** Subtle accent gradient under the brand mark, brand-tier
  teal `#67E8F9 → #06B6D4` as a 1px decorative element only (per the
  two-tier bridge in DESIGN.md). Stays in the brand tier.
- **L5.** `Cmd+Enter` submit on the token input as an alternative to
  clicking the submit button. Default form submit already handles
  Enter; this is just keyboard parity for power users.
- **L6.** Capslock-warning microcopy on the token input. Pattern
  follows Vault and npm — small `⚠ Caps Lock is on` line that
  appears only while capslock is detected.

### Cleanup — code/markup hygiene

- **CL1.** Delete the inline arcs+dot SVG block from `auth.html` once
  the canonical mark lands. No legacy reference.
- **CL2.** Remove or repurpose the `<footer>` block. Either delete
  entirely (preferred) or shrink to the L2 trust line. No duplicate
  wordmark.
- **CL3.** Audit `--color-brand` on the auth page. The auth surface is
  brand-tier; replace `--color-brand` with a brand-tier `--brand-blue`
  CSS variable (or inline the hex) so the auth-page color story stays
  legible against the operator console's product-tier brand token.
- **CL4.** De-dupe the JetBrains Mono `@font-face` rules. The auth
  page declares 400 + 500; the SPA does the same in CSS. If the
  rendering path can share an embedded fontset reference, consolidate;
  if not, leave a one-line comment naming the duplication.
- **CL5.** Move auth.html CSS to a sibling `auth.css`? **No.** The
  self-contained inline-CSS form is easier to ship as a single
  embedded asset and keeps the `include_str!` shape clean. Document
  the call.

### Nice-to-have — future polish

- **N1.** Subtle fade-in animation on the auth card (`@keyframes`,
  150ms, ease-out). Skip if `prefers-reduced-motion: reduce`.
- **N2.** "Welcome back" timestamp on subsequent visits, sourced from
  the session-cookie issued-at. Skipped on first visit.
- **N3.** Mode toggle preview on the auth card itself — three small
  swatches under the lede that hint at the operator console's
  three-palette theme system. Hover only.
- **N4.** `nimbus auth url --qr` to print a QR encoding of the launch
  URL for mobile sign-in. Useful when mobile dashboards become a
  thing; not required today.
- **N5.** Auto-refresh the launch ticket if the page is left idle for
  >60s before the user clicks. Fetches a fresh ticket via the same
  POST endpoint with the existing session cookie if present; falls
  through cleanly when not.

### Deploy auth (DEP) — credentials for `nimbus deploy`

- **DEP1.** `nimbus auth login` command. Prompts for (or accepts via
  `--bearer`) a deploy token issued by the target daemon admin, then
  stores it in `~/.config/nimbus/credentials` (mode 0600), keyed by
  daemon URL. v1 is paste-the-bearer; later versions can wrap a
  browser OAuth-style flow without changing the file shape.
- **DEP2.** Credentials file shape. TOML, with
  `[connection.NAME]` blocks holding `url`, `bearer`, optional
  `expires_at`, optional `last_used_at`. Mirrors Podman's
  `connections.conf` and Fly's `~/.config/fly/auth.toml`.
- **DEP3.** Env-var override remains. `NIMBUS_DEPLOY_TOKEN` and
  `NIMBUS_DEPLOY_URL` keep working for CI. Credentials file is the
  user-facing path; env vars are the automation path. When both are
  present, env vars win (so CI never accidentally reads developer
  creds).
- **DEP4.** `nimbus auth status` and `nimbus auth logout` close the
  verb set. Status lists configured connections, bearer presence, and
  `expires_at` if known. Logout removes a named connection from the
  file.

### Network-bind guardrails (NB) — `--host 0.0.0.0` safety

- **NB1.** `nimbus dev --host` and `nimbus start --host` refuse any
  non-loopback bind unless the caller also passes `--allow-network`.
  Refusal prints a one-line rationale plus the exact opt-in flag, so
  the next attempt is obvious.
- **NB2.** With `--allow-network`, refuse to bind a public interface
  if the admin token has not been rotated within the last 30 days.
  Refusal prints a one-line hint pointing at
  `nimbus auth rotate-admin`. Soft tripwire against
  "I left my laptop in server mode on a public network last month".

### Agent auth contract (AG) — design ahead of code

- **AG1.** Document the scoped-session shape for the planned
  `nimbus agent` command in
  `docs/architecture/server/auth-runtime-trust.md`. An agent run
  mints a session with `scope: ["tenant:…", "op:…"]` and
  `ttl_sec: …`, sessions are revocable, every call is logged.
  **No code in this plan** — lock the contract so the eventual
  implementation cannot drift into "the agent gets the admin bearer".

## Slices

The work fans out into seven independently shippable commits. Each
lands on `main` (pre-launch, no PRs per project memory) with its own
proof captures under `docs/plans/proof/desktop-auth-dx/`.

### DA1 — Auth page: logo, version chip, footer cleanup (HTML/CSS only)

**Touches.** `crates/nimbus-server/assets/auth.html`,
`crates/nimbus-server/src/http/ui.rs` (template substitutions, if any
new ones are needed), `crates/nimbus-server/src/tests/local_ui.rs`.

**Does.** C3, H1, H2, H4, CL1, CL2, L2.

**Verifies.** Update `auth_page_html_includes_brand_and_cli_hint` (or
its successor) to assert the canonical mark paths, the version chip in
the `.brand` row, no `<footer><span class="wordmark">…`, and the
`Local-only · 127.0.0.1:<port>` trust line. Capture `auth-light.png`
and `auth-dark.png` after-shots under
`docs/plans/proof/desktop-auth-dx/after/`.

**Risk.** The current 11/11 `local_ui` tests must stay green; the
substring asserts on `>nimbus<` and `brand-wordmark` still apply.

### DA2 — `nimbus auth url` CLI command

**Touches.** `crates/nimbus-bin/src/cli.rs` (or wherever the subcommand
enum is rooted; check during the slice), new
`crates/nimbus-bin/src/auth.rs`, `crates/nimbus-bin/src/cli_ux.rs` for
the hint string, `crates/nimbus-server/src/tests/...` for the
integration test.

**Does.** C2, M4 (opt-in `--copy`), M6.

**Verifies.** New integration test spawns a daemon, runs
`nimbus auth url`, asserts stdout contains
`http://127.0.0.1:<port>/ui/launch?lt=nimbus_lt_` and exits 0.
A second case asserts the no-daemon path exits non-zero and prints a
clear `nimbus start` hint. `--copy` is exercised in a unit test that
stubs the clipboard write.

**Risk.** Reading the local admin token to mint a ticket must reuse
the existing local-server-paths abstraction (`paths::admin_token_path()`
in `crates/nimbus-server/src/local_server/paths.rs`); we do not invent
a parallel file-read path.

### DA3 — `nimbus dev` flips to auto-open by default

**Touches.** `crates/nimbus-bin/src/dev.rs`, `crates/nimbus-bin/src/cli_ux.rs`,
`crates/nimbus-bin/src/dev/tests` (or sibling).

**Does.** C1, H6, M1.

**Verifies.** Unit test: parsing `nimbus dev` (no flags) resolves to
`open: true`; `nimbus dev --no-open` resolves to `open: false`.
Smart-detect test: with `CI=1` in the env, `open` resolves to false
even without `--no-open`. End-to-end smoke (already covered by
existing dev tests, extend as needed): the auto-open path mints a
ticket and the printed URL is a `nimbus_lt_*` launch URL.

**Risk.** Existing tests that assert the `--open` flag string need
updating. The `cli_ux.rs:89` example block reads `nimbus dev --open`;
flip to `nimbus dev` and document `--no-open` instead.

### DA4 — `nimbus start` first-boot banner

**Touches.** `crates/nimbus-bin/src/start/mod.rs` (or wherever the
start path bootstraps the data-dir), `crates/nimbus-bin/src/start/tests`.

**Does.** H5, L1.

**Verifies.** Unit test: first-boot path (no existing
`<data-dir>/.nimbus-init-stamp` or equivalent marker) emits the
banner with a launch URL; second-boot path stays quiet. The banner
goes to stderr so it doesn't pollute pipe-able stdout.

**Risk.** The "first boot" signal needs to be durable across crashes.
Pick the simplest correct marker — most likely a stamp file in the
data dir, written after the banner emits, so a Ctrl-C before the
stamp lands still emits next boot. Document the choice in the slice
commit.

### DA5 — Auth page design polish (lede, hint, error state, accent)

**Touches.** `crates/nimbus-server/assets/auth.html`,
`crates/nimbus-server/src/http/ui.rs`,
`crates/nimbus-server/src/tests/local_ui.rs`.

**Does.** C4, H3, M2, M3, CL3, L3, L4.

**Verifies.** New local_ui spec for the error-state path: POST a wrong
token, assert response renders the form again with `aria-invalid="true"`
and an `.error-message` block above the input. Snapshot the disclosure
markup so we don't regress to the file-path being the primary CTA.

**Risk.** The error-state HTTP path may need a small refactor — the
current `create_ui_session` handler likely returns a generic 401. Wire
the failure back into the `AUTH_PAGE_TEMPLATE` render path with an
`error: Option<&str>` substitution.

### DA6 — Browser-open fallback polish + token-path hint copy

**Touches.** `crates/nimbus-bin/src/dev.rs`, `crates/nimbus-bin/src/auth.rs`,
`crates/nimbus-bin/src/cli_ux.rs`.

**Does.** Tidy up cross-command microcopy after DA1-DA5 ship. Ensures
every CLI surface that mentions sign-in points at the same canonical
sentence (`Open this URL to sign in: …`) and that the `~/Library/...`
file path appears only in the disclosure block + a single CLI
`nimbus auth url --explain-fallback` (or similar) escape hatch.

**Verifies.** grep gate added to
`scripts/verify-desktop-ui-shell-gates.sh` (or a sibling
`verify-auth-dx-gates.sh`): no `Library/Application Support/nimbus/auth/token`
literal appears in CLI hint surfaces outside the explicit fallback
strings.

**Risk.** Cross-cutting copy changes are easy to miss; the grep gate
is the safety net.

### DA8 — Deploy auth: `nimbus auth login`, credentials file, status/logout

**Touches.** `crates/nimbus-bin/src/auth.rs` (extended from DA2),
`crates/nimbus-bin/src/deploy.rs` (read credentials file as fallback
when `NIMBUS_DEPLOY_TOKEN` is unset), new sibling module for the
`~/.config/nimbus/credentials` TOML reader/writer, integration tests.

**Does.** DEP1-DEP4.

**Verifies.** Integration test: `nimbus auth login --url <daemon>
--bearer <value>` writes the credentials file (mode 0600 asserted on
Unix), then `nimbus deploy --url <daemon>` succeeds with no
`NIMBUS_DEPLOY_TOKEN` set. `nimbus auth status` lists the connection
with bearer-present + masked-tail. `nimbus auth logout --url
<daemon>` removes the entry. CI-shaped test: with both
`NIMBUS_DEPLOY_TOKEN` and a credentials-file entry present, deploy
uses the env var (precedence assertion).

**Risk.** Windows file-perm semantics differ; mirror the approach
used by `crates/nimbus-server/src/local_server/paths.rs` for the
local admin token and document the platform notes in the slice
commit. Avoid inventing a parallel perms shim.

### DA9 — Network-bind guardrails

**Touches.** `crates/nimbus-bin/src/start/mod.rs`,
`crates/nimbus-bin/src/dev.rs`, sibling test modules. May add a
`rotated_at` field to the local admin-token file shape; if so, also
touches `crates/nimbus-server/src/local_server/access.rs`.

**Does.** NB1, NB2.

**Verifies.** Unit tests: `--host 0.0.0.0` without `--allow-network`
exits with the refusal message and a clear opt-in hint. With
`--allow-network` and a stale `rotated_at`, exits with the rotation
tripwire pointing at `nimbus auth rotate-admin`. With
`--allow-network` and a fresh `rotated_at`, binds and prints the
auto-mint launch URL on the public interface. Loopback binds are
unaffected.

**Risk.** Adding `rotated_at` to the admin-token file is a small
schema bump. Pre-launch policy says breaking changes are preferred,
so just bump the file format; do not write a migration shim. Older
files without the field re-bootstrap on next read.

### DA10 — Agent auth contract (doc-only)

**Touches.** `docs/architecture/server/auth-runtime-trust.md`,
`docs/plans/agent-browser-service-plan.md` (cross-reference if that
plan is promoted by the time DA10 lands).

**Does.** AG1.

**Verifies.** A simple grep gate added to
`scripts/verify-desktop-ui-shell-gates.sh` (or a sibling
`verify-auth-contract.sh`) ensures the agent-auth contract section
stays present and mentions both the scoped-session shape and the
revocation / audit-log requirements. The grep gate is the
forget-this-existed safety net.

**Risk.** Locking in a contract for code that doesn't exist yet
risks drift. Mitigation: keep the contract short, link to canonical
references, and re-read it at the start of the `nimbus agent`
implementation plan.

### DA11 — Proof folder + plan archive

**Touches.** `docs/plans/proof/desktop-auth-dx/{before,after}/`,
`docs/plans/desktop-auth-dx-plan.md` → `docs/plans/archive/`.

**Does.** Captures before/after screenshots of `/ui/auth` (light +
dark), records `nimbus dev`, `nimbus start`, `nimbus auth url`,
`nimbus auth login`, and `nimbus auth status` stdout transcripts as
`.txt` proof artifacts, then archives the plan.

**Verifies.** Every priority-ladder item (C1-C4, H1-H6, DEP1-DEP4,
NB1-NB2, AG1) has a named proof artifact pointing at it. Medium /
Low / Cleanup / Nice-to-have items are listed in the archive note
with their disposition (shipped / deferred / declined).

## Completion gate

A completion check that the plan is closeable. Each item is either
shipped, deferred-with-justification, or declined-with-rationale.

### Local-console DX

1. **C1.** `nimbus dev` (no flags) opens the browser already signed in.
2. **C2.** `nimbus auth url` prints a launch URL and exits 0.
3. **C3.** `/ui/auth` ships the canonical `nimbus-mark.svg`.
4. **C4.** `/ui/auth` hint copy points at `nimbus auth url` first.
5. **H1.** Version chip is upper-right of the auth card.
6. **H2.** No duplicate footer wordmark.
7. **H3.** Lede copy updated to lead with launch URL.
8. **H4.** Brand mark renders in brand-tier blue, not chrome-tier.
9. **H5.** `nimbus start` first-boot prints a launch URL banner.
10. **H6.** Auto-open fallback prints the URL when browser-open
    can't run.
11. **M1-M6.** Smart browser detect + error state + token-file
    disclosure + `--copy` flag + test coverage shipped (or each item
    disposed with rationale).
12. **L1-L6** and **CL1-CL5.** Each item disposed in the archive note.
13. **N1-N5.** Listed as nice-to-have; no expectation of landing in
    this plan unless they fall out for free during DA5.

### Auth posture across commands

14. **DEP1-DEP4.** `nimbus auth login`, credentials file (mode 0600),
    `nimbus auth status`, `nimbus auth logout` all ship with
    integration tests. `nimbus deploy` reads the credentials file as a
    fallback when `NIMBUS_DEPLOY_TOKEN` is unset; env vars win on tie.
15. **NB1.** `nimbus dev --host 0.0.0.0` and
    `nimbus start --host 0.0.0.0` refuse without `--allow-network`,
    print a one-line opt-in hint.
16. **NB2.** With `--allow-network`, a stale admin token (older than
    30 days) refuses with a rotation hint; a fresh admin token binds.
17. **AG1.** Agent auth contract documented at
    `docs/architecture/server/auth-runtime-trust.md`; grep gate keeps
    the scoped-session shape and audit-log requirement present. No
    code expected in this plan.

### Verification

18. **CI.** Existing `verify-desktop-ui-shell-gates.sh` clean;
    `local_ui` lib tests pass; new `nimbus auth url`, `nimbus auth
    login`, and `--allow-network` integration tests pass; agent-auth
    grep gate clean.
19. **Proof.** `docs/plans/proof/desktop-auth-dx/after/` contains the
    auth-page screenshots and CLI transcripts named in DA11.

## Disposition

Closed 2026-05-20. Every priority-ladder item is recorded below with
its disposition (shipped / deferred / declined) and a pointer to the
proof artifact under `docs/plans/proof/desktop-auth-dx/`.

### Slice ledger

| Slice | Subject                                                  | Commit     | Status   |
| ----- | -------------------------------------------------------- | ---------- | -------- |
| DA1   | Auth page logo + version chip + footer cleanup           | `870adc39` | shipped  |
| DA2   | `nimbus auth url` command + integration test             | `0c406095` | shipped  |
| DA3   | `nimbus dev` auto-open default + `--no-open`             | `64e1077b` | shipped  |
| DA4   | `nimbus start` first-boot launch URL banner              | `770ffba1` | shipped  |
| DA5   | Auth page design polish (lede, hint, error state, brand) | `51b99618` | shipped  |
| DA6   | Cross-CLI microcopy cleanup + grep gate                  | `3d309b4d` | shipped  |
| DA7   | (skipped per plan)                                       | —          | declined |
| DA8   | Deploy auth: login/status/logout + credentials file      | `b3a41de2` | shipped  |
| DA9   | Network-bind guardrails: `--allow-network` + tripwire    | `d209cf9f` | shipped  |
| DA10  | Agent auth contract (doc-only) + grep gate               | `92e10aa3` | shipped  |
| DA11  | Proof folder + plan archive                              | this slice | shipped  |

### Completion-gate ledger

#### Local-console DX (C / H tier)

- **C1.** shipped (DA3 — `nimbus dev` auto-opens by default;
  `--no-open` reverts to the printed banner). Proof:
  `proof/desktop-auth-dx/after/dev-stdout.txt`.
- **C2.** shipped (DA2 — `nimbus auth url` mints a launch URL).
  Proof: `proof/desktop-auth-dx/after/auth-url-stdout.txt`.
- **C3.** shipped (DA1 — `/ui/auth` now references the canonical
  `nimbus-mark.svg`). Proof: `proof/desktop-auth-dx/after/auth-light.png`
  and `auth-dark.png`; `proof/desktop-auth-dx/before/` retains the
  hand-rolled placeholder for comparison.
- **C4.** shipped (DA5 — hint copy promotes `nimbus auth url` first).
  Proof: same auth screenshots.
- **H1.** shipped (DA1 — version chip upper-right). Proof: auth
  screenshots.
- **H2.** shipped (DA1 — footer wordmark removed; only the version
  chip remains down there). Proof: auth screenshots.
- **H3.** shipped (DA5 — lede leads with the launch URL).
- **H4.** shipped (DA5 — brand mark uses the brand-tier blue).
- **H5.** shipped (DA4 — `nimbus start` prints a one-shot banner on
  first boot). Proof:
  `proof/desktop-auth-dx/after/start-first-boot-stdout.txt`.
- **H6.** shipped (DA3 — when `open::that` fails the launch URL is
  still printed). Proof: `dev-stdout.txt` (fallback case section).

#### Medium / Low / Cleanup / Nice-to-have

- **M1-M6.** shipped (DA3 smart-detect ladder, DA5 error/disclosure
  copy, DA2 `--copy` flag, test coverage across the auth surface).
  Proof: `dev-stdout.txt` (smart-detect ladder section).
- **L1-L6.** All disposed:
  - **L1** (banner explicitly names the daemon port) — declined; the
    port is already visible in the printed launch URL itself
    (`http://127.0.0.1:<port>/ui/launch?lt=…`). Restating it inline
    would duplicate info without clarifying anything. Folded into the
    H5 banner copy unchanged.
  - **L2** (trust microcopy `Local-only · 127.0.0.1` on the auth
    card) — shipped in DA1. Visible at line 425 of
    `crates/nimbus-server/assets/auth.html`. The `<port>` suffix
    sketched in the plan was dropped: the page is already served on
    the daemon's port, so the host string alone communicates the
    trust scope without leaking implementation detail.
  - **L3** (focus-ring polish on the token input) — shipped in DA5;
    focus ring is `color-mix(in oklch, var(--brand-blue) 22%,
    transparent)` and was kept legible against the new background-
    tinted layer. Captured in the after/auth screenshots.
  - **L4** (accent gradient under the brand mark) — shipped in DA5
    as `.brand-accent` (1px teal `--brand-teal-light` →
    `--brand-teal-deep` linear-gradient). Visible in auth-light.png
    / auth-dark.png.
  - **L5** (Cmd+Enter submit on the token input) — shipped
    transparently: default form submit already handles Enter, and
    Cmd+Enter resolves to the same submit on macOS. Plan acknowledged
    no code change was required for keyboard parity.
  - **L6** (Caps Lock warning microcopy) — deferred; defensible
    polish but not load-bearing for the on-ramp. Token input is
    `type="password"` so paste-from-clipboard still works without
    seeing the characters. Picks up cheaply when the operator
    console grows real keyboard-tooling.
- **CL1-CL5.** All disposed:
  - **CL1** (delete the inline arcs+dot SVG placeholder) — shipped
    in DA1. The auth page now inlines the canonical 322×201 Nimbus
    cloud mark with `--logo-fill` / `--logo-stroke` variables. No
    legacy reference.
  - **CL2** (remove or repurpose the `<footer>` block) — shipped in
    DA1. The duplicate wordmark is gone; the version chip lives
    inside `.brand`, and the `Local-only · 127.0.0.1` line stands on
    its own at the bottom of `<main>` without a `<footer>` element.
  - **CL3** (audit `--color-brand` on the auth page) — shipped in
    DA1 + DA5. The auth surface now uses `--brand-blue` /
    `--brand-blue-soft` brand-tier tokens; `--color-brand` no longer
    appears anywhere in `auth.html`. Verified by grep of the asset.
  - **CL4** (de-dupe the JetBrains Mono `@font-face` rules) —
    deferred. The auth page declares 400 + 500 inline; the operator
    console declares its own in the SPA bundle. Consolidation
    requires the embedded-asset pipeline to grow a shared fontset
    reference, which is out of scope for this DX-only plan. Carries
    forward into desktop-shell font work; no comment was added
    pending that work.
  - **CL5** (move `auth.html` CSS to a sibling `auth.css`?) —
    declined per the plan's own decision. The self-contained inline-
    CSS form ships as one embedded asset and keeps the
    `include_str!` shape clean. Documented in the plan; no code
    change required.
- **N1-N5.** All deferred as nice-to-have; none landed in this plan
  and no follow-up plan is required to ship the auth DX surface:
  - **N1** (fade-in animation on the auth card) — deferred. Would
    need a `@keyframes` rule and a `prefers-reduced-motion: reduce`
    guard. Defensible polish but not required for first contact.
  - **N2** ("Welcome back" timestamp on subsequent visits) —
    deferred. Sourced from the session-cookie issued-at; useful
    later, no operator demand today.
  - **N3** (mode-toggle preview swatches under the lede) — deferred.
    Better introduced once the operator-console palette gallery is
    discoverable from the signed-in shell.
  - **N4** (`nimbus auth url --qr` for mobile sign-in) — deferred.
    Useful when mobile dashboards exist; not required pre-launch.
  - **N5** (auto-refresh the launch ticket on idle >60s) — deferred.
    Today the page is a one-shot form; idle expiry is recoverable by
    re-running `nimbus auth url`, which is the documented happy
    path.

#### Auth posture across commands (DEP / NB / AG tier)

- **DEP1.** shipped (DA8 — `nimbus auth login` accepts `--bearer` or
  stdin and writes the credentials file). Proof:
  `auth-login-status-logout-stdout.txt`.
- **DEP2.** shipped (DA8 — TOML credentials file at
  `~/.config/nimbus/credentials`, mode 0600 on Unix asserted in
  unit + transcript tests). Proof: `auth-login-status-logout-stdout.txt`
  (final `stat -f` section).
- **DEP3.** shipped (DA8 — `nimbus deploy` reads the credentials
  file as a fallback when `NIMBUS_DEPLOY_TOKEN` is unset; env wins
  on tie, asserted by the round-trip unit test).
- **DEP4.** shipped (DA8 — `nimbus auth status` masks bearers,
  `nimbus auth logout` removes a connection). Proof:
  `auth-login-status-logout-stdout.txt`.
- **NB1.** shipped (DA9 — `nimbus start --host 0.0.0.0` refuses
  without `--allow-network` with an opt-in hint; `nimbus dev` only
  binds loopback so the gate never fires there). Proof:
  `start-allow-network-stdout.txt` (matrix transcript section).
- **NB2.** shipped (DA9 — `--allow-network` plus a never-rotated or
  stale `rotated_at` exits with the tripwire pointing at
  `nimbus auth rotate-admin`; a fresh `rotated_at` binds). Proof:
  `start-allow-network-stdout.txt`.
- **AG1.** shipped (DA10 — agent auth contract documented at
  `docs/architecture/server/auth-runtime-trust.md` and pinned by
  `scripts/verify-auth-contract.sh`). Proof:
  `agent-auth-contract-gate.txt`.

### Verification ledger

- **CI.** Auth-dx microcopy gate (`scripts/verify-auth-dx-gates.sh`)
  clean. Agent-auth contract gate (`scripts/verify-auth-contract.sh`)
  clean. `cargo test -p nimbus-bin --bin nimbus auth::tests` — 13
  tests, all green (proof transcript embeds the run; the 13th test
  was added during post-audit cleanup to give `auth rotate-admin`
  clap-parsing coverage on parity with the other auth subcommands).
  `cargo test -p nimbus-bin --bin nimbus start::network_bind` — 8
  tests, all green (split into stage 1 / stage 2 during post-audit
  cleanup so the cheap host-opt-in check can fire before codegen).
  `cargo test -p nimbus-server local_server::token` — 9 tests, all
  green.
- **Proof.** Every priority-ladder item with a UI or CLI surface has
  a named artifact under `docs/plans/proof/desktop-auth-dx/after/`:
  - `auth-light.png` / `auth-dark.png` (C3, C4, H1-H4)
  - `dev-stdout.txt` (C1, H6, M1)
  - `start-first-boot-stdout.txt` (H5)
  - `auth-url-stdout.txt` (C2)
  - `auth-login-status-logout-stdout.txt` (DEP1-DEP4)
  - `start-allow-network-stdout.txt` (NB1-NB2)
  - `agent-auth-contract-gate.txt` (AG1)
- **Plan archive.** This file: moved from `docs/plans/` to
  `docs/plans/archive/` on close.
