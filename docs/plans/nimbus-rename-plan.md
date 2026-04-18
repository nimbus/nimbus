# Rename & Relocate: agentstation/neovex -> nimbus/nimbus

Canonical execution plan for renaming the project from "neovex" to "nimbus" and
relocating all repositories from the `agentstation` GitHub organization to
`nimbus`.

## Status

`pending` -- not yet started.

## Prerequisites

- `docs/plans/nimbus-rename-satellite-repos-plan.md` -- rename internals of
  `nimbus-machine-os`, `nimbus-crun`, and `homebrew-tap` repos so both sides of
  cross-repo interfaces agree on names at release time.

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
| **deno_core fork** `agentstation/deno_core` | `nimbus/deno_core` |
| **rusty_v8 fork** `agentstation/rusty_v8` | `nimbus/rusty_v8` |
| **Docker image** `ghcr.io/agentstation/neovex-machine-os` | `ghcr.io/nimbus/nimbus-machine-os` |
| **Binary** `neovex` | `nimbus` |
| **Rust crates** `neovex-*` | `nimbus-*` |
| **npm scope** `@neovex/*` | `@nimbus/*` |
| **npm package** `neovex` | `nimbus` |
| **Env vars** `NEOVEX_*` | `NIMBUS_*` |
| **Dot directory** `.neovex/` | `.nimbus/` |
| **Network iface** `neovex0` | `nimbus0` |
| **Systemd units** `neovex.service`, `neovex.socket` | `nimbus.service`, `nimbus.socket` |
| **Metadata namespace** `neovex_provider` | `nimbus_provider` |
| **Homebrew cask** `agentstation/tap/neovex` | `nimbus/tap/nimbus` |
| **Release assets** `neovex_linux_arm64.tar.gz` | `nimbus_linux_arm64.tar.gz` |

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
   - `agentstation/deno_core` -> `nimbus/deno_core`
   - `agentstation/rusty_v8` -> `nimbus/rusty_v8`

2. **Create new `nimbus/homebrew-tap`** repo (do NOT transfer
   `agentstation/homebrew-tap` -- it is shared with other agentstation products).
   Delete `Casks/neovex.rb` from `agentstation/homebrew-tap`.
   See satellite plan for details.

3. **Verify GitHub redirects** work for old URLs.

4. **Update local clone remote**:
   ```sh
   git remote set-url origin git@github.com:nimbus/nimbus.git
   ```

5. **Move local directory** (recommended):
   ```
   ~/src/github.com/agentstation/neovex  ->  ~/src/github.com/nimbus/nimbus
   ```

6. **Migrate Claude Code project memory** to the new project path.

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
- `[workspace.dependencies]` keys: `neovex-*` -> `nimbus-*`
- `[patch.crates-io]` git URLs: `agentstation/deno_core` -> `nimbus/deno_core`,
  `agentstation/rusty_v8` -> `nimbus/rusty_v8`

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
  - `crates/nimbus-engine/src/persistence_config.rs` (postgres, mysql, libsql)
  - `crates/nimbus-storage/src/libsql.rs`
  - `crates/nimbus-storage/src/postgres.rs`
  - `crates/nimbus-storage/src/mysql.rs`
- `"neovex_meta_"` -> `"nimbus_meta_"` in test fixtures:
  - `crates/nimbus-engine/src/tests/libsql_replica_provider.rs`
  - `crates/nimbus-engine/src/tests/mysql_provider.rs`
  - `crates/nimbus-storage/src/tests/libsql_provider.rs`
  - `crates/nimbus-storage/src/tests/mysql_provider.rs`

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

- Allowed git sources: `agentstation/deno_core` -> `nimbus/deno_core`,
  `agentstation/rusty_v8` -> `nimbus/rusty_v8`
- License reference: `"Neovex Community License"` -> `"Nimbus Community License"`
  (if renaming the license)

**Key files:**
- `Cargo.toml` (root)
- `deny.toml`
- `crates/*/Cargo.toml` (9 files)
- All `.rs` files (~200+ references across ~50+ files)
- `crates/nimbus-bin/src/machine/assets/*.tmpl` (2 template files)

---

## Phase 2: Rename JS Packages

### 2a. Rename package directory

```
packages/neovex/  ->  packages/nimbus/
```

`packages/codegen/` and `packages/convex/` keep their directory names.

### 2b. Update package.json files

- Root `package.json`: `"name": "neovex-workspace"` -> `"nimbus-workspace"`,
  workspaces path `packages/neovex` -> `packages/nimbus`
- `packages/nimbus/package.json`: `"name": "neovex"` -> `"nimbus"`
- `packages/codegen/package.json`: `"name": "@neovex/codegen"` ->
  `"@nimbus/codegen"`, bin `"neovex-codegen"` -> `"nimbus-codegen"`
- `packages/convex/package.json`: update deps `"neovex"` -> `"nimbus"`,
  `"@neovex/codegen"` -> `"@nimbus/codegen"`
- Demo `package.json` files: update any deps referencing `neovex` or `@neovex/*`

### 2c. Update JS source imports and codegen templates

- All `import ... from "neovex"` -> `"nimbus"`
- All `import ... from "@neovex/codegen"` -> `"@nimbus/codegen"`
- `packages/codegen/src/emit/generated_files.mjs`: Update codegen marker
  `// Generated by @neovex/codegen` -> `// Generated by @nimbus/codegen`
- `packages/codegen/src/emit/runtime_bundle_preamble.mjs`: any neovex refs
- `packages/convex/src/cli.mjs`: any neovex refs

### 2d. Regenerate demo generated files

After updating the codegen templates, regenerate all files in:
- `demos/convex/http/convex/_generated/`
- `demos/convex/html/convex/_generated/`
- `demos/convex/node/convex/_generated/`

These all contain `// Generated by @neovex/codegen. Do not edit by hand.` headers.

### 2e. Regenerate package-lock.json

```sh
npm install
```

**Key files:**
- `package.json` (root + 3 packages + demo apps)
- `packages/codegen/src/emit/*.mjs`
- `packages/convex/src/cli.mjs`
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

**Cross-repo references:**
- `agentstation/neovex-machine-os` -> `nimbus/nimbus-machine-os` (workflow refs,
  dispatches, watch)
- `ghcr.io/agentstation/neovex-machine-os` -> `ghcr.io/nimbus/nimbus-machine-os`
- `owner: agentstation` -> `owner: nimbus`
- `repositories: neovex-machine-os` -> `repositories: nimbus-machine-os`
- `neovex_version` -> `nimbus_version` (workflow input names)
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

### 3c. .github/workflows/verify-neovex-crun-patch.yml

- **Rename file** to `verify-nimbus-crun-patch.yml`
- Update self-reference in paths-filter

### 3d. .github/actionlint.yaml

- Self-hosted runner label: `neovex-machine-os` -> `nimbus-machine-os`

**Key files:**
- `.github/workflows/release.yml` (~60+ changes)
- `.github/workflows/ci.yml` (~12 changes)
- `.github/workflows/verify-neovex-crun-patch.yml` (rename + update)
- `.github/actionlint.yaml`

---

## Phase 4: Scripts

### 4a. Rename script files

| Before | After |
|--------|-------|
| `scripts/verify-neovex-machine-diagnostics-helper.sh` | `scripts/verify-nimbus-machine-diagnostics-helper.sh` |
| `scripts/verify-neovex-machine-guest-proof-helper.sh` | `scripts/verify-nimbus-machine-guest-proof-helper.sh` |
| `scripts/verify-neovex-machine-service-proof-helper.sh` | `scripts/verify-nimbus-machine-service-proof-helper.sh` |
| `scripts/collect-neovex-machine-guest-proof.sh` | `scripts/collect-nimbus-machine-guest-proof.sh` |
| `scripts/collect-neovex-machine-diagnostics.sh` | `scripts/collect-nimbus-machine-diagnostics.sh` |
| `scripts/collect-neovex-machine-service-proof.sh` | `scripts/collect-nimbus-machine-service-proof.sh` |
| `scripts/recreate-neovex-machine.sh` | `scripts/recreate-nimbus-machine.sh` |

### 4b. Update script contents

Inside all scripts under `scripts/`:
- `NEOVEX_*` -> `NIMBUS_*` env vars
- `neovex` -> `nimbus` binary/path references
- `agentstation` -> `nimbus` org references
- `.neovex/` -> `.nimbus/` directory references

Also update `scripts/single-flight.sh`:
- `NEOVEX_SINGLE_FLIGHT_DIR` -> `NIMBUS_SINGLE_FLIGHT_DIR`
- `.neovex/single-flight` -> `.nimbus/single-flight`

**Key files:** all files under `scripts/` (~10+ files)

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
  `recreate-neovex-machine` -> `recreate-nimbus-machine`
- Script paths: `scripts/collect-neovex-*` -> `scripts/collect-nimbus-*`, etc.
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
- **`CLAUDE.md`**: Update workspace layout table (all crate names), verification
  commands, all repo references, all doc path references
- **`AGENTS.md`**: Update project name references
- **`ARCHITECTURE.md`**: Update crate names and org references
- **`SECURITY.md`**: Advisory URL `agentstation/neovex` -> `nimbus/nimbus`
- **`CHANGELOG.md`**: Update all comparison URLs
- **`README.md`** and **`README.new.md`**:
  - CI/Codecov/Release badges
  - Homebrew badge and install command
  - Download URLs and binary name in examples
  - Attestation: `--owner agentstation` -> `--owner nimbus`
- **`LICENSE`**: Update if it references "Neovex"
- **`deny.toml`**: `"Neovex Community License"` -> `"Nimbus Community License"`

---

## Phase 7: Documentation

Bulk update all docs (~30+ files). Global replacements across all
`docs/**/*.md`, applied in this order:

1. `agentstation/neovex-machine-os` -> `nimbus/nimbus-machine-os`
2. `agentstation/neovex-crun` -> `nimbus/nimbus-crun`
3. `agentstation/homebrew-tap` -> `nimbus/homebrew-tap`
4. `agentstation/neovex` -> `nimbus/nimbus`
5. `agentstation` -> `nimbus` (remaining org-only refs)
6. `ghcr.io/agentstation/neovex-machine-os` -> `ghcr.io/nimbus/nimbus-machine-os`
7. `neovex-machine-os` -> `nimbus-machine-os` (prose references)
8. `neovex-crun` -> `nimbus-crun`
9. `NEOVEX_` -> `NIMBUS_` (env vars in docs)
10. `neovex` -> `nimbus` in product name contexts (careful: only where it refers
    to the product, not internal code)

Files to update (non-exhaustive):
- `docs/README.md`
- `docs/reference/cli.md`
- `docs/reference/microvm-service-baseline.md`
- `docs/reference/macos-machine-flow.md`
- `docs/reference/krun-vmm-host-validation.md`
- `docs/reference/macos-machine-flow.md`
- `docs/plans/machine-lifecycle-hardening-plan.md`
- `docs/plans/machine-cli-dx-plan.md`
- `docs/plans/windows-machine-support-plan.md`
- `docs/plans/distribution-plan.md`
- `docs/plans/install-script-plan.md`
- `docs/plans/encryption-at-rest-plan.md`
- `docs/plans/raw-v8-warm-backend-plan.md`
- `docs/plans/archive/*.md` (all archived plans)
- `docs/research/*.md` (all research docs)
- `docs/prompts/*.md`
- `docs/security/*.md`
- `docs/architecture/*.md`

---

## Phase 8: Memory & Claude Config

- Update Claude Code project memory directory (new project path after local dir
  move)
- Update `MEMORY.md` entries referencing neovex
- `.claude/settings.local.json` contains hardcoded paths with
  `agentstation/neovex` for permissions -- needs manual update

---

## Execution Order

Phases must be executed in this order due to dependencies:

1. **Phase 0** -- manual GitHub transfers (must happen first)
2. **Phase 1** -- Rust crates (core rename, generates new Cargo.lock)
3. **Phase 2** -- JS packages (depends on directory structure from Phase 1)
4. **Phase 3** -- CI/CD workflows (references crate/package names from 1-2)
5. **Phase 4** -- Scripts (references binary/env names from Phase 1)
6. **Phase 5** -- Makefile (references everything)
7. **Phase 6** -- Config & top-level files
8. **Phase 7** -- Documentation (bulk text replacement, lowest risk)
9. **Phase 8** -- Memory & Claude config (housekeeping)

---

## Verification

After all changes:

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

# No stale "neovex" references remain (except convex compat package)
rg "neovex" --type rust --type toml --type yml --type json --type sh \
   --glob '!Cargo.lock' --glob '!package-lock.json' --glob '!packages/convex/**'
# Should return 0 hits

# No stale "agentstation" references remain
rg "agentstation" --glob '!Cargo.lock' --glob '!package-lock.json'
# Should return 0 hits

# Verify binary name
cargo run -p nimbus-bin -- --help | head -5
# Should show "nimbus" not "neovex"
```

---

## Risk Notes

- **GitHub redirects**: GitHub auto-redirects old repo URLs after transfer.
  Update everything anyway for correctness.
- **GHCR images**: Old `ghcr.io/agentstation/*` images stop working once the
  org changes. New images must be pushed under `ghcr.io/nimbus/*`.
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

---

## Satellite Repos

Internal renames for `nimbus-machine-os`, `nimbus-crun`, and `homebrew-tap` are
covered by the prerequisite plan:
`docs/plans/nimbus-rename-satellite-repos-plan.md`.

The forked dependency repos (`nimbus/deno_core`, `nimbus/rusty_v8`) preserve
upstream names and need no internal renames.
