# Rename Satellite Repos: nimbus-machine-os, nimbus-crun, homebrew-tap

Prerequisite execution plan for renaming the satellite repositories before the
main repo rename in `docs/plans/nimbus-rename-plan.md`. Covers the three repos
that contain internal "nimbus" or "nimbus" references requiring updates.

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
1. Transfer nimbus-specific repos to `nimbus` org and create new
   `nimbus/homebrew-tap` (Phase 0 of main plan; `nimbus/homebrew-tap`
   stays in nimbus since it is shared with other products).
   Note: the Deno-family fork is `nimbus/deno` (monorepo), not the
   old standalone `nimbus/deno` which is historical only.
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
  the shared `nimbus/homebrew-tap` token is not reused.
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
| `nimbus/deno` | `DENOBOT_PAT`, `DENOBOT_GIST_PAT`, `CARGO_REGISTRY_TOKEN`, `APPLE_CODESIGN_KEY`, `APPLE_CODESIGN_PASSWORD`, `AZURE_CLIENT_ID`, `AZURE_TENANT_ID`, `AZURE_SUBSCRIPTION_ID`, `GCP_SA_KEY`, `S3_SECRET_ACCESS_KEY`, `WPT_FYI_PW`, `NODE_COMPAT_SLACK_TOKEN`, `NODE_COMPAT_SLACK_CHANNEL`; vars `S3_ACCESS_KEY_ID`, `S3_ENDPOINT`, `S3_REGION` | Reprovision only if Nimbus intends to run upstream Deno release/publish/compat workflows. Otherwise disable those workflows or restrict them to the upstream `denoland/deno` condition and document that only tag reachability is required for Nimbus/Nimbus. |
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

Formerly `nimbus/nimbus-machine-os`. Builds the Fedora bootc-based guest
Linux VM image for macOS machine support. The most complex satellite repo.
~150+ lowercase "nimbus", ~25 uppercase "NIMBUS", ~70 "nimbus" references
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
| Repo name `nimbus-machine-os` | `nimbus-machine-os` |
| GHCR image `ghcr.io/nimbus/nimbus-machine-os` | `ghcr.io/nimbus/nimbus-machine-os` |
| Disk artifact `nimbus-machine-os.raw.gz` | `nimbus-machine-os.raw.gz` |
| OCI archive `nimbus-machine-os.ociarchive` | `nimbus-machine-os.ociarchive` |
| Guest binary `/usr/local/bin/nimbus` | `/usr/local/bin/nimbus` |
| Guest data dir `/var/lib/nimbus/` | `/var/lib/nimbus/` |
| Guest control dir `/var/lib/nimbus/control/` | `/var/lib/nimbus/control/` |
| Guest socket `/run/nimbus/nimbus.sock` | `/run/nimbus/nimbus.sock` |
| Systemd unit `nimbus.socket` | `nimbus.socket` |
| Systemd unit `nimbus.service` | `nimbus.service` |
| OCI annotations `io.nimbus.machine.*` | `io.nimbus.machine.*` |
| OCI media types `application/vnd.nimbus.machine.*` | `application/vnd.nimbus.machine.*` |
| Config files `999-nimbus-machine.conf` etc. | `999-nimbus-machine.conf` etc. |
| Env vars `NIMBUS_MACHINE_OS_*` | `NIMBUS_MACHINE_OS_*` |
| Env var `NIMBUS_BIB_IMAGE` | `NIMBUS_BIB_IMAGE` |
| Workflow input `nimbus_version` | `nimbus_version` |
| Workflow input `nimbus_artifact_name` | `nimbus_artifact_name` |
| Artifact name `nimbus-machine-os-arm64` | `nimbus-machine-os-arm64` |
| CLI flags `--nimbus-binary`, `--nimbus-version` | `--nimbus-binary`, `--nimbus-version` |
| License `Nimbus Community License` | `Nimbus Community License` |

### Changes required

#### MOS-1: Workflows (.github/workflows/)

**`build.yml`** (~39 references, 489 lines):
- Workflow input descriptions: `"Nimbus release tag"` -> `"Nimbus release tag"`
- Input names if exposed to callers: `nimbus_version` -> `nimbus_version`,
  `nimbus_artifact_name` -> `nimbus_artifact_name`
- Default image reference: `ghcr.io/nimbus/nimbus-machine-os:v0.0.0-dev`
  -> `ghcr.io/nimbus/nimbus-machine-os:v0.0.0-dev`
- Checkout repository: `nimbus/nimbus-machine-os` ->
  `nimbus/nimbus-machine-os`
- Temp directory names: `${RUNNER_TEMP}/nimbus-machine-os` ->
  `${RUNNER_TEMP}/nimbus-machine-os` (layout, release dirs too)
- API calls to fetch latest tag: `repos/nimbus/nimbus/releases/latest` ->
  `repos/nimbus/nimbus/releases/latest`
- Download URL: `github.com/nimbus/nimbus/releases/download/*/nimbus_linux_arm64.tar.gz`
  -> `github.com/nimbus/nimbus/releases/download/*/nimbus_linux_arm64.tar.gz`
- Tar extraction: `nimbus_linux_arm64.tar.gz` -> `nimbus_linux_arm64.tar.gz`
- Binary output var: `nimbus_binary=${RUNNER_TEMP}/nimbus` ->
  `nimbus_binary=${RUNNER_TEMP}/nimbus`
- Build script flags: `--nimbus-binary`, `--nimbus-version` ->
  `--nimbus-binary`, `--nimbus-version`
- GHCR push target: `ghcr.io/nimbus/nimbus-machine-os:${tag}` ->
  `ghcr.io/nimbus/nimbus-machine-os:${tag}`
- Source repository URL: `github.com/nimbus/nimbus-machine-os` ->
  `github.com/nimbus/nimbus-machine-os`
- Env vars: `NIMBUS_MACHINE_OS_REGISTRY_USERNAME`,
  `NIMBUS_MACHINE_OS_REGISTRY_PASSWORD` -> `NIMBUS_MACHINE_OS_*`
- GitHub App: `owner: nimbus`, `repositories: nimbus-machine-os` ->
  `owner: nimbus`, `repositories: nimbus-machine-os`
- Error messages referencing `nimbus-machine-os` and `nimbus`
- Release notes mentioning `nimbus/nimbus`
- Artifact stage names: `nimbus-machine-os-arm64`,
  `nimbus-machine-os-arm64-publish` -> `nimbus-machine-os-*`
- Release repo: `release_repository="nimbus/nimbus-machine-os"` ->
  `"nimbus/nimbus-machine-os"`
- Build output artifact: `nimbus-machine-os.raw.gz` -> `nimbus-machine-os.raw.gz`
- App credentials: after the rename, this workflow still needs
  `MACHINE_OS_RELEASE_APP_PRIVATE_KEY` when invoked as a reusable workflow that
  publishes or creates a release. Ensure the main repo passes the secret and the
  machine-os repo has its own copy for direct or dispatch-driven release paths.

**`publish.yml`** (~22 references, 254 lines):
- Input descriptions: `"Nimbus release tag"` -> `"Nimbus release tag"`,
  `"nimbus/nimbus"` -> `"nimbus/nimbus"`
- Checkout repository: `nimbus/nimbus-machine-os` ->
  `nimbus/nimbus-machine-os`
- Temp directory names: same pattern as build.yml
- Error messages: `"nimbus-machine-os"` -> `"nimbus-machine-os"`
- Env vars: `NIMBUS_MACHINE_OS_REGISTRY_USERNAME`,
  `NIMBUS_MACHINE_OS_REGISTRY_PASSWORD` -> `NIMBUS_MACHINE_OS_*`
- GHCR publish target: `ghcr.io/nimbus/nimbus-machine-os` ->
  `ghcr.io/nimbus/nimbus-machine-os`
- Release title/notes: all nimbus/nimbus references
- Release repo: `release_repository='nimbus/nimbus-machine-os'` ->
  `'nimbus/nimbus-machine-os'`
- Build output: `nimbus-machine-os.raw.gz` -> `nimbus-machine-os.raw.gz`
- Artifact name: `nimbus-machine-os-arm64-publish` -> `nimbus-machine-os-*`
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
  renames. The file may have no direct `nimbus` strings today, but it must still
  point at valid workflow, container, or package directories after the
  `nimbus-machine-os` rename.

#### MOS-2: Containerfile and image build scripts

**`images/Containerfile`** (3 references):
- `COPY nimbus /usr/local/bin/nimbus` -> `COPY nimbus /usr/local/bin/nimbus`
- `RUN chmod 0755 /usr/local/bin/nimbus` -> `/usr/local/bin/nimbus`

**`images/build.sh`** (~25 references, 191 lines):
- Usage text: `"Build the Nimbus Fedora CoreOS guest image"` -> `"Nimbus"`
- CLI flags: `--nimbus-binary`, `--nimbus-version` ->
  `--nimbus-binary`, `--nimbus-version`
- Variables: `nimbus_binary`, `nimbus_version` -> `nimbus_binary`,
  `nimbus_version`
- Default image name: `localhost/nimbus-machine-os:dev` ->
  `localhost/nimbus-machine-os:dev`
- Error messages: `"nimbus binary does not exist"` etc.
- Binary copy: `install ... "${context_dir}/nimbus"` -> `"${context_dir}/nimbus"`
- OCI archive: `nimbus-machine-os.ociarchive` -> `nimbus-machine-os.ociarchive`
- Env var: `NIMBUS_BIB_IMAGE` -> `NIMBUS_BIB_IMAGE`
- Compressed output: `nimbus-machine-os.raw.gz` -> `nimbus-machine-os.raw.gz`
- Build manifest vars: `nimbus_binary`, `nimbus_version`,
  `nimbus_binary_sha256` -> `nimbus_*`

**`images/build-common.sh`** (~6 references):
- Data directories: `/var/lib/nimbus/control`, `/var/lib/nimbus/data` ->
  `/var/lib/nimbus/*`
- Config files dropped into image:
  - `/etc/containers/registries.conf.d/999-nimbus-machine.conf` ->
    `999-nimbus-machine.conf`
  - `/etc/ssh/sshd_config.d/99-nimbus-machine-sshd.conf` ->
    `99-nimbus-machine-sshd.conf`
  - `/etc/sysctl.d/10-nimbus-machine-inotify.conf` ->
    `10-nimbus-machine-inotify.conf`
- chmod paths: `/var/lib/nimbus /var/lib/nimbus/control /var/lib/nimbus/data` ->
  `/var/lib/nimbus/*`

#### MOS-3: OCI packaging and publishing scripts

**`scripts/package-oci.sh`** (~19 references):
- Custom OCI media types (critical -- consumed by the main repo's image puller):
  - `application/vnd.nimbus.machine.disk.layer.v1.raw+gzip` ->
    `application/vnd.nimbus.machine.disk.layer.v1.raw+gzip`
  - `application/vnd.nimbus.machine.disk.layer.v1.raw+zstd` ->
    `application/vnd.nimbus.machine.disk.layer.v1.raw+zstd`
  - `application/vnd.nimbus.machine.disk.layer.v1.raw` ->
    `application/vnd.nimbus.machine.disk.layer.v1.raw`
  - `application/vnd.nimbus.machine.disk.layer.v1.blob` ->
    `application/vnd.nimbus.machine.disk.layer.v1.blob`
- OCI manifest annotations:
  - `io.nimbus.machine.attestation.repository` ->
    `io.nimbus.machine.attestation.repository`
  - `io.nimbus.machine.nimbus.version` ->
    `io.nimbus.machine.nimbus.version`
- Env vars: `NIMBUS_MACHINE_OS_PACKAGE_TEST_ARCH`,
  `NIMBUS_MACHINE_OS_SOURCE_REPOSITORY_URL`,
  `NIMBUS_MACHINE_OS_ATTESTATION_REPOSITORY`,
  `NIMBUS_MACHINE_OS_VERSION` -> `NIMBUS_MACHINE_OS_*`
- Default values: `nimbus/nimbus-machine-os` -> `nimbus/nimbus-machine-os`
- CLI flag: `--nimbus-version` -> `--nimbus-version`
- Variable: `nimbus_version` -> `nimbus_version`

**`scripts/publish.sh`** (~15 references):
- Usage text: `"Push a packaged Nimbus machine"` -> `"Nimbus"`
- Env vars: `NIMBUS_MACHINE_OS_REGISTRY_USERNAME`,
  `NIMBUS_MACHINE_OS_REGISTRY_PASSWORD` -> `NIMBUS_MACHINE_OS_*`
- Example URLs: `ghcr.io/nimbus/nimbus-machine-os` ->
  `ghcr.io/nimbus/nimbus-machine-os`
- Error messages referencing env var names

**`scripts/build.sh`** (wrapper, ~15 references):
- Usage text: `"Build the nimbus-machine-os guest image"` -> `"nimbus-machine-os"`
- CLI flags: `--nimbus-binary`, `--nimbus-version` ->
  `--nimbus-binary`, `--nimbus-version`
- Variables: `nimbus_binary`, `nimbus_version` -> `nimbus_*`
- Env var: `NIMBUS_MACHINE_OS_BUILD_WRAPPER_TEST_UNAME` ->
  `NIMBUS_MACHINE_OS_BUILD_WRAPPER_TEST_UNAME`
- Temp dirs: `/tmp/nimbus-machine-os` -> `/tmp/nimbus-machine-os`
- Error messages: `"nimbus binary not found"` etc.
- Delegation: passes `--nimbus-binary`, `--nimbus-version` to `images/build.sh`

#### MOS-4: Verification scripts

**`scripts/verify-recipe.sh`** (~4 references):
- `grep -F 'COPY nimbus /usr/local/bin/nimbus'` -> `'COPY nimbus /usr/local/bin/nimbus'`
- `nimbus_binary="${temp_dir}/nimbus"` -> `nimbus_binary=...`
- Env vars: `NIMBUS_MACHINE_OS_BUILD_TEST_UNAME`,
  `NIMBUS_MACHINE_OS_BUILD_TEST_UID` -> `NIMBUS_MACHINE_OS_*`

**`scripts/verify-build-helper.sh`** (~2 references):
- `NIMBUS_MACHINE_OS_BUILD_WRAPPER_TEST_UNAME` -> `NIMBUS_*`
- `/tmp/nimbus-machine-os-out` -> `/tmp/nimbus-machine-os-out`

**`scripts/verify-oci-layout-helper.sh`** (~11 references):
- All test assertion strings:
  - `ghcr.io/nimbus/nimbus-machine-os:v1.2.3` ->
    `ghcr.io/nimbus/nimbus-machine-os:v1.2.3`
  - `github.com/nimbus/nimbus-machine-os` ->
    `github.com/nimbus/nimbus-machine-os`
  - `nimbus/nimbus` -> `nimbus/nimbus` (attestation repo)
  - `io.nimbus.machine.*` -> `io.nimbus.machine.*` (annotation assertions)
- Success message: `"verified nimbus machine-os"` -> `"verified nimbus machine-os"`

**`scripts/verify-publish-helper.sh`** (~14 references):
- All test assertion strings same pattern as oci-layout helper
- Env vars: `NIMBUS_MACHINE_OS_REGISTRY_USERNAME`,
  `NIMBUS_MACHINE_OS_REGISTRY_PASSWORD` -> `NIMBUS_MACHINE_OS_*`
- Additional reference assertions for `ghcr.io/nimbus/nimbus-machine-os:next`
- Success message: `"verified nimbus machine-os publish wrapper"` -> `"nimbus"`

#### MOS-5: Documentation and license

**`README.md`** (~25+ references):
- Title: `# nimbus-machine-os` -> `# nimbus-machine-os`
- All prose: `"nimbus"` -> `"nimbus"`, `"Nimbus"` -> `"Nimbus"`
- GHCR URL: `ghcr.io/nimbus/nimbus-machine-os` ->
  `ghcr.io/nimbus/nimbus-machine-os`
- Main repo URLs: `github.com/nimbus/nimbus` ->
  `github.com/nimbus/nimbus`
- Download examples: `nimbus_linux_arm64.tar.gz` -> `nimbus_linux_arm64.tar.gz`
- CLI flags in examples: `--nimbus-binary`, `--nimbus-version`
- Output dirs: `--output-dir /tmp/nimbus-machine-os` -> `/tmp/nimbus-machine-os`
- OCI annotations: `io.nimbus.machine.*` -> `io.nimbus.machine.*`
- Cross-repo references: `nimbus/nimbus-machine-os`,
  `nimbus/nimbus`

**`images/README.md`** (~16 references):
- Title: `# Nimbus Machine OS Recipe` -> `# Nimbus Machine OS Recipe`
- All same patterns as root README
- Artifact names: `nimbus-machine-os.ociarchive`, `nimbus-machine-os.raw.gz`
- Publish examples: `ghcr.io/nimbus/nimbus-machine-os`

**`LICENSE`** (~7 references):
- `Nimbus Community License 1.0` -> `Nimbus Community License 1.0`
- `Copyright (c) 2026 the Nimbus contributors` -> `Nimbus contributors`
- All other `Nimbus` references in license text

#### MOS-6: Verification

```sh
# After all changes:
rg "nimbus" .
rg "nimbus" .
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

Formerly `nimbus/nimbus-crun`. Builds a patched crun binary with TSI port
mapping support for the krun backend. Small repo: 9 files, ~39 lowercase
"nimbus", ~8 capitalized "Nimbus" (LICENSE), 1 "nimbus" reference.

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
| Repo name `nimbus-crun` | `nimbus-crun` |
| Binary install path `/usr/libexec/nimbus/crun` | `/usr/libexec/nimbus/crun` |
| Release assets `nimbus-crun-linux-amd64` | `nimbus-crun-linux-amd64` |
| Release assets `nimbus-crun-linux-arm64` | `nimbus-crun-linux-arm64` |
| Builder image tag `nimbus-crun-builder:*` | `nimbus-crun-builder:*` |
| Git tag `v1.27-nimbus.2` | `v1.27-nimbus.2` |
| License `Nimbus Community License` | `Nimbus Community License` |

### Changes required

#### CRUN-1: Build script (scripts/build.sh, ~16 references)

- Usage text: `"build-nimbus-crun.sh"` -> `"build-nimbus-crun.sh"`
  (or just `"build.sh"` since the script name is already `build.sh`)
- Prose: `"Build the checked-in patched nimbus crun binary"` -> `"nimbus crun"`
- Example paths: `--output /tmp/nimbus-crun-stage/crun` ->
  `/tmp/nimbus-crun-stage/crun`
- Install path: `--install-path /usr/libexec/nimbus/crun` ->
  `/usr/libexec/nimbus/crun`
- Default output: `${TMPDIR:-/tmp}/nimbus-crun-stage/crun` ->
  `nimbus-crun-stage/crun`
- Error message: `"build-nimbus-crun.sh requires a Linux host"` -> `nimbus`
- Temp dir: `mktemp -d .../nimbus-crun-build.XXXXXX` -> `nimbus-crun-build.*`

#### CRUN-2: Verification scripts

**`scripts/verify-fedora-userspace.sh`** (~8 references):
- Usage text: `"verify-nimbus-crun-fedora-userspace.sh"` -> `"nimbus"`
- Prose: `"Run the nimbus crun patch + build helpers"` -> `"nimbus"`
- Example paths: `--output-dir /tmp/nimbus-crun-fedora-output` -> `nimbus-*`
- Temp dirs: `nimbus-crun-fedora-output.*`, `nimbus-crun-fedora-build.*` ->
  `nimbus-crun-*`

**`scripts/verify-patch.sh`**: Check for any nimbus references (likely minimal
since it verifies the upstream patch applies cleanly).

#### CRUN-3: Workflow (.github/workflows/build.yml, ~20 references)

- Workflow name: `"Build nimbus-crun"` -> `"Build nimbus-crun"`
- Job name: `"Build nimbus-crun (linux ${{ matrix.arch }})"` -> `"nimbus-crun"`
- Temp dirs: `${RUNNER_TEMP}/nimbus-crun-output`, `nimbus-crun-build` ->
  `nimbus-crun-*`
- Docker builder tag: `nimbus-crun-builder:${{ matrix.arch }}` ->
  `nimbus-crun-builder:*`
- Binary rename: `mv .../crun .../nimbus-crun-linux-${{ matrix.arch }}` ->
  `nimbus-crun-linux-*`
- Artifact names: `nimbus-crun-linux-${{ matrix.arch }}` ->
  `nimbus-crun-linux-*`
- Release asset paths: all `nimbus-crun-linux-amd64`, `nimbus-crun-linux-arm64`
- Checksum generation: `sha256sum nimbus-crun-linux-*` -> `nimbus-crun-linux-*`
- Release title: `"nimbus-crun ${{ github.ref_name }}"` -> `"nimbus-crun"`
- Verification step: `./nimbus-crun-linux-amd64 --version` ->
  `./nimbus-crun-linux-amd64`
- Release permissions: keep `contents: write`, `id-token: write`, and
  `attestations: write`; verify the `nimbus-crun` repo/org default token policy
  permits these explicit scopes.
- Third-party actions: add `docker/setup-buildx-action@v3` and
  `docker/build-push-action@v6` to any org selected-action allowlist.

#### CRUN-4: Docker builder

**`.github/container/Dockerfile.builder`**: Check for any nimbus references in
the builder image definition (likely minimal -- it installs build dependencies).

**`.github/dependabot.yml`**:
- Verify package ecosystem directories and any path filters after workflow and
  Docker builder path changes. The file may have no direct `nimbus` strings
  today, but path validity should be part of the crun repo rename review.

#### CRUN-5: Patch file

**`patches/0001-krun-add-tsi-port-mapping-via-oci-annotation.patch`**: The C
code in the patch is upstream-generic (reads `krun.port_map` OCI annotation,
not nimbus-branded). Check the patch header/commit message for any nimbus
references.

#### CRUN-6: Documentation and license

**`README.md`** (~8 references):
- Title: `# nimbus-crun` -> `# nimbus-crun`
- Link: `[nimbus](https://github.com/nimbus/nimbus)` ->
  `[nimbus](https://github.com/nimbus/nimbus)`
- Asset names: `nimbus-crun-linux-amd64`, `nimbus-crun-linux-arm64`
- Install path: `/usr/libexec/nimbus/crun` -> `/usr/libexec/nimbus/crun`
- Example paths: `--output /tmp/nimbus-crun`, `/tmp/nimbus-crun --version`

**`LICENSE`** (~7 references):
- `Nimbus Community License 1.0` -> `Nimbus Community License 1.0`
- All other `Nimbus` references in license text

#### CRUN-7: Git tag format

Current tag: `v1.27-nimbus.2`. Future tags should use `v1.27-nimbus.2` or
similar. No existing tags need to be moved (pre-launch, no consumers).

#### CRUN-8: Verification

```sh
rg "nimbus" .
rg "nimbus" .
# Both should return 0 hits

# Verify build script help text:
head -20 scripts/build.sh  # should reference nimbus-crun

# Verify workflow asset names:
grep -F "nimbus-crun-linux" .github/workflows/build.yml
```

---

## Repo 3: nimbus/homebrew-tap (NEW repo)

`nimbus/homebrew-tap` is a **shared tap** serving 6 products (nimbus,
starmap, tokenizer, vhs, pocket, tydirium). It should NOT be transferred to
the nimbus org. Instead, create a new `nimbus/homebrew-tap` repo containing
only the nimbus cask, and delete `Casks/nimbus.rb` from the nimbus tap.

### What the nimbus tap contains (for context)

```
Casks/nimbus.rb       <- the only file that concerns us
Casks/starmap.rb      <- unrelated product, stays in nimbus
Casks/tokenizer.rb    <- unrelated product, stays in nimbus
Casks/vhs.rb          <- unrelated product, stays in nimbus
Formula/pocket.rb     <- unrelated product, stays in nimbus
Formula/tydirium.rb   <- unrelated product, stays in nimbus
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
placeholder adapted from the current `Casks/nimbus.rb`:

- `cask "nimbus"` (not `"nimbus"`)
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

#### TAP-3: Delete nimbus cask from nimbus tap

Remove `Casks/nimbus.rb` from `nimbus/homebrew-tap`. The other 5 products
remain untouched in that repo. Also remove the Nimbus install section and any
other user-facing Nimbus install references from the shared tap README after
the new `nimbus/homebrew-tap` path is verified.

#### TAP-4: Verification

```sh
# Verify new tap works:
brew tap nimbus/tap
brew install nimbus/tap/nimbus
nimbus --version

# Verify old tap still serves other products:
brew install nimbus/tap/starmap
```

---

## Repo 4: nimbus/deno + nimbus/rusty_v8 (forked dependency repos)

Formerly `nimbus/deno` (Deno-family monorepo fork) and
`nimbus/rusty_v8` (V8 binding fork). These repos do **not** get
internal renames -- they preserve upstream identifiers (`deno_core`,
`deno_node`, `rusty_v8`, etc.) so the workspace `Cargo.toml`
`[patch.crates-io]` patch surface continues to match published crate names.

What DOES change:

1. **Origin URL** -- the GitHub transfer in main-plan Phase 0 redirects
   `nimbus/deno` -> `nimbus/deno` and `nimbus/rusty_v8` ->
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
   reference the former `nimbus` URL or owner. Optional cleanup.

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
# cannot publish with stale denoland/nimbus credentials.
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
Phase 0 (main plan): Transfer nimbus-machine-os, nimbus-crun,
                      deno, rusty_v8 to nimbus org
                      (homebrew-tap stays in nimbus)
         |
         v
   +----------+---------+---------+---------+
   |          |         |         |         |
   v          v         v         v         v
 MOS-1..6  CRUN-1..8  TAP-1..4  Repo 4    Delete nimbus.rb
 (machine)  (crun)    (new repo) (forks:   from nimbus
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
   `nimbus_version` and `nimbus_artifact_name` to machine-os `build.yml`. Both
   must rename these inputs in the same release window.

2. **machine-os OCI media types** -- the main repo's image puller parses
   `application/vnd.nimbus.machine.disk.layer.v1.*` media types. Both sides must
   agree on the new `vnd.nimbus.machine.*` types.

3. **machine-os OCI annotations** -- the main repo reads
   `io.nimbus.machine.attestation.repository` and
   `io.nimbus.machine.nimbus.version` from image manifests. Both must rename.

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
- **GHCR namespace**: Old images at `ghcr.io/nimbus/*` will become
  inaccessible once the org transfer completes. No migration needed since there
  are no production users.
- **Crun patch**: The C patch itself is upstream-generic (reads `krun.port_map`
  OCI annotation). Only the surrounding build/packaging infrastructure uses
  "nimbus" branding.
- **Homebrew tap split**: `nimbus/homebrew-tap` stays in nimbus.
  A new `nimbus/homebrew-tap` is created for nimbus only. The main repo's
  release workflow `HOMEBREW_TAP_TOKEN` must have write access to the new repo.
- **OCI media types**: `application/vnd.nimbus.machine.*` is a custom media
  type used by the main repo's OCI image puller. Both sides must agree on the
  rename or image pulling will break.
- **No Nimbus/Nimbus published packages**: No Nimbus-owned crates.io, npm, apt,
  or COPR packages exist yet, so no registry migration is needed for the main
  product. This does not apply to the forked Deno/V8 automation, which carries
  upstream publish/release workflows; handle those through the Repo 4 workflow
  posture decision.
- **License rename**: Both machine-os and crun repos use the
  `Nimbus Community License`. Coordinate with the main repo's LICENSE update.

---

## Out of Scope

The following are discovered but not covered by this plan:

- **Other nimbus products in `nimbus/homebrew-tap`**: starmap,
  tokenizer, vhs, pocket, tydirium, and shared tap infrastructure stay in
  nimbus. Removing `Casks/nimbus.rb` and removing user-facing Nimbus
  install/docs references from the shared tap README are in scope for this
  rename; changing unrelated product formulas/casks is not.
- **Other nimbus repos** (starmap, tokenizer, vhs, pocket, tydirium,
  nix-packages, etc.): These are independent products that stay in nimbus.
  They do not reference "nimbus" internally.
