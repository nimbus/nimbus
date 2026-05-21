# Plan: Nimbus Libkrun Runtime Stack

Focused active plan for making the patched krun service stack installable and
reproducible from Nimbus-owned release artifacts.

This plan follows the completed sandbox hardening proof:
`docs/plans/archive/sandbox-microvm-hardening-plan.md`.

---

## Status

- **Status:** `active`
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
| `nimbus-crun` repo | local `main` is ahead of `origin/main` by parser hardening commit `576e1f9`; `origin/main` has `7f7eab0` refreshing to upstream crun `1.27.1`; published releases are `v1.27-nimbus.1` and `v1.27-nimbus.2`, both before `7f7eab0`; stale local `v1.27-neovex.1` tag was removed from this worktree | build against `nimbus-libkrun`, embed private lib lookup, verify bind-address symbol, tag and publish `v1.27.1-nimbus.1` |
| `scripts/install.sh` | downloads `nimbus` and `nimbus-crun`; Debian prints manual upstream libkrun/libkrunfw build instructions; Fedora installs distro `libkrun`/`libkrunfw` | resolve/download/install `nimbus-libkrun`; stop telling users to build upstream libkrun; stop using distro libkrun for Nimbus service execution |
| `scripts/verify-install.sh` and inline verifier | check `+LIBKRUN` and generic shared-library presence | verify private lib path plus `krun_set_port_map_with_bind_address` symbol |
| `scripts/verify-install-helper.sh` | mocked dry-run and latest-release fixtures know only `nimbus-crun` | add `nimbus-libkrun` version, release API, checksums, dry-run, and uninstall assertions |
| `packaging/linux-distribution-contract.env` | only pins `NIMBUS_CRUN_VERSION=v1.27-nimbus.2` | update to `NIMBUS_CRUN_VERSION=v1.27.1-nimbus.1`; add `NIMBUS_CRUN_UPSTREAM_VERSION=1.27.1`, `NIMBUS_LIBKRUN_VERSION=v1.17.4-nimbus.1`, and `NIMBUS_LIBKRUN_UPSTREAM_VERSION=1.17.4` so tag and upstream source versions are both explicit |
| `Makefile` | package targets require Nimbus and nimbus-crun artifacts only | add required nimbus-libkrun artifacts/version inputs to Linux package and Fedora SRPM targets |
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
| `scripts/prepare-linux-vmm-validation-bundle.sh` | prepares old crun source/build validation flow | update command bundle so Linux validation uses the paired `nimbus-crun` + `nimbus-libkrun` stack |
| `README.md`, `docs/plans/distribution-plan.md`, `docs/operating/updates.md` | user-facing docs still describe distro/manual libkrun assumptions in places | document `nimbus-libkrun` as a private runtime package and update Linux install/update expectations |

Archived plans such as `docs/plans/archive/install-script-plan.md` stay
historical. Current docs and scripts must stop pointing users at upstream
manual libkrun builds as a supported install path.

## Phase Status Ledger

| Phase | Status | Goal | Verification |
| --- | --- | --- | --- |
| NLS0 | `done` | Audit current repo impact and choose the Nimbus libkrun naming/distribution shape. | This plan records the reviewed files and naming decision. |
| NLS1 | `todo` | Create and tag the Nimbus-owned `nimbus/nimbus-libkrun` source repo. | `git ls-remote` shows `main` and `v1.17.4-nimbus.1`; tag contains the validated bind-address hook commit. |
| NLS2 | `todo` | Add `nimbus-libkrun` CI/release artifacts for Linux amd64/arm64. | Release has runtime archives, checksums, provenance, symbol proof, and libkrunfw version proof. |
| NLS3 | `todo` | Rebuild `nimbus-crun` against the Nimbus-private libkrun stack. | `v1.27.1-nimbus.1` resolves private libkrun, has `+LIBKRUN`, and fails if the bind-address symbol is absent. |
| NLS4 | `todo` | Update direct install/uninstall/verify flows. | Install helper dry-runs and real Linux proof install `nimbus`, `nimbus-libkrun`, and `nimbus-crun` together. |
| NLS5 | `todo` | Update deb/rpm, apt, and COPR builders/workflows. | Package helper tests produce three packages/SRPMs and dependency metadata uses `nimbus-libkrun`. |
| NLS6 | `todo` | Capture fresh Linux service smoke from installed artifacts and close docs. | Debian 13 and Fedora proof show localhost-only krun smoke plus private library resolution. |

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

Status: `todo`

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

### NLS2: Libkrun Release Artifacts

Status: `todo`

Deliverables:

- add build helper(s) in `nimbus/nimbus-libkrun`
- build patched libkrun for Linux amd64 and arm64
- build or bundle pinned upstream `libkrunfw` `5.3.0`
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
- release checksums verify for both archives
- attestation verification works for the release archives
- README names the private install path and says the package does not replace
  system `libkrun`

### NLS3: Paired `nimbus-crun` Build

Status: `todo`

Deliverables:

- update `nimbus/nimbus-crun` build helpers to consume
  `nimbus-libkrun` headers/pkgconfig/libs instead of Fedora distro
  `libkrun-devel`
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
- `git describe --tags` on the release commit resolves to
  `v1.27.1-nimbus.1`
- `nimbus-crun --version` shows `+LIBKRUN`
- `readelf -d nimbus-crun-linux-amd64` or equivalent proof shows private
  RUNPATH/RPATH when used
- `ldd /usr/libexec/nimbus/crun` on a proof host resolves `libkrun` to
  `/usr/libexec/nimbus/lib/...`
- missing `krun_set_port_map_with_bind_address` remains fail-closed
- `gh release view v1.27.1-nimbus.1 --repo nimbus/nimbus-crun` shows release
  notes naming upstream crun `1.27.1` and paired `nimbus-libkrun`
  `v1.17.4-nimbus.1`

### NLS4: Direct Install Script

Status: `todo`

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

### NLS5: Linux Packages And Release Mirror

Status: `todo`

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

### NLS6: Fresh Host Proof And Closeout

Status: `todo`

Deliverables:

- update `README.md`, `docs/plans/distribution-plan.md`,
  `docs/operating/updates.md`, and relevant sandbox docs
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
- `ldd /usr/libexec/nimbus/crun` resolves private libkrun
- `nm -D /usr/libexec/nimbus/lib/libkrun.so.1.17.4` shows
  `krun_set_port_map_with_bind_address`
- `NIMBUS_KRUN_SMOKE_NON_LOOPBACK_HOST=<host-ip> cargo test -p
  nimbus-sandbox --test krun_linux_smoke
  krun_backend_image_backed_smoke_pulls_and_boots_busybox -- --ignored
  --nocapture` passes from the installed stack
- `docs/plans/security/sandbox-isolation-audit.md` still states F4 is closed
  only for the patched paired stack

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
ldd /usr/libexec/nimbus/crun | grep /usr/libexec/nimbus/lib/libkrun
nm -D /usr/libexec/nimbus/lib/libkrun.so.1.17.4 | grep krun_set_port_map_with_bind_address
NIMBUS_KRUN_SMOKE_NON_LOOPBACK_HOST=<host-ip> cargo test -p nimbus-sandbox --test krun_linux_smoke krun_backend_image_backed_smoke_pulls_and_boots_busybox -- --ignored --nocapture
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
