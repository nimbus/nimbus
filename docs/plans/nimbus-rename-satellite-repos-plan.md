# Rename Satellite Repos: neovex-machine-os, neovex-crun, homebrew-tap

Prerequisite execution plan for renaming the satellite repositories before the
main repo rename in `docs/plans/nimbus-rename-plan.md`. Covers the three repos
that contain internal "neovex" or "agentstation" references requiring updates.

The forked dependency repos (`nimbus/deno`, `nimbus/rusty_v8`) preserve
upstream names and need no internal renames, but they **do** need a locker
tag re-publish under the new org so the main repo's `[patch.crates-io]` and
`deny.toml` allow-list can resolve. See "Repo 4" below.

## Status

`pending` -- not yet started.

## Relationship to Main Rename Plan

This plan is a **prerequisite release gate** for
`docs/plans/nimbus-rename-plan.md`: the satellite repositories do not need to
finish before the GitHub transfer itself, but they must finish before the main
repo's renamed release pipeline is allowed to run end-to-end. The main repo's
release workflow calls into `nimbus-machine-os` workflows and pushes to
`homebrew-tap` via API, so both sides must agree on names at release time.

**Execution order:**
1. Transfer neovex-specific repos to `nimbus` org and create new
   `nimbus/homebrew-tap` (Phase 0 of main plan; `agentstation/homebrew-tap`
   stays in agentstation since it is shared with other products).
   Note: the Deno-family fork is `agentstation/deno` (monorepo), not the
   old standalone `agentstation/deno_core` which is historical only.
2. Execute this satellite plan (rename internals of each repo and complete the
   token/action/ruleset audit below)
3. Execute main plan Phases 1-8 (rename main repo internals)
4. Cut a release to verify end-to-end (main repo release triggers machine-os
   build and homebrew-tap update)

---

## Cross-Repo Admin and Token Audit

This plan changes more than file names. Each satellite repo has its own
GitHub Actions settings, release permissions, packages, rulesets, secrets,
variables, and integrations. Before the main repo cuts a renamed release,
verify these settings in the `nimbus` org. Use the human/admin prerequisite
packet in `docs/plans/nimbus-rename-plan.md` as the canonical handoff checklist
for credentials, secrets, service-console access, and manual confirmations.

- **All satellite repos**: default branch, branch protection/rulesets, tag
  protection/rulesets, required checks, release creation permissions,
  Dependabot/security settings, webhooks, deploy keys, installed GitHub Apps,
  and third-party action allowlists.
- **`nimbus/nimbus-machine-os`**: Actions workflow permissions allow the
  requested `contents`, `packages`, `id-token`, and `attestations` scopes;
  GHCR package visibility and package ownership match the release policy;
  any registry username/password secrets used by the workflow are reissued as
  `NIMBUS_MACHINE_OS_*` or intentionally replaced with `GITHUB_TOKEN`.
  `MACHINE_OS_RELEASE_APP_ID` and `MACHINE_OS_RELEASE_APP_PRIVATE_KEY` are
  required here as well as in `nimbus/nimbus`: `publish.yml` uses them to mint
  an Actions-read/Contents-read token against the source repo's staged artifact,
  and reusable `build.yml` can use them for Contents-write/Packages-write when
  publishing machine-os releases.
- **`nimbus/nimbus-crun`**: release workflow can create releases and upload
  assets under the new tag format; Docker builder/cache settings do not point
  at the old repo name; tag rules allow `v*-nimbus.*` tags.
- **`nimbus/homebrew-tap`**: the release token principal has `contents: write`
  to this repo only, branch/ruleset policy allows automated cask updates, and
  the shared `agentstation/homebrew-tap` token is not reused.
- **`nimbus/deno` and `nimbus/rusty_v8`**: Actions/release settings,
  large-release-asset permissions, artifact retention, branch/tag protections,
  and any upstream-sync secrets survive transfer or the upstream release
  workflows are intentionally disabled in the Nimbus forks. `rusty_v8` release
  assets are especially important because the main repo consumes prebuilt locker
  assets.

Concrete satellite secret/variable inventory to reconcile:

GitHub secret values are write-only. Use `gh secret list` to confirm expected
names exist, then verify value contents through the credential owner or
source-of-truth vault, or reissue credentials when old repo/org/domain scopes
could be embedded.

| Repo | Secret/variable | Decision |
|------|-----------------|----------|
| `nimbus/nimbus-machine-os` | `MACHINE_OS_RELEASE_APP_ID`, `MACHINE_OS_RELEASE_APP_PRIVATE_KEY` | Provision from the same App strategy chosen in the main plan. Required for artifact download and release/GHCR publishing paths. |
| `nimbus/nimbus-machine-os` | `NIMBUS_MACHINE_OS_REGISTRY_USERNAME`, `NIMBUS_MACHINE_OS_REGISTRY_PASSWORD` | Prefer replacing with `GITHUB_TOKEN`/App token. If still needed, scope only to `ghcr.io/nimbus/nimbus-machine-os`. |
| `nimbus/nimbus-crun` | none currently beyond `GITHUB_TOKEN` | Verify repo token can create releases, upload assets, attest, and use Docker Buildx. |
| `nimbus/homebrew-tap` | no workflow secrets expected in the tap | Write access comes from `HOMEBREW_TAP_TOKEN` stored on `nimbus/nimbus`; verify the token principal is limited to this tap. |
| `nimbus/deno` | `DENOBOT_PAT`, `DENOBOT_GIST_PAT`, `CARGO_REGISTRY_TOKEN`, `APPLE_CODESIGN_KEY`, `APPLE_CODESIGN_PASSWORD`, `AZURE_CLIENT_ID`, `AZURE_TENANT_ID`, `AZURE_SUBSCRIPTION_ID`, `GCP_SA_KEY`, `S3_SECRET_ACCESS_KEY`, `WPT_FYI_PW`, `NODE_COMPAT_SLACK_TOKEN`, `NODE_COMPAT_SLACK_CHANNEL`; vars `S3_ACCESS_KEY_ID`, `S3_ENDPOINT`, `S3_REGION` | Reprovision only if Nimbus intends to run upstream Deno release/publish/compat workflows. Otherwise disable those workflows or restrict them to the upstream `denoland/deno` condition and document that only tag reachability is required for Neovex/Nimbus. |
| `nimbus/rusty_v8` | `DENOBOT_PAT` plus default `GITHUB_TOKEN` release permissions | Reprovision only if Nimbus intends to cut new `rusty_v8` releases from this fork. Otherwise disable release/update workflows and manually verify transferred locker tags/assets remain reachable. |

Concrete third-party action allowlist additions by satellite repo:

- `nimbus/nimbus-machine-os`: `actions/*`, especially
  `actions/create-github-app-token@v2`, `actions/cache@v5`,
  `actions/download-artifact@v8`, `actions/upload-artifact@v7`,
  `actions/attest@v4`.
- `nimbus/nimbus-crun`: `actions/*`, `docker/setup-buildx-action@v3`,
  `docker/build-push-action@v6`, `actions/attest@v4`.
- `nimbus/deno`: `actions/*`, `denoland/setup-deno`, `dsherret/rust-toolchain-file`,
  `cargo-bins/cargo-binstall`, `softprops/action-gh-release`,
  `google-github-actions/auth`, `google-github-actions/setup-gcloud`,
  `azure/login`, `Azure/artifact-signing-action`.
- `nimbus/rusty_v8`: `actions/*`, `denoland/setup-deno`,
  `dsherret/rust-toolchain-file`, `cargo-bins/cargo-binstall`,
  `softprops/action-gh-release`.

Action-version coordination note: the main repo currently uses
`actions/create-github-app-token@v3`, while machine-os currently uses `@v2`.
The version mismatch is not itself a rename blocker, but do not accidentally
mix parameter semantics during the coordination window. Either preserve each
repo's known-good version while only changing owner/repository inputs, or
upgrade both deliberately in a separate verified action-version change.

Suggested live checks once the repos exist:

```sh
for repo in nimbus-machine-os nimbus-crun homebrew-tap deno rusty_v8; do
  gh secret list --repo "nimbus/${repo}"
  gh variable list --repo "nimbus/${repo}"
  gh api "repos/nimbus/${repo}/actions/permissions"
  gh api "repos/nimbus/${repo}/rulesets"
  gh api "repos/nimbus/${repo}/keys"
  gh api "repos/nimbus/${repo}/hooks"
done

gh api orgs/nimbus/actions/permissions
gh api orgs/nimbus/actions/permissions/selected-actions
gh api orgs/nimbus/actions/runner-groups
gh api orgs/nimbus/actions/runners
```

---

## Repo 1: nimbus/nimbus-machine-os

Formerly `agentstation/neovex-machine-os`. Builds the Fedora bootc-based guest
Linux VM image for macOS machine support. The most complex satellite repo.
~150+ lowercase "neovex", ~25 uppercase "NEOVEX", ~70 "agentstation" references
across 15 files.

### File inventory

```
README.md
LICENSE
images/Containerfile
images/build.sh
images/build-common.sh
images/README.md
images/bootc-image-builder.toml
scripts/build.sh
scripts/package-oci.sh
scripts/publish.sh
scripts/verify-recipe.sh
scripts/verify-build-helper.sh
scripts/verify-oci-layout-helper.sh
scripts/verify-publish-helper.sh
.github/workflows/build.yml
.github/workflows/publish.yml
.github/dependabot.yml
```

### Naming map

| Before | After |
|--------|-------|
| Repo name `neovex-machine-os` | `nimbus-machine-os` |
| GHCR image `ghcr.io/agentstation/neovex-machine-os` | `ghcr.io/nimbus/nimbus-machine-os` |
| Disk artifact `neovex-machine-os.raw.gz` | `nimbus-machine-os.raw.gz` |
| OCI archive `neovex-machine-os.ociarchive` | `nimbus-machine-os.ociarchive` |
| Guest binary `/usr/local/bin/neovex` | `/usr/local/bin/nimbus` |
| Guest data dir `/var/lib/neovex/` | `/var/lib/nimbus/` |
| Guest control dir `/var/lib/neovex/control/` | `/var/lib/nimbus/control/` |
| Guest socket `/run/neovex/neovex.sock` | `/run/nimbus/nimbus.sock` |
| Systemd unit `neovex.socket` | `nimbus.socket` |
| Systemd unit `neovex.service` | `nimbus.service` |
| OCI annotations `io.neovex.machine.*` | `io.nimbus.machine.*` |
| OCI media types `application/vnd.neovex.machine.*` | `application/vnd.nimbus.machine.*` |
| Config files `999-neovex-machine.conf` etc. | `999-nimbus-machine.conf` etc. |
| Env vars `NEOVEX_MACHINE_OS_*` | `NIMBUS_MACHINE_OS_*` |
| Env var `NEOVEX_BIB_IMAGE` | `NIMBUS_BIB_IMAGE` |
| Workflow input `neovex_version` | `nimbus_version` |
| Workflow input `neovex_artifact_name` | `nimbus_artifact_name` |
| Artifact name `neovex-machine-os-arm64` | `nimbus-machine-os-arm64` |
| CLI flags `--neovex-binary`, `--neovex-version` | `--nimbus-binary`, `--nimbus-version` |
| License `Neovex Community License` | `Nimbus Community License` |

### Changes required

#### MOS-1: Workflows (.github/workflows/)

**`build.yml`** (~39 references, 489 lines):
- Workflow input descriptions: `"Neovex release tag"` -> `"Nimbus release tag"`
- Input names if exposed to callers: `neovex_version` -> `nimbus_version`,
  `neovex_artifact_name` -> `nimbus_artifact_name`
- Default image reference: `ghcr.io/agentstation/neovex-machine-os:v0.0.0-dev`
  -> `ghcr.io/nimbus/nimbus-machine-os:v0.0.0-dev`
- Checkout repository: `agentstation/neovex-machine-os` ->
  `nimbus/nimbus-machine-os`
- Temp directory names: `${RUNNER_TEMP}/neovex-machine-os` ->
  `${RUNNER_TEMP}/nimbus-machine-os` (layout, release dirs too)
- API calls to fetch latest tag: `repos/agentstation/neovex/releases/latest` ->
  `repos/nimbus/nimbus/releases/latest`
- Download URL: `github.com/agentstation/neovex/releases/download/*/neovex_linux_arm64.tar.gz`
  -> `github.com/nimbus/nimbus/releases/download/*/nimbus_linux_arm64.tar.gz`
- Tar extraction: `neovex_linux_arm64.tar.gz` -> `nimbus_linux_arm64.tar.gz`
- Binary output var: `neovex_binary=${RUNNER_TEMP}/neovex` ->
  `nimbus_binary=${RUNNER_TEMP}/nimbus`
- Build script flags: `--neovex-binary`, `--neovex-version` ->
  `--nimbus-binary`, `--nimbus-version`
- GHCR push target: `ghcr.io/agentstation/neovex-machine-os:${tag}` ->
  `ghcr.io/nimbus/nimbus-machine-os:${tag}`
- Source repository URL: `github.com/agentstation/neovex-machine-os` ->
  `github.com/nimbus/nimbus-machine-os`
- Env vars: `NEOVEX_MACHINE_OS_REGISTRY_USERNAME`,
  `NEOVEX_MACHINE_OS_REGISTRY_PASSWORD` -> `NIMBUS_MACHINE_OS_*`
- GitHub App: `owner: agentstation`, `repositories: neovex-machine-os` ->
  `owner: nimbus`, `repositories: nimbus-machine-os`
- Error messages referencing `neovex-machine-os` and `agentstation`
- Release notes mentioning `agentstation/neovex`
- Artifact stage names: `neovex-machine-os-arm64`,
  `neovex-machine-os-arm64-publish` -> `nimbus-machine-os-*`
- Release repo: `release_repository="agentstation/neovex-machine-os"` ->
  `"nimbus/nimbus-machine-os"`
- Build output artifact: `neovex-machine-os.raw.gz` -> `nimbus-machine-os.raw.gz`
- App credentials: after the rename, this workflow still needs
  `MACHINE_OS_RELEASE_APP_PRIVATE_KEY` when invoked as a reusable workflow that
  publishes or creates a release. Ensure the main repo passes the secret and the
  machine-os repo has its own copy for direct or dispatch-driven release paths.

**`publish.yml`** (~22 references, 254 lines):
- Input descriptions: `"Neovex release tag"` -> `"Nimbus release tag"`,
  `"agentstation/neovex"` -> `"nimbus/nimbus"`
- Checkout repository: `agentstation/neovex-machine-os` ->
  `nimbus/nimbus-machine-os`
- Temp directory names: same pattern as build.yml
- Error messages: `"neovex-machine-os"` -> `"nimbus-machine-os"`
- Env vars: `NEOVEX_MACHINE_OS_REGISTRY_USERNAME`,
  `NEOVEX_MACHINE_OS_REGISTRY_PASSWORD` -> `NIMBUS_MACHINE_OS_*`
- GHCR publish target: `ghcr.io/agentstation/neovex-machine-os` ->
  `ghcr.io/nimbus/nimbus-machine-os`
- Release title/notes: all neovex/agentstation references
- Release repo: `release_repository='agentstation/neovex-machine-os'` ->
  `'nimbus/nimbus-machine-os'`
- Build output: `neovex-machine-os.raw.gz` -> `nimbus-machine-os.raw.gz`
- Artifact name: `neovex-machine-os-arm64-publish` -> `nimbus-machine-os-*`
- App credentials: this workflow reads `MACHINE_OS_RELEASE_APP_ID` from repo
  variables and `MACHINE_OS_RELEASE_APP_PRIVATE_KEY` from repo secrets to mint
  a token against `inputs.source_repository`. Provision both on
  `nimbus/nimbus-machine-os` and verify the App can read Actions artifacts from
  `nimbus/nimbus`.

**Note:** The reusable workflow is pinned at `@release-workflow-v1`. After
renaming, create a new tag or update the existing ref so the main repo can call
`nimbus/nimbus-machine-os/.github/workflows/build.yml@release-workflow-v1`.

**`.github/dependabot.yml`**:
- Verify package ecosystem directories and any path filters after file/script
  renames. The file may have no direct `neovex` strings today, but it must still
  point at valid workflow, container, or package directories after the
  `nimbus-machine-os` rename.

#### MOS-2: Containerfile and image build scripts

**`images/Containerfile`** (3 references):
- `COPY neovex /usr/local/bin/neovex` -> `COPY nimbus /usr/local/bin/nimbus`
- `RUN chmod 0755 /usr/local/bin/neovex` -> `/usr/local/bin/nimbus`

**`images/build.sh`** (~25 references, 191 lines):
- Usage text: `"Build the Neovex Fedora CoreOS guest image"` -> `"Nimbus"`
- CLI flags: `--neovex-binary`, `--neovex-version` ->
  `--nimbus-binary`, `--nimbus-version`
- Variables: `neovex_binary`, `neovex_version` -> `nimbus_binary`,
  `nimbus_version`
- Default image name: `localhost/neovex-machine-os:dev` ->
  `localhost/nimbus-machine-os:dev`
- Error messages: `"neovex binary does not exist"` etc.
- Binary copy: `install ... "${context_dir}/neovex"` -> `"${context_dir}/nimbus"`
- OCI archive: `neovex-machine-os.ociarchive` -> `nimbus-machine-os.ociarchive`
- Env var: `NEOVEX_BIB_IMAGE` -> `NIMBUS_BIB_IMAGE`
- Compressed output: `neovex-machine-os.raw.gz` -> `nimbus-machine-os.raw.gz`
- Build manifest vars: `neovex_binary`, `neovex_version`,
  `neovex_binary_sha256` -> `nimbus_*`

**`images/build-common.sh`** (~6 references):
- Data directories: `/var/lib/neovex/control`, `/var/lib/neovex/data` ->
  `/var/lib/nimbus/*`
- Config files dropped into image:
  - `/etc/containers/registries.conf.d/999-neovex-machine.conf` ->
    `999-nimbus-machine.conf`
  - `/etc/ssh/sshd_config.d/99-neovex-machine-sshd.conf` ->
    `99-nimbus-machine-sshd.conf`
  - `/etc/sysctl.d/10-neovex-machine-inotify.conf` ->
    `10-nimbus-machine-inotify.conf`
- chmod paths: `/var/lib/neovex /var/lib/neovex/control /var/lib/neovex/data` ->
  `/var/lib/nimbus/*`

#### MOS-3: OCI packaging and publishing scripts

**`scripts/package-oci.sh`** (~19 references):
- Custom OCI media types (critical -- consumed by the main repo's image puller):
  - `application/vnd.neovex.machine.disk.layer.v1.raw+gzip` ->
    `application/vnd.nimbus.machine.disk.layer.v1.raw+gzip`
  - `application/vnd.neovex.machine.disk.layer.v1.raw+zstd` ->
    `application/vnd.nimbus.machine.disk.layer.v1.raw+zstd`
  - `application/vnd.neovex.machine.disk.layer.v1.raw` ->
    `application/vnd.nimbus.machine.disk.layer.v1.raw`
  - `application/vnd.neovex.machine.disk.layer.v1.blob` ->
    `application/vnd.nimbus.machine.disk.layer.v1.blob`
- OCI manifest annotations:
  - `io.neovex.machine.attestation.repository` ->
    `io.nimbus.machine.attestation.repository`
  - `io.neovex.machine.neovex.version` ->
    `io.nimbus.machine.nimbus.version`
- Env vars: `NEOVEX_MACHINE_OS_PACKAGE_TEST_ARCH`,
  `NEOVEX_MACHINE_OS_SOURCE_REPOSITORY_URL`,
  `NEOVEX_MACHINE_OS_ATTESTATION_REPOSITORY`,
  `NEOVEX_MACHINE_OS_VERSION` -> `NIMBUS_MACHINE_OS_*`
- Default values: `agentstation/neovex-machine-os` -> `nimbus/nimbus-machine-os`
- CLI flag: `--neovex-version` -> `--nimbus-version`
- Variable: `neovex_version` -> `nimbus_version`

**`scripts/publish.sh`** (~15 references):
- Usage text: `"Push a packaged Neovex machine"` -> `"Nimbus"`
- Env vars: `NEOVEX_MACHINE_OS_REGISTRY_USERNAME`,
  `NEOVEX_MACHINE_OS_REGISTRY_PASSWORD` -> `NIMBUS_MACHINE_OS_*`
- Example URLs: `ghcr.io/agentstation/neovex-machine-os` ->
  `ghcr.io/nimbus/nimbus-machine-os`
- Error messages referencing env var names

**`scripts/build.sh`** (wrapper, ~15 references):
- Usage text: `"Build the neovex-machine-os guest image"` -> `"nimbus-machine-os"`
- CLI flags: `--neovex-binary`, `--neovex-version` ->
  `--nimbus-binary`, `--nimbus-version`
- Variables: `neovex_binary`, `neovex_version` -> `nimbus_*`
- Env var: `NEOVEX_MACHINE_OS_BUILD_WRAPPER_TEST_UNAME` ->
  `NIMBUS_MACHINE_OS_BUILD_WRAPPER_TEST_UNAME`
- Temp dirs: `/tmp/neovex-machine-os` -> `/tmp/nimbus-machine-os`
- Error messages: `"neovex binary not found"` etc.
- Delegation: passes `--neovex-binary`, `--neovex-version` to `images/build.sh`

#### MOS-4: Verification scripts

**`scripts/verify-recipe.sh`** (~4 references):
- `grep -F 'COPY neovex /usr/local/bin/neovex'` -> `'COPY nimbus /usr/local/bin/nimbus'`
- `neovex_binary="${temp_dir}/neovex"` -> `nimbus_binary=...`
- Env vars: `NEOVEX_MACHINE_OS_BUILD_TEST_UNAME`,
  `NEOVEX_MACHINE_OS_BUILD_TEST_UID` -> `NIMBUS_MACHINE_OS_*`

**`scripts/verify-build-helper.sh`** (~2 references):
- `NEOVEX_MACHINE_OS_BUILD_WRAPPER_TEST_UNAME` -> `NIMBUS_*`
- `/tmp/neovex-machine-os-out` -> `/tmp/nimbus-machine-os-out`

**`scripts/verify-oci-layout-helper.sh`** (~11 references):
- All test assertion strings:
  - `ghcr.io/agentstation/neovex-machine-os:v1.2.3` ->
    `ghcr.io/nimbus/nimbus-machine-os:v1.2.3`
  - `github.com/agentstation/neovex-machine-os` ->
    `github.com/nimbus/nimbus-machine-os`
  - `agentstation/neovex` -> `nimbus/nimbus` (attestation repo)
  - `io.neovex.machine.*` -> `io.nimbus.machine.*` (annotation assertions)
- Success message: `"verified neovex machine-os"` -> `"verified nimbus machine-os"`

**`scripts/verify-publish-helper.sh`** (~14 references):
- All test assertion strings same pattern as oci-layout helper
- Env vars: `NEOVEX_MACHINE_OS_REGISTRY_USERNAME`,
  `NEOVEX_MACHINE_OS_REGISTRY_PASSWORD` -> `NIMBUS_MACHINE_OS_*`
- Additional reference assertions for `ghcr.io/agentstation/neovex-machine-os:next`
- Success message: `"verified neovex machine-os publish wrapper"` -> `"nimbus"`

#### MOS-5: Documentation and license

**`README.md`** (~25+ references):
- Title: `# neovex-machine-os` -> `# nimbus-machine-os`
- All prose: `"neovex"` -> `"nimbus"`, `"Neovex"` -> `"Nimbus"`
- GHCR URL: `ghcr.io/agentstation/neovex-machine-os` ->
  `ghcr.io/nimbus/nimbus-machine-os`
- Main repo URLs: `github.com/agentstation/neovex` ->
  `github.com/nimbus/nimbus`
- Download examples: `neovex_linux_arm64.tar.gz` -> `nimbus_linux_arm64.tar.gz`
- CLI flags in examples: `--neovex-binary`, `--neovex-version`
- Output dirs: `--output-dir /tmp/neovex-machine-os` -> `/tmp/nimbus-machine-os`
- OCI annotations: `io.neovex.machine.*` -> `io.nimbus.machine.*`
- Cross-repo references: `agentstation/neovex-machine-os`,
  `agentstation/neovex`

**`images/README.md`** (~16 references):
- Title: `# Neovex Machine OS Recipe` -> `# Nimbus Machine OS Recipe`
- All same patterns as root README
- Artifact names: `neovex-machine-os.ociarchive`, `neovex-machine-os.raw.gz`
- Publish examples: `ghcr.io/agentstation/neovex-machine-os`

**`LICENSE`** (~7 references):
- `Neovex Community License 1.0` -> `Nimbus Community License 1.0`
- `Copyright (c) 2026 the Neovex contributors` -> `Nimbus contributors`
- All other `Neovex` references in license text

#### MOS-6: Verification

```sh
# After all changes:
rg "neovex" .
rg "agentstation" .
# Both should return 0 hits

# Verify build script accepts new flags:
bash scripts/build.sh --help  # should show --nimbus-binary, --nimbus-version

# Verify Containerfile references:
grep -F '/usr/local/bin/nimbus' images/Containerfile

# Verify OCI media types in package script:
grep -F 'vnd.nimbus.machine' scripts/package-oci.sh

# Verify config file names in build-common:
grep -F 'nimbus-machine' images/build-common.sh

# Verify verification scripts pass with new names:
bash scripts/verify-recipe.sh
bash scripts/verify-build-helper.sh
bash scripts/verify-oci-layout-helper.sh
bash scripts/verify-publish-helper.sh
```

---

## Repo 2: nimbus/nimbus-crun

Formerly `agentstation/neovex-crun`. Builds a patched crun binary with TSI port
mapping support for the krun backend. Small repo: 9 files, ~39 lowercase
"neovex", ~8 capitalized "Neovex" (LICENSE), 1 "agentstation" reference.

### File inventory

```
README.md
LICENSE
patches/0001-krun-add-tsi-port-mapping-via-oci-annotation.patch
scripts/build.sh
scripts/verify-fedora-userspace.sh
scripts/verify-patch.sh
.github/container/Dockerfile.builder
.github/dependabot.yml
.github/workflows/build.yml
```

### Naming map

| Before | After |
|--------|-------|
| Repo name `neovex-crun` | `nimbus-crun` |
| Binary install path `/usr/libexec/neovex/crun` | `/usr/libexec/nimbus/crun` |
| Release assets `neovex-crun-linux-amd64` | `nimbus-crun-linux-amd64` |
| Release assets `neovex-crun-linux-arm64` | `nimbus-crun-linux-arm64` |
| Builder image tag `neovex-crun-builder:*` | `nimbus-crun-builder:*` |
| Git tag `v1.27-neovex.1` | `v1.27-nimbus.1` |
| License `Neovex Community License` | `Nimbus Community License` |

### Changes required

#### CRUN-1: Build script (scripts/build.sh, ~16 references)

- Usage text: `"build-neovex-crun.sh"` -> `"build-nimbus-crun.sh"`
  (or just `"build.sh"` since the script name is already `build.sh`)
- Prose: `"Build the checked-in patched neovex crun binary"` -> `"nimbus crun"`
- Example paths: `--output /tmp/neovex-crun-stage/crun` ->
  `/tmp/nimbus-crun-stage/crun`
- Install path: `--install-path /usr/libexec/neovex/crun` ->
  `/usr/libexec/nimbus/crun`
- Default output: `${TMPDIR:-/tmp}/neovex-crun-stage/crun` ->
  `nimbus-crun-stage/crun`
- Error message: `"build-neovex-crun.sh requires a Linux host"` -> `nimbus`
- Temp dir: `mktemp -d .../neovex-crun-build.XXXXXX` -> `nimbus-crun-build.*`

#### CRUN-2: Verification scripts

**`scripts/verify-fedora-userspace.sh`** (~8 references):
- Usage text: `"verify-neovex-crun-fedora-userspace.sh"` -> `"nimbus"`
- Prose: `"Run the neovex crun patch + build helpers"` -> `"nimbus"`
- Example paths: `--output-dir /tmp/neovex-crun-fedora-output` -> `nimbus-*`
- Temp dirs: `neovex-crun-fedora-output.*`, `neovex-crun-fedora-build.*` ->
  `nimbus-crun-*`

**`scripts/verify-patch.sh`**: Check for any neovex references (likely minimal
since it verifies the upstream patch applies cleanly).

#### CRUN-3: Workflow (.github/workflows/build.yml, ~20 references)

- Workflow name: `"Build neovex-crun"` -> `"Build nimbus-crun"`
- Job name: `"Build neovex-crun (linux ${{ matrix.arch }})"` -> `"nimbus-crun"`
- Temp dirs: `${RUNNER_TEMP}/neovex-crun-output`, `neovex-crun-build` ->
  `nimbus-crun-*`
- Docker builder tag: `neovex-crun-builder:${{ matrix.arch }}` ->
  `nimbus-crun-builder:*`
- Binary rename: `mv .../crun .../neovex-crun-linux-${{ matrix.arch }}` ->
  `nimbus-crun-linux-*`
- Artifact names: `neovex-crun-linux-${{ matrix.arch }}` ->
  `nimbus-crun-linux-*`
- Release asset paths: all `neovex-crun-linux-amd64`, `neovex-crun-linux-arm64`
- Checksum generation: `sha256sum neovex-crun-linux-*` -> `nimbus-crun-linux-*`
- Release title: `"neovex-crun ${{ github.ref_name }}"` -> `"nimbus-crun"`
- Verification step: `./neovex-crun-linux-amd64 --version` ->
  `./nimbus-crun-linux-amd64`
- Release permissions: keep `contents: write`, `id-token: write`, and
  `attestations: write`; verify the `nimbus-crun` repo/org default token policy
  permits these explicit scopes.
- Third-party actions: add `docker/setup-buildx-action@v3` and
  `docker/build-push-action@v6` to any org selected-action allowlist.

#### CRUN-4: Docker builder

**`.github/container/Dockerfile.builder`**: Check for any neovex references in
the builder image definition (likely minimal -- it installs build dependencies).

**`.github/dependabot.yml`**:
- Verify package ecosystem directories and any path filters after workflow and
  Docker builder path changes. The file may have no direct `neovex` strings
  today, but path validity should be part of the crun repo rename review.

#### CRUN-5: Patch file

**`patches/0001-krun-add-tsi-port-mapping-via-oci-annotation.patch`**: The C
code in the patch is upstream-generic (reads `krun.port_map` OCI annotation,
not neovex-branded). Check the patch header/commit message for any neovex
references.

#### CRUN-6: Documentation and license

**`README.md`** (~8 references):
- Title: `# neovex-crun` -> `# nimbus-crun`
- Link: `[neovex](https://github.com/agentstation/neovex)` ->
  `[nimbus](https://github.com/nimbus/nimbus)`
- Asset names: `neovex-crun-linux-amd64`, `neovex-crun-linux-arm64`
- Install path: `/usr/libexec/neovex/crun` -> `/usr/libexec/nimbus/crun`
- Example paths: `--output /tmp/neovex-crun`, `/tmp/neovex-crun --version`

**`LICENSE`** (~7 references):
- `Neovex Community License 1.0` -> `Nimbus Community License 1.0`
- All other `Neovex` references in license text

#### CRUN-7: Git tag format

Current tag: `v1.27-neovex.1`. Future tags should use `v1.27-nimbus.1` or
similar. No existing tags need to be moved (pre-launch, no consumers).

#### CRUN-8: Verification

```sh
rg "neovex" .
rg "agentstation" .
# Both should return 0 hits

# Verify build script help text:
head -20 scripts/build.sh  # should reference nimbus-crun

# Verify workflow asset names:
grep -F "nimbus-crun-linux" .github/workflows/build.yml
```

---

## Repo 3: nimbus/homebrew-tap (NEW repo)

`agentstation/homebrew-tap` is a **shared tap** serving 6 products (neovex,
starmap, tokenizer, vhs, pocket, tydirium). It should NOT be transferred to
the nimbus org. Instead, create a new `nimbus/homebrew-tap` repo containing
only the nimbus cask, and delete `Casks/neovex.rb` from the agentstation tap.

### What the agentstation tap contains (for context)

```
Casks/neovex.rb       <- the only file that concerns us
Casks/starmap.rb      <- unrelated product, stays in agentstation
Casks/tokenizer.rb    <- unrelated product, stays in agentstation
Casks/vhs.rb          <- unrelated product, stays in agentstation
Formula/pocket.rb     <- unrelated product, stays in agentstation
Formula/tydirium.rb   <- unrelated product, stays in agentstation
README.md, CLAUDE.md, nix-packages/ (submodule)
```

### Changes required

#### TAP-1: Create nimbus/homebrew-tap repo

Create a new `nimbus/homebrew-tap` repository with minimal structure:

```
nimbus/homebrew-tap/
  Casks/nimbus.rb     <- will be auto-created by first release
  README.md
```

The `Casks/nimbus.rb` file will be auto-generated by the main repo's
`release.yml` workflow on the first nimbus release. Optionally seed it with a
placeholder adapted from the current `Casks/neovex.rb`:

- `cask "nimbus"` (not `"neovex"`)
- `name "nimbus"`
- `homepage "https://github.com/nimbus/nimbus"`
- `binary "nimbus"`
- All download URLs: `nimbus/nimbus/releases/download/*/nimbus_*`
- Caveats: `"Nimbus has been installed!"`, `nimbus --help`, etc.

#### TAP-2: Write README.md for new tap

Minimal README:

```markdown
# nimbus/homebrew-tap

Homebrew tap for [nimbus](https://github.com/nimbus/nimbus).

## Install

    brew install nimbus/tap/nimbus
```

#### TAP-3: Delete neovex cask from agentstation tap

Remove `Casks/neovex.rb` from `agentstation/homebrew-tap`. The other 5 products
remain untouched in that repo. Also remove the Neovex install section and any
other user-facing Neovex install references from the shared tap README after
the new `nimbus/homebrew-tap` path is verified.

#### TAP-4: Verification

```sh
# Verify new tap works:
brew tap nimbus/tap
brew install nimbus/tap/nimbus
nimbus --version

# Verify old tap still serves other products:
brew install agentstation/tap/starmap
```

---

## Repo 4: nimbus/deno + nimbus/rusty_v8 (forked dependency repos)

Formerly `agentstation/deno` (Deno-family monorepo fork) and
`agentstation/rusty_v8` (V8 binding fork). These repos do **not** get
internal renames -- they preserve upstream identifiers (`deno_core`,
`deno_node`, `rusty_v8`, etc.) so the workspace `Cargo.toml`
`[patch.crates-io]` patch surface continues to match published crate names.

What DOES change:

1. **Origin URL** -- the GitHub transfer in main-plan Phase 0 redirects
   `agentstation/deno` -> `nimbus/deno` and `agentstation/rusty_v8` ->
   `nimbus/rusty_v8`. Confirm redirects then update local clone remotes.

2. **Locker tag re-publish** -- the canonical main-repo `Cargo.toml`
   `[patch.crates-io]` pins these forks at locker tags (currently
   `v2.7.14-locker.41` for the deno-family crates and `v147.4.0-locker.2`
   for `rusty_v8`). After transfer:
   - Verify the existing tags survive the transfer (GitHub usually preserves
     refs on org rename) and are reachable at the new URL.
   - If the main-repo rename PR cuts a fresh locker tag (e.g. to capture
     incidental fork fixes alongside the rename), publish it under
     `nimbus/deno` / `nimbus/rusty_v8` and bump the main-repo `Cargo.toml`
     to that tag in main-plan Phase 1b.
   - Either way, the renamed git URL must resolve at the pinned tag before
     `make deny` (main-plan Phase 1g) runs successfully.

3. **CI badge / README links** in each fork repo's `README.md` if any
   reference the former `agentstation` URL or owner. Optional cleanup.

4. **Workflow posture decision** -- these forks carry upstream automation that
   can publish crates, npm packages, cloud artifacts, signed binaries, GitHub
   releases, and Slack/GCP/S3 side effects. Decide before transfer whether the
   Nimbus forks will run those workflows:
   - **Tag-only dependency forks (recommended for the rename):** disable or
     restrict upstream release/publish/update workflows in `nimbus/deno` and
     `nimbus/rusty_v8`, preserve transferred tags/assets, and verify main-repo
     `cargo fetch` plus `make deny`.
   - **Actively releasing forks:** reprovision every listed secret/variable,
     configure org action allowlists for Azure/GCP/Deno actions, recreate
     release/tag protections, and perform a dry-run release on disposable
     locker tags before repinning the main repo.

### Verification

```sh
# Inside the renamed local checkouts
git -C ~/src/github.com/nimbus/deno    remote -v   # origin -> nimbus/deno
git -C ~/src/github.com/nimbus/rusty_v8 remote -v  # origin -> nimbus/rusty_v8

# Main-repo Cargo.toml + Cargo.lock resolve
cd ~/src/github.com/nimbus/nimbus
cargo fetch        # must succeed without unknown-git failures
make deny          # must succeed once allow-git lists nimbus/* URLs

# If upstream release workflows remain enabled, verify their secrets/vars are
# intentionally present; otherwise verify they are disabled or gated so they
# cannot publish with stale denoland/agentstation credentials.
gh secret list --repo nimbus/deno
gh variable list --repo nimbus/deno
gh secret list --repo nimbus/rusty_v8
gh variable list --repo nimbus/rusty_v8
```

### Out of scope for this section

Internal symbol renames inside the Deno/V8 forks. The forks deliberately
mirror upstream so they can absorb upstream patches; renaming internals
would diverge from upstream and is not in scope for this rename.

---

## Execution Order

Within this satellite plan, repos can be updated independently and in parallel.
However, they must all complete before the main repo cuts a release with the
new names.

```
Phase 0 (main plan): Transfer neovex-machine-os, neovex-crun,
                      deno, rusty_v8 to nimbus org
                      (homebrew-tap stays in agentstation)
         |
         v
   +----------+---------+---------+---------+
   |          |         |         |         |
   v          v         v         v         v
 MOS-1..6  CRUN-1..8  TAP-1..4  Repo 4    Delete neovex.rb
 (machine)  (crun)    (new repo) (forks:   from agentstation
   |          |         |        deno +    /homebrew-tap
   |          |         |        rusty_v8       |
   |          |         |        locker tag     |
   |          |         |        re-publish)    |
   +----------+---------+---------+---------+
         |
         v
Main plan Phases 1-8: Rename main repo internals
   (Phase 1g `make deny` requires Repo 4 locker tags to resolve)
         |
         v
First release under nimbus/nimbus: end-to-end verification
```

### Coordination with main repo

The main repo's release workflow calls into machine-os and homebrew-tap. Both
sides of these interfaces must be updated atomically (or close to it):

1. **machine-os workflow inputs** -- the main repo's `release.yml` passes
   `neovex_version` and `neovex_artifact_name` to machine-os `build.yml`. Both
   must rename these inputs in the same release window.

2. **machine-os OCI media types** -- the main repo's image puller parses
   `application/vnd.neovex.machine.disk.layer.v1.*` media types. Both sides must
   agree on the new `vnd.nimbus.machine.*` types.

3. **machine-os OCI annotations** -- the main repo reads
   `io.neovex.machine.attestation.repository` and
   `io.neovex.machine.neovex.version` from image manifests. Both must rename.

4. **homebrew-tap API path** -- the main repo's `release.yml` pushes to
   `repos/nimbus/homebrew-tap/contents/Casks/nimbus.rb`. The new
   `nimbus/homebrew-tap` repo must exist and be accessible to the release
   workflow's `HOMEBREW_TAP_TOKEN`.

5. **GHCR image reference** -- the main repo's default image reference
   (`ghcr.io/nimbus/nimbus-machine-os:v{version}`) must match what machine-os
   publishes.

Since there are no users and no production releases, the simplest approach is
to update all repos in a single coordinated push, then cut a test release to
verify the full pipeline.

---

## Risk Notes

- **Workflow ref tag**: machine-os `build.yml` is pinned at
  `@release-workflow-v1`. After transfer+rename, verify the tag still resolves
  or create a new one.
- **GHCR namespace**: Old images at `ghcr.io/agentstation/*` will become
  inaccessible once the org transfer completes. No migration needed since there
  are no production users.
- **Crun patch**: The C patch itself is upstream-generic (reads `krun.port_map`
  OCI annotation). Only the surrounding build/packaging infrastructure uses
  "neovex" branding.
- **Homebrew tap split**: `agentstation/homebrew-tap` stays in agentstation.
  A new `nimbus/homebrew-tap` is created for nimbus only. The main repo's
  release workflow `HOMEBREW_TAP_TOKEN` must have write access to the new repo.
- **OCI media types**: `application/vnd.neovex.machine.*` is a custom media
  type used by the main repo's OCI image puller. Both sides must agree on the
  rename or image pulling will break.
- **No Neovex/Nimbus published packages**: No Neovex-owned crates.io, npm, apt,
  or COPR packages exist yet, so no registry migration is needed for the main
  product. This does not apply to the forked Deno/V8 automation, which carries
  upstream publish/release workflows; handle those through the Repo 4 workflow
  posture decision.
- **License rename**: Both machine-os and crun repos use the
  `Neovex Community License`. Coordinate with the main repo's LICENSE update.

---

## Out of Scope

The following are discovered but not covered by this plan:

- **Other agentstation products in `agentstation/homebrew-tap`**: starmap,
  tokenizer, vhs, pocket, tydirium, and shared tap infrastructure stay in
  agentstation. Removing `Casks/neovex.rb` and removing user-facing Neovex
  install/docs references from the shared tap README are in scope for this
  rename; changing unrelated product formulas/casks is not.
- **Other agentstation repos** (starmap, tokenizer, vhs, pocket, tydirium,
  nix-packages, etc.): These are independent products that stay in agentstation.
  They do not reference "neovex" internally.
