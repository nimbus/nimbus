# Final Nimbus Release Readiness Plan

Status: in progress
Owner: Nimbus release work
Created: 2026-05-25

## Purpose

This plan ties the recently standardized Nimbus fork releases back into the
`nimbus/nimbus` product release. A final Nimbus binary should only be released
after the source fork stack is healthy, Nimbus consumes the intended released
fork tags, the release source tree is clean, full Nimbus verification passes,
and the published GitHub release artifacts can be downloaded and re-verified.

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
  - `nimbus/deno` `v2.8.0-nimbus.2`
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
- Hosted fork Actions status:
  - `nimbus/rusty_v8` active-branch CI succeeded on
    `9b77553883f1117ab3df62709b8673b803ed721b`.
  - `nimbus/bun` active-branch graph update succeeded on
    `ad0e1d2bbc6690651e04f10eaf1dcdf8a6c0de57`.
  - `nimbus/deno`, `nimbus/nimbus-crun`, and `nimbus/nimbus-libkrun` have no
    active-branch hosted runs to treat as release blockers.
- Investigated the latest `nimbus/deno` hosted run on release ref
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

Status: completed 2026-05-25

Prove the Nimbus repo pins and installer/package surfaces consume the intended
Nimbus fork releases.

Success criteria:

- `Cargo.toml` and `Cargo.lock` point to `nimbus/deno` `v2.8.0-nimbus.2` and
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
  confirmed active pins:
  - `Cargo.toml` patch entries use `nimbus/deno` `v2.8.0-nimbus.2` and
    `nimbus/rusty_v8` `v149.0.0-nimbus.1`.
  - `Cargo.lock` resolves Deno-family crates to
    `363de88e0dd6cd87c60704bc8e373dea202817e4` and rusty_v8 to
    `9b77553883f1117ab3df62709b8673b803ed721b`.
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

Status: pending

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

### NRR4 - Full Nimbus Verification

Status: pending

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
- `npm run docs:validate-refs:strict` passes.

### NRR5 - Local Release Binary and Archive Contract

Status: pending

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

### NRR6 - Clean Release Commit and Tag

Status: pending

Create the final release commit and tag only after all local gates above pass.

Success criteria:

- `git status --short` is empty in the release worktree before tagging.
- The release commit is on `main` or the explicitly documented release branch.
- The annotated tag `vX.Y.Z` points to the verified release commit.
- The tag is pushed only after final local verification has passed.
- No unrelated dirty files or untracked artifacts are included in the release
  commit or tag.

### NRR7 - Hosted Release Workflow and Artifacts

Status: pending

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
  nimbus/deno v2.8.0-nimbus.2, nimbus/rusty_v8 v149.0.0-nimbus.1,
  nimbus/bun source tag nimbus-bun-jsc-proof-main-20260525,
  nimbus/nimbus-crun v1.27.1-nimbus.2, and
  nimbus/nimbus-libkrun v1.18.1-nimbus.1.
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
