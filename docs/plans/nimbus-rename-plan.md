# Rename & Relocate: agentstation/neovex -> nimbus/nimbus

Canonical execution plan for renaming the project from "neovex" to "nimbus" and
relocating all repositories from the `agentstation` GitHub organization to
`nimbus`.

## Status

`pending` -- not yet started.

## Prerequisites

- `docs/plans/nimbus-rename-satellite-repos-plan.md` -- rename internals of
  `nimbus-machine-os`, `nimbus-crun`, `homebrew-tap`, and re-publish the
  Deno/`rusty_v8` fork locker tags under `nimbus/*` (Repo 4) so both sides
  of cross-repo interfaces and the `[patch.crates-io]` URL surface agree on
  names at release time.

## Context

The project is pre-launch with zero users. No backwards compatibility is
needed -- clean break everywhere. The `nimbus` GitHub organization is owned and
ready to receive transfers.

## Naming Map

| Before | After |
|--------|-------|
| **GitHub org** `agentstation` | `nimbus` |
| **Main repo** `agentstation/neovex` | `nimbus/nimbus` |
| **Machine OS repo** `agentstation/neovex-machine-os` | `nimbus/nimbus-machine-os` |
| **Crun repo** `agentstation/neovex-crun` | `nimbus/nimbus-crun` |
| **Homebrew** `agentstation/homebrew-tap` | `nimbus/homebrew-tap` (new repo, not transferred) |
| **Deno fork** `agentstation/deno` | `nimbus/deno` |
| **rusty_v8 fork** `agentstation/rusty_v8` | `nimbus/rusty_v8` |
| **Docker image** `ghcr.io/agentstation/neovex-machine-os` | `ghcr.io/nimbus/nimbus-machine-os` |
| **Binary** `neovex` | `nimbus` |
| **Rust crates** `neovex-*` | `nimbus-*` |
| **npm scope** `@neovex/*` | `@nimbus/*` |
| **npm packages** `neovex`, `@neovex/codegen`, `@neovex/firebase`, `@neovex/mongodb` | `nimbus`, `@nimbus/codegen`, `@nimbus/firebase`, `@nimbus/mongodb` |
| **Env vars** `NEOVEX_*` | `NIMBUS_*` |
| **Dot directory** `.neovex/` | `.nimbus/` |
| **Network iface** `neovex0` | `nimbus0` |
| **Systemd units** `neovex.service`, `neovex.socket` | `nimbus.service`, `nimbus.socket` |
| **Metadata namespace** `neovex_provider` | `nimbus_provider` |
| **Metadata schema/database default** `neovex_metadata` | `nimbus_metadata` |
| **Control plane DB** `neovex-control.db` / `neovex-control.sqlite3` | `nimbus-control.db` / `nimbus-control.sqlite3` |
| **Encryption sidecar ext** `.neovex-enc` | `.nimbus-enc` |
| **Bench label prefix** `neovex-libsql-replica-bench-*` | `nimbus-libsql-replica-bench-*` |
| **Homebrew cask** `agentstation/tap/neovex` | `nimbus/tap/nimbus` |
| **Release assets** `neovex_linux_arm64.tar.gz` | `nimbus_linux_arm64.tar.gz` |
| **COPR project** `agentstation/neovex` | `nimbus/nimbus` |
| **Install script URL** `neovex.dev/install.sh` | `nimbus.dev/install.sh` (or equivalent) |
| **Domain** `neovex.dev` | TBD (nimbus domain) |
| **APT GPG email** `apt@neovex.dev` | `apt@nimbus.dev` (or equivalent) |
| **Homebrew cask token** `neovex-dev` | `nimbus-dev` |
| **HTTP headers** `x-neovex-*` | `x-nimbus-*` |
| **WebSocket protocol** `neovex.v1`, `neovex.v2` | `nimbus.v1`, `nimbus.v2` |
| **V8 runtime ops** `op_neovex_*` | `op_nimbus_*` |
| **JS runtime globals** `__neovex*` | `__nimbus*` |
| **Deno extension** `ext:neovex_node22/*` | `ext:nimbus_node22/*` |
| **OCI annotations** `io.neovex.machine.*` | `io.nimbus.machine.*` |
| **Cloud-Functions virtual imports** `__neovex_cloud_functions_*`, `__neovex_firebase_*`, `__neovex_functions_framework__` | `__nimbus_*` (codegen) |
| **esbuild namespace** `neovex-cloud-functions` | `nimbus-cloud-functions` |
| **Differential field** `parsed.neovexOnly` | `parsed.nimbusOnly` |
| **Diagnostic capture token** `capture.neovex_machine_status` / `neovex-machine-status.txt` | `capture.nimbus_machine_status` / `nimbus-machine-status.txt` |
| **Org email handle** `AgentStation <opensource@agentstation.ai>` / `<oss@agentstation.ai>` | `Nimbus <opensource@nimbus.dev>` / `<oss@nimbus.dev>` (or chosen domain) |
| **Deploy admin token env** `NEOVEX_DEPLOY_TOKEN` | `NIMBUS_DEPLOY_TOKEN` |
| **Firebase admin scratch dir** `.neovex/firebase/` | `.nimbus/firebase/` |

The `convex` compatibility package keeps its name (third-party compat layer,
not neovex-branded).

---

## Phase 0: GitHub Repo Transfers & Forks (Manual)

GitHub admin operations done by the repo owner via the GitHub UI or API. Must
complete before any code changes.

1. **Transfer repos** from `agentstation` to `nimbus`:
   - `agentstation/neovex` -> `nimbus/nimbus`
   - `agentstation/neovex-machine-os` -> `nimbus/nimbus-machine-os`
   - `agentstation/neovex-crun` -> `nimbus/nimbus-crun`
   - `agentstation/deno` -> `nimbus/deno`
   - `agentstation/rusty_v8` -> `nimbus/rusty_v8`

2. **Create new `nimbus/homebrew-tap`** repo (do NOT transfer
   `agentstation/homebrew-tap` -- it is shared with other agentstation products).
   Delete `Casks/neovex.rb` from `agentstation/homebrew-tap`.
   See satellite plan for details.

3. **Verify GitHub redirects** work for old URLs.

4. **Update local clone remotes**:
   ```sh
   git remote set-url origin git@github.com:nimbus/nimbus.git
   # Also update the Deno fork and rusty_v8 fork local checkouts:
   git -C ~/src/github.com/agentstation/deno remote set-url origin git@github.com:nimbus/deno.git
   git -C ~/src/github.com/agentstation/rusty_v8 remote set-url origin git@github.com:nimbus/rusty_v8.git
   ```

5. **Move local directories** (recommended):
   ```
   ~/src/github.com/agentstation/neovex     -> ~/src/github.com/nimbus/nimbus
   ~/src/github.com/agentstation/deno        -> ~/src/github.com/nimbus/deno
   ~/src/github.com/agentstation/rusty_v8    -> ~/src/github.com/nimbus/rusty_v8
   ```

6. **Migrate Claude Code project memory** to the new project path.

---

## Phase 0a: Re-provision Secrets, Variables, Apps, and Pages

The release pipeline is held together by ~7 GitHub secrets, 4 repository
variables, 1 GitHub App, 1 Codecov project, 1 COPR project, 1 GPG key, and
1 GitHub Pages site. None of these are stored in the repo; they are
configured in GitHub/Codecov/COPR consoles and must be re-provisioned
under the new org/repo before any release workflow on `nimbus/nimbus` can
succeed end-to-end.

This phase is **manual admin work** owned by the repo owner.

### 0a.1 Repository secrets (Settings -> Secrets and variables -> Actions)

Port these secrets from `agentstation/neovex` to `nimbus/nimbus`. Some are
straight copies; others require re-issuance under the new identity.

| Secret | Used in | Action |
|--------|---------|--------|
| `GOOGLESOURCE_COOKIE` | ci.yml (5x), release.yml (2x) | Copy value as-is. chromium.googlesource.com auth cookie, not org-bound. |
| `CODECOV_TOKEN` | ci.yml | **Re-issue.** Onboard `nimbus/nimbus` on Codecov; Codecov assigns a new repo-bound token. |
| `COPR_CONFIG` | copr-srpms.yml | **Re-issue if COPR project moves.** If renaming COPR project to `nimbus/nimbus` (see contract.env), regenerate `~/.config/copr` for the new project owner and store as the secret. If COPR project stays under agentstation owner, keep existing config. |
| `MACHINE_OS_RELEASE_APP_PRIVATE_KEY` | release.yml | **Re-issue or re-install App.** See 0a.3. |
| `HOMEBREW_TAP_TOKEN` | release.yml (Update Homebrew cask) | **Re-issue.** Generate a fine-grained PAT (or App installation token) with write access to `nimbus/homebrew-tap` only. The current token grants access to `agentstation/homebrew-tap`, which is staying in agentstation; do not reuse. |
| `APT_REPOSITORY_SIGNING_KEY` | apt-repo.yml | **Re-issue.** GPG private key currently has identity `Neovex Apt Repo <apt@neovex.dev>`. Generate a new key with the new identity and target email domain (e.g. `Nimbus Apt Repo <apt@nimbus.dev>`), publish the new public key alongside the apt repo. Old key remains valid for previously-signed releases but new releases sign with the new key. |
| `APT_REPOSITORY_SIGNING_PASSPHRASE` | apt-repo.yml | Re-issue alongside the new GPG key. |

### 0a.2 Repository variables (Settings -> Secrets and variables -> Actions -> Variables)

| Variable | Used in | Action |
|----------|---------|--------|
| `MACHINE_OS_RELEASE_APP_ID` | release.yml | Set to the new App's numeric ID (see 0a.3). |
| `APT_REPOSITORY_PUBLISH` | linux-distribution-release.yml | Copy value (boolean). |
| `COPR_SUBMIT_RELEASES` | linux-distribution-release.yml | Copy value (boolean). |
| `APT_REPOSITORY_CNAME` | apt-repo.yml | **Update value.** The CNAME the apt repo's GitHub Pages site serves under (currently likely `apt.neovex.dev`). Set to the new domain (`apt.nimbus.dev` or chosen). DNS for the new CNAME must point to GitHub Pages before the next release. |

### 0a.3 GitHub App: machine-os release dispatcher

`release.yml:280-286` uses a GitHub App to mint a short-lived token that
can dispatch and observe the satellite repo's release workflow. The App is
currently scoped to `owner: agentstation, repositories: neovex-machine-os`
(literal strings hardcoded in `release.yml:284-285` -- see Phase 3a).

Choose one of:

- **(A) Reinstall existing App on `nimbus` org.** GitHub Apps support
  multi-org installation. Install the existing App on `nimbus`, grant it
  access to `nimbus/nimbus-machine-os` (workflows: read/write, contents:
  read). Reuse the existing `MACHINE_OS_RELEASE_APP_ID` and private key
  secret. Lowest churn.

- **(B) Create a new GitHub App owned by the `nimbus` org.** Generate a
  new App ID and private key. Update the `MACHINE_OS_RELEASE_APP_ID`
  variable and `MACHINE_OS_RELEASE_APP_PRIVATE_KEY` secret with new
  values. Cleaner separation, more setup.

Either way the hardcoded `owner:` / `repositories:` strings in
`release.yml:284-285` must be rewritten in Phase 3a.

### 0a.4 Codecov project

- Add `nimbus/nimbus` to Codecov (re-onboarding).
- Generate new upload token; set as the `CODECOV_TOKEN` secret on
  `nimbus/nimbus`.
- The README/Codecov badge URL changes to the new repo path; covered in
  Phase 6.
- Historical coverage on `agentstation/neovex` is read-only after transfer
  but does not block the new repo's pipeline.

### 0a.5 COPR project

`packaging/linux-distribution-contract.env` declares
`COPR_PROJECT=agentstation/neovex`. Decide:

- Keep the COPR project at `agentstation/neovex` (no rename) -- contract
  stays as-is, but downstream package references mismatch the renamed
  GitHub repo. **Not recommended** -- downstream packagers will be
  confused.
- Rename the COPR project to `nimbus/nimbus` (or create new) -- update
  `COPR_PROJECT` value in contract.env, regenerate `COPR_CONFIG` secret.
  Existing built RPMs at the old project URL stop receiving updates.
  **Recommended.**

### 0a.6 GitHub Pages site (apt repository)

`apt-repo.yml:212-218` deploys to the `github-pages` environment. After
the org transfer:

- Verify GitHub Pages is enabled on `nimbus/nimbus` (Settings -> Pages).
- Source: GitHub Actions (workflow-based deployment).
- Custom domain: set via `APT_REPOSITORY_CNAME` repo variable (0a.2). DNS
  CNAME record at the new domain must point at `nimbus.github.io`.
- HTTPS enforcement: enable.
- The published apt repo URL (used by end-user `apt-get` configurations
  in `scripts/install.sh` and docs) changes; this is covered in Phase 4.

### 0a.7 GitHub App: dependabot, codeql, etc.

- `.github/dependabot.yml` -- no neovex/agentstation refs; carries over
  with the repo transfer. Verify Dependabot security updates are enabled
  on `nimbus/nimbus` (Settings -> Code security and analysis).
- CodeQL -- not currently configured; no action needed.

### 0a.8 Repository settings

- Branch protection rules on `main` -- recreate on `nimbus/nimbus` if not
  preserved by the transfer (GitHub usually preserves protections on org
  rename, but transfer to a new org sometimes resets them; verify).
- Required status checks -- after the workflow file `verify-neovex-crun-patch.yml`
  is renamed (Phase 3c), the required-checks list on `main` must be
  updated to reference the new check name `Verify krun infrastructure`
  (the workflow's display name) on its new file path.
- Default branch -- confirm `main`.
- Issues / Discussions / Wiki -- preserve settings.
- Actions permissions -- "Allow all actions and reusable workflows" or
  the equivalent allowlist must include any third-party actions used (see
  the Phase 0a.9 list).

### 0a.9 Third-party action allowlist (Settings -> Actions -> General)

If the `nimbus` org enforces an allowlist of third-party actions (likely
yes for an enterprise-trust posture), the allowlist must permit all of
these used by the workflows:

- `actions/*` (checkout, setup-go, setup-node, upload-artifact,
  download-artifact, upload-pages-artifact, deploy-pages, attest,
  create-github-app-token, cache)
- `Swatinem/rust-cache@v2`
- `dtolnay/rust-toolchain@stable`
- `taiki-e/install-action@cargo-deny`, `taiki-e/install-action@cargo-llvm-cov`
- `codecov/codecov-action@v6`
- `shogo82148/actions-setup-perl@v1`
- `orhun/git-cliff-action@v4`

Plus the cross-repo reusable workflow:
- `nimbus/nimbus-machine-os/.github/workflows/build.yml@release-workflow-v1`

(After rename. Currently `agentstation/neovex-machine-os/...`.)

### 0a.10 Self-hosted runners

`.github/actionlint.yaml` registers `neovex-machine-os` as a known
self-hosted runner label. Searching this repo's `runs-on:` values shows
no job actually uses that label -- all jobs target `ubuntu-latest`,
`ubuntu-24.04-arm`, or `${{ matrix.runner }}` (which resolves to
GitHub-hosted images). The label exists for actionlint to validate the
satellite repo's `runs-on:` references when they are pulled in via the
reusable workflow at `release.yml:256`.

Action items:

- Rename the actionlint label to `nimbus-machine-os` (Phase 3h).
- If actual self-hosted runners are registered with the
  `neovex-machine-os` label on the GitHub org (org-level runner, not
  repo-level), update their labels in the org runner config. This is
  outside the repo and must be coordinated with whoever administers the
  runner fleet.
- If no self-hosted runners actually exist with that label (the label is
  aspirational/satellite-only), the actionlint rename is sufficient.

Verify with: org Settings -> Actions -> Runners -> filter by label.

### 0a.11 GHCR images (org-level)

- Old images at `ghcr.io/agentstation/neovex-machine-os:*` will become
  unreachable when the satellite repo transfers. Confirm no consumer
  outside this org pins the old image URL.
- Verify package visibility on `ghcr.io/nimbus/nimbus-machine-os` (the
  satellite repo's release workflow publishes here) matches the policy
  desired (public/private).
- The release workflow's `permissions: packages: write` must be granted
  at the org level for the workflow to push to GHCR.

### 0a.12 Verification

Before proceeding to Phase 0.5, verify:

```sh
# Each secret/var must list-non-empty in the new repo
gh secret list  --repo nimbus/nimbus
gh variable list --repo nimbus/nimbus

# GitHub App is installed on nimbus org with access to nimbus-machine-os
gh api orgs/nimbus/installations --jq '.installations[] | {id, app_slug, repository_selection}'

# Pages is configured
gh api repos/nimbus/nimbus/pages

# Codecov repo exists (HTTP probe)
curl -fsSL https://app.codecov.io/gh/nimbus/nimbus >/dev/null && echo OK

# COPR project exists (HTTP probe; assumes COPR rename to nimbus/nimbus)
curl -fsSL https://copr.fedorainfracloud.org/api_3/project?ownername=nimbus\&projectname=nimbus >/dev/null && echo OK
```

---

## Phase 0.5: Capture Baseline Reference Counts

Before any rewrite, record per-file and total reference counts so post-phase
verification can confirm zero residuals (case-insensitive, includes
capitalized `Neovex`, `AgentStation`, etc.):

```sh
mkdir -p .rename-audit
rg -c -i 'neovex|agentstation' \
   --glob '!Cargo.lock' --glob '!package-lock.json' \
   --glob '!node_modules/**' --glob '!target/**' \
   --glob '!data/**' --glob '!demos/convex/vendor/**' \
   --glob '!.rename-audit/**' \
   > .rename-audit/baseline-counts.txt

# Internal-symbol baselines (must reach zero except in convex compat):
rg -c '__neovex|op_neovex|neovexHost|x-neovex|ext:neovex|Symbol\.for\("neovex' \
   --glob '!node_modules/**' --glob '!target/**' \
   > .rename-audit/baseline-internal-symbols.txt
```

Re-run the same commands after each phase that touches source/scripts/CI and
diff against the baseline. The expected residuals after Phase 7 are listed in
the **Verification** section (convex compat package, vendored bundles,
Cargo.lock, package-lock.json, data files, and any explicitly preserved
upstream references).

`.rename-audit/` is a scratch directory: add to `.gitignore` if you want to
keep it across runs, otherwise delete after the rename merges.

---

## Phase 1: Rename Rust Crates & Workspace

### 1a. Rename crate directories

```
crates/neovex/         -> crates/nimbus/
crates/neovex-bin/     -> crates/nimbus-bin/
crates/neovex-core/    -> crates/nimbus-core/
crates/neovex-engine/  -> crates/nimbus-engine/
crates/neovex-runtime/ -> crates/nimbus-runtime/
crates/neovex-sandbox/ -> crates/nimbus-sandbox/
crates/neovex-server/  -> crates/nimbus-server/
crates/neovex-storage/ -> crates/nimbus-storage/
crates/neovex-testing/ -> crates/nimbus-testing/
```

### 1b. Update workspace root Cargo.toml

- `[workspace] members` paths: `crates/neovex-*` -> `crates/nimbus-*`
- `[workspace.package] repository`: `agentstation/neovex` -> `nimbus/nimbus`
- `[patch.crates-io]`:
  - **Canonical published form** points the 20 deno-family entries at
    `git = "https://github.com/agentstation/deno"` with the current locker
    tag (e.g. `tag = "v2.7.14-locker.38"`), and the 1 v8 entry at
    `git = "https://github.com/agentstation/rusty_v8"` with its own locker
    tag (`v147.4.0-locker.1`). Rewrite both to the renamed forks
    (`nimbus/deno` and `nimbus/rusty_v8`) preserving the locker tag.
  - **Temporary Codex debug state**: the working tree may have those 20
    entries replaced with `path = "/Users/jack/src/github.com/agentstation/deno/..."`
    overrides while iterating against the local Deno worktree. Before
    committing the rename, restore the canonical git+tag form (under
    `nimbus/deno`); do not ship the local-path overrides as the renamed
    baseline. If debugging continues post-rename, the path overrides should
    point at the renamed local checkout (`~/src/github.com/nimbus/deno`).
  - `Cargo.lock` regeneration in 1f must follow the renamed git URLs and the
    re-tagged locker (see satellite plan for the locker-tag re-publish step
    on the renamed Deno fork).

### 1c. Update each crate's Cargo.toml

For every crate:
- `[package] name`: `neovex-*` -> `nimbus-*`
- `[package] repository`: `agentstation/neovex` -> `nimbus/nimbus`
- `[[bin]] name`: `neovex` -> `nimbus` (in nimbus-bin)
- `[dependencies]` internal refs: `neovex-*` -> `nimbus-*`
- `[dev-dependencies]` internal refs: same

### 1d. Update all Rust source code

Global find-replace across all `.rs` files, most-specific first to avoid
partial matches.

**Crate imports and qualified paths:**
- `neovex_testing` -> `nimbus_testing`
- `neovex_storage` -> `nimbus_storage`
- `neovex_server` -> `nimbus_server`
- `neovex_sandbox` -> `nimbus_sandbox`
- `neovex_runtime` -> `nimbus_runtime`
- `neovex_engine` -> `nimbus_engine`
- `neovex_core` -> `nimbus_core`
- `neovex_bin` -> `nimbus_bin`

**CLI command name:**
- `#[command(name = "neovex"` -> `#[command(name = "nimbus"` in main.rs
- Test cases: `["neovex",` -> `["nimbus",` throughout main.rs and machine/mod.rs

**Env var names (constants and string literals):**
- `NEOVEX_` -> `NIMBUS_` (all env var references)
- `NEOVEX_SOURCE_REPO` constant -> `NIMBUS_SOURCE_REPO` with value `"nimbus/nimbus"`

**Metadata namespace defaults (affects database schema names):**
- `"neovex_provider"` -> `"nimbus_provider"` in:
  - `crates/nimbus-bin/src/main.rs` (libsql default)
  - `crates/nimbus-engine/src/persistence_config.rs` -- 3 distinct defaults
    at lines 616 (libsql), 635 (postgres), 654 (mysql); update all three
  - `crates/nimbus-storage/src/libsql.rs` (line ~126)
  - `crates/nimbus-storage/src/postgres/config.rs` (line ~16)
  - `crates/nimbus-storage/src/mysql.rs` (line ~68)
- `"neovex_metadata"` -> `"nimbus_metadata"` (default postgres
  `metadata_schema` and mysql `metadata_database`):
  - `crates/nimbus-engine/src/persistence_config.rs`
  - `crates/nimbus-storage/src/postgres/config.rs`
  - `crates/nimbus-storage/src/mysql.rs`
- `"neovex_meta_"` -> `"nimbus_meta_"` in test fixtures:
  - `crates/nimbus-engine/src/tests/libsql_replica_provider.rs`
  - `crates/nimbus-engine/src/tests/mysql_provider.rs`
  - `crates/nimbus-storage/src/tests/libsql_provider.rs`
  - `crates/nimbus-storage/src/tests/mysql_provider.rs`

**Control plane storage filenames:**
- `"neovex-control.db"` -> `"nimbus-control.db"` in
  `crates/nimbus-storage/src/async_storage/engine.rs` (line ~34)
- `"neovex-control.sqlite3"` -> `"nimbus-control.sqlite3"` in
  `crates/nimbus-storage/src/async_storage/engine.rs` (line ~35)
- `LocalKeySubject::control_plane("neovex-control.db")` ->
  `"nimbus-control.db"` in `crates/nimbus-storage/src/encryption/subject.rs`
  and `crates/nimbus-storage/src/encryption/tests.rs`

**Encryption sidecar extension:**
- `".neovex-enc"` -> `".nimbus-enc"` in
  `crates/nimbus-storage/src/encryption/*` (subject + tests)

**Bench fixture labels:**
- `"neovex-libsql-replica-bench-..."` -> `"nimbus-libsql-replica-bench-..."`
  in `crates/nimbus-engine/benches/libsql_replica_provider_benchmarks/fixtures.rs`
  (line ~117)

**Dot directory paths:**
- `".neovex/"` -> `".nimbus/"` (license path, manifest path, single-flight)
- `DEFAULT_LICENSE_PATH` in `crates/nimbus-server/src/license/mod.rs`

**Network constants** in `crates/nimbus-sandbox/src/backends/oci/network.rs`:
- `DEFAULT_NETWORK_NAME: &str = "neovex"` -> `"nimbus"`
- `DEFAULT_NETWORK_INTERFACE: &str = "neovex0"` -> `"nimbus0"`

**Systemd unit references** in `crates/nimbus-bin/src/machine/bootstrap.rs`:
- `"neovex.socket"` -> `"nimbus.socket"`
- `"neovex.service"` -> `"nimbus.service"`

**Guest paths:**
- `"/.neovex/neovex-guest-user-switch"` -> `"/.nimbus/nimbus-guest-user-switch"` in:
  - `crates/nimbus-sandbox/src/backends/krun/vm.rs`
  - `crates/nimbus-sandbox/tests/krun_linux_smoke.rs`

**Machine path construction:**
- `.join("neovex").join("machine")` -> `.join("nimbus").join("machine")` in
  `crates/nimbus-bin/src/machine/mod.rs`

**Docker image references:**
- `ghcr.io/agentstation/neovex-machine-os` -> `ghcr.io/nimbus/nimbus-machine-os`

**OCI media types** (must match satellite repo rename):
- `application/vnd.neovex.machine.disk.layer.v1.*` ->
  `application/vnd.nimbus.machine.disk.layer.v1.*`
  (in `crates/nimbus-bin/src/machine/manager.rs` test fixtures)

**OCI annotation keys** in `crates/nimbus-bin/src/machine/manager.rs`:
- `io.neovex.machine.attestation.repository` -> `io.nimbus.machine.attestation.repository`
- `io.neovex.machine.neovex.version` -> `io.nimbus.machine.nimbus.version`
  (both constant names and test fixture values)

**HTTP header names:**
- `x-neovex-admin-token` -> `x-nimbus-admin-token` in:
  - `crates/nimbus-server/src/router.rs`
  - `crates/nimbus-server/src/local_server/policy.rs`
  - `crates/nimbus-server/src/tests/local_server_security.rs`
- `x-neovex-surface` -> `x-nimbus-surface` in:
  - `crates/nimbus-server/src/adapters/cloud_functions/http.rs`
- `x-neovex-http` -> `x-nimbus-http` in:
  - `crates/nimbus-server/src/adapters/cloud_functions/http.rs`

**WebSocket protocol identifiers:**
- `"neovex.v1"` -> `"nimbus.v1"` in `crates/nimbus-bin/src/token.rs`
- `"neovex.v2"` -> `"nimbus.v2"` in:
  - `crates/nimbus-testing/src/websocket_fixture.rs`
  - `crates/nimbus-server/src/error_envelope.rs`
  - `crates/nimbus-server/src/tests/local_ui.rs`
- `"neovex.v3"` -> `"nimbus.v3"` in:
  - `crates/nimbus-server/src/tests/websocket_protocol.rs`

**V8 runtime op names** (Deno ops registered in the V8 runtime):
- `op_neovex_*` -> `op_nimbus_*` globally across all `.rs` and `.js` files.
  These appear in Rust op declarations, JS `globalThis.__neovexAsyncHostValue()`
  calls, and codegen emit templates. Major families:
  - `op_neovex_ctx_query`, `op_neovex_ctx_mutation`, `op_neovex_ctx_action`
  - `op_neovex_ctx_paginated_query`, `op_neovex_ctx_query_paginate`
  - `op_neovex_http_route`
  - `op_neovex_runtime_stat`, `op_neovex_runtime_mkdir`, etc.

**Deno extension namespace** in `crates/nimbus-runtime/`:
- `ext:neovex_node22/*` -> `ext:nimbus_node22/*` in:
  - `src/runtime/bootstrap/node22_runtime.rs` (extension declaration)
  - `src/runtime/bootstrap/js/node22_runtime_bootstrap.js` (imports)
  - `src/runtime/bootstrap/transpile.rs` (import rewrites)
  - `src/module_loader.rs` (module resolution)

**JS runtime globals and internal symbols** (in `.rs` inline JS, bootstrap
`.js`, and codegen `.mjs` templates):
- `globalThis.__neovexInvoke` -> `globalThis.__nimbusInvoke`
- `globalThis.__neovexInvokeNamedLocal` -> `globalThis.__nimbusInvokeNamedLocal`
- `globalThis.__neovexCreateContext` -> `globalThis.__nimbusCreateContext`
- `globalThis.__neovexAsyncHostValue` -> `globalThis.__nimbusAsyncHostValue`
- `globalThis.__neovexSyncHostValue` -> `globalThis.__nimbusSyncHostValue`
- `globalThis.__neovexHostValue` -> `globalThis.__nimbusHostValue`
  (used in `crates/nimbus-runtime/src/runtime/tests/basic_invocation.rs:485`)
- `globalThis.__neovexPerfHooksBuiltin` -> `globalThis.__nimbusPerfHooksBuiltin`
- `error.neovexHostError` / `"neovexHostError"` -> `"nimbusHostError"` in error
  propagation (codegen templates, bootstrap JS, inline test JS)
- `globalThis.__neovexTargets` -> `globalThis.__nimbusTargets` in cloud
  functions codegen
- `globalThis.__neovexRuntimeContract` -> `globalThis.__nimbusRuntimeContract`
- `__neovex_internal:codegen` -> `__nimbus_internal:codegen` function name
- `__neovexCompileTime` -> `__nimbusCompileTime` in compile-time interpreter
- `__neovexResolver` -> `__nimbusResolver` in planner/evaluate

**JS Symbol.for identifiers:**
- `Symbol.for("neovex.runtimeEnvOverlay")` -> `Symbol.for("nimbus.runtimeEnvOverlay")`
  in `crates/nimbus-runtime/src/runtime/bootstrap/source.rs` and bootstrap JS
- `Symbol.for("neovex.runtimeEnvDeleted")` -> `Symbol.for("nimbus.runtimeEnvDeleted")`
- `Symbol.for("neovex.readlinePromptPatched")` -> `Symbol.for("nimbus.readlinePromptPatched")`
  in `crates/nimbus-runtime/src/module_loader.rs`
- `Symbol.for("neovex.readlineTabCompletePatched")` ->
  `Symbol.for("nimbus.readlineTabCompletePatched")`

**JS codegen marker constants** in `packages/codegen/src/constants.mjs`:
- `__neovexConvexArg` -> `__nimbusConvexArg`
- `__neovexConvexOperation` -> `__nimbusConvexOperation`
- `__neovexConvexQueryState` -> `__nimbusConvexQueryState`
- `__neovexConvexRequest` -> `__nimbusConvexRequest`
- `__neovexConvexResult` -> `__nimbusConvexResult`
- `__neovexConvexHttpResponse` -> `__nimbusConvexHttpResponse`

**Repo references in code (comments, strings, attestation):**
- `agentstation/neovex-machine-os` -> `nimbus/nimbus-machine-os`
- `agentstation/neovex-crun` -> `nimbus/nimbus-crun`
- `agentstation/neovex` -> `nimbus/nimbus`

**Log and error messages:**
- `"loaded neovex license"` -> `"loaded nimbus license"` in main.rs
- `"neovex license warning"` -> `"nimbus license warning"` in main.rs
- `"neovex listening"` -> `"nimbus listening"` in main.rs
- `"neovex uses TSI networking"` -> `"nimbus uses TSI networking"` in
  service/compose.rs
- `"not yet supported by neovex"` -> `"not yet supported by nimbus"` in
  service/compose.rs

**Hardcoded local paths in tests/benchmarks** (remove or make generic):
- `/Users/jack/src/github.com/agentstation/neovex/` references in:
  - `crates/nimbus-bin/src/machine/backend.rs`
  - `crates/nimbus-bin/src/machine/client.rs`
  - `crates/nimbus-engine/benches/mysql-provider-benchmarks.rs`
  - `crates/nimbus-engine/benches/postgres-provider-benchmarks.rs`

### 1e. Rename asset template files

```
crates/nimbus-bin/src/machine/assets/neovex.socket.tmpl  -> nimbus.socket.tmpl
crates/nimbus-bin/src/machine/assets/neovex.service.tmpl -> nimbus.service.tmpl
```

Update descriptions inside templates:
- `"Neovex API Socket"` -> `"Nimbus API Socket"`
- `"Neovex API Service"` -> `"Nimbus API Service"`

Update code that references template filenames (in bootstrap.rs or manager.rs).

Update template variable names:
- `{guest_neovex_socket}` -> `{guest_nimbus_socket}`
- `{guest_neovex_bin}` -> `{guest_nimbus_bin}`
- `{guest_neovex_data_dir}` -> `{guest_nimbus_data_dir}`
- `{guest_neovex_control_dir}` -> `{guest_nimbus_control_dir}`

### 1f. Regenerate Cargo.lock

```sh
cargo generate-lockfile
```

### 1g. Update deny.toml

`[sources] allow-git` currently has two entries:
- `https://github.com/agentstation/deno_core` -- **stale**; Cargo.toml no
  longer references this URL (the workspace moved to the monorepo fork
  `agentstation/deno`). Delete this entry rather than rewriting it.
- `https://github.com/agentstation/rusty_v8` -- live; rewrite to
  `https://github.com/nimbus/rusty_v8`.

After the satellite plan re-publishes the renamed Deno fork at the locker
tag under `nimbus/deno`, the canonical Cargo.toml `[patch.crates-io]` will
contain a `git = "https://github.com/nimbus/deno"` URL. If your `deny.toml`
denies `unknown-git`, add `https://github.com/nimbus/deno` to `allow-git`.

License reference (if renaming the license):
- `"Neovex Community License"` -> `"Nimbus Community License"` in any
  `deny.toml`/source-header references and in `LICENSE`/`LICENSING.md`.

Order constraint: run this step **after** the satellite plan re-tags the
renamed Deno fork (so the locker tag exists at `nimbus/deno`) and after
1b/1f update Cargo.toml + Cargo.lock to point at the new URL+tag, otherwise
`make deny` will fail.

**Key files:**
- `Cargo.toml` (root)
- `deny.toml`
- `crates/*/Cargo.toml` (9 files)
- All `.rs` files (~200+ references across ~50+ files)
- `crates/nimbus-bin/src/machine/assets/*.tmpl` (2 template files)

---

## Phase 2: Rename JS Packages

### 2a. Rename package directories

```
packages/neovex/  ->  packages/nimbus/
```

`packages/codegen/`, `packages/convex/`, `packages/firebase/`, and
`packages/mongodb/` keep their directory names.

### 2b. Update package.json files

- Root `package.json`:
  - `"name": "neovex-workspace"` -> `"nimbus-workspace"`
  - workspaces path `packages/neovex` -> `packages/nimbus`
  - workspaces path `demos/neovex/html` -> `demos/nimbus/html`
  - npm script references: `cargo run -p neovex-bin` -> `nimbus-bin`
- `packages/nimbus/package.json`: `"name": "neovex"` -> `"nimbus"`
- `packages/codegen/package.json`: `"name": "@neovex/codegen"` ->
  `"@nimbus/codegen"`, bin `"neovex-codegen"` -> `"nimbus-codegen"`
- `packages/firebase/package.json`: `"name": "@neovex/firebase"` ->
  `"@nimbus/firebase"`
- `packages/mongodb/package.json`: `"name": "@neovex/mongodb"` ->
  `"@nimbus/mongodb"`
- `packages/convex/package.json`: update deps `"neovex"` -> `"nimbus"`,
  `"@neovex/codegen"` -> `"@nimbus/codegen"`
- Demo `package.json` files (explicit list -- update each):
  - `demos/firebase/html/package.json`: dep `"@neovex/firebase": "*"` ->
    `"@nimbus/firebase": "*"`
  - `demos/mongodb/node/package.json`: dep `"@neovex/mongodb": "0.1.22"` ->
    `"@nimbus/mongodb": "0.1.22"`
  - `demos/neovex/html/package.json`: name `"neovex-html"` -> `"nimbus-html"`,
    dep `"neovex": "*"` -> `"nimbus": "*"`
  - `demos/convex/{html,http,node}/package.json`: keep `"convex": "*"` dep
    (compat package keeps its name) -- only update other neovex refs

### 2c. Rename demo directories

```
demos/neovex/  ->  demos/nimbus/
```

`demos/convex/`, `demos/firebase/`, and `demos/mongodb/` keep their names.

### 2d. Update JS source imports and codegen templates

- All `import ... from "neovex"` -> `"nimbus"`
- All `import ... from "@neovex/codegen"` -> `"@nimbus/codegen"`
- All `import ... from "@neovex/firebase"` -> `"@nimbus/firebase"`
- All `import ... from "@neovex/mongodb"` -> `"@nimbus/mongodb"`
- `packages/codegen/src/emit/generated_files.mjs`: Update codegen marker
  `// Generated by @neovex/codegen` -> `// Generated by @nimbus/codegen`
- `packages/codegen/src/emit/runtime_bundle_preamble.mjs`: any neovex refs
- `packages/codegen/src/cloud_functions/bundle.mjs`: virtual import
  identifiers and esbuild plugin namespace. Rename each:
  - `__neovex_cloud_functions_entry__` -> `__nimbus_cloud_functions_entry__`
  - `__neovex_cloud_functions_shared__` -> `__nimbus_cloud_functions_shared__`
  - `__neovex_firebase_functions_v2__` -> `__nimbus_firebase_functions_v2__`
  - `__neovex_firebase_functions_v2_firestore__` -> `__nimbus_firebase_functions_v2_firestore__`
  - `__neovex_firebase_functions_v2_https__` -> `__nimbus_firebase_functions_v2_https__`
  - `__neovex_firebase_admin_app__` -> `__nimbus_firebase_admin_app__`
  - `__neovex_firebase_admin_firestore__` -> `__nimbus_firebase_admin_firestore__`
  - `__neovex_functions_framework__` -> `__nimbus_functions_framework__`
  - esbuild namespace string `"neovex-cloud-functions"` ->
    `"nimbus-cloud-functions"`
- `packages/convex/src/differential.mjs`: parsed-result field
  `parsed.neovexOnly` -> `parsed.nimbusOnly` (lines ~135, ~143, ~653)
- `packages/convex/src/cli.mjs`: any neovex refs
- `scripts/convex-demo-overlay.mjs`: neovex references (~6 refs)
- `demos/index.html`: neovex references
- `demos/neovex/html/src/main.ts`: neovex references
- `demos/firebase/html/src/main.ts`: neovex references
- `demos/mongodb/node/script.ts`: neovex references

### 2e. Regenerate demo generated files and vendor bundle

After updating the codegen templates, regenerate all files in:
- `demos/convex/http/convex/_generated/`
- `demos/convex/html/convex/_generated/`
- `demos/convex/node/convex/_generated/`

These all contain `// Generated by @neovex/codegen. Do not edit by hand.` headers.

Also rebuild `demos/convex/vendor/browser.bundle.js`. The pre-built bundle
embeds runtime constants (`NEOVEX_WEBSOCKET_PROTOCOL`,
`NEOVEX_CLIENT_CAPABILITIES`, etc.) that must be regenerated from the
renamed SDK source rather than sed-rewritten in place. Treat the bundle as
a build artifact: delete and rebuild via the demo's bundler entry, then
commit the regenerated file.

Demo env vars and script literals to update:
- `demos/convex/node/script.ts`: `NEOVEX_NATIVE_URL`, `NEOVEX_CONVEX_URL`,
  `NEOVEX_NODE_DEMO_AUTHOR` -> `NIMBUS_*`
- `demos/mongodb/node/script.ts`: `NEOVEX_MONGODB_HOST`,
  `NEOVEX_MONGODB_PORT` -> `NIMBUS_*`
- `demos/convex/http/src/main.ts`, `demos/convex/html/src/main.tsx`,
  `demos/convex/html/src/App.tsx`: `VITE_NEOVEX_*` -> `VITE_NIMBUS_*`

### 2f. Update top-level tests/ directory

- `tests/demos.smoke.md`: Update `cargo run -p neovex-bin` -> `nimbus-bin`,
  demo URL path `demos/neovex/html/` -> `demos/nimbus/html/`
- `tests/runtime/node/networking-canaries/package.json`:
  `"name": "neovex-networking-canaries"` -> `"nimbus-networking-canaries"`
- `tests/runtime/node/networking-canaries/README.md`: `neovex-runtime` ->
  `nimbus-runtime`
- `tests/runtime/node/networking-canaries/bundles/{express,fastify,axios,socket-io,undici}.mjs`:
  each contains `globalThis.__neovexInvoke` and `x-neovex-trace` header --
  rewrite to `__nimbusInvoke` / `x-nimbus-trace` in all 5 bundle fixtures
- Regenerate `tests/runtime/node/networking-canaries/package-lock.json`

### 2g. Regenerate package-lock.json

```sh
npm install
```

**Key files:**
- `package.json` (root + 5 packages + demo apps)
- `packages/codegen/src/emit/*.mjs`
- `packages/convex/src/cli.mjs`
- `scripts/convex-demo-overlay.mjs`
- `demos/index.html`
- Demo source files (`demos/*/src/*.ts`, `demos/*/node/*.ts`)
- All `_generated/` files in demos
- `package-lock.json`

---

## Phase 3: CI/CD & Workflows

### 3a. .github/workflows/release.yml (~60+ references)

The most reference-dense file.

**Build artifacts:**
- `cargo build --release -p neovex-bin` -> `nimbus-bin`
- `neovex_linux_arm64.tar.gz` -> `nimbus_linux_arm64.tar.gz` (all OS/arch)
- `neovex_${{ matrix.os }}_${{ matrix.arch }}` -> `nimbus_*` (artifact names)
- `target/release/neovex` -> `target/release/nimbus` (binary path)
- `target/release/neovex.exe` -> `target/release/nimbus.exe` (Windows)
- `neovex_*` -> `nimbus_*` (download patterns)

**Cross-repo references (specific line numbers):**
- `release.yml:256` -- reusable workflow `uses:` reference
  `agentstation/neovex-machine-os/.github/workflows/build.yml@release-workflow-v1`
  -> `nimbus/nimbus-machine-os/.github/workflows/build.yml@release-workflow-v1`
  (the `release-workflow-v1` tag itself stays unless the satellite plan
  cuts a new ref)
- `release.yml:262` -- `image_reference: docker://ghcr.io/agentstation/neovex-machine-os:...`
  -> `docker://ghcr.io/nimbus/nimbus-machine-os:...`
- `release.yml:284-285` -- hardcoded App scope strings
  `owner: agentstation` -> `owner: nimbus`,
  `repositories: neovex-machine-os` -> `repositories: nimbus-machine-os`
- `release.yml:312` -- GH API path
  `repos/agentstation/neovex-machine-os/actions/workflows/publish.yml/dispatches`
  -> `repos/nimbus/nimbus-machine-os/...`
- `release.yml:329, 347, 356, 357` -- `gh` CLI flags
  `--repo agentstation/neovex-machine-os` -> `--repo nimbus/nimbus-machine-os`
- `release.yml:472, 487, 494, 498, 517, 527, 530, 537` -- Homebrew cask
  metadata + GH API paths embed `agentstation/neovex` and
  `agentstation/homebrew-tap/contents/Casks/neovex.rb`. Rewrite to
  `nimbus/nimbus` and `nimbus/homebrew-tap/contents/Casks/nimbus.rb`.

**Generic class-level rewrites (apply across release.yml):**
- `neovex_version` -> `nimbus_version` (workflow input names)
- `neovex_artifact_name` -> `nimbus_artifact_name`
- `neovex-machine-os-${{ github.run_id }}` -> `nimbus-machine-os-*`

**Homebrew cask formula (lines ~384-457):**
- `cask "neovex"` -> `cask "nimbus"`
- `name "neovex"` -> `name "nimbus"`
- `homepage` URL -> `nimbus/nimbus`
- `binary "neovex"` -> `binary "nimbus"`
- All download URLs: `agentstation/neovex/releases/.../neovex_*` ->
  `nimbus/nimbus/releases/.../nimbus_*`
- Caveats text: `neovex --help` -> `nimbus --help`, etc.
- `/tmp/neovex.rb` -> `/tmp/nimbus.rb`
- `repos/agentstation/homebrew-tap/contents/Casks/neovex.rb` ->
  `repos/nimbus/homebrew-tap/contents/Casks/nimbus.rb`
- Commit messages: `"Update neovex to"` -> `"Update nimbus to"`, etc.
- xattr command: `staged_path}/neovex` -> `staged_path}/nimbus`

**Attestation:**
- `--owner agentstation` -> `--owner nimbus`
- Attestation URLs pointing at `agentstation/neovex`

**Error messages:**
- `"neovex-machine-os release workflow"` -> `"nimbus-machine-os release workflow"`

### 3b. .github/workflows/ci.yml

- `cargo test -p neovex-runtime` -> `nimbus-runtime`
- `--exclude neovex-runtime` -> `--exclude nimbus-runtime`
- `NEOVEX_REQUIRE_EXTERNAL_PROVIDER_FIXTURES` -> `NIMBUS_*`
- `NEOVEX_TEST_POSTGRES_URL` -> `NIMBUS_TEST_POSTGRES_URL`
- `NEOVEX_MYSQL_URL` -> `NIMBUS_MYSQL_URL`
- `NEOVEX_LIBSQL_URL` -> `NIMBUS_LIBSQL_URL`
- `NEOVEX_LIBSQL_ADMIN_URL` -> `NIMBUS_LIBSQL_ADMIN_URL`
- `--name neovex-libsql-coverage` -> `nimbus-libsql-coverage`
- `neovex_coverage_probe` -> `nimbus_coverage_probe` (libsql namespace)
- `docker logs neovex-libsql-coverage` -> `nimbus-libsql-coverage`
- `bash -n scripts/collect-neovex-*` and
  `bash scripts/verify-neovex-*` invocations follow the script-rename in
  Phase 4a (~6 lines)
- `shared-key: ci-ubuntu-stable` cache key is project-agnostic; **no
  rename required** but worth verifying after the workspace rename so
  Phase 1 doesn't accidentally invalidate every cache entry without
  cause. (rust-cache@v2 already partitions by Cargo.lock hash so any
  invalidation will be intentional.)
- `GOOGLESOURCE_COOKIE` references are pass-through secret reads; no
  workflow-level rename needed (the secret value itself is provisioned
  in Phase 0a.1).
- `CODECOV_TOKEN` reference is a secret read; the token value must be
  re-issued for the renamed Codecov project (Phase 0a.4).

### 3c. .github/workflows/verify-neovex-crun-patch.yml

- **Rename file** to `verify-nimbus-crun-patch.yml`.
- Update self-references in `paths:` filter inside the same file (currently
  lines 21 and 41 each list `.github/workflows/verify-neovex-crun-patch.yml`;
  rewrite both to the new filename in the same edit so the workflow still
  triggers on its own changes).
- The workflow body has no other neovex/agentstation refs (display name is
  "Verify krun infrastructure"). After the file rename + path-filter update,
  the body is clean.
- **Branch protection**: required-status-check entries on `main` reference
  this workflow's display name, not its filename. The display name does not
  change, so required-checks should continue to resolve. Verify after
  rename.

### 3d. .github/workflows/linux-distribution-release.yml (~13 references)

Orchestrates Linux packaging workflows. References to update:
- Input descriptions: `"Neovex GitHub release tag"` -> `"Nimbus GitHub release tag"`
- Calls to reusable workflows referencing `neovex_version`,
  `neovex_crun_version` input names

### 3e. .github/workflows/linux-packages.yml (~33 references)

Builds `.deb` and `.rpm` packages.
- Workflow input names: `neovex_version` -> `nimbus_version`,
  `neovex_crun_version` -> `nimbus_crun_version`
- Input descriptions: `"Neovex release version"` -> `"Nimbus"`
- All internal references to `neovex-bin`, `neovex-crun`, `neovex_linux_*`
  artifact names
- Embedded `gh release download` calls reference
  `--repo agentstation/neovex` and `--repo agentstation/neovex-crun`
  -- rewrite to `nimbus/nimbus` and `nimbus/nimbus-crun`

### 3f. .github/workflows/apt-repo.yml (~40 references)

Builds and publishes the apt repository.
- Workflow input names: `neovex_version` -> `nimbus_version`,
  `neovex_crun_version` -> `nimbus_crun_version`
- Input descriptions: `"Neovex release version"` -> `"Nimbus"`
- All repository/package name references
- Embedded download URLs at lines ~106 and ~111:
  `https://github.com/agentstation/neovex/releases/download/${NEOVEX_RELEASE_TAG}/neovex_linux_*`
  -> rewrite host org and artifact prefix
- Step env-var names referenced inside steps:
  `NEOVEX_RELEASE_TAG`, `NEOVEX_VERSION` -> `NIMBUS_*`
- The `environment: github-pages` block stays (Pages env is GitHub-managed,
  not project-named) but the deployment target URL is determined by
  Pages settings on the renamed repo (Phase 0a.6) and the
  `APT_REPOSITORY_CNAME` repo variable.

### 3g. .github/workflows/copr-srpms.yml (~36 references)

Builds and submits SRPMs to COPR.
- Workflow input names: `neovex_version` -> `nimbus_version`,
  `neovex_crun_version` -> `nimbus_crun_version`
- Input descriptions: `"agentstation/neovex"` -> `"nimbus/nimbus"`,
  `"agentstation/neovex-crun"` -> `"nimbus/nimbus-crun"`
- COPR project: `agentstation/neovex` -> `nimbus/nimbus`
- Embedded `gh release download` calls reference
  `--repo agentstation/neovex` and `--repo agentstation/neovex-crun` --
  rewrite to `nimbus/nimbus` and `nimbus/nimbus-crun`
- The `secrets: inherit` flow propagates `COPR_CONFIG` from the caller
  (`linux-distribution-release.yml`); the `COPR_CONFIG` secret value must
  be regenerated for the renamed COPR project (Phase 0a.5).

### 3h. .github/actionlint.yaml

- Self-hosted runner label: `neovex-machine-os` -> `nimbus-machine-os`

**Key files:**
- `.github/workflows/release.yml` (~60+ changes)
- `.github/workflows/ci.yml` (~20 changes)
- `.github/workflows/verify-neovex-crun-patch.yml` (rename + update)
- `.github/workflows/linux-distribution-release.yml` (~13 changes)
- `.github/workflows/linux-packages.yml` (~33 changes)
- `.github/workflows/apt-repo.yml` (~40 changes)
- `.github/workflows/copr-srpms.yml` (~36 changes)
- `.github/actionlint.yaml`

---

## Phase 4: Scripts

### 4a. Rename script files

| Before | After |
|--------|-------|
| `scripts/build-neovex-guest-user-switch.sh` | `scripts/build-nimbus-guest-user-switch.sh` |
| `scripts/build-neovex-machine-guest-binary.sh` | `scripts/build-nimbus-machine-guest-binary.sh` |
| `scripts/collect-neovex-homebrew-cask-proof.sh` | `scripts/collect-nimbus-homebrew-cask-proof.sh` |
| `scripts/collect-neovex-machine-cli-proof.sh` | `scripts/collect-nimbus-machine-cli-proof.sh` |
| `scripts/collect-neovex-machine-diagnostics.sh` | `scripts/collect-nimbus-machine-diagnostics.sh` |
| `scripts/collect-neovex-machine-guest-proof.sh` | `scripts/collect-nimbus-machine-guest-proof.sh` |
| `scripts/collect-neovex-machine-service-proof.sh` | `scripts/collect-nimbus-machine-service-proof.sh` |
| `scripts/recreate-neovex-machine.sh` | `scripts/recreate-nimbus-machine.sh` |
| `scripts/verify-build-neovex-machine-guest-binary-helper.sh` | `scripts/verify-build-nimbus-machine-guest-binary-helper.sh` |
| `scripts/verify-neovex-homebrew-cask-proof-helper.sh` | `scripts/verify-nimbus-homebrew-cask-proof-helper.sh` |
| `scripts/verify-neovex-machine-cli-proof-helper.sh` | `scripts/verify-nimbus-machine-cli-proof-helper.sh` |
| `scripts/verify-neovex-machine-diagnostics-helper.sh` | `scripts/verify-nimbus-machine-diagnostics-helper.sh` |
| `scripts/verify-neovex-machine-guest-proof-helper.sh` | `scripts/verify-nimbus-machine-guest-proof-helper.sh` |
| `scripts/verify-neovex-machine-recreate-helper.sh` | `scripts/verify-nimbus-machine-recreate-helper.sh` |
| `scripts/verify-neovex-machine-service-proof-helper.sh` | `scripts/verify-nimbus-machine-service-proof-helper.sh` |

### 4b. Update script contents

Inside all scripts under `scripts/`:
- `NEOVEX_*` -> `NIMBUS_*` env vars
- `neovex` -> `nimbus` binary/path references
- `agentstation` -> `nimbus` org references
- `.neovex/` -> `.nimbus/` directory references

**Scripts with significant reference counts (sorted by density):**

| Script | ~Refs |
|--------|-------|
| `scripts/install.sh` | 146 |
| `scripts/build-fedora-release-srpms.sh` | 109 |
| `scripts/build-linux-release-packages.sh` | 88 |
| `scripts/verify-build-fedora-release-srpms-helper.sh` | 85 |
| `scripts/verify-build-linux-release-packages-helper.sh` | 27 |
| `scripts/verify-install.sh` | 25 |
| `scripts/verify-build-apt-repository-helper.sh` | 20 |
| `scripts/verify-release-archive-layout-helper.sh` | 18 |
| `scripts/verify-release-archive-layout.sh` | 18 |
| `scripts/verify-install-helper.sh` | 13 |
| `scripts/build-apt-repository.sh` | 11 |
| `scripts/build-neovex-machine-guest-binary.sh` | 11 |
| `scripts/verify-release-version-contract.sh` | 7 |
| `scripts/verify-build-neovex-machine-guest-binary-helper.sh` | 7 |
| `scripts/convex-demo-overlay.mjs` | 6 |
| `scripts/verify-runtime-separation.sh` | 5 |
| `scripts/build-neovex-guest-user-switch.sh` | 4 |
| `scripts/verify-runtime-separation-helper.sh` | 4 |

Also update `scripts/single-flight.sh`:
- `NEOVEX_SINGLE_FLIGHT_DIR` -> `NIMBUS_SINGLE_FLIGHT_DIR`
- `.neovex/single-flight` -> `.nimbus/single-flight`

**Install script special notes:** `scripts/install.sh` is the most
reference-dense script (146 refs). Group the rewrite into these distinct
sub-categories so none gets skipped:

1. Domain URLs: `neovex.dev` install URL, docs URL.
2. Release URLs: `agentstation/neovex`, `agentstation/neovex-crun` GitHub API
   endpoints (must follow the renamed repos).
3. Binary literals: `neovex` throughout, including `neovex --version`,
   `neovex --help` command examples.
4. Install paths: `/usr/bin/neovex`, `/usr/libexec/neovex/crun`.
5. Env vars: all `NEOVEX_*` references.
6. Homebrew cask token default: `neovex-dev`.

**Linux packaging scripts special notes:**
`scripts/build-fedora-release-srpms.sh` (109 refs) and
`scripts/build-linux-release-packages.sh` (88 refs) contain:
- RPM spec package names: `neovex`, `neovex-crun`
- Description strings: `"Neovex host CLI"`, etc.
- File paths inside packages: `/usr/bin/neovex`, `/usr/libexec/neovex/crun`,
  `/usr/share/doc/neovex/`
- Homepage URLs: `https://github.com/agentstation/neovex`

**APT repository script notes:**
`scripts/verify-build-apt-repository-helper.sh` contains:
- GPG key identity: `"Neovex Apt Repo <apt@neovex.dev>"` -- the email domain
  must match the target domain
- Maintainer field: `AgentStation <oss@agentstation.ai>` -> rewrite both the
  capitalized org token and the `agentstation.ai` email domain (decide the
  target email domain before the rename; default suggestion `oss@nimbus.dev`)

**Packager / org-email occurrences (capitalized + `agentstation.ai`):**
- `scripts/build-fedora-release-srpms.sh` (lines ~163, ~223): packager
  `AgentStation <opensource@agentstation.ai>`
- `scripts/verify-build-apt-repository-helper.sh` (line ~33): maintainer
  `AgentStation <oss@agentstation.ai>`

These literals are NOT caught by lower-case `agentstation` -> `nimbus` greps
because of the capitalization and the `.ai` TLD. Run a separate explicit
sweep:
```sh
rg -n 'AgentStation|agentstation\.ai' --glob '!node_modules/**' \
   --glob '!target/**' --glob '!Cargo.lock' --glob '!package-lock.json'
```

**Diagnostics capture token + filename:**
- `scripts/collect-neovex-machine-diagnostics.sh` (lines ~279, ~280): token
  `capture.neovex_machine_status` -> `capture.nimbus_machine_status`, output
  filename `neovex-machine-status.txt` -> `nimbus-machine-status.txt`
- `scripts/recreate-neovex-machine.sh` (lines ~335-338),
  `scripts/verify-neovex-machine-diagnostics-helper.sh`,
  `scripts/verify-neovex-machine-recreate-helper.sh` reference the same
  `neovex-machine-status.txt` filename literal -- update in lockstep with
  the collector.

**Homebrew cask proof scripts notes:**
`scripts/collect-neovex-homebrew-cask-proof.sh` and
`scripts/verify-neovex-homebrew-cask-proof-helper.sh` contain:
- Cask token default: `neovex-dev`
- Caskroom paths: `/Caskroom/neovex-dev/`

### 4c. Update script fixtures and support files

- `scripts/fixtures/crun-spec-config.json`: `"hostname": "neovex-test"` ->
  `"nimbus-test"`
- `scripts/runtime/node/generate_matrix.py`:
  - `DENO_REPO` path: `agentstation/deno` -> `nimbus/deno`
  - User-Agent: `neovex-node-compat-generator/1` -> `nimbus-node-compat-generator/1`

**Key files:** all files under `scripts/` (~30+ files, including fixtures and
node_compat)

---

## Phase 5: Makefile

~30+ references to update:

- `.PHONY` target list: rename all `*neovex*` targets to `*nimbus*`
- `cargo build --release -p neovex-bin` -> `nimbus-bin`
- `cargo bench -p neovex-engine` -> `nimbus-engine`
- `cargo run -p neovex-bin` -> `nimbus-bin`
- `cargo install --path crates/neovex-bin` -> `crates/nimbus-bin`
- All Make target names: `collect-neovex-machine-*` -> `collect-nimbus-machine-*`,
  `verify-neovex-machine-*` -> `verify-nimbus-machine-*`,
  `recreate-neovex-machine` -> `recreate-nimbus-machine`,
  `build-neovex-machine-guest-binary` -> `build-nimbus-machine-guest-binary`
- Script paths: `scripts/collect-neovex-*` -> `scripts/collect-nimbus-*`,
  `scripts/build-neovex-*` -> `scripts/build-nimbus-*`, etc.
- `NEOVEX_*` env vars passed to scripts
- Comments: `agentstation/neovex-crun` -> `nimbus/nimbus-crun`,
  `agentstation/neovex-machine-os` -> `nimbus/nimbus-machine-os`
- Prose: `"Neovex machine"` -> `"Nimbus machine"`, `"neovex runtime path"` ->
  `"nimbus runtime path"`

---

## Phase 6: Configuration & Top-Level Files

- **`cliff.toml`**: `owner = "agentstation"` -> `"nimbus"`, `repo = "neovex"` ->
  `"nimbus"`
- **`.gitignore`**: `.neovex` and `**/.neovex` -> `.nimbus` and `**/.nimbus`
- **`compose.yaml`**: (~13 references) `neovex` -> `nimbus` in service
  commands, database names, usernames, passwords, healthcheck commands
- **`CLAUDE.md`**: Update workspace layout table (all crate names), verification
  commands, all repo references, all doc path references
- **`AGENTS.md`**: Update project name references, local worktree paths
  (`~/src/github.com/agentstation/deno` -> `~/src/github.com/nimbus/deno`, etc.)
- **`ARCHITECTURE.md`**: Update crate names and org references
- **`SECURITY.md`**: Advisory URL `agentstation/neovex` -> `nimbus/nimbus`
- **`CHANGELOG.md`**: Update all comparison URLs
- **`README.md`**:
  - CI/Codecov/Release badges
  - Homebrew badge and install command
  - Download URLs and binary name in examples
  - Attestation: `--owner agentstation` -> `--owner nimbus`
  (Note: a `README.new.md` staging file referenced in earlier drafts of this
  plan is not present in the working tree. If a staged rename README is
  introduced before execution, apply the same edits to it.)
- **`LICENSE`**: Update if it references "Neovex"
- **`COMMERCIAL.md`**: (~3 references) Update Neovex/agentstation references
- **`CONTRIBUTING.md`**: (~3 references) Update project name references
- **`LICENSING.md`**: (~17 references) Update all Neovex license name
  references, org references
- **`TRADEMARKS.md`**: (~11 references) Update all Neovex trademark references
- **`deny.toml`**: `"Neovex Community License"` -> `"Nimbus Community License"`
- **`packaging/linux-distribution-contract.env`**:
  - `NEOVEX_CRUN_VERSION` -> `NIMBUS_CRUN_VERSION`
  - `COPR_PROJECT=agentstation/neovex` -> `nimbus/nimbus`
- **`.codex/config.toml`**: Comment `"Neovex project-local"` -> `"Nimbus"`,
  `writable_roots` paths: `agentstation/deno` -> `nimbus/deno`,
  `agentstation/rusty_v8` -> `nimbus/rusty_v8`
- **`.codex/rules/neovex.rules`**: Rename file to `nimbus.rules`, update
  comment `"Neovex project-local"` -> `"Nimbus"`, update crate names in
  `cargo test -p neovex-runtime` -> `nimbus-runtime`, `neovex-engine` ->
  `nimbus-engine`

---

## Phase 7: Documentation

Bulk update all docs (~30+ files). Global replacements across all
`docs/**/*.md`, applied in this order:

1. `agentstation/neovex-machine-os` -> `nimbus/nimbus-machine-os`
2. `agentstation/neovex-crun` -> `nimbus/nimbus-crun`
3. `agentstation/homebrew-tap` -> `nimbus/homebrew-tap`
4. `agentstation/deno_core` -> `nimbus/deno` (historical references -- repo no
   longer exists as standalone; the fork is now `agentstation/deno` which
   becomes `nimbus/deno`)
5. `agentstation/deno` -> `nimbus/deno`
6. `agentstation/rusty_v8` -> `nimbus/rusty_v8`
7. `agentstation/neovex` -> `nimbus/nimbus`
8. `agentstation` -> `nimbus` (remaining org-only refs)
9. `ghcr.io/agentstation/neovex-machine-os` -> `ghcr.io/nimbus/nimbus-machine-os`
10. `neovex-machine-os` -> `nimbus-machine-os` (prose references)
11. `neovex-crun` -> `nimbus-crun`
12. `NEOVEX_` -> `NIMBUS_` (env vars in docs)
13. `neovex` -> `nimbus` in product name contexts (careful: only where it refers
    to the product, not internal code)

In addition to the generic global replacements, these docs reference
specific neovex-named defaults that must change in lockstep with the code
defaults updated in Phase 1d:

- `docs/operating/storage-backends.md` (lines ~37, ~44, ~57, ~64, ~140, ~160):
  default `metadata_schema` / `metadata_database` value `neovex_metadata`
  must become `nimbus_metadata` to match the renamed Rust default
- `docs/operating/encryption.md` (lines ~71, ~107): `.neovex-enc` sidecar
  extension references must match the renamed Rust extension
- `docs/operating/deploy-admin-api.md`: `NEOVEX_DEPLOY_TOKEN` env var and
  `.neovex/firebase/` path references must match Phase 1d / Phase 6 changes

Files to update (non-exhaustive):
- `docs/README.md`
- `docs/operating/cli.md`
- `docs/operating/storage-backends.md`
- `docs/operating/encryption.md`
- `docs/operating/deploy-admin-api.md`
- `docs/adapters/convex/ai-guidelines.md`
- `docs/adapters/convex/compatibility.md`
- `docs/adapters/firebase/compatibility.md`
- `docs/adapters/firebase/migration.md`
- `docs/adapters/firebase/auth-contract.md`
- `docs/adapters/cloud-functions/compatibility.md`
- `docs/adapters/cloud-functions/migration.md`
- `docs/architecture/sandbox/microvm-service-baseline.md`
- `docs/architecture/sandbox/macos-machine-flow.md`
- `docs/architecture/sandbox/krun-vmm-host-validation.md`
- `docs/architecture/runtime/adapter-boundary.md`
- `docs/architecture/runtime/node-compat-surface-matrix.md`
- `docs/architecture/server/adapter-expectations.md`
- `docs/architecture/server/auth-runtime-trust.md`
- `docs/architecture/testing/reliability-posture.md`
- `docs/architecture/testing/ci-failure-investigation.md`
- `docs/plans/distribution-plan.md`
- `docs/plans/install-script-plan.md`
- `docs/plans/archive/encryption-at-rest-plan.md`
- `docs/plans/archive/raw-v8-warm-backend-plan.md`
- `docs/plans/archive/node-compatible-runtime-plan.md`
- `docs/plans/archive/node-lts-compatibility-plan.md`
- `docs/plans/archive/neovex-init-plan.md`
- `docs/plans/windows-machine-support-plan.md`
- `docs/plans/archive/localhost-server-security-plan.md`
- `docs/plans/archive/architecture-seam-cleanliness-plan.md`
- `docs/plans/archive/deployment-auth-runtime-boundary-plan.md`
- `docs/plans/archive/repo-architecture-and-seam-hardening-plan.md`
- `docs/plans/archive/*.md` (all archived plans)
- `docs/plans/research/*.md` (all research docs)
- `docs/plans/prompts/*.md`
- `docs/plans/security/*.md`
- `docs/architecture/*.md`

---

## Phase 8: Memory, Agent Config & Cleanup

- Update Claude Code project memory directory (new project path after local dir
  move)
- Update `MEMORY.md` entries referencing neovex
- `.claude/settings.local.json` contains hardcoded paths with
  `agentstation/neovex` for permissions -- needs manual update
- Delete or rename `.codex_tmp/neovex-local-deno-workspace-patch.toml` if still
  present (scratch file, safe to remove)
- The `.neovex/` runtime directory at the repo root (contains `single-flight/`)
  will be recreated as `.nimbus/` on first run after `.gitignore` is updated

---

## Execution Order

Phases must be executed in this order due to dependencies:

1. **Phase 0** -- manual GitHub transfers (must happen first)
2. **Phase 0a** -- secrets, vars, Apps, Pages, Codecov, COPR, GPG-key
   re-provisioning. Manual admin work; can begin in parallel with Phase 0.5
   but must complete before any release workflow runs end-to-end on the
   renamed repo.
3. **Phase 0.5** -- capture baseline reference counts (cheap, prevents
   silent residuals; rerun after each phase below)
4. **Phase 1** -- Rust crates (core rename, generates new Cargo.lock).
   Phase 1g (deny.toml) requires the satellite plan's Repo 4 locker tag
   re-publish to have completed.
5. **Phase 2** -- JS packages (depends on directory structure from Phase 1)
6. **Phase 3** -- CI/CD workflows (references crate/package names from 1-2,
   AND the secrets/vars/Apps/Pages re-provisioned in Phase 0a)
7. **Phase 4** -- Scripts (references binary/env names from Phase 1)
8. **Phase 5** -- Makefile (references everything)
9. **Phase 6** -- Config & top-level files
10. **Phase 7** -- Documentation (bulk text replacement, lowest risk)
11. **Phase 8** -- Memory & Claude config (housekeeping)

Run `make check && make clippy && make deny` after Phase 1 and again after
Phase 2 (see Verification > Per-phase gate). Compile failures here surface
missed strings while the diff is still small.

---

## Verification

### Per-phase verification gate

Run after Phase 1 (and again at the end of Phase 2) to catch missed
internal strings before they cascade:

```sh
make check         # workspace cargo check
make clippy        # full lint
make deny          # allow-git list, license list, ban list
```

Treat any failure as a missed string in the prior phase, not as an
allowable regression.

### Final verification

After all phases:

```sh
# Rust builds and all crate names resolve
cargo check --workspace

# Format check
cargo fmt --all --check

# Clippy
make clippy

# Tests pass
make test

# Deny check (allowed git sources updated)
make deny

# JS builds
npm install && npm run build --workspaces --if-present

# JS tests
npm run test --workspaces --if-present

# No stale "neovex" references remain (except convex compat package and
# explicitly-preserved upstream/historical references)
rg "neovex" --type rust --type toml --type yml --type json --type sh \
   --type js --type ts --glob '*.mjs' --glob '*.py' \
   --glob '!Cargo.lock' --glob '!package-lock.json' --glob '!packages/convex/**' \
   --glob '!node_modules/**' --glob '!data/**' \
   --glob '!.rename-audit/**'
# Should return 0 hits

# No stale "agentstation" references remain
rg "agentstation" --glob '!Cargo.lock' --glob '!package-lock.json' \
   --glob '!node_modules/**' --glob '!.rename-audit/**'
# Should return 0 hits

# Capitalized org / org-email residuals (NOT caught by the lower-case grep)
rg "Neovex|AgentStation|agentstation\.ai|neovex\.dev" \
   --glob '!Cargo.lock' --glob '!package-lock.json' \
   --glob '!node_modules/**' --glob '!packages/convex/**' \
   --glob '!.rename-audit/**'
# Should be 0 (or, if domain change is staged separately, expected residuals
# are limited to domain references documented in the deferred-domain note)

# No stale internal symbols remain
rg "__neovex|op_neovex|neovexHost|x-neovex|ext:neovex|Symbol\.for\(\"neovex" \
   --glob '!Cargo.lock' --glob '!package-lock.json' \
   --glob '!node_modules/**' --glob '!target/**' \
   --glob '!.rename-audit/**'
# Should return 0 hits

# Verify binary name
cargo run -p nimbus-bin -- --help | head -5
# Should show "nimbus" not "neovex"
```

### Expected residuals (not failures)

These are intentionally preserved:
- `packages/convex/**` -- third-party compat package keeps the `convex` name
- `Cargo.lock`, `package-lock.json` -- regenerated, not text-edited
- `data/**` -- pre-rename data files; see Risk Notes for handling
- `demos/convex/vendor/browser.bundle.js` -- only zero-residual after
  regeneration in Phase 2e

### CI/release dry-run

After all in-repo phases complete, run these end-to-end checks on the
renamed repo to catch missed env-var/secret/App/Pages provisioning:

```sh
# 1. Trigger a CI run on a branch (catches missed NEOVEX_*/secret refs)
git checkout -b rename/verify
git commit --allow-empty -m "verify CI on renamed repo"
git push -u origin rename/verify
gh pr create --fill --draft
gh run watch --exit-status

# 2. Manually dispatch verify-nimbus-crun-patch.yml (if file rename landed)
gh workflow run verify-nimbus-crun-patch.yml --repo nimbus/nimbus
gh run watch --exit-status

# 3. Dry-run the release contract verifier on a candidate tag
git tag v0.0.0-rename-dry-run
git push origin v0.0.0-rename-dry-run
# Watch verify-release-contract job; it will fail at "Require machine-os
# release app credentials" if Phase 0a.1/0a.2 are not provisioned.
# Delete the dry-run tag afterwards:
git tag -d v0.0.0-rename-dry-run
git push origin :refs/tags/v0.0.0-rename-dry-run

# 4. Manually dispatch linux-distribution-release.yml with a known tag
gh workflow run linux-distribution-release.yml \
   --repo nimbus/nimbus \
   -f release_tag=v0.0.0-rename-dry-run \
   -f publish_apt_repo=false \
   -f submit_to_copr=false
# Catches missed gh-API repo paths, COPR_CONFIG, GPG signing key issues.
```

A real production release tag should only be cut after items 1-4 each
pass cleanly. Items 3 and 4 in particular exercise the cross-repo App
token and the renamed satellite-repo workflow reference, which cannot be
validated by static text grep.

---

## Risk Notes

- **GitHub redirects**: GitHub auto-redirects old repo URLs after transfer.
  Update everything anyway for correctness.
- **GHCR images**: Old `ghcr.io/agentstation/*` images stop working once the
  org changes. New images must be pushed under `ghcr.io/nimbus/*`.
- **GitHub App reinstall**: The `MACHINE_OS_RELEASE_APP` (used by
  `release.yml:280` to dispatch satellite-repo workflows) is currently
  scoped to `owner: agentstation, repositories: neovex-machine-os`. It
  must either be reinstalled on the `nimbus` org with access to
  `nimbus/nimbus-machine-os`, or replaced by a new App owned by the
  `nimbus` org. See Phase 0a.3.
- **APT GPG key re-issuance**: The signing key identity
  `Neovex Apt Repo <apt@neovex.dev>` does not match the new domain.
  Generating a new key is unavoidable; downstream apt clients must
  re-import the new public key. Pre-launch this is just an installer-doc
  update, but plan for it in scripts/install.sh and apt onboarding docs.
- **Codecov re-onboarding**: `CODECOV_TOKEN` is repo-bound on Codecov's
  side. The new repo must be onboarded and a new token issued; historical
  coverage on the old repo URL becomes read-only.
- **COPR project rename**: `COPR_PROJECT=agentstation/neovex` in
  `packaging/linux-distribution-contract.env` -- if the COPR project is
  not renamed in lockstep, downstream Fedora users pulling from
  `agentstation/neovex` continue to receive the old binary name `neovex`,
  diverging from the renamed source. Strongly prefer renaming the COPR
  project to `nimbus/nimbus` at the same time as the GitHub rename.
- **Homebrew tap token rescope**: `HOMEBREW_TAP_TOKEN` currently grants
  write access to `agentstation/homebrew-tap` (which stays in
  agentstation, hosting other products). After rename the release
  workflow writes to `nimbus/homebrew-tap`. Re-issue the token scoped to
  the new tap; do not extend the old token's scope to span both orgs.
- **GitHub Pages CNAME**: `APT_REPOSITORY_CNAME` is a repo variable, not a
  secret. After rename, set it to the new domain (e.g. `apt.nimbus.dev`)
  and update DNS to point at `nimbus.github.io` before the next release
  cuts a Pages deploy. The deploy will succeed without a CNAME but will
  serve from `nimbus.github.io/nimbus/` until the variable is set.
- **Branch protection drift**: After a cross-org transfer, GitHub
  sometimes resets branch protection settings (status checks, required
  reviews). Verify protections on `nimbus/nimbus#main` immediately after
  transfer and recreate if needed. After Phase 3c renames
  `verify-neovex-crun-patch.yml`, the workflow's display name is
  unchanged so required checks should resolve, but verify.
- **Cargo.lock**: Must be fully regenerated since git source URLs change.
- **No crates.io publish**: All crates have `publish = false`, no registry
  concerns.
- **No npm publish**: All packages are private, no registry concerns.
- **Homebrew tap**: The formula in `homebrew-tap` repo also needs updating.
- **Self-hosted runner labels**: `.github/actionlint.yaml` references
  `neovex-machine-os` as a runner label -- actual runner config also needs
  updating (infrastructure concern, outside this repo).
- **Metadata namespaces**: `neovex_provider` -> `nimbus_provider` is safe
  because there are no production databases.
- **Network interface**: Changing `neovex0` -> `nimbus0` is safe pre-launch.
- **Deno fork URL**: Canonical `[patch.crates-io]` form points at
  `https://github.com/agentstation/deno` with the locker tag (currently
  `v2.7.14-locker.38` for the deno-family crates and `v147.4.0-locker.1`
  for `rusty_v8`). The Cargo.toml in the working tree may temporarily use
  `path = "..."` overrides while Codex iterates against the local Deno
  worktree -- restore the canonical git+tag form before merging the rename.
  The `deny.toml` `allow-git` entry for `agentstation/deno_core` is stale
  and must be deleted (not rewritten); add `nimbus/deno` if `unknown-git`
  is denied.
- **`.cargo/config.toml` `RUSTY_V8_VERSION` drift**: the env value
  `147.0.0-locker.2` does not match the v8 crate tag `v147.4.0-locker.1`
  in `Cargo.toml`. This is an existing drift, not introduced by the rename,
  but the rename PR is the natural moment to either reconcile or document
  the mismatch -- decide before Phase 1f regenerates `Cargo.lock`.
- **Checked-in data files**: `data/neovex-control.db` (~3.6MB) and
  `data/demo.redb` (~1.5MB) are committed binary artifacts. Decide
  rename-vs-regenerate-vs-delete explicitly during Phase 1:
  - Rename to `data/nimbus-control.db` to keep parity with the renamed
    Rust filename literal; or
  - Delete and let the runtime recreate on first boot under the renamed
    filename. (Plan recommendation: delete -- the file is not source of
    truth, regeneration verifies the renamed code path end-to-end.)
- **Vendor bundle regeneration**: `demos/convex/vendor/browser.bundle.js`
  must be deleted and rebuilt from the renamed SDK in Phase 2e, not
  sed-rewritten in place. A sed rewrite leaves binary minified state
  inconsistent with the renamed source.
- **Linux distribution packaging**: The apt-repo, COPR, and linux-packages
  workflows are new since the original plan and contain significant neovex
  references. The `packaging/linux-distribution-contract.env` file contains
  `NEOVEX_CRUN_VERSION` and `COPR_PROJECT=agentstation/neovex`.
- **Install script domain**: `scripts/install.sh` references `neovex.dev` for
  download URLs and documentation links. The target domain must be decided
  before the rename.
- **APT GPG email**: `apt@neovex.dev` is used as the GPG key identity for apt
  repository signing. The target email domain must match the new domain.
- **WebSocket protocol identifiers**: `neovex.v1` and `neovex.v2` are wire
  protocol names. Safe to rename pre-launch (no existing clients).
- **HTTP headers**: `x-neovex-admin-token`, `x-neovex-surface`, `x-neovex-http`
  are custom HTTP headers. Safe to rename pre-launch.
- **V8 runtime ops and JS globals**: `op_neovex_*` and `__neovex*` are internal
  runtime symbols. They must be renamed consistently between Rust op
  declarations, JS codegen templates, bootstrap JS, and inline test JS
  snippets.

---

## Satellite Repos

Internal renames for `nimbus-machine-os`, `nimbus-crun`, `homebrew-tap`, and
the locker-tag re-publish for `nimbus/deno` + `nimbus/rusty_v8` (Repo 4) are
covered by the prerequisite plan:
`docs/plans/nimbus-rename-satellite-repos-plan.md`.

The forked dependency repos (`nimbus/deno`, `nimbus/rusty_v8`) preserve
upstream names and need no internal symbol renames; only the origin URL and
locker tag publication move to the new org. See satellite plan Repo 4.
