# RRC7 Distribution Evidence

Date: 2026-08-28

Result: provisional pass for every locally testable release artifact. RRC7
remains blocked because the clean v0.1.46 candidate cannot build until the
RRC1 Deno WebSocket egress commits have reachable immutable references. The
public apt and COPR install proofs also remain open in the distribution plan.
No tag, release, package channel, or other public artifact was published.

## Inputs and Boundaries

- Nimbus campaign branch: `codex/release-readiness-2026-08` at
  `0c8bdd363` after the RRC7 repairs.
- Intended next release: v0.1.46. The source, Rust workspace packages,
  JavaScript packages, checked-in application locks, and changelog use
  version 0.1.46.
- Latest public stable release: v0.1.45, published 2026-07-06. No v0.1.46
  tag or release exists.
- Provisional integrated macOS binary:
  `/private/tmp/nimbus-release-candidate-875c1dc65b4d/nimbus`, version
  0.1.45, SHA-256
  `875c1dc65b4dec6a72fda5518628b0c417bb9c3416bf0ed7ab93f6c57cf0df0f`.
- Linux distribution comparison: real packages built from public v0.1.44
  and v0.1.45 assets, then installed and upgraded in fresh Debian 13 and
  Fedora 42 containers on `nimbus@minicloud`.
- Current supported release matrix: Linux x86_64 and arm64 archives plus
  macOS arm64. Windows is intentionally absent until its runtime path returns
  to the supported release matrix.

The v0.1.44-to-v0.1.45 comparison proves the current published distribution
machinery and upgrade contracts. It does not substitute for an exact v0.1.46
candidate replay.

## Fail-Before and Repairs

1. The published-release verifier required a Windows ZIP even though the
   supported release matrix intentionally removed Windows. Commits
   `dfea25523` and `52b8fc93b` add an explicit `--skip-windows` contract,
   require the obsolete asset to be absent in that mode, and cover the
   supported and stale-asset cases.
2. Direct `curl | sh` installs discarded the archive `LICENSE` and
   `README.md`. The installer now owns both files under
   `${NIMBUS_PREFIX}/share/doc/nimbus`, validates them after install, heals
   older same-version installs that lack them, and removes only its owned
   document directory during uninstall.
3. The installer used any `nimbus` on `PATH` for its same-version check. A
   separate package channel could therefore suppress installation into the
   requested prefix. The check now uses only
   `${NIMBUS_PREFIX}/bin/nimbus`.
4. The macOS install verifier ignored a valid custom-prefix binary. Both the
   standalone and embedded verifier now prefer the requested prefix.
5. Document verification could borrow license files from another package
   channel after it had selected a prefix-owned binary. Commit `e771e5fac`
   makes that mixed-ownership state fail closed. Package and Homebrew document
   fallbacks remain available only when the requested prefix does not own the
   binary.
6. The repository still identified itself as v0.1.45 although that release
   already exists. The release inputs now use the unused v0.1.46 version. The
   version contract checks all Git-tracked `.nimbus/packages` scaffold locks,
   ignores unrelated nested dependencies, ignores untracked files, and emits
   a named mismatch for a malformed tracked lock. Commits `7e9a48ad6` and
   `0c8bdd363` contain the final scope rules.
7. An initial changelog regeneration collapsed historical release headings.
   The repair preserves every older heading and adds only the cumulative
   v0.1.46 section.
8. Dead release verifier helpers and stale API constants obscured the active
   contract. They were removed. All affected shell scripts pass Bash, Dash
   where applicable, and ShellCheck parsing.

## Deterministic Verification

| Check | Result |
|---|---|
| `make verify-install-helper` | pass, 57 of 57 tests |
| `make verify-release-version-contract VERSION=v0.1.46` | pass |
| `make verify-release-archive-layout-helper` | pass |
| `make verify-release-oci-image-helper` | pass |
| `make verify-release-oci-image-live-helper` | pass; supported no-Windows bundle has 12 attestations and a stale Windows asset is rejected |
| `make verify-release-oci-image-build-helper` | pass with the real container build path |
| `make verify-build-linux-release-packages-helper` | pass with `nfpm` 2.47.0 |
| Homebrew cask proof helper | pass |
| Bash syntax for changed scripts | pass |
| Dash syntax for `scripts/install.sh` | pass |
| ShellCheck for changed scripts | pass with no diagnostics |
| `git diff --check` | pass |

Adversarial version-contract probes also passed. An untracked malformed lock
was ignored. A malformed Git-tracked scaffold lock failed with a named
`mismatch:` instead of a Python stack trace. An upstream
`.nimbus/packages/convex/node_modules/esbuild` version did not cause a false
Nimbus-version failure.

## Published Release and OCI Evidence

The complete live verifier ran against public v0.1.45 without a skip:

```text
make verify-release-oci-image-live \
  TAG=v0.1.45 \
  OUTPUT_DIR=/private/tmp/nimbus-release-readiness-v0.1.45-oci-live-skip-windows \
  RUNTIME=docker \
  SKIP_WINDOWS=1
```

It verified release metadata, checksums, archive and license layouts, absent
optional Bun/JSC assets by policy, release-asset attestations, OCI attestation,
SBOM evidence, vulnerability evidence, image pull, and the runtime health
smoke. The published multi-architecture digest was
`sha256:6ac752708fb67ca817264f8686f945e95770c70355c475695d892096e22724ac`.
The published Linux x86_64 Nimbus binary SHA-256 was
`0b693f9fb00b6f6e4991bf870337ab2aa3d80c25fd9733fe58f08d32c3243cae`.

The current runtime tuple was also downloaded and checksum-verified for both
architectures:

- `nimbus/nimbus-crun` v1.27.1-nimbus.2; amd64 SHA-256
  `401ff1076ff0f34d7c0d367bbe7269b0df937a904be5102707838e0a0deca43`.
- `nimbus/nimbus-libkrun` v1.18.1-nimbus.1; amd64 archive SHA-256
  `a277ac30676cb32812f574dc91e598a2594bdb400173458afaa48d63e8854e11`.

The final release-tuple replay supersedes those earlier fork versions for the
candidate. Commit `fb56b7816bc29e67b1973370feefdbfae03d860a` binds crun
`v1.29.1-nimbus.2`, libkrun `v1.19.4-nimbus.3`, and libkrunfw 5.5.0.

The repository helpers passed for all release-tuple surfaces. These surfaces
include the Krun bundle, both drills, the Linux validation bundle, both package
formats, the apt repository, Fedora SRPMs, and the installer. The installer
helper passed 63 tests. The package helpers built, rebuilt, installed, and
queried x86_64 and aarch64 artifacts. The online fork gate verified both
release tags and the current libkrunfw companion release.

## Linux Packages and Upgrade Evidence

The repository builder produced real Nimbus, nimbus-crun, and
nimbus-libkrun DEB and RPM packages for v0.1.44 and v0.1.45. Their checksum
manifests passed locally and after transfer to `nimbus@minicloud`.

In a disposable Debian 13 container, the raw package payload had the required
license, README, and copyright files. The official slim image had a general
dpkg documentation exclusion; removing that container-only policy restored
the payload during install. The complete v0.1.44 tuple installed, reported
`+LIBKRUN`, upgraded Nimbus to v0.1.45, and passed version, help, and package
queries. Marker: `RRC7_DEBIAN_UPGRADE_PASS`.

In a disposable Fedora 42 container, license metadata and payload checks
passed. The v0.1.44 tuple installed, reported `+LIBKRUN`, and upgraded to
v0.1.45. Marker: `RRC7_FEDORA_UPGRADE_PASS`.

Both test containers were removed. The Linux host package database was not
changed.

## Direct Installer Evidence

The repaired install and verification scripts ran in a disposable Debian 13
container on `nimbus@minicloud`:

1. Install the public v0.1.44 Nimbus, nimbus-libkrun, and nimbus-crun tuple.
2. Verify the prefix-owned binary and both documents.
3. Run `verify-install.sh` with zero failures. Its three warnings were
   environmental: no KVM, NetworkManager, or `readelf` in the minimal
   container.
4. Upgrade to v0.1.45 and verify the version and documents again.
5. Uninstall and prove removal of the binary, runtime tuple, and installer
   document directory.

Marker: `RRC7_DIRECT_INSTALL_UPGRADE_UNINSTALL_PASS`.

The minimal container did not include GitHub CLI, so the direct installer
could only warn about release attestation there. The independent live OCI
verifier checked the same public release attestations. This environmental
warning is not promoted to a pass for an untested path.

## Independent Review

Opus 5 reviewed the full RRC7 branch slice from `2a7279e19` through
`0c8bdd363` in repeated repair loops.

- The first review found five P2/P3 issues: collapsed changelog history,
  overbroad parent-directory removal, version-scan scope, dead verifier code,
  and package fixture coverage. Verified findings were repaired.
- Later reviews found prefix document borrowing, missing negative verifier
  coverage, untracked/malformed lock handling, and nested dependency false
  positives. Each verified finding received a regression and repair.
- The final review accepted no actionable finding. It explicitly checked
  version coherence, Git-tracked scaffold lock semantics, prefix and document
  ownership, DEB/RPM and Homebrew contracts, the supported no-Windows release
  path, and the concrete tests.
- Secret scanning was clean on every review pass.

## Routed Public Work

The distribution plan remains the owner of public channel actions:

- D1 direct install and D6 OCI are complete for the current published release.
- D2 apt is in progress. Package and signed-repository builders exist, but
  public Pages/custom-domain cutover and a fresh public `apt install` proof
  remain.
- D3 COPR is in progress. SRPM and package builders exist, but live COPR
  publication and a fresh public `dnf copr enable` install proof remain.
- D4 Homebrew and machine distribution are complete for the current published
  release.
- D5 cloud images remain todo.

RRC7 did not publish, tag, push, change credentials, or alter a public package
channel.

## RRC7 Decision

Every locally testable RRC7 artifact and upgrade path has a provisional pass.
The release is still blocked for two independent reasons:

1. The exact v0.1.46 candidate cannot build until the RRC1 Deno commits have
   reachable immutable references. All current live candidate work therefore
   uses either the preserved provisional binary or public v0.1.44/v0.1.45
   release assets.
2. Public apt and COPR channel installation has no evidence yet. Those actions
   require publication authority and remain routed to the distribution plan.

The final RRC8 verdict must remain NO-GO unless these blocked conditions and
every other red matrix condition gain direct evidence.
