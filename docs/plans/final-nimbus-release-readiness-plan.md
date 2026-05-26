# Final Nimbus Release Readiness Plan

Status: in progress
Owner: Nimbus release work
Created: 2026-05-25

## Purpose

This plan ties the recently standardized Nimbus fork releases back into the
`nimbus/nimbus` product release. A final Nimbus binary should only be released
after the source fork stack is healthy, Nimbus consumes the intended released
fork tags, the release source tree is clean, downstream machine/desktop
alignment has been classified, full Nimbus verification passes, and the
published GitHub release artifacts can be downloaded and re-verified.

The release decision is intentionally evidence-driven. A fork tag existing on
GitHub is necessary, but it is not enough. The release baseline must prove that
the current Nimbus source compiles, tests, packages, and runs against those
exact fork inputs.

## Current Snapshot

Verified while writing the plan:

- Current live `nimbus/nimbus` release: `v0.1.31`, published 2026-05-15.
- Current workspace package version in `Cargo.toml`: `0.1.31`.
- `v0.1.31` already exists locally and on GitHub, so the next release tag must
  be a new version, expected to be `v0.1.32` unless the execution audit chooses
  and documents a different SemVer bump.
- Live fork releases already published:
  - `nimbus/rusty_v8` `v149.0.0-nimbus.1`
  - `nimbus/nimbus-crun` `v1.27.1-nimbus.2`
  - `nimbus/nimbus-libkrun` `v1.18.1-nimbus.1`
- Source-only fork tags consumed by Nimbus:
  - `nimbus/deno` `v2.8.0-nimbus.5`
  - `nimbus/bun` source tag `nimbus-bun-jsc-proof-main-20260525`
- The active Bun default branch is now `nimbus/bun-main-20260525`, while the
  immutable adapter source contract remains the proof tag above.
- The current checkout has unrelated dirty generated files, package-lock churn,
  screenshots, and an untracked plan. The release must either cleanly resolve
  those changes or run from a dedicated clean release worktree so no unreviewed
  local state enters the release.

## Release Principles

- Release from a clean worktree. If the primary checkout contains unrelated
  dirty user work, create a dedicated release worktree from the intended
  release commit and run all release gates there.
- Do not publish a release when any fork or Nimbus verification failure is
  Nimbus-owned and unresolved.
- If a fork gate fails because the matching upstream base fails in the same
  way, record the upstream-base evidence, link the upstream issue or commit
  range, and block or defer only if the failure affects Nimbus' consumed
  capability.
- Do not replace the single-binary default with optional Bun/JSC adapter
  artifacts. The base `nimbus` binary must still work without the optional
  adapter installed.
- Treat `nimbus/machine-os` as a release-coupled downstream. If the Nimbus
  release workflow builds or publishes a machine image, the machine-os source
  recipe, machine-os CI recipe, and Nimbus release workflow must agree on the
  same pinned bootc base image before the Nimbus tag is created.
- Keep `nimbus/desktop` and `packages/nimbus-ui` aligned, but classify their
  release coupling precisely: the embedded operator UI ships from this repo,
  while the desktop shell is a separate artifact that must either be green or
  have any unrelated hosted failure documented with evidence before release.
- Keep the release version, crate versions, JS workspace package versions,
  lockfile entries, changelog heading, tag, archive names, and package helpers
  in one consistent version contract.

## Execution Plan

### NRR0 - Release Target and Source Baseline

Status: completed 2026-05-25

Decide the exact release version and source commit.

Success criteria:

- `gh release view --repo nimbus/nimbus` confirms the current latest release.
- `git tag --list 'v*' --sort=-v:refname` confirms the chosen release tag does
  not already exist locally.
- `git ls-remote --tags git@github.com:nimbus/nimbus.git <tag>` confirms the
  chosen release tag does not already exist remotely.
- The selected source commit is recorded by full SHA.
- If the primary checkout is dirty, a clean release worktree is created and all
  later gates run from that worktree.

Evidence:

- Selected release version: `v0.1.32`.
- `gh release view --repo nimbus/nimbus --json tagName,isDraft,isPrerelease,publishedAt,url,targetCommitish`
  returned current live release `v0.1.31`, non-draft, non-prerelease,
  published `2026-05-15T00:32:23Z`.
- `git tag --list 'v*' --sort=-v:refname | head -10` showed latest local tag
  `v0.1.31`; no local `v0.1.32` tag exists.
- `git ls-remote --tags git@github.com:nimbus/nimbus.git v0.1.32` returned no
  rows; no remote `v0.1.32` tag exists.
- Selected starting source commit:
  `7669d2672f98c27473659860296292f19dce3b24`.
- The primary checkout is dirty with unrelated generated Convex files,
  `package-lock.json` churn, screenshots, and an untracked plan, so the release
  gates will run from clean worktree
  `/Users/jack/src/github.com/nimbus/nimbus-worktrees/final-release-v0.1.32`
  on branch `codex/final-release-v0.1.32`.
- `git -C /Users/jack/src/github.com/nimbus/nimbus-worktrees/final-release-v0.1.32 status --short --branch`
  reported only `## codex/final-release-v0.1.32`.

### NRR1 - Fork Stack Health

Status: completed 2026-05-25

Verify every fork that Nimbus depends on is on the standardized branch/tag,
has the expected GitHub default branch, and has a clean local checkout.

Success criteria:

- `bash scripts/verify-fork-upstream-standardization.sh` passes.
- `gh repo view` confirms default branches:
  - `nimbus/deno` -> `nimbus/v2.8.0`
  - `nimbus/rusty_v8` -> `nimbus/v149.0.0`
  - `nimbus/bun` -> `nimbus/bun-main-20260525`
  - `nimbus/nimbus-crun` -> `nimbus/1.27.1`
  - `nimbus/nimbus-libkrun` -> `nimbus/v1.18.1`
- `gh release view` confirms the expected released fork artifacts:
  - `nimbus/rusty_v8` `v149.0.0-nimbus.1`
  - `nimbus/nimbus-crun` `v1.27.1-nimbus.2`
  - `nimbus/nimbus-libkrun` `v1.18.1-nimbus.1`
- If a fork has a failing local or hosted gate, the failure is reproduced and
  classified as either Nimbus-owned or upstream-owned with concrete evidence.
  Nimbus-owned failures are fixed before continuing.

Evidence:

- `bash scripts/verify-fork-upstream-standardization.sh` passed from the clean
  release worktree with `fork-standardization: pass`.
- The verifier confirmed all five local fork checkouts are clean and on the
  expected branches:
  - `nimbus/deno`: `nimbus/v2.8.0`
  - `nimbus/rusty_v8`: `nimbus/v149.0.0`
  - `nimbus/bun`: `nimbus/bun-main-20260525`
  - `nimbus/nimbus-crun`: `nimbus/1.27.1`
  - `nimbus/nimbus-libkrun`: `nimbus/v1.18.1`
- `gh repo view ... --json nameWithOwner,defaultBranchRef,description,url`
  confirmed the same live GitHub default branches and the capability-focused
  repository descriptions for all five forks.
- `gh release view` confirmed non-draft, non-prerelease releases:
  - `nimbus/rusty_v8` `v149.0.0-nimbus.1`, published
    `2026-05-25T20:52:35Z`
  - `nimbus/nimbus-libkrun` `v1.18.1-nimbus.1`, published
    `2026-05-25T21:50:09Z`
  - `nimbus/nimbus-crun` `v1.27.1-nimbus.2`, published
    `2026-05-25T21:54:06Z`
- `nimbus/deno` is source-tagged rather than release-asset-tagged for Nimbus'
  Cargo consumption. NRR4 exposed Deno-owned embedded Node hardening gaps, so
  the fork was updated, committed, tagged, and pushed on `nimbus/v2.8.0`:
  - commit `c0d530232406238305a69586769ef62d7d65e4de`
    (`runtime: harden embedded node vm and zlib`), annotated source tag
    `v2.8.0-nimbus.3`
  - `git ls-remote --tags git@github.com:nimbus/deno.git v2.8.0-nimbus.3`
    returned the remote tag object `a9e9fb577d86698e669c921c5ca6607234f029c9`
  - commit `9225357ba8697cf2c998eef62571779957a7a90c`
    (`runtime: return sqlite config errors`), annotated source tag
    `v2.8.0-nimbus.4`
  - `git push origin nimbus/v2.8.0 v2.8.0-nimbus.4` pushed branch
    `c0d5302324..9225357ba8` and created tag `v2.8.0-nimbus.4`
  - commit `37b6333a1f703db523efe8a703d36f2152ad087a`
    (`runtime: update DNS and TLS security dependencies`), annotated source
    tag `v2.8.0-nimbus.5`
  - `git push origin nimbus/v2.8.0 v2.8.0-nimbus.5` pushed branch
    `9225357ba8..37b6333a1f` and created tag `v2.8.0-nimbus.5`
  - Deno fork security-update verification passed:
    `cargo fmt --all --check`;
    `env CARGO_ENCODED_RUSTFLAGS=... cargo check -p deno_net -p deno_fetch -p deno_tls -p deno_node -p deno_node_sqlite`;
    `env CARGO_ENCODED_RUSTFLAGS=... cargo test -p deno_net -p deno_fetch -p deno_tls -- --test-threads=1`
    (`deno_fetch` 14 passed, `deno_net` 24 passed, `deno_tls` 3 passed)
  - `gh run list --repo nimbus/deno --branch nimbus/v2.8.0 --limit 5`
    returned no hosted active-branch runs, so Nimbus' product verification in
    NRR4 remains the release gate for the consumed Deno source tag.
- Hosted fork Actions status:
  - `nimbus/rusty_v8` active-branch CI succeeded on
    `9b77553883f1117ab3df62709b8673b803ed721b`.
  - `nimbus/bun` active-branch graph update succeeded on
    `ad0e1d2bbc6690651e04f10eaf1dcdf8a6c0de57`.
  - `nimbus/deno`, `nimbus/nimbus-crun`, and `nimbus/nimbus-libkrun` have no
    active-branch hosted runs to treat as release blockers.
- Investigated the earlier `nimbus/deno` hosted run on release ref
  `v2.8.0-nimbus.2`: it failed in upstream Deno's broad CI workflow while
  fetching the git-pinned `nimbus/rusty_v8` dependency on GitHub Windows
  runners, with `path too long` under
  `third_party/rust/chromium_crates_io/vendor/icu_calendar-v2/...`. This is a
  fork-hosted CI plumbing gap in the upstream Deno workflow shape, not a
  Nimbus runtime source failure. The Deno fork is source-only for Nimbus Cargo
  consumption, and Nimbus' own release workflow already enables Windows
  long-path handling before its release build. The release remains gated by
  NRR4/NRR7 compiling and testing Nimbus against the exact pinned Deno/rusty_v8
  tags.

### NRR2 - Nimbus Consumes the Released Forks

Status: completed 2026-05-26 after Deno source-tag amendments

Prove the Nimbus repo pins and installer/package surfaces consume the intended
Nimbus fork releases.

Success criteria:

- `Cargo.toml` and `Cargo.lock` point to `nimbus/deno` `v2.8.0-nimbus.5` and
  `nimbus/rusty_v8` `v149.0.0-nimbus.1`.
- Installer/package defaults point to `nimbus-crun` `v1.27.1-nimbus.2` and
  `nimbus-libkrun` `v1.18.1-nimbus.1`.
- Bun/JSC adapter source constants point to
  `nimbus-bun-jsc-proof-main-20260525`.
- Active code, scripts, and docs contain no active `*-locker.*` release pins.
- `make verify-bun-jsc-runtime-contract`,
  `make verify-bun-jsc-adapter-package`, and
  `make verify-bun-jsc-release-assets` pass.

Evidence:

- `rg -n "v2\\.8\\.0-nimbus\\.2|v149\\.0\\.0-nimbus\\.1|v1\\.27\\.1-nimbus\\.2|v1\\.18\\.1-nimbus\\.1|nimbus-bun-jsc-proof-main-20260525|locker-v|v[0-9][^[:space:]]*-locker" Cargo.toml Cargo.lock scripts .github Makefile docs/adapters docs/architecture docs/operating docs/plans/final-nimbus-release-readiness-plan.md docs/plans/fork-upstream-standardization-plan.md docs/plans/bun-jsc-distribution-and-release-plan.md docs/plans/bun-jsc-linked-adapter-plan.md docs/plans/README.md`
  originally confirmed active pins before the Deno hardening amendment.
- After the Deno hardening and security-dependency amendments, `Cargo.toml`
  patch entries use `nimbus/deno` `v2.8.0-nimbus.5` and `nimbus/rusty_v8`
  `v149.0.0-nimbus.1`.
- `Cargo.lock` resolves Deno-family crates to
  `37b6333a1f703db523efe8a703d36f2152ad087a` and rusty_v8 to
  `9b77553883f1117ab3df62709b8673b803ed721b`.
- `rg -n "/Users/jack/src/github.com/nimbus/deno|v2\\.8\\.0-nimbus\\.[234]|source = \"git\\+https://github.com/nimbus/deno" Cargo.toml Cargo.lock`
  confirmed the release worktree has no local Deno path override and no stale
  `v2.8.0-nimbus.2`, `v2.8.0-nimbus.3`, or `v2.8.0-nimbus.4` lock/pin after
  repinning to `v2.8.0-nimbus.5`.
  The remaining active pins are:
  - install/package helpers use `nimbus-crun` `v1.27.1-nimbus.2` and
    `nimbus-libkrun` `v1.18.1-nimbus.1`.
  - Bun/JSC source constants use `nimbus-bun-jsc-proof-main-20260525`.
- `rg -n "locker-v|v[0-9][^[:space:]]*-locker" Cargo.toml Cargo.lock scripts .github Makefile`
  returned no active code/package pins.
- `make verify-bun-jsc-runtime-contract` passed from the clean release
  worktree after building UI prerequisites and compiling the pinned
  Deno/rusty_v8 dependencies. The gate reported:
  - runtime policy and memory semantics: `11 passed`
  - Bun/JSC pool scaffold contract: `10 passed`
  - Convex runtime lane registry contract: `15 passed`
  - runtime diagnostics API contract: `2 passed`
  - tenant admission profile: `1 passed`
  - operator UI runtime diagnostics contract: `2 files passed`, `5 tests`
  - final line: `Bun/JSC runtime contract gate: pass`
- `make verify-bun-jsc-adapter-package` passed:
  `verified: Bun/JSC adapter package helper accepts a good fixture ... and native leaks`.
- `make verify-bun-jsc-release-assets` passed:
  `verified: Bun/JSC release asset helper accepts absent-optional and good assets ... and tampered adapter packages`.
- The clean release worktree remained clean after these gates:
  `git status --short --branch` reported only
  `## codex/final-release-v0.1.32`.

### NRR3 - Version Bump and Release Metadata

Status: completed 2026-05-25

Prepare the release version consistently across Rust, JS, lockfiles, and
changelog.

Success criteria:

- Workspace package version and all non-UI publishable package versions are
  updated to the chosen version.
- Local package dependency pins and `package-lock.json` workspace entries match
  the chosen version.
- `CHANGELOG.md` has a heading for the chosen version.
- `make verify-release-version-contract VERSION=<tag>` passes.
- The version commit contains only intended release metadata and any required
  root-cause fixes from earlier gates.

Evidence:

- Bumped the Rust workspace package version, Cargo.lock workspace package
  versions, JS workspace package versions, local JS dependency pins, and
  package-lock workspace entries from `0.1.31` to `0.1.32`.
- Added an explicit private `packages/nimbus-ui` version so every
  `packages/*` workspace participates in the release-version contract
  instead of relying on a special-case verifier skip.
- Updated `CHANGELOG.md` with the `0.1.32` release heading and release
  summary for fork standardization, Bun/JSC optional runtime posture, tenant
  isolation, enterprise policy/egress, artifact provenance, desktop UI, CI, and
  release-helper work.
- `cargo update -w --offline` updated the 10 Nimbus workspace packages in
  `Cargo.lock` from `0.1.31` to `0.1.32`.
- `npm install --package-lock-only --ignore-scripts --offline` reported
  `up to date`, audited 525 packages, and found 0 vulnerabilities; unrelated
  npm lockfile metadata normalization was not retained in the release diff.
- `make verify-release-version-contract VERSION=v0.1.32` passed with
  `verified: release version contract matches v0.1.32`.
- `git diff --check` passed.

### NRR3A - Machine OS, Desktop, and UI Alignment

Status: completed 2026-05-26

Verify release-coupled downstream repositories and UI surfaces before starting
the final full Nimbus verification sweep.

Success criteria:

- `nimbus/machine-os` is clean and current against `origin/main` before any
  machine-os source edits are made.
- The supported Fedora bootc base is verified from an authoritative Fedora or
  registry source, and the pinned digest is recorded.
- The `nimbus/machine-os` recipe, machine-os hosted CI image pulls/cache, and
  `nimbus/nimbus` release workflow all reference the same pinned Fedora bootc
  base image.
- Any stale machine-os base digest that would break the release workflow is
  reproduced or classified with concrete evidence.
- `nimbus/machine-os` local gates pass:
  - `git diff --check`
  - `bash -n` for the machine-os shell scripts
  - `bash scripts/verify-recipe.sh`
  - `bash scripts/verify-build-helper.sh`
  - `bash scripts/verify-oci-layout-helper.sh`
  - `bash scripts/verify-provider-artifact-contracts.sh`
  - `bash scripts/verify-publish-helper.sh`
  - `bash scripts/verify-selinux-avc-gate.sh`
  - `actionlint .github/workflows/ci.yml .github/workflows/publish.yml`
- If machine-os source changes are needed, they are committed and pushed to
  `nimbus/machine-os` `main` before the Nimbus release tag is created, because
  the Nimbus release workflow consumes `MACHINE_OS_SOURCE_REF=main`.
- Nimbus' machine-os release-reference helper gates pass from the release
  worktree:
  - `bash scripts/verify-machine-os-release-ref-contract.sh --machine-os-repo <path>`
  - `bash scripts/verify-machine-os-release-ref-contract-helper.sh`
  - `bash scripts/verify-machine-os-release-default-gate-helper.sh`
  - `bash scripts/verify-bootc-default-promotion-gate-helper.sh`
- `packages/nimbus-ui` is version-aligned with the release metadata and covered
  by the JS typecheck/test/build gates.
- `nimbus/desktop` is clean/current, its relationship to this release is
  documented, and any hosted desktop failure is either fixed or classified as
  non-blocking for the Nimbus CLI/server binary release with exact evidence.
- Local Nimbus repos under `~/src/github.com/nimbus/*` are checked for dirty or
  unmerged work that should have entered `nimbus/nimbus` before release; any
  relevant finding is fixed or explicitly recorded as unrelated.

Evidence:

- External release context: Fedora Magazine announced Fedora Linux 44 on
  2026-04-28
  (`https://fedoramagazine.org/announcing-fedora-linux-44/`), and the
  Fedora/CentOS bootc docs identify
  `quay.io/fedora/fedora-bootc:<release>` as the Fedora bootc base image
  family (`https://fedora.gitlab.io/bootc/docs/bootc/base-images/`). The
  release uses the Fedora 44 bootc tag rather than `latest`.
- Before source edits, `nimbus/machine-os` reported clean
  `## main...origin/main`.
- `podman manifest inspect quay.io/fedora/fedora-bootc:44` returned the live
  multi-arch manifest list; the `linux/arm64` digest is
  `sha256:3ca807c0d2836ca425031a52dfe7fda69ca55a22c54fa78c068a22f43d6489b6`.
- `podman manifest inspect
  quay.io/fedora/fedora-bootc@sha256:3ca807c0d2836ca425031a52dfe7fda69ca55a22c54fa78c068a22f43d6489b6`
  returned the underlying manifest payload in a `podman` single-image parse
  error; its annotations included `org.opencontainers.image.version:
  44.20260525.0` and `ostree.linux: 7.0.9-205.fc44.aarch64`.
- The previously pinned machine-os recipe digest
  `sha256:5f2aa40538a71e32eba8dcdf9059dda10600bac68acef4588cb1aecedcfc6fe2`
  and CI digest
  `sha256:187d480948fe37a4cc55211b8a594adfc4f85a7d17ac1991331bf98272eb8f94`
  both failed with `manifest unknown`; keeping either pin would block the
  machine-os release lane.
- Updated `nimbus/machine-os` local recipe files and CI cache image pulls to
  the live Fedora 44 arm64 digest above.
- Updated `nimbus/nimbus` `.github/workflows/release.yml`
  `MACHINE_OS_FEDORA_BOOTC_IMAGE` to the same live Fedora 44 arm64 digest.
- `nimbus/machine-os` local gates passed after the digest refresh:
  - `git diff --check`
  - `bash -n scripts/build.sh scripts/check-selinux-avcs.sh scripts/package-oci.sh scripts/publish.sh scripts/write-sbom.sh scripts/verify-recipe.sh scripts/verify-build-helper.sh scripts/verify-oci-layout-helper.sh scripts/verify-provider-artifact-contracts.sh scripts/verify-publish-helper.sh scripts/verify-selinux-avc-gate.sh`
  - `bash scripts/verify-recipe.sh`
  - `bash scripts/verify-build-helper.sh`
  - `bash scripts/verify-oci-layout-helper.sh`
  - `bash scripts/verify-provider-artifact-contracts.sh`
  - `bash scripts/verify-publish-helper.sh`
  - `bash scripts/verify-selinux-avc-gate.sh`
  - `actionlint .github/workflows/ci.yml .github/workflows/publish.yml`
- Committed and pushed `nimbus/machine-os` main commit
  `68dd822b0290869e0af4794a19d869d9d2d8caba`
  (`Refresh Fedora bootc base digest`).
- Hosted `nimbus/machine-os` CI succeeded on commit
  `68dd822b0290869e0af4794a19d869d9d2d8caba`:
  `https://github.com/nimbus/machine-os/actions/runs/26423279945`.
- Nimbus release-worktree machine-os gates passed after the release workflow
  digest refresh:
  - `bash scripts/verify-machine-os-release-ref-contract.sh --machine-os-repo /Users/jack/src/github.com/nimbus/machine-os`
  - `bash scripts/verify-machine-os-release-ref-contract-helper.sh`
  - `bash scripts/verify-machine-os-release-default-gate-helper.sh`
  - `bash scripts/verify-bootc-default-promotion-gate-helper.sh`
- `packages/nimbus-ui/package.json` is versioned `0.1.32`, and
  `package-lock.json` contains the matching `packages/nimbus-ui` workspace
  entry at `0.1.32`; JS typecheck/test/build coverage remains part of NRR4.
- `nimbus/desktop` was clean on `main...origin/main` before alignment work.
  Hosted desktop `package` and `e2e` had been failing on commit
  `45fcea07fae3108f485e238655dcf526d2c377ad` because Windows runners executed
  `src/main/upgrade/runner.spec.ts` and the injected Darwin platform probe used
  host-Windows path joining. Fixed the root cause by making
  `findOnSanitizedPath` join paths with `path.posix` for Darwin/Linux and
  `path.win32` for Windows, committed and pushed
  `1ac13a90a6432656c7d8bece49d5aaf2348a397a`
  (`Fix upgrade runner path probing on Windows CI`).
- `nimbus/desktop` local gates passed after the fix:
  - `npm run lint`
  - `npm run typecheck`
  - `npm run test` (`17` files passed, `184` tests passed)
  - `npm run build:main`
- Hosted `nimbus/desktop` CI succeeded on commit
  `1ac13a90a6432656c7d8bece49d5aaf2348a397a`:
  `https://github.com/nimbus/desktop/actions/runs/26423345140`.
- The next `nimbus/desktop` commit
  `fc7b2ec8dc1f30928c061e8cd41e18b6742988ed` updated the e2e auth-page
  assertions. Hosted desktop runs on that commit all succeeded:
  - `ci`: `https://github.com/nimbus/desktop/actions/runs/26425345103`
  - `package`: `https://github.com/nimbus/desktop/actions/runs/26425345105`
  - `e2e`: `https://github.com/nimbus/desktop/actions/runs/26425345124`
- Local repo audit:
  - `nimbus/deno`, `nimbus/rusty_v8`, `nimbus/nimbus-crun`,
    `nimbus/nimbus-libkrun`, `nimbus/machine-os`, and `nimbus/desktop` are
    clean on their release/default branches after the alignment pushes.
  - The primary `nimbus/nimbus` checkout remains intentionally dirty with
    unrelated Convex generated files, `package-lock.json`, an untracked
    node-compat plan, and desktop-auth screenshots; release work continues from
    the clean release worktree to avoid including that state.
  - `nimbus/homebrew-tap` has an untracked `nix-packages/` directory; it is not
    consumed by the `v0.1.32` source release gate.
  - `nimbus/claude-skill-convex` and `nimbus/codex-plugin-convex` are local
    uncommitted skill/plugin scaffolds, not Nimbus binary release inputs.

### NRR4 - Full Nimbus Verification

Status: completed 2026-05-26

Run the product gates that must be green before any final binary release.

Success criteria:

- `cargo fmt --all --check` passes.
- `make check` passes.
- `make clippy` passes.
- `make test` passes.
- `make deny` passes.
- `npm run typecheck` passes.
- `npm run test` passes.
- `npm run build` passes.
- `make verify-harness` passes.
- `make verify-tenant-isolation-conformance` passes.
- `make verify-enterprise-policy-egress` passes.
- `make verify-artifact-provenance` passes.
- `make proof-helpers` passes.
- Any available strict docs/reference validation is run; if no such command is
  available, record that explicitly.

Evidence so far:

- `cargo fmt --all --check` passed after the Deno repin and node-compat
  fixture-source cleanup.
- `make check` passed against the repinned `nimbus/deno`
  `v2.8.0-nimbus.4` source tag.
- `make clippy` passed against the repinned `nimbus/deno`
  `v2.8.0-nimbus.4` source tag.
- Focused repin proof passed:
  - `cargo test -p nimbus-runtime --lib node22_process_finalization -- --test-threads=1`:
    `3 passed; 0 failed; 1 ignored; 531 filtered out`
  - `cargo test -p nimbus-runtime --lib node_compat_supplementary_module_bridge -- --test-threads=1`:
    `3 passed; 0 failed; 0 ignored; 532 filtered out`
  - `cargo test -p nimbus-runtime --lib node22_loader_context_followup_vm -- --test-threads=1`:
    `9 passed; 0 failed; 5 ignored; 521 filtered out`
  - `cargo test -p nimbus-runtime --lib node22_loader_context_zlib_foundation_batch_fixture -- --test-threads=1`:
    `1 passed; 0 failed; 0 ignored; 534 filtered out`
- First full `make test` attempt failed in
  `node22-node-tools-sqlite-foundation-batch` because Deno's
  `deno_node_sqlite` called `panic!("Failed to set db config")` from an
  `op2` constructor when `sqlite3_db_config` returned an error. That aborted
  the host process (`SIGABRT`) instead of returning a JavaScript `SqliteError`,
  so it was treated as a release blocker and fixed in the Deno fork.
- Deno fork verification for the sqlite fix:
  - `cargo fmt --all --check` passed in `/Users/jack/src/github.com/nimbus/deno`
  - `env CARGO_ENCODED_RUSTFLAGS=... cargo check -p deno_node_sqlite` passed
    with the macOS linker override documented for one-off fork verification
  - `cargo test -p nimbus-runtime --lib node22_node_tools_sqlite_foundation_batch_fixture -- --test-threads=1`
    passed against temporary local Deno commit
    `9225357ba8697cf2c998eef62571779957a7a90c`
  - the same focused Nimbus test passed after publishing and repinning to
    `nimbus/deno` `v2.8.0-nimbus.4`:
    `1 passed; 0 failed; 0 ignored; 534 filtered out`
- A pre-repin Node22 release slice passed after host-process snapshot hardening
  and broad replay classification:
  `cargo test -p nimbus-runtime --lib node22_ -- --test-threads=1`:
  `125 passed; 0 failed; 89 ignored; 321 filtered out`.
- The supplementary module bridge fixture source was moved out of an ignored
  `node_modules` source path while still staging into `node_modules` at runtime;
  this prevents a clean checkout from depending on ignored local fixture files.
- Full `make test` passed after the `v2.8.0-nimbus.4` repin and default-lane
  node-compat hardening. This is now pre-security-repin evidence; NRR4 must
  rerun after the `v2.8.0-nimbus.5` security dependency repin. Key
  release-blocking summaries from the run:
  - `nimbus-runtime`: `359 passed; 0 failed; 178 ignored; finished in 1993.06s`
  - `nimbus-sandbox`: `157 passed; 0 failed`
  - `nimbus-server`: `841 passed; 0 failed; 9 ignored`
  - `nimbus-storage`: `206 passed; 0 failed; 2 ignored`
  - workspace doc tests completed with `0 failed`
- The HTTPS TLS session lane passed under the full workspace run through the
  subprocess wrapper:
  `node22_networking_https_tls_session_batch_fixture ... ok`; the child proof
  remained ignored as intended.
- The `node:sqlite` process-global lane is no longer part of the default
  workspace gate. The exact proof lane remains available and passed with:
  `cargo test -p nimbus-runtime --lib runtime::tests::node_compat::node22_node_tools_sqlite_foundation_batch_fixture -- --ignored --exact --nocapture`
  -> `1 passed; 0 failed; 0 ignored; 536 filtered out`.
- Deno security dependency repin evidence:
  - `nimbus/deno` `v2.8.0-nimbus.5` updates Hickory to `0.26.1` and
    `rustls-webpki` to `0.103.13`, removing the active `cargo-deny`
    advisories for `hickory-proto 0.25.2` and `rustls-webpki 0.102.8`.
  - `make deny` passed on the repinned Nimbus graph:
    `advisories ok, bans ok, licenses ok, sources ok`.
- Final `v2.8.0-nimbus.5` verification passed:
  - `cargo fmt --all --check`
  - `make check`: `cargo check --workspace` finished successfully.
  - `make clippy`: `cargo clippy --workspace --all-targets -- -D warnings`
    finished successfully.
  - `make test`: full workspace run passed. Key summaries:
    `nimbus-runtime` `359 passed; 0 failed; 178 ignored`;
    `nimbus-sandbox` `157 passed; 0 failed`;
    `nimbus-server` `841 passed; 0 failed; 9 ignored`;
    `nimbus-storage` `206 passed; 0 failed; 2 ignored`; workspace doctests
    completed with `0 failed`.
  - `npm run typecheck` passed for all workspaces. `nimbus-ui@0.1.32`
    codegen emitted the existing TanStack route-helper warnings, then
    `tsc --noEmit` passed.
  - `npm run test` passed, including `nimbus-ui` `42` test files and `278`
    tests.
  - `npm run build` passed, including the embedded `nimbus-ui@0.1.32`
    production build. Existing route-helper, Node `module.register()`, and
    Vite chunk-size warnings were emitted.
  - The host ran out of disk during the first `make verify-harness` retry
    while compiling server/runtime artifacts. This was not a harness assertion
    failure. Cleaning only the main checkout's Cargo build artifacts with
    `cargo clean` freed `156.0GiB`; the release worktree target cache was
    preserved.
  - `make verify-harness` then passed: storage and engine generated-history
    corpus checks passed; server generated-history and transport-liveness
    campaigns passed; runtime liveness/integrity cases passed.
  - `make verify-tenant-isolation-conformance` passed: tenant isolation
    conformance reported `21 scenarios, 12 allowed, 9 denied`, and production
    image admission reported `6 passed; 0 failed`.
  - `make verify-enterprise-policy-egress` passed all `8` sections covering
    policy fixtures, CLI fixtures, compose egress lowering, service-manager
    materialization, sandbox enforcement, egress proxy enforcement, audit
    redaction/export, and drift scanning.
  - `make verify-artifact-provenance` passed all `5` sections covering
    Cosign/SLSA/SBOM verifier adapters, runtime bundle provenance admission,
    tenant image admission with canonical OCI parsing, the operator SBOM policy
    hook, and production Compose image admission.
  - `make proof-helpers` passed, including SQLCipher, machine
    guest/service/Homebrew helpers, Bun/JSC adapter package and release asset
    helpers, Linux release package helper, and the install helper
    (`35` install-script checks).
  - `make verify-bun-jsc-runtime-contract` passed all `7` sections, including
    runtime policy/memory semantics, Bun/JSC pool scaffold, Convex lane
    registry, diagnostics API, tenant admission, and UI diagnostics.
  - `bash scripts/verify-bun-jsc-in-process-lockdown.sh` passed locally
    against `/Users/jack/src/github.com/oven-sh/bun`. The native embed probe
    reported deny-by-default permission hooks, deny-all resolver policy, memory
    pressure/shrink evidence, and the product-first policy
    `fresh_vm_or_discard_with_outer_quota_required`.
  - `make verify-bun-jsc-adapter-package` passed.
  - `make verify-bun-jsc-release-assets` passed.
  - `git diff --check` passed after the current changes.
- Strict docs reference validation was attempted with
  `npm run docs:validate-refs:strict`, but the repo does not define that npm
  script. `npm run` lists build/typecheck/test and adapter demo scripts only,
  so the unavailable docs-ref lane is recorded explicitly rather than claimed.

### NRR5 - Local Release Binary and Archive Contract

Status: completed 2026-05-26

Build the local release binary and verify the release helper contracts before
creating the tag.

Success criteria:

- `make release` passes.
- `target/release/nimbus --version` reports the chosen version.
- `make verify-release-archive-layout-helper` passes.
- `make verify-install-helper` passes.
- `make verify-build-linux-release-packages-helper` passes.
- `make verify-build-fedora-release-srpms-helper` passes.
- If optional Bun/JSC adapter release artifacts are produced locally, they pass
  `scripts/verify-bun-jsc-release-assets.sh`; if absent, the verifier records
  absence as policy.

Evidence:

- `make release` passed from the clean release worktree:
  `Finished 'release' profile [optimized] target(s) in 12m 53s`.
- `target/release/nimbus --version` reported `nimbus 0.1.32`.
- `make verify-release-archive-layout-helper` passed:
  `verified: release archive layout helper accepts the shipped layout and rejects a broken macOS helper bundle`.
- `make verify-install-helper` passed `35` install-script checks, including
  checksum spoof rejection and optional Bun/JSC adapter installer hardening.
- `make verify-build-linux-release-packages-helper` passed:
  deterministic `nimbus`, `nimbus-libkrun`, `nimbus-crun`, and
  `nimbus-bun-jsc-adapter` deb/rpm manifests rendered; actual package creation
  was skipped because `nfpm` is not installed on the host.
- `make verify-build-fedora-release-srpms-helper` initially failed because the
  sandbox could not access Docker/Podman sockets. `docker --version` and
  `podman --version` confirmed the tools exist, and rerunning the helper with
  container access passed under Fedora 42:
  reusable `nimbus`, `nimbus-libkrun`, and `nimbus-crun` source RPMs were
  built, x86_64 RPMs were installed, and aarch64 RPM metadata was
  query-verified from the release artifacts.
- Optional Bun/JSC release-asset policy was already verified in NRR4 with
  `make verify-bun-jsc-release-assets`, which passed for absent-optional,
  good-asset, missing-SBOM, missing-SLSA, checksum-mismatch, and tampered
  package cases.

### NRR6 - Clean Release Commit and Tag

Status: completed 2026-05-26

Create the final release commit and tag only after all local gates above pass.

Success criteria:

- `git status --short` is empty in the release worktree before tagging.
- The release commit is on `main` or the explicitly documented release branch.
- The annotated tag `vX.Y.Z` points to the verified release commit.
- The tag is pushed only after final local verification has passed.
- No unrelated dirty files or untracked artifacts are included in the release
  commit or tag.

Evidence:

- The vetted release changes were committed as
  `41c857cb6a3f9a4f30bfeeb6f11622294ba58543`
  (`Prepare Nimbus v0.1.32 release`).
- Before tagging, `git status --short --branch` reported only
  `## codex/final-release-v0.1.32`, and `git diff --check HEAD` passed.
- `git merge-base --is-ancestor origin/main HEAD` passed, proving the release
  commit included the latest fetched `origin/main` CI/release workflow work.
- `git tag --list v0.1.32` returned no local tag before creation, and
  `git ls-remote --tags origin v0.1.32` returned no remote tag.
- The annotated tag `v0.1.32` was created locally and
  `git rev-parse HEAD v0.1.32^{commit}` showed both resolve to
  `41c857cb6a3f9a4f30bfeeb6f11622294ba58543`.
- `git push origin HEAD:main v0.1.32` advanced remote `main` from
  `2d2e7b71` to `41c857cb` and created remote tag `v0.1.32`.

### NRR7 - Hosted Release Workflow and Artifacts

Status: in progress

Let the tag-driven GitHub workflow produce the cross-platform release, then
verify the live artifacts.

Success criteria:

- `.github/workflows/release.yml` completes successfully for the release tag.
- The machine-os dispatch/publish gate succeeds or the release is blocked with
  exact evidence.
- The GitHub release exists for the tag and is not a draft.
- Expected core assets exist:
  - `nimbus_darwin_arm64.tar.gz`
  - `nimbus_linux_x86_64.tar.gz`
  - `nimbus_linux_arm64.tar.gz`
  - `nimbus_windows_x86_64.zip`
  - `install.sh`
  - `checksums-sha256.txt`
- Downloaded release artifacts pass:
  - `scripts/verify-release-archive-layout.sh --artifacts-dir <downloaded>`
  - `scripts/verify-bun-jsc-release-assets.sh --artifacts-dir <downloaded> --checksums <downloaded>/checksums-sha256.txt`
  - checksum verification for every listed asset
- Release notes mention the fork stack consumed by this release, including
  Deno/rusty_v8, Bun/JSC adapter posture, nimbus-crun, and nimbus-libkrun.

Evidence:

- `gh run list --repo nimbus/nimbus --workflow release.yml --limit 5`
  reported tag-triggered run `26448883465` for `v0.1.32` in progress, started
  `2026-05-26T12:47:21Z`.
- The first attempt failed before product compilation in several lanes with
  GitHub-hosted checkout/action-download errors that included
  `Your account is suspended` and codeload archive failures. The exact action
  archive URLs were reachable immediately afterward, so
  `gh run rerun --repo nimbus/nimbus 26448883465 --failed` was used to rerun
  the failed jobs from the same tag.
- The rerun passed checkout and Rust setup, then exposed a Nimbus-owned release
  workflow bug: `Build (aarch64-unknown-linux-gnu)` failed in
  `cargo build --release -p nimbus-bin` because `nimbus-server/build.rs`
  requires `packages/nimbus-ui/dist/index.html`, while `release.yml` was
  invoking Cargo directly instead of satisfying the Makefile UI prerequisite.
- The release workflow was fixed to mirror CI's canonical shape: add a
  `ui-artifacts` leader job that runs `npm ci` and `make build-ui`, then make
  each release binary build lane depend on and download those artifacts before
  running Cargo. This preserves one UI build per release run rather than
  duplicating UI compilation across platform lanes.
- Local verification after the workflow fix:
  `bash scripts/verify-machine-os-release-ref-contract.sh` passed;
  `bash scripts/verify-machine-os-release-ref-contract-helper.sh` passed;
  `git diff --check` passed. `bash scripts/verify-ci-modernization.sh`
  passed `11/12` local structural checks and failed only the live "latest CI
  run on main" query, which is not a release workflow syntax or pinning check.

### NRR8 - Post-Release Consumer Proof

Status: pending

Prove the released binary is installable and still reports the intended
runtime/sandbox posture.

Success criteria:

- `scripts/install.sh --dry-run` or the supported dry-run path resolves the
  new release.
- A downloaded Linux or macOS release binary reports the chosen version.
- Runtime diagnostics still show Deno/V8/Node lanes and Bun/JSC as an optional
  adapter-backed lane, with the default binary preserving no-link behavior.
- Packaging helper outputs reference the new Nimbus version and the
  standardized `nimbus-crun`/`nimbus-libkrun` versions.
- The plan is updated with final evidence, commit SHA, tag, release URL, and
  any known residual risks.

## Completion Gate

The release is complete only when:

- all NRR0-NRR8 items are marked complete with evidence;
- every failure discovered during fork or Nimbus verification is either fixed
  or documented as upstream-owned and non-blocking for Nimbus' consumed
  capability;
- machine-os, desktop, and UI alignment has been verified or any separate
  artifact-cadence failure has been classified with concrete non-blocking
  evidence;
- the release source worktree is clean;
- the live GitHub release artifacts pass downloaded verification; and
- the active goal is marked complete with the release URL and final token
  usage.

## Goal Prompt

```text
/goal Complete docs/plans/final-nimbus-release-readiness-plan.md autonomously.

Verifiable success criteria:
- Execute NRR0-NRR8 in order, updating each item status and evidence in the
  plan as work completes.
- Use a clean release worktree if the primary checkout contains unrelated dirty
  work; never include unrelated generated files, screenshots, or untracked
  plans in the release commit.
- Verify the live fork stack and fix any Nimbus-owned fork failures before
  continuing. If a fork failure is upstream-owned, prove that with a matching
  upstream-base reproduction or hosted evidence and record why it is
  non-blocking or block the release.
- Prove Nimbus consumes the standardized fork releases:
  nimbus/deno v2.8.0-nimbus.5, nimbus/rusty_v8 v149.0.0-nimbus.1,
  nimbus/bun source tag nimbus-bun-jsc-proof-main-20260525,
  nimbus/nimbus-crun v1.27.1-nimbus.2, and
  nimbus/nimbus-libkrun v1.18.1-nimbus.1.
- Verify release-coupled downstream alignment before final gates: nimbus/machine-os
  must use the current supported pinned Fedora bootc base consistently across
  its recipe, CI workflow, helper tests, and the nimbus/nimbus release workflow;
  changed machine-os source must pass local shell/helper/actionlint gates and
  be committed/pushed to main before the Nimbus release tag is created.
- Verify packages/nimbus-ui version/test/build alignment and classify
  nimbus/desktop hosted status. Fix any release-blocking desktop/UI issue, or
  record exact evidence when a separate desktop artifact failure is non-blocking
  for the Nimbus CLI/server release.
- Select a new Nimbus release version, update Rust/JS/package-lock/changelog
  metadata, and pass make verify-release-version-contract VERSION=<tag>.
- Pass the full local gate set: cargo fmt --all --check, make check,
  make clippy, make test, make deny, npm run typecheck, npm run test,
  npm run build, make verify-harness, make verify-tenant-isolation-conformance,
  make verify-enterprise-policy-egress, make verify-artifact-provenance,
  make proof-helpers, make release, make verify-release-archive-layout-helper,
  make verify-install-helper, make verify-build-linux-release-packages-helper,
  and make verify-build-fedora-release-srpms-helper.
- Create and push the release tag only after the release worktree is clean and
  the local gates pass.
- Monitor the tag-driven GitHub release workflow to success, then download and
  verify the published release artifacts with the archive-layout, Bun/JSC
  adapter, and checksum verifiers.
- Update this plan with final evidence, release tag, release URL, source SHA,
  verification commands, and any residual risks before marking the goal
  complete.
```
