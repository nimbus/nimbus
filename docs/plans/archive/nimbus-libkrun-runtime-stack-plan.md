# Plan: Nimbus Libkrun Runtime Stack

Archived plan for making the patched krun service stack installable and
reproducible from Nimbus-owned release artifacts.

This plan follows the completed sandbox hardening proof:
`docs/plans/archive/sandbox-microvm-hardening-plan.md`.

---

## Status

- **Status:** `done`
- **Archived:** 2026-05-21
- **Primary owner:** this plan
- **Parent distribution plan:** `docs/plans/distribution-plan.md`
- **Security proof baseline:**
  `docs/plans/archive/sandbox-microvm-hardening-plan.md`
- **Runtime stack components:**
  - `nimbus/nimbus`
  - `nimbus/nimbus-crun`
  - new `nimbus/nimbus-libkrun`
  - upstream `containers/libkrunfw` pinned as a source input, not forked

## Decision

Use `nimbus/nimbus-libkrun`, not `nimbus/libkrun`.

Rationale:

- this is not a general-purpose upstream fork for users to consume as
  `libkrun`
- Nimbus needs a private, paired runtime-stack package that should not collide
  with distro `libkrun` or system Podman/crun installations
- the naming mirrors `nimbus/nimbus-crun` and makes the package name
  `nimbus-libkrun` natural
- the repo still preserves upstream `containers/libkrun` history and keeps an
  `upstream` remote; it is just created as a Nimbus-owned repo rather than a
  GitHub fork relationship

Tag format follows the existing `nimbus-crun` family, but always uses the
exact upstream patch version going forward:
`v<upstream-version>-nimbus.<patch-revision>`.

Initial tags:

- `nimbus/nimbus-libkrun`: `v1.17.4-nimbus.1`
- `nimbus/nimbus-crun`: `v1.27.1-nimbus.1` for the first paired release
  that includes upstream crun `1.27.1`, commit `7f7eab0`, parser hardening
  commit `576e1f9`, and the paired `nimbus-libkrun` dependency

Do not rely on an upstream PR or upstream release for this work. Upstream can
remain a future possibility, but it is not part of this plan's completion
criteria.

Published `nimbus-crun` releases observed on 2026-05-21 are historical
pre-`1.27.1` tags:

- `v1.27-nimbus.1` points at `18bcc54`
- `v1.27-nimbus.2` points at `bb12d6b`

Those tags should not be rewritten. Future crun releases must not continue the
ambiguous `v1.27-nimbus.N` line once the upstream base is `1.27.1`.

## Product Shape

Nimbus Linux installs must use this private stack:

```text
/usr/bin/nimbus
/usr/libexec/nimbus/crun
/usr/libexec/nimbus/lib/libkrun.so.1.17.4
/usr/libexec/nimbus/lib/libkrun.so.1 -> libkrun.so.1.17.4
/usr/libexec/nimbus/lib/libkrun.so -> libkrun.so.1
/usr/libexec/nimbus/lib/libkrunfw.so.5.3.0
/usr/libexec/nimbus/lib/libkrunfw.so.5 -> libkrunfw.so.5.3.0
/usr/libexec/nimbus/lib/libkrunfw.so -> libkrunfw.so.5
```

`/usr/libexec/nimbus/crun` must resolve libkrun from
`/usr/libexec/nimbus/lib`, preferably through an embedded RUNPATH such as
`$ORIGIN/lib` or `$ORIGIN/../lib` depending on final layout. Do not require
operators to edit `ld.so.conf`, and do not replace the system `crun`,
`libkrun`, or `libkrunfw`.

The `nimbus-libkrun` release artifact may bundle the unmodified pinned
`libkrunfw` runtime library so Debian/Ubuntu and Fedora use the same validated
Nimbus private stack. If a future libkrunfw patch is needed, split it into a
separate `nimbus/nimbus-libkrunfw` plan; do not expand this one silently.

## Current Impact Inventory

Reviewed on 2026-05-21.

| Area | Current state | Required change |
| --- | --- | --- |
| `nimbus-crun` repo | `main` and tag `v1.27.1-nimbus.1` now point at `0c584de`; release includes upstream crun `1.27.1`, parser hardening commit `576e1f9`, paired nimbus-libkrun build commit `acf9b05`, and include-path fix `0c584de`; historical published releases `v1.27-nimbus.1` and `v1.27-nimbus.2` remain unchanged | done for NLS3; package/install paths must now consume `v1.27.1-nimbus.1` |
| `scripts/install.sh` | downloads `nimbus` and `nimbus-crun`; Debian prints manual upstream libkrun/libkrunfw build instructions; Fedora installs distro `libkrun`/`libkrunfw` | resolve/download/install `nimbus-libkrun`; stop telling users to build upstream libkrun; stop using distro libkrun for Nimbus service execution |
| `scripts/verify-install.sh` and inline verifier | check `+LIBKRUN` and generic shared-library presence | verify private lib path plus `krun_set_port_map_with_bind_address` symbol |
| `scripts/verify-install-helper.sh` | mocked dry-run and latest-release fixtures know only `nimbus-crun` | add `nimbus-libkrun` version, release API, checksums, dry-run, and uninstall assertions |
| `packaging/linux-distribution-contract.env` | only pins `NIMBUS_CRUN_VERSION=v1.27-nimbus.2` | update to `NIMBUS_CRUN_VERSION=v1.27.1-nimbus.1`; add `NIMBUS_CRUN_UPSTREAM_VERSION=1.27.1`, `NIMBUS_LIBKRUN_VERSION=v1.17.4-nimbus.1`, and `NIMBUS_LIBKRUN_UPSTREAM_VERSION=1.17.4` so tag and upstream source versions are both explicit |
| `Makefile` | package targets require Nimbus and nimbus-crun artifacts only | add required nimbus-libkrun artifacts/version inputs to Linux package and Fedora SRPM targets |
| `nimbus-crun` build surface | `scripts/build.sh`, `scripts/verify-fedora-userspace.sh`, `.github/container/Dockerfile.builder`, `.github/workflows/build.yml`, and `README.md` still consume Fedora `libkrun-devel`/repo libkrun and publish only raw crun binaries | consume the released `nimbus-libkrun` archive or source artifact, set pkg-config/lib paths from that private tree, prove RUNPATH/RPATH, update release notes, and publish the exact-base `v1.27.1-nimbus.1` release |
| `scripts/build-linux-release-packages.sh` | emits `nimbus` and `nimbus-crun`; `nimbus-crun` depends on distro `libkrun` and `libkrunfw` | emit `nimbus-libkrun`; make `nimbus-crun` depend on `nimbus-libkrun`; stage private libs under `/usr/libexec/nimbus/lib` |
| `scripts/verify-build-linux-release-packages-helper.sh` | asserts `nimbus-crun` depends on `libkrun`/`libkrunfw` | assert `nimbus-crun` depends on `nimbus-libkrun` and package files include private libs |
| `scripts/build-fedora-release-srpms.sh` | wraps released `nimbus` and `nimbus-crun`; `nimbus-crun` RPM requires distro `libkrun`/`libkrunfw` | wrap `nimbus-libkrun` release assets into an SRPM/RPM; make `nimbus-crun` require `nimbus-libkrun` |
| `scripts/verify-build-fedora-release-srpms-helper.sh` | rebuilds two SRPMs and verifies distro libkrun deps | rebuild three SRPMs and verify installed private runtime stack |
| `.github/workflows/linux-distribution-release.yml` | resolves and passes only `nimbus_crun_version` | resolve/pass `nimbus_libkrun_version` to all Linux downstream workflows |
| `.github/workflows/linux-packages.yml` | downloads Nimbus and nimbus-crun release assets | download nimbus-libkrun release archive and pass it to the package builder |
| `.github/workflows/apt-repo.yml` | builds repo from generated `nimbus` and `nimbus-crun` debs | include generated `nimbus-libkrun` debs for both arches |
| `.github/workflows/copr-srpms.yml` | downloads nimbus/nimbus-crun assets and submits two SRPMs | download nimbus-libkrun assets and submit/build three SRPMs |
| `scripts/verify-build-apt-repository-helper.sh` | fixture repo includes `nimbus` and `nimbus-crun` only | add `nimbus-libkrun` fixture packages and dependency metadata |
| `scripts/collect-vmm-package-versions.sh` and `scripts/check-vmm-host.sh` | report system `libkrun`/`libkrunfw` | report Nimbus-private libkrun stack and bind-address symbol when installed |
| `scripts/prepare-linux-vmm-validation-bundle.sh` and `scripts/verify-linux-vmm-validation-bundle-helper.sh` | prepare and assert the old crun source/build validation flow with `scripts/build-nimbus-crun.sh` | update the bundle so Linux validation can consume released `nimbus-crun` + `nimbus-libkrun` artifacts, while keeping source-build commands only as developer diagnostics |
| `docs/architecture/sandbox/*` and `docs/plans/security/sandbox-isolation-audit.md` | stable sandbox docs name patched crun/libkrun, old `minicloud` libkrun branch, and source-build runbooks | document the published `nimbus-libkrun` fork/tag as the production stack owner while preserving historical proof context |
| `README.md`, `docs/plans/distribution-plan.md`, `docs/operating/updates.md` | user-facing docs still describe distro/manual libkrun assumptions in places | document `nimbus-libkrun` as a private runtime package and update Linux install/update expectations |

Archived plans such as `docs/plans/archive/install-script-plan.md` stay
historical. Current docs and scripts must stop pointing users at upstream
manual libkrun builds as a supported install path.

## Phase Status Ledger

| Phase | Status | Goal | Verification |
| --- | --- | --- | --- |
| NLS0 | `done` | Audit current repo impact and choose the Nimbus libkrun naming/distribution shape. | This plan records the reviewed files and naming decision. |
| NLS1 | `done` | Create and tag the Nimbus-owned `nimbus/nimbus-libkrun` source repo. | `git ls-remote` shows `main` and `v1.17.4-nimbus.1`; tag contains the validated bind-address hook commit. |
| NLS2 | `done` | Add `nimbus-libkrun` CI/release artifacts for Linux amd64/arm64. | Release has runtime archives, checksums, provenance, symbol proof, and libkrunfw version proof. |
| NLS3 | `done` | Rebuild `nimbus-crun` against the Nimbus-private libkrun stack. | `v1.27.1-nimbus.1` has `$ORIGIN/lib` RUNPATH, `+LIBKRUN`, release provenance, and fail-closed missing-symbol build gating. |
| NLS4 | `done` | Update direct install/uninstall/verify flows. | Install helper dry-runs and real Linux proof install `nimbus`, `nimbus-libkrun`, and `nimbus-crun` together. |
| NLS5 | `done` | Update deb/rpm, apt, and COPR builders/workflows. | Package helper tests produce three packages/SRPMs and dependency metadata uses `nimbus-libkrun`. |
| NLS6 | `done` | Capture fresh Linux service smoke from installed artifacts and close docs. | Debian 13 and Fedora proof show localhost-only krun smoke plus private library resolution. |

## Phase Details

### NLS0: Audit And Naming

Status: `done`

Deliverables:

- review current `nimbus-crun` fork/tag/build conventions
- review Nimbus install, package, apt, COPR, docs, and verification paths
- decide repo/package naming and installation shape

Acceptance criteria:

- plan lists every known touched script/workflow/doc family
- repo naming and tag format are explicit
- no upstream PR or upstream-release dependency remains in the plan

### NLS1: Source Repo Bootstrap

Status: `done`

Deliverables:

- create local worktree at `~/src/github.com/nimbus/nimbus-libkrun`
- preserve upstream `containers/libkrun` history
- configure `upstream` as `https://github.com/containers/libkrun.git`
- configure `origin` as the Nimbus-owned repo
- apply or cherry-pick the validated bind-address hook from `minicloud`
  commit `fc13a8e`
- tag `v1.17.4-nimbus.1`
- push `main` and the tag to `nimbus/nimbus-libkrun`

Acceptance criteria:

- `git remote -v` shows both `origin` and `upstream`
- `git describe --tags` on `main` resolves to the Nimbus tag
- `git diff v1.17.4..v1.17.4-nimbus.1` contains only the bind-address hook
  and its tests
- `git ls-remote --tags origin v1.17.4-nimbus.1` returns the tag

Suggested bootstrap shape:

```bash
git clone https://github.com/containers/libkrun.git ~/src/github.com/nimbus/nimbus-libkrun
cd ~/src/github.com/nimbus/nimbus-libkrun
git remote rename origin upstream
git checkout -b main v1.17.4
# apply/cherry-pick validated bind-address hook from minicloud fc13a8e
git remote add origin git@github.com:nimbus/nimbus-libkrun.git
git tag -a v1.17.4-nimbus.1 -m "nimbus-libkrun v1.17.4-nimbus.1"
git push origin main v1.17.4-nimbus.1
```

Closeout evidence:

- GitHub repo: `https://github.com/nimbus/nimbus-libkrun`
- Visibility/shape: public, non-fork, default branch `main`
- `main`: `555972548245c7df12930dd837baef05ce529578`
- tag: `v1.17.4-nimbus.1`
- source base: upstream `v1.17.4`
- Nimbus patch commit: `5559725 Add krun TSI bind address hook`
- proof source: minicloud `nimbus-bind-address` commit `fc13a8e`
- remote tag cleanup: accidental upstream `v0.2.0` tag was deleted from
  `origin`; local `push.followTags=false` prevents future incidental upstream
  tag publication from this worktree
- verification:
  - `git describe --tags --always` -> `v1.17.4-nimbus.1`
  - `git diff --stat v1.17.4..v1.17.4-nimbus.1` shows only the validated
    bind-address hook delta
  - `git ls-remote --heads origin main` returns `5559725...`
  - `git ls-remote --tags origin` returns only `v1.17.4-nimbus.1`

### NLS2: Libkrun Release Artifacts

Status: `done`

Deliverables:

- add build helper(s) in `nimbus/nimbus-libkrun`
- build patched libkrun for Linux amd64 and arm64
- build or bundle pinned upstream `libkrunfw` `5.3.0`
- expose pkg-config metadata that `nimbus-crun` can consume without consulting
  distro `libkrun-devel`
- publish release assets on `v*` tags
- publish checksums and GitHub artifact attestations

Expected release assets:

- `nimbus-libkrun-linux-amd64.tar.gz`
- `nimbus-libkrun-linux-arm64.tar.gz`
- `checksums.txt`

Each archive should contain the private runtime library tree under a stable
relative prefix, for example:

```text
lib/libkrun.so.1.17.4
lib/libkrun.so.1
lib/libkrun.so
lib/libkrunfw.so.5.3.0
lib/libkrunfw.so.5
lib/libkrunfw.so
include/libkrun.h
lib/pkgconfig/libkrun.pc
```

Acceptance criteria:

- `cargo test -p libkrun port_map_tests -- --nocapture` passes in CI or the
  equivalent repository-local test path
- `nm -D lib/libkrun.so.1.17.4` shows
  `krun_set_port_map_with_bind_address`
- `pkg-config --define-prefix --libs libkrun` resolves against the extracted
  archive's private `lib/pkgconfig/libkrun.pc`
- release checksums verify for both archives
- attestation verification works for the release archives
- README names the private install path and says the package does not replace
  system `libkrun`

Closeout evidence:

- release URL:
  `https://github.com/nimbus/nimbus-libkrun/releases/tag/v1.17.4-nimbus.1`
- release workflow:
  `https://github.com/nimbus/nimbus-libkrun/actions/runs/26258037658`
- published assets:
  - `nimbus-libkrun-linux-amd64.tar.gz`
  - `nimbus-libkrun-linux-arm64.tar.gz`
  - `checksums.txt`
- checksums:
  - `nimbus-libkrun-linux-amd64.tar.gz`:
    `cce21c5d7fe9cd6d245e114a41e5680df3c8b88fdb982c7e05a3356d1f5c8f48`
  - `nimbus-libkrun-linux-arm64.tar.gz`:
    `56f9c851365d9a0b661a597fb385b71555d9bd9cb5adee300d357c7848198c47`
- GitHub release metadata:
  - `isDraft=false`
  - `isPrerelease=false`
  - asset digest for `checksums.txt`:
    `sha256:2c799dd7f1ee294d7c802f26f97529483932815101727759984a401ab4bc7ba7`
- provenance:
  - `gh attestation verify ... --repo nimbus/nimbus-libkrun` succeeded for
    both release archives
  - attested subjects include both archives and `checksums.txt`
  - signer identity:
    `https://github.com/nimbus/nimbus-libkrun/.github/workflows/release.yml@refs/heads/main`
- macOS download proof:
  - `shasum -a 256 -c checksums.txt` returned `OK` for both release archives
  - `tar -tzf` for both archives showed the expected `lib/`, `include/`,
    `lib/pkgconfig/libkrun.pc`, `NIMBUS_LIBKRUN_RELEASE.txt`,
    `libkrun.so.1.17.4`, and `libkrunfw.so.5.3.0` layout
- minicloud Linux verifier proof:
  - Debian 13 host: `minicloud`, `Linux 6.12.88+deb13-amd64`
  - `sha256sum -c checksums.txt` returned `OK` for both release archives
  - `scripts/verify-release-archive.sh --archive ...amd64.tar.gz` succeeded
  - `scripts/verify-release-archive.sh --archive ...arm64.tar.gz` succeeded
  - verifier reported `verified.libkrun=.../lib/libkrun.so.1.17.4`,
    `verified.libkrunfw=.../lib/libkrunfw.so.5.3.0`, and
    `verified.pkg_config=-L.../lib -lkrun`

### NLS3: Paired `nimbus-crun` Build

Status: `done`

Deliverables:

- update `nimbus/nimbus-crun` build helpers to consume
  `nimbus-libkrun` headers/pkgconfig/libs instead of Fedora distro
  `libkrun-devel`
- update `nimbus/nimbus-crun` `.github/container/Dockerfile.builder`,
  `.github/workflows/build.yml`, `scripts/verify-fedora-userspace.sh`, and
  `README.md` so CI and local proof use the paired private libkrun stack
- embed or otherwise prove private library resolution for
  `/usr/libexec/nimbus/crun`
- update README and release notes to say address-bearing port maps require the
  paired `nimbus-libkrun` package
- tag and publish `v1.27.1-nimbus.1` paired with `v1.17.4-nimbus.1`
- keep `v1.27-nimbus.1` and `v1.27-nimbus.2` as historical tags; do not
  rewrite published releases
- remove any remaining `neovex` naming from branch/tag/release references
  before publishing

Acceptance criteria:

- `scripts/verify-patch.sh` still passes against crun `1.27.1`
- parser malformed-input harness still passes
- `scripts/verify-fedora-userspace.sh` builds against the extracted
  `nimbus-libkrun` artifact rather than Fedora `libkrun-devel`
- `git describe --tags` on the release commit resolves to
  `v1.27.1-nimbus.1`
- `nimbus-crun --version` shows `+LIBKRUN`
- `readelf -d nimbus-crun-linux-amd64` or equivalent proof shows private
  RUNPATH/RPATH when used
- crun's krun handler loads `libkrun` with `dlopen`; therefore `ldd` is not
  expected to list `libkrun`, and `readelf` RUNPATH plus the NLS6 service smoke
  prove private runtime resolution
- missing `krun_set_port_map_with_bind_address` remains fail-closed
- `gh release view v1.27.1-nimbus.1 --repo nimbus/nimbus-crun` shows release
  notes naming upstream crun `1.27.1` and paired `nimbus-libkrun`
  `v1.17.4-nimbus.1`

Closeout evidence:

- release URL:
  `https://github.com/nimbus/nimbus-crun/releases/tag/v1.27.1-nimbus.1`
- release workflow:
  `https://github.com/nimbus/nimbus-crun/actions/runs/26259058355`
- green main workflow before tagging:
  `https://github.com/nimbus/nimbus-crun/actions/runs/26258974164`
- release commit/tag:
  - `git describe --tags --always` -> `v1.27.1-nimbus.1`
  - `v1.27.1-nimbus.1` points at `0c584de`
  - `0c584de` includes `7f7eab0`, `576e1f9`, and `acf9b05`
- published assets:
  - `nimbus-crun-linux-amd64`
  - `nimbus-crun-linux-arm64`
  - `checksums.txt`
- checksums:
  - `nimbus-crun-linux-amd64`:
    `401ff1076ff0f34d7c0d367bbe72669b0df937a904be5102707838e0a0deca43`
  - `nimbus-crun-linux-arm64`:
    `fbc3aad6c2b79dc4345272a887dd2bedee820f22e35219ff59238ae8b130eb1a`
- GitHub release metadata:
  - `isDraft=false`
  - `isPrerelease=false`
  - asset digest for `checksums.txt`:
    `sha256:c6a304ad8f67978996e9307148dc973002db9e373b3449d66a8939f46516c5e0`
- provenance:
  - `gh attestation verify ... --repo nimbus/nimbus-crun` succeeded
  - attested subjects include both binaries and `checksums.txt`
  - signer identity:
    `https://github.com/nimbus/nimbus-crun/.github/workflows/build.yml@refs/tags/v1.27.1-nimbus.1`
- minicloud Linux build proof:
  - build helper consumed extracted
    `nimbus-libkrun-linux-amd64.tar.gz` from `v1.17.4-nimbus.1`
  - build helper reported
    `build.libkrun.root=/tmp/nimbus-crun-build-proof-libkrun`
  - build helper reported
    `build.libkrun.shared_object=.../lib/libkrun.so.1.17.4`
  - build helper reported
    `build.libkrun.pkg_config=-L.../lib -lkrun`
  - build helper reported `build.libkrun.linkage=dlopen`
  - output version showed `+LIBKRUN`
  - `readelf -d` showed `RUNPATH` of `$ORIGIN/lib`
- minicloud missing-symbol proof:
  - fake libkrun root without `krun_set_port_map_with_bind_address` failed
    before build with exit code `69`
- published binary proof:
  - minicloud `sha256sum -c checksums.txt` returned `OK` for both release
    binaries
  - published amd64 binary `--version` showed `+LIBKRUN`
  - `readelf -d` for both published amd64 and arm64 binaries showed
    `Library runpath: [$ORIGIN/lib]`

### NLS4: Direct Install Script

Status: `done`

Deliverables:

- add `NIMBUS_LIBKRUN_VERSION`
- add `nimbus/nimbus-libkrun` release API/download constants
- add `--libkrun-version <tag>` or a paired-stack version flag
- download, verify, attest, and install `nimbus-libkrun`
- install private libs before `nimbus-crun`
- uninstall private libs and empty private dirs
- update `verify_installation` to prove the private bind-address hook

Acceptance criteria:

- `bash -n scripts/install.sh`
- `dash -n scripts/install.sh`
- `bash -n scripts/verify-install.sh`
- `bash scripts/verify-install-helper.sh`
- Linux dry-run prints `nimbus-libkrun` version and
  `/usr/libexec/nimbus/lib`
- direct install no longer prints Debian manual upstream build instructions
- Fedora direct install no longer accepts distro `libkrun` as sufficient for
  Nimbus service execution
- verifier fails if `krun_set_port_map_with_bind_address` is missing

Closeout evidence, 2026-05-21:

- local syntax gates passed:
  `bash -n scripts/install.sh`, `dash -n scripts/install.sh`,
  `bash -n scripts/verify-install.sh`, and
  `bash -n scripts/verify-install-helper.sh`
- local helper gate passed:
  `bash scripts/verify-install-helper.sh` reported
  `verified: install script helper passed 29 tests`
- Debian 13 `minicloud` helper gate passed:
  `bash /home/nimbus/src/github.com/nimbus/nimbus/scripts/verify-install-helper.sh`
  reported `verified: install script helper passed 31 tests`
- Debian 13 `minicloud` installed-stack verifier passed:
  `bash /home/nimbus/src/github.com/nimbus/nimbus/scripts/verify-install.sh`
  reported `result supported (0 failures)` with `nimbus 0.1.31`,
  `nimbus-libkrun v1.17.4-nimbus.1`, private
  `/usr/libexec/nimbus/lib/libkrun.so.1`,
  `/usr/libexec/nimbus/lib/libkrunfw.so.5`,
  `krun_set_port_map_with_bind_address`, `+LIBKRUN`, and
  `$ORIGIN/lib` RUNPATH
- Debian 13 `minicloud` idempotence proof passed:
  `/home/nimbus/src/github.com/nimbus/nimbus/scripts/install.sh --skip-deps --yes --version v0.1.31 --libkrun-version v1.17.4-nimbus.1 --crun-version v1.27.1-nimbus.1`
  skipped already installed `nimbus`, `nimbus-libkrun`, and `nimbus-crun`,
  then reported `Verification passed`

### NLS5: Linux Packages And Release Mirror

Status: `done`

Deliverables:

- add `NIMBUS_LIBKRUN_VERSION` to `packaging/linux-distribution-contract.env`
- update `NIMBUS_CRUN_VERSION` to `v1.27.1-nimbus.1`
- add explicit upstream source-version variables for crun and libkrun
- update `make build-linux-release-packages`
- update `make build-fedora-release-srpms`
- update `scripts/build-linux-release-packages.sh`
- update `scripts/build-fedora-release-srpms.sh`
- update helper tests for both builders
- update `.github/workflows/linux-packages.yml`
- update `.github/workflows/apt-repo.yml`
- update `.github/workflows/copr-srpms.yml`
- update `.github/workflows/linux-distribution-release.yml`
- update `scripts/verify-build-apt-repository-helper.sh`

Acceptance criteria:

- package builder renders `nimbus`, `nimbus-crun`, and `nimbus-libkrun`
  manifests
- `nimbus-crun` package depends on `nimbus-libkrun`
- no Nimbus package depends on distro `libkrun` or `libkrunfw` for service
  execution
- apt repository helper includes all three package names for both arches
- Fedora SRPM helper builds/rebuilds all three SRPMs
- workflows pass `nimbus_libkrun_version` through their inputs/outputs
- release/build logs print both Nimbus release tags and upstream source
  versions

Closeout evidence, 2026-05-21:

- local syntax gates passed:
  `bash -n scripts/build-linux-release-packages.sh`,
  `bash -n scripts/verify-build-linux-release-packages-helper.sh`,
  `bash -n scripts/build-fedora-release-srpms.sh`,
  `bash -n scripts/verify-build-fedora-release-srpms-helper.sh`, and
  `bash -n scripts/verify-build-apt-repository-helper.sh`
- local workflow lint passed:
  `actionlint .github/workflows/linux-packages.yml .github/workflows/apt-repo.yml .github/workflows/copr-srpms.yml .github/workflows/linux-distribution-release.yml`
- local helper gate passed:
  `bash scripts/verify-build-linux-release-packages-helper.sh` reported
  `verified: linux package builder rendered deterministic nimbus/nimbus-libkrun/nimbus-crun deb/rpm manifests`
- Debian 13 `minicloud` package helper gate passed with the same
  three-package manifest proof
- Debian 13 `minicloud` apt repository helper gate passed:
  `bash scripts/verify-build-apt-repository-helper.sh` reported
  `verified: apt repository builder produced signed metadata via local`
- Debian 13 `minicloud` Fedora/COPR helper gate passed under Podman:
  `bash scripts/verify-build-fedora-release-srpms-helper.sh` reported
  reusable `nimbus`, `nimbus-libkrun`, and `nimbus-crun` source RPMs,
  installed x86_64 RPMs, and query-verified aarch64 RPM metadata
- `packaging/linux-distribution-contract.env` now pins
  `NIMBUS_CRUN_VERSION=v1.27.1-nimbus.1`,
  `NIMBUS_CRUN_UPSTREAM_VERSION=1.27.1`,
  `NIMBUS_LIBKRUN_VERSION=v1.17.4-nimbus.1`, and
  `NIMBUS_LIBKRUN_UPSTREAM_VERSION=1.17.4`

### NLS6: Fresh Host Proof And Closeout

Status: `done`

Deliverables:

- update `README.md`, `docs/plans/distribution-plan.md`,
  `docs/operating/updates.md`, `docs/architecture/sandbox/microvm-service-baseline.md`,
  `docs/architecture/sandbox/krun-vmm-host-validation.md`,
  `docs/architecture/sandbox/krun-sandbox-backend-smoke.md`, and
  `docs/plans/security/sandbox-isolation-audit.md`
- run fresh Debian 13 proof
- run fresh Fedora proof
- rerun the krun localhost-only smoke from installed artifacts
- archive this plan when complete

Acceptance criteria:

- fresh Debian direct install or apt-package proof installs all three Nimbus
  runtime-stack components without manual libkrun build instructions
- fresh Fedora proof does not use distro `libkrun` for Nimbus service
  execution
- `/usr/libexec/nimbus/crun --version` shows `+LIBKRUN`
- `readelf -d /usr/libexec/nimbus/crun` shows private `$ORIGIN/lib` RUNPATH
- `nm -D /usr/libexec/nimbus/lib/libkrun.so.1.17.4` shows
  `krun_set_port_map_with_bind_address`
- the root VMM krun smoke passes from the installed stack with
  `NIMBUS_KRUN_SMOKE_NON_LOOPBACK_HOST=<host-ip>` and the image-backed
  `krun_backend_image_backed_smoke_pulls_and_boots_busybox` test binary
- `docs/plans/security/sandbox-isolation-audit.md` still states F4 is closed
  only for the patched paired stack

Closeout evidence:

- Local script gates passed:
  `bash -n scripts/collect-vmm-package-versions.sh`,
  `bash -n scripts/check-vmm-host.sh`,
  `bash -n scripts/prepare-linux-vmm-validation-bundle.sh`,
  `bash -n scripts/verify-linux-vmm-validation-bundle-helper.sh`,
  `bash scripts/verify-linux-vmm-validation-bundle-helper.sh`, and
  `git diff --check -- scripts/collect-vmm-package-versions.sh scripts/check-vmm-host.sh scripts/prepare-linux-vmm-validation-bundle.sh scripts/verify-linux-vmm-validation-bundle-helper.sh`.
- Debian 13 `minicloud` helper proof passed:
  `bash scripts/verify-linux-vmm-validation-bundle-helper.sh`.
- Debian 13 released-artifact staging proof passed after downloading
  `nimbus-crun-linux-amd64` from `nimbus-crun v1.27.1-nimbus.1` and
  `nimbus-libkrun-linux-amd64.tar.gz` from
  `nimbus-libkrun v1.17.4-nimbus.1`: generated LH3 reported
  `stage.source=released-artifacts`, `stage.nimbus_libkrun_root=<stage>`,
  `crun version 1.27.1-dirty`, and `+LIBKRUN`, with
  `<stage>/lib/libkrun.so.1` and `<stage>/lib/libkrunfw.so.5` present for
  `$ORIGIN/lib` resolution.
- Debian 13 root-context host proof passed:
  `sudo bash scripts/check-vmm-host.sh` ended with `result supported`.
- Debian 13 root-context collector showed
  `nimbus.libkrun version=v1.17.4-nimbus.1`,
  `nimbus.libkrun.symbol present krun_set_port_map_with_bind_address`,
  `nimbus.crun.version ... +LIBKRUN`, and
  `nimbus.crun.runpath present $ORIGIN/lib`.
- Debian 13 root VMM smoke passed from the installed stack:
  `sudo env NIMBUS_KRUN_SMOKE_WORKDIR=/tmp/nimbus-krun-smoke-nls6-root-unwrapped NIMBUS_KRUN_SMOKE_RUNTIME=/usr/libexec/nimbus/crun NIMBUS_KRUN_SMOKE_CONMON=/usr/bin/conmon NIMBUS_KRUN_SMOKE_BUILDAH=/usr/bin/buildah NIMBUS_KRUN_SMOKE_NON_LOOPBACK_HOST=192.168.4.29 target/debug/deps/krun_linux_smoke-3c2c39554d8244a8 krun_backend_image_backed_smoke_pulls_and_boots_busybox --ignored --nocapture`
  reported `1 passed`, with `192.168.4.29:18081` connection refused.
- Fedora proof remained package/channel proof in Fedora 42 userspace:
  `bash scripts/verify-build-fedora-release-srpms-helper.sh` rebuilt
  reusable `nimbus`, `nimbus-libkrun`, and `nimbus-crun` SRPMs, installed the
  x86_64 RPM stack, and query-verified aarch64 RPM metadata/files without any
  `nimbus-crun` dependency on distro `libkrun` or `libkrunfw`.
- Documentation closeout updated `README.md`,
  `docs/plans/distribution-plan.md`, `docs/operating/updates.md`,
  `docs/architecture/sandbox/microvm-service-baseline.md`,
  `docs/architecture/sandbox/krun-vmm-host-validation.md`,
  `docs/architecture/sandbox/krun-sandbox-backend-smoke.md`, and
  `docs/plans/security/sandbox-isolation-audit.md`.

## Verification Command Set

Nimbus repo:

```bash
bash -n scripts/install.sh
dash -n scripts/install.sh
bash -n scripts/verify-install.sh
bash scripts/verify-install-helper.sh
bash scripts/verify-build-linux-release-packages-helper.sh
bash scripts/verify-build-apt-repository-helper.sh
bash scripts/verify-build-fedora-release-srpms-helper.sh
git diff --check
```

`nimbus/nimbus-crun`:

```bash
bash -n scripts/build.sh
bash -n scripts/verify-patch.sh
bash -n scripts/verify-port-map-parser.sh
bash scripts/verify-patch.sh ~/src/github.com/containers/crun
git tag --points-at HEAD | grep '^v1\.27\.1-nimbus\.1$'
gh release view v1.27.1-nimbus.1 --repo nimbus/nimbus-crun
```

`nimbus/nimbus-libkrun`:

```bash
cargo test -p libkrun port_map_tests -- --nocapture
make
nm -D target/release/libkrun.so.1.17.4 | grep krun_set_port_map_with_bind_address
git diff --check
```

Linux host proof:

```bash
readelf -d /usr/libexec/nimbus/crun | grep '$ORIGIN/lib'
nm -D /usr/libexec/nimbus/lib/libkrun.so.1.17.4 | grep krun_set_port_map_with_bind_address
sudo env NIMBUS_KRUN_SMOKE_NON_LOOPBACK_HOST=<host-ip> NIMBUS_KRUN_SMOKE_RUNTIME=/usr/libexec/nimbus/crun target/debug/deps/krun_linux_smoke-* krun_backend_image_backed_smoke_pulls_and_boots_busybox --ignored --nocapture
```

## Stop Conditions

Do not mark this plan done until:

- `nimbus/nimbus-libkrun` exists as a Nimbus-owned source repo with tag
  `v1.17.4-nimbus.1`
- `nimbus-crun` `v1.27.1-nimbus.1` release artifacts are paired with that
  libkrun release
- direct install and package install paths use `nimbus-libkrun`
- fresh Linux proof shows the installed stack preserves localhost-only TSI
  exposure

Block instead of papering over if:

- private lib resolution cannot be made deterministic without global
  `ld.so.conf` changes
- CI cannot build libkrun/libkrunfw reproducibly for amd64 and arm64
- release/package signing or provenance cannot cover the new artifact source
