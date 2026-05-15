# Plan: Machine-OS Repository Stewardship

Canonical execution plan for turning `nimbus/machine-os` from a
Podman-machine-os-derived fork shape into a first-party Nimbus bootc appliance
repository with a small, readable project structure and a release pipeline that
is clear enough to earn enterprise trust.

This plan does not change the macOS default image by itself. The bootc default
was already promoted by `docs/plans/bootc-machine-default-plan.md`. This plan
cleans up ownership, branch hygiene, structure, docs, verification, and
pipeline boundaries now that the direct bootc architecture is the real path.

---

## Status

- **Status:** `archived/done`
- **Primary owner:** this plan
- **Implementation repository:** `nimbus/machine-os`
- **Cross-repo release owner:** `nimbus/nimbus` release workflow plus
  `nimbus/machine-os` publish workflow
- **Baseline date:** 2026-05-14

## Inputs

- `docs/plans/bootc-machine-default-plan.md`
- `docs/plans/research/bootc-machine-architecture-for-nimbus.md`
- `docs/architecture/sandbox/macos-machine-flow.md`
- `/Users/jack/src/github.com/nimbus/machine-os`
- `/Users/jack/src/github.com/containers/podman-machine-os`
- `/Users/jack/src/github.com/containers/podman`

## Current Finding

`nimbus/machine-os` has diverged far enough from
`containers/podman-machine-os` that the repo should no longer be organized as
a downstream copy of Podman's FCOS image factory.

Podman's repo remains useful as a compatibility reference for the AppleHV disk
artifact contract and Podman-machine behavior, but the implementation shape is
different:

- Podman builds a customized Fedora CoreOS/WSL machine image family with
  CoreOS/COSA imports, FCOS conversion details, and Podman-machine validation.
- Nimbus builds a direct Fedora bootc appliance from a digest-pinned
  `fedora-bootc:44` base and a digest-pinned `bootc-image-builder`.
- Nimbus bakes the matching Linux arm64 `nimbus` release binary into the guest
  image and treats that binary as image content, not a host-side scp repair.
- Nimbus owns bootc-native machine config, baked systemd units, SELinux policy,
  SBOM/checksum/provenance assets, and `ghcr.io/nimbus/machine-os:<tag>`.
- The canonical publish context is now the `nimbus/machine-os` repository so
  GHCR, GitHub Releases, and attestations are attached to the repository that
  owns the machine image.

Windows support does **not** make the current macOS artifact reusable as-is.
The reviewed Windows plan (`docs/plans/windows-machine-support-plan.md`) and
Podman source agree on provider-specific artifact shapes:

- macOS AppleHV/LibKrun consumes a raw disk selected from OCI with
  `disktype=applehv`.
- Windows WSL2 consumes a Tar rootfs imported with `wsl --import` and then
  configured with shell bootstrap, not Ignition and not a raw AppleHV disk.
- Future Windows Hyper-V would consume a VHDX-style artifact and remains
  deferred behind WSL2.

That means future Windows support should add provider-specific artifact
families, not pretend one disk image works everywhere. It does **not** require
renaming `image/` back to `images/`: `image/` is the current production bootc
appliance recipe, while provider artifacts are release outputs selected by
provider metadata. Only introduce additional recipe directories if WSL2 or
Hyper-V truly need different guest content, not merely a different output
format.

The current `nimbus/machine-os` branch state also still carries migration
clutter:

- `main` is the canonical long-lived branch.
- Stale local or remote migration branches include
  `release-workflow-v1`, `codex/bootc-machine-default`, and
  `codex/release-workflow-v1-bootc`.
- Existing public tags should be audited before deletion. New immutable
  release records should use the paired Nimbus release tags and GitHub
  Releases.

## Enterprise Trust Principles

The cleaned-up repo should make the right path obvious:

- **First-party appliance:** `machine-os` is a Nimbus-owned bootc guest image,
  not a generic Fedora image, not a Podman-machine-os clone, and not a place
  for host application code.
- **Exact release coupling:** a published machine image records the exact
  Nimbus version and machine-os source revision used to build it.
- **Immutable provenance:** public docs and release assets point to digest
  references, base image digests, builder image digests, SBOMs, checksums, and
  GitHub attestations.
- **Least privilege:** the cross-repo release app only mints tokens for the
  exact repository and permission boundary needed. GHCR publishing happens
  with the `nimbus/machine-os` workflow token.
- **Small branch model:** `main` is the only long-lived development branch.
  Release branches are introduced only for an intentional enterprise
  maintenance line.
- **Bootc-native by default:** no Ignition or FCOS compatibility structure is
  presented as the normal path.
- **Podman-compatible artifact, not Podman-shaped repo:** retain the
  `disktype=applehv` disk artifact semantics that Nimbus needs, while dropping
  inherited structure that only exists for Podman's FCOS build system.
- **Provider-explicit artifacts:** macOS AppleHV raw, future Windows WSL2 Tar,
  and future Hyper-V VHDX are different artifact contracts. The repo should
  document and verify each contract instead of implying that the current raw
  disk is universal.
- **Attribution without confusion:** preserve license and attribution for any
  inherited code or ideas, but make the README, package metadata, and GitHub
  source labels unambiguously point at `nimbus/machine-os`.

## Target Repository Shape

The target structure should be compact and purpose-owned:

```text
nimbus/machine-os
  README.md
  LICENSE
  SECURITY.md
  AGENTS.md
  docs/
    architecture.md
    artifact-contract.md
    local-build.md
    release-contract.md
    security-selinux.md
  image/
    Containerfile
    bootc-image-builder.toml
    build.sh
    README.md
  scripts/
    build.sh
    package-oci.sh
    publish.sh
    write-sbom.sh
    check-selinux-avcs.sh
    verify-*.sh
    lib/
      summary.sh
      oci.sh
      workflow.sh
  .github/
    workflows/
      ci.yml
      publish.yml
    dependabot.yml
```

The exact filenames can remain slightly different when that preserves working
release contracts, but the ownership boundaries should hold:

- `image/` owns the bootc recipe and guest image content.
- Provider-specific outputs should be expressed as artifact packaging/release
  targets from that recipe where possible: `applehv` raw now, `wsl` Tar later,
  and `hyperv` VHDX only if the deferred Hyper-V provider is promoted.
- `scripts/build.sh` is the stable local and CI entrypoint for producing a raw
  disk build output.
- `scripts/package-oci.sh` is the stable entrypoint for transforming that raw
  disk into the Podman-compatible OCI layout.
- `scripts/publish.sh` is the stable entrypoint for GHCR publish plus digest
  evidence.
- `docs/` explains the architecture and operating contract instead of burying
  those details in a long README.
- `.github/workflows/ci.yml` verifies structure and deterministic helper
  gates; `.github/workflows/publish.yml` is dispatch-only and externally
  visible.

## Build And Release Contract

The current two-phase release shape is the right model and should be kept:

1. `nimbus/nimbus` builds the Linux arm64 Nimbus binary.
2. `nimbus/nimbus` checks out `nimbus/machine-os` at `MACHINE_OS_SOURCE_REF`.
3. `nimbus/nimbus` stages a complete internal machine-os artifact as soon as
   Linux arm64 is ready.
4. No external machine-os release or GHCR publish happens until all Nimbus CLI
   release targets pass.
5. `nimbus/nimbus` dispatches `nimbus/machine-os/.github/workflows/publish.yml`.
6. `nimbus/machine-os` hydrates the staged bundle and publishes
   `ghcr.io/nimbus/machine-os:<tag>` from its own repository context.
7. `nimbus/machine-os` creates or updates the machine-os GitHub Release,
   uploads release evidence, and attests the release assets.
8. The final `nimbus/nimbus` release waits on the machine-os publish result.

This gives the machine image its own package ownership without losing the
important version coupling between the host release and the baked guest binary.

Standalone `nimbus/machine-os` tag or manual builds should stay validation
lanes unless the project intentionally creates an independent machine-os
maintenance stream.

## Non-Goals

- Do not flip the macOS default image in this plan.
- Do not reintroduce the abandoned FCOS-derived MOS3A build path.
- Do not add release branches unless there is a named maintenance policy.
- Do not delete public tags or releases without an explicit audit and a
  written reason.
- Do not remove attribution or license obligations inherited from the original
  Podman-machine-os fork history.

## Completion Evidence Matrix

| Phase | Required evidence |
|-------|-------------------|
| MOR0: Plan And Baseline | This plan exists, is registered as active, and records the Podman-vs-Nimbus divergence plus current branch/tag cleanup targets. |
| MOR1: Repo Identity | README and repository docs describe `nimbus/machine-os` as the canonical first-party bootc appliance; stale "future default" and fork-shaped language is gone; license/attribution posture is explicit. |
| MOR2: Branch And Tag Hygiene | `main` is the only long-lived branch locally and remotely; stale migration branches are deleted or documented as intentionally retained; release tags are audited without silently rewriting public history. |
| MOR3: Structure Cleanup | Bootc recipe, release scripts, verification helpers, and docs are organized by ownership; `proofs/direct-fedora-bootc` is promoted, archived, or removed so the production path is not labeled as a proof. |
| MOR4: Pipeline Hardening | Machine-os CI and publish workflows use current action inputs, minimal permissions, deterministic helper gates, and path filters matching the final directory shape; the Nimbus release verifier enforces the cross-repo contract. |
| MOR5: Security And Provenance | Docs and release assets identify base digest, builder digest, embedded Nimbus version/hash, SBOM, checksums, attestations, SELinux policy stance, and real-guest AVC gate expectations. |
| MOR6: Local Ergonomics | A developer can read one local-build doc, run help for every public script, and understand what can be verified on macOS versus a Linux arm64 builder. |
| MOR7: Closeout | Focused verification passes in both repos, docs/plans index is updated, and this plan is either marked done or left with only explicitly scoped follow-up items. |

## Phase Ledger

| Phase | Status | Gate |
|-------|--------|------|
| MOR0: Plan And Baseline | `done` | Plan file added and registered; Podman-vs-Nimbus divergence and branch cleanup candidates recorded. |
| MOR1: Repo Identity | `done` | `nimbus/machine-os` README now describes the current first-party bootc appliance; new architecture, artifact, release, security, local build, `SECURITY.md`, and `AGENTS.md` docs added. |
| MOR2: Branch And Tag Hygiene | `done` | Stale local and remote migration branches were pruned; only `main` and `origin/main` remain. Tags were audited and retained as historical release records. |
| MOR3: Structure Cleanup | `done` | Production recipe moved to `image/`, obsolete `proofs/direct-fedora-bootc/` was removed, and standalone validation workflow was renamed to `.github/workflows/ci.yml`. |
| MOR4: Pipeline Hardening | `done` | Machine-os CI path filters include docs, README, SECURITY, and AGENTS changes; standalone validation workflow is `.github/workflows/ci.yml`; Nimbus release cache paths and release-ref verifiers enforce `machine-os/image/` and reject legacy `machine-os/images/`; actionlint and release-contract helper checks pass. |
| MOR5: Security And Provenance | `done` | Security/provenance docs are explicit; latest `v0.1.31` release evidence was audited for required assets, base/builder digests, embedded Nimbus hash, source revision, SBOM/checksum/digest assets, and GitHub attestation behavior. |
| MOR6: Local Ergonomics | `done` | Local build docs cover Linux builder requirements, macOS-friendly checks, and common failures; public build/package/publish/SELinux/script help entrypoints were verified. |
| MOR7: Closeout | `done` | Focused verification passed in both repos, branch cleanup is complete, and plan status is reconciled. |

### Provider Artifact Follow-Up

The repository is structurally ready to add Windows artifacts without undoing
the cleanup. The first prep slice is now complete in `nimbus/machine-os`:

- `scripts/package-oci.sh` accepts a generic `--artifact` input while keeping
  `--raw-disk` as the current macOS-compatible alias.
- `scripts/verify-provider-artifact-contracts.sh` proves that AppleHV raw,
  WSL rootfs Tar, and deferred Hyper-V VHDX artifacts get distinct
  provider selectors and media types.
- `docs/provider-artifacts.md` records that WSL2 and Hyper-V artifacts are
  prepared contracts only, not supported release outputs.

- keep `image/` as the single current production recipe while the guest content
  is shared;
- extend packaging and release metadata when a Windows WSL2 provider is ready
  to consume a Tar rootfs;
- keep Hyper-V VHDX as a separate future artifact lane, not part of the first
  Windows support target;
- update the cross-repo release verifier only when the Windows artifact is
  promoted to a supported release output.

## Execution Detail

### MOR0: Plan And Baseline

Tasks:

- add this plan to `docs/plans/`
- register this plan as active in `docs/plans/README.md`
- record the confirmed divergence from `containers/podman-machine-os`
- record current branch cleanup candidates
- keep `docs/plans/bootc-machine-default-plan.md` as completed implementation
  baseline rather than resuming it

Verification:

- `git diff --check`
- plan index links to this file

### MOR1: Repo Identity

Tasks:

- update the root `nimbus/machine-os` README so it says the bootc image is the
  current Nimbus macOS machine OS path, not only a replacement candidate
- split long operating details from README into `docs/`
- add or update `SECURITY.md` with vulnerability reporting and SELinux gate
  expectations
- add a small `AGENTS.md` in `nimbus/machine-os` with durable repo-specific
  rules: bootc-native default, no FCOS compatibility rebuilds, preserve
  release evidence, and run deterministic helper checks before finishing
- document inherited Podman-machine-os attribution without making Podman the
  implied current architecture owner

Verification:

- README has no stale "future default" wording
- `rg -n "FCOS|Ignition|replacement path|future default|Podman" README.md docs`
  shows only intentional compatibility/history references

### MOR2: Branch And Tag Hygiene

Tasks:

- verify stale branches are merged or superseded:
  `release-workflow-v1`, `codex/bootc-machine-default`, and
  `codex/release-workflow-v1-bootc`
- delete stale local branches after verification
- delete stale remote branches when they do not own active work
- keep `origin/main` as the canonical branch and origin HEAD
- audit existing tags and releases before deciding whether any prelaunch tags
  should be deleted, retained, or marked superseded
- document that release branches are absent by design until a named
  maintenance policy exists

Verification:

- `git branch -a --sort=-committerdate`
- `git log --oneline --decorate --all --simplify-by-decoration --max-count=40`
- `gh release list --repo nimbus/machine-os`

### MOR3: Structure Cleanup

Tasks:

- rename `images/` to `image/` and update:
  - `nimbus/machine-os` workflows and scripts
  - `nimbus/nimbus` release workflow cache keys and path references
  - `nimbus/nimbus` release-contract verifiers
- remove `proofs/direct-fedora-bootc/` so the production path is not labeled
  as a proof
- move duplicated or long-form README content into `docs/`
- consider a `scripts/lib/` split only where it reduces repeated summary,
  OCI, or workflow parsing logic
- keep public shell entrypoints stable unless verifiers and docs are updated
  in the same change

Verification:

- `bash scripts/verify-recipe.sh`
- `bash scripts/verify-build-helper.sh`
- `bash scripts/verify-oci-layout-helper.sh`
- `bash scripts/verify-publish-helper.sh`
- `bash scripts/verify-selinux-avc-gate.sh`

### MOR4: Pipeline Hardening

Tasks:

- rename `build.yml` to `ci.yml` only if doing so improves clarity without
  breaking dispatch expectations
- keep `publish.yml` dispatch-only and externally visible
- ensure action inputs use current names such as `client-id`
- keep GHCR publishing in `nimbus/machine-os` with `packages: write` on that
  workflow only
- keep the release app scoped to cross-repo checkout, dispatch, and artifact
  download needs
- update Nimbus release verifiers to enforce final workflow names, job names,
  permissions, and publish ordering
- keep `build-machine-os` staged-only and `publish-machine-os` gated by all
  Nimbus release targets

Verification:

- in `nimbus/machine-os`: workflow syntax and helper checks
- in `nimbus/nimbus`:
  `bash scripts/verify-machine-os-release-ref-contract-helper.sh`
- in `nimbus/nimbus`:
  `bash scripts/verify-machine-os-release-ref-contract.sh --machine-os-repo /Users/jack/src/github.com/nimbus/machine-os`
- `actionlint .github/workflows/release.yml .github/workflows/ci.yml` when
  available, with the actual file list adjusted to final names

### MOR5: Security And Provenance

Tasks:

- document base image and builder image digest policy
- document the embedded Nimbus binary version/hash policy
- document the SELinux domain, socket label, bootupd compatibility overlay,
  and real guest AVC promotion gate
- document SBOM/checksum/attestation release evidence
- ensure OCI annotations and release assets still identify:
  - `org.opencontainers.image.source=https://github.com/nimbus/machine-os`
  - `org.opencontainers.image.revision=<machine-os source revision>`
  - `io.nimbus.machine.attestation.repository=nimbus/machine-os`
  - `io.nimbus.machine.nimbus.version=<embedded Nimbus tag>`
- ensure package ownership and source metadata point to `nimbus/machine-os`

Verification:

- `bash scripts/verify-oci-layout-helper.sh`
- `bash scripts/verify-publish-helper.sh`
- a release asset audit on the latest published tag

### MOR6: Local Ergonomics

Tasks:

- document local Linux arm64 build requirements and expected disk needs
- document what a macOS developer can verify without a Linux image build
- make all public scripts support `--help`
- keep deterministic helper tests runnable on macOS where possible
- add troubleshooting for rootful Podman, BIB disk space, GHCR auth, and
  SELinux AVC evidence

Verification:

- `bash scripts/build.sh --help`
- `bash scripts/package-oci.sh --help`
- `bash scripts/publish.sh --help`
- `bash scripts/check-selinux-avcs.sh --help`

### MOR7: Closeout

Tasks:

- run focused verification in `nimbus/machine-os`
- run cross-repo release-contract verification in `nimbus/nimbus`
- update this phase ledger with actual evidence
- move this plan from active execution to current reference or archive after
  completion

Verification:

- `git diff --check` in both repos
- focused shell helper checks
- actionlint when available
- no unexpected dirty work outside the planned files

## Execution Log

| Date | Entry |
|------|-------|
| 2026-05-14 | Plan opened after confirming `nimbus/machine-os` is now a direct Fedora bootc appliance repo, while `containers/podman-machine-os` remains FCOS/CoreOS/WSL-shaped. Cleanup should preserve the proven bootc release contract and remove fork-shaped project clutter. |
| 2026-05-14 | MOR0 and MOR1 started. Added first-party machine-os identity docs in `nimbus/machine-os`: README, `AGENTS.md`, `SECURITY.md`, `docs/architecture.md`, `docs/artifact-contract.md`, `docs/local-build.md`, `docs/release-contract.md`, and `docs/security-selinux.md`. Updated machine-os `build.yml` path filters so docs/security/readme changes run validation. Verification passed in `nimbus/machine-os`: `bash scripts/verify-recipe.sh`, `bash scripts/verify-build-helper.sh`, `bash scripts/verify-fedora-bootc-proof.sh`, `bash scripts/verify-oci-layout-helper.sh`, `bash scripts/verify-publish-helper.sh`, `bash scripts/verify-selinux-avc-gate.sh`, shell syntax checks, and `git diff --check`. |
| 2026-05-14 | MOR2 completed. `release-workflow-v1`, `codex/bootc-machine-default`, and `codex/release-workflow-v1-bootc` were confirmed as migration-era branches superseded by `main`; local stale branches were deleted, and remote `release-workflow-v1` plus `codex/bootc-machine-default` were deleted. `git branch -a --sort=-committerdate` now shows only `main`, `origin/main`, and `origin/HEAD`. `gh release list --repo nimbus/machine-os --limit 30` shows public GitHub Releases for `v0.1.28`, `v0.1.29`, `v0.1.30`, and `v0.1.31`; existing tags were retained as historical records. |
| 2026-05-14 | MOR3 completed and MOR4 advanced. In `nimbus/machine-os`, renamed `images/` to `image/`, removed the obsolete `proofs/direct-fedora-bootc/` lane and `scripts/verify-fedora-bootc-proof.sh`, and renamed `.github/workflows/build.yml` to `.github/workflows/ci.yml`. Updated machine-os docs, helper scripts, path filters, and cache keys to the new structure. In `nimbus/nimbus`, updated the release workflow cache key and release-ref verifier to require `machine-os/image/` and reject legacy `machine-os/images/` paths; updated the macOS machine-flow reference to `.github/workflows/ci.yml`. Verification passed in `nimbus/machine-os`: `bash scripts/verify-recipe.sh`, `bash scripts/verify-build-helper.sh`, `bash scripts/verify-oci-layout-helper.sh`, `bash scripts/verify-publish-helper.sh`, `bash scripts/verify-selinux-avc-gate.sh`, shell syntax checks including `image/build.sh` and `image/build-common.sh`, `actionlint .github/workflows/ci.yml .github/workflows/publish.yml`, and `git diff --check`. Verification passed in `nimbus/nimbus`: `bash scripts/verify-machine-os-release-ref-contract-helper.sh`, `bash scripts/verify-machine-os-release-ref-contract.sh --machine-os-repo /Users/jack/src/github.com/nimbus/machine-os`, `actionlint .github/workflows/release.yml`, and `git diff --check`. |
| 2026-05-14 | MOR5 completed. Audited `nimbus/machine-os` release `v0.1.31`: release target `495469ecf8cce364824adca1e05a289a252381b6`, assets `build-summary.txt`, `checksums.txt`, `machine-image-reference.txt`, `nimbus-machine-os.raw.gz`, `nimbus-machine-os.sbom.cdx.json`, `oci-layout-summary.txt`, `publish-summary.txt`, and `published-digests.txt`. `build-summary.txt` records Fedora bootc base `quay.io/fedora/fedora-bootc@sha256:5f2aa40538a71e32eba8dcdf9059dda10600bac68acef4588cb1aecedcfc6fe2`, BIB `quay.io/centos-bootc/bootc-image-builder@sha256:754fc17718f977313885379e2c779066aba7d15af88fe04b486baec74759f574`, rootfs `ext4`, Nimbus version `v0.1.31`, source revision `495469ecf8cce364824adca1e05a289a252381b6`, embedded Nimbus binary SHA-256 `ac7edc3b1969f97eeb63b800c6a5bb40e3bf29065ccc4fbf9bca8f289f2f6644`, SBOM SHA-256 `37578acb1aaedd1bad888fb8708d022544ce07ca4759ce72561fd0d91b16f54a`, compressed raw disk SHA-256 `5160048bc0dde3f51a2aee3c5dc1f1bde17fffe9e22dd752a2d5f62ab417afc8`, and SELinux expectation `container-runtime-domain-container-socket-policy-plus-fedora-bootupd-compat-plus-runtime-avc-gate`. `machine-image-reference.txt` records `ghcr.io/nimbus/machine-os:v0.1.31@sha256:9f9a0e24df20812ce7c779de3d53987381e522cadada0ab4acaf8664fd0d76eb`; `oci-layout-summary.txt` records `disk_type=applehv`, `source_repository_url=https://github.com/nimbus/machine-os`, `attestation_repository=nimbus/machine-os`, and `nimbus_version=v0.1.31`. `gh attestation verify /private/tmp/machine-os-v0.1.31-audit/build-summary.txt --repo nimbus/machine-os --source-ref refs/heads/main --format json` passed; a tag-ref attestation check failed as expected because `publish.yml` is workflow-dispatched from `refs/heads/main` while checking out the exact source revision. This behavior is documented in `nimbus/machine-os/docs/release-contract.md`. |
| 2026-05-14 | MOR6 completed. Verified help entrypoints in `nimbus/machine-os`: `bash scripts/build.sh --help`, `bash image/build.sh --help`, `bash scripts/package-oci.sh --help`, `bash scripts/publish.sh --help`, and `bash scripts/check-selinux-avcs.sh --help`. `docs/local-build.md` now documents Linux arm64/rootful Podman requirements, macOS-friendly deterministic checks, publishing boundaries, and common disk/rootless/SELinux troubleshooting. |
| 2026-05-14 | MOR7 completed. Final checks passed in `nimbus/machine-os`: old structure search `rg -n "proofs/|verify-fedora-bootc-proof|images/|/images|build.yml" README.md AGENTS.md SECURITY.md docs image scripts .github/workflows/ci.yml` returned no matches; `bash scripts/verify-recipe.sh`, `bash scripts/verify-build-helper.sh`, `bash scripts/verify-oci-layout-helper.sh`, `bash scripts/verify-publish-helper.sh`, `bash scripts/verify-selinux-avc-gate.sh`, shell syntax checks, `actionlint .github/workflows/ci.yml .github/workflows/publish.yml`, and `git diff --check` passed. Final checks passed in `nimbus/nimbus`: `bash scripts/verify-machine-os-release-ref-contract-helper.sh`, `bash scripts/verify-machine-os-release-ref-contract.sh --machine-os-repo /Users/jack/src/github.com/nimbus/machine-os`, `actionlint .github/workflows/release.yml`, and `git diff --check` passed. |
