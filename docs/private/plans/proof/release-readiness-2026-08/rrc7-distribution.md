# RRC7 Distribution Evidence

Date: 2026-09-06

Result: `RRC7_DISTRIBUTION_BLOCKED`.

The exact v0.1.46 candidate passes every distribution check that does not
publish Nimbus product state. The release archives, native Linux lifecycle,
OCI image, DEB packages, and RPM packages pass. Public apt and COPR channels
remain blocked because they need product publication and fresh public install
proofs. RRC7 did not publish a Nimbus tag, release, package, or OCI image.

## Candidate and Boundaries

- Nimbus commit:
  `7d0ca18a709d1b78e087bc4a69c8a96bee6f32b9`.
- Deno commit: `95413e012ee9f73e7f652e1e7b1ad9e351b9a8df`.
- Deno immutable release: `v2.9.6-nimbus.5`.
- rusty_v8 immutable release: `v150.4.0-nimbus.1`.
- Runtime features: `v8-pointer-compression`.
- Release version: v0.1.46.
- Target: `x86_64-unknown-linux-gnu`.
- Exact hosted CI run: `34031004029`.
- Exact hosted artifact ID: `9989541287`.
- Exact hosted artifact name:
  `linux-release-candidate-7d0ca18a709d1b78e087bc4a69c8a96bee6f32b9`.

The hosted artifact identity file records the exact Nimbus SHA, fork tags,
target, runtime feature, release profile, disabled publication, and Nimbus
version. The archive came from the full-LTO release-candidate job. Local
package and OCI checks used only that extracted archive and the immutable
crun and libkrun release inputs.

## Exact Release Archive

The `nimbus_linux_x86_64.tar.gz` archive is 89,755,826 bytes. Its SHA-256 is
`fe8707c29a6f6a9dc41c645342175db449f4871f055ec23d7bc5e2fc84f967cc`.
It contains exactly these files:

- `nimbus`
- `README.md`
- `LICENSE`

The executable is 243,093,280 bytes. Its SHA-256 is
`156f97f1f6fac96f85c091cdd5dfe582456a18b0094576d134ae0c696aa310fd`.
It reports `nimbus 0.1.46`. The archive README and license match the candidate
repository files byte for byte.

## Native Linux Lifecycle

A fresh state root on `nimbus@minicloud.local` used the extracted full-LTO
binary. The following checks pass:

1. Server start and health.
2. Reject unauthenticated access.
3. Tenant creation.
4. Schema and index creation.
5. Create, read, update, delete, and pagination operations.
6. WebSocket operation.
7. Scheduler operation.
8. Diagnostic output.
9. Graceful shutdown.
10. Restart durability from the same state root.
11. Tenant deletion and process cleanup.

The test used Node v24.16.0, which is in the supported Node 24 line. The
binary hash after transfer matched the archive hash.

## Exact OCI Image

Candidate binding: Nimbus
`7d0ca18a709d1b78e087bc4a69c8a96bee6f32b9`, Deno
`95413e012ee9f73e7f652e1e7b1ad9e351b9a8df`, and upstream baseline
`b57a2d680891de852d5576e65ccaea787b005431`.

The final local image used the exact archive with the repository Dockerfile.
Podman image ID
`a78dd6052a6382290412e7502652baf805d8bc3e5f673c7554ce5801ac8d990b`
uses Docker manifest media type
`application/vnd.docker.distribution.manifest.v2+json`. Its metadata records
v0.1.46 and the exact Nimbus SHA. Its health check is:

```text
["CMD","curl","-fsS","http://127.0.0.1:8080/health"]
```

The repository OCI smoke verifies the version. It also verifies non-root UID
and GID 10001.
It also verifies the README, license, writable state, and absence of host
development tools. Token rotation, bind, start, and health behavior pass.

A first Podman build used OCI manifest format and passed its live product
smoke. Podman correctly reported that this format cannot carry the Dockerfile
health check. The final Docker-format build closes that packaging difference.
No image push occurred.

## Exact DEB and RPM Packages

The repository package builder used nFPM v2.45.0, the same version as the
release workflow. It combined the exact Nimbus archive with checksum-verified
crun v1.29.1-nimbus.2 and libkrun v1.19.4-nimbus.3 inputs.

| Package | Bytes | SHA-256 |
|---|---:|---|
| `nimbus_0.1.46_amd64.deb` | 92,083,630 | `5b30f750fe9cfdd19ef5757f87801abf70a63ab91284f3e38f75b49b4c2029d4` |
| `nimbus-libkrun_1.19.4~nimbus.3_amd64.deb` | 10,481,250 | `e269cf7e9db7bf5d398b1fb7f52ef91dfe172ac35015ad8b36fa19200523cfc1` |
| `nimbus-crun_1.29.1~nimbus.2_amd64.deb` | 1,402,458 | `dbec5ed73fa3ca905b792e9c39970c6ec8aa77b4878cf53b0ed42c58b241c5fc` |
| `nimbus-0.1.46-1.x86_64.rpm` | 95,297,245 | `4fc473822dc25ddf137532fa7e0c91bfb7a4601ec9d9709ea9a3e9214501195f` |
| `nimbus-libkrun-1.19.4~nimbus.3-1.x86_64.rpm` | 10,963,694 | `8b487c0a905a0e8e8009f29888d09b95d560dc8560b311bf1acb9568dc090569` |
| `nimbus-crun-1.29.1~nimbus.2-1.x86_64.rpm` | 1,442,808 | `ed5732d9a6b98745560827ea48fef770d11c2c5709c5c508bf1da02674859e86` |

All six checksum entries pass after transfer to `minicloud.local`.

A fresh Debian 13 slim container installed the three DEB packages. It reported
Nimbus 0.1.46, nimbus-crun 1.29.1-nimbus.2, and nimbus-libkrun
1.19.4-nimbus.3. Debian slim excludes most `/usr/share/doc` files by base-image
policy. Package inspection proved that the DEB contains the README, license,
and copyright files. The complete check passed after removal of only that
container policy. Marker: `RRC8_DEBIAN_PACKAGE_INSTALL_PASS`.

A fresh Fedora 42 container installed the three RPM packages and reported the
same product and runtime tuple. The README and license were present. Marker:
`RRC8_FEDORA_PACKAGE_INSTALL_PASS`.

Both tests used disposable containers. The Linux host package database did
not change.

## Upgrade and Installer Comparison

Earlier RRC7 work tested the currently published channel without changing it:

- Fresh Debian 13 and Fedora 42 containers installed the public v0.1.44 tuple.
  Both containers upgraded it to public v0.1.45.
- A fresh Debian 13 container installed, verified, upgraded, and uninstalled
  the direct-install tuple.
- The live public v0.1.45 verifier passed release metadata, checksums, and
  archive and license layout. Attestations, SBOM evidence, vulnerability
  evidence, image pull, and runtime health also passed.
- Installer, version-contract, archive-layout, OCI, package, and Homebrew
  helpers pass. The installer helper contains 63 tests after its final repair.

These comparisons prove the existing public upgrade and installer contracts.
The exact v0.1.46 checks above prove the new local candidate artifacts.

## Fail-Before Repairs

RRC7 repaired verified distribution defects before the exact replay:

- The release verifier now supports the intentional no-Windows release matrix
  and rejects a stale Windows asset in that mode.
- Direct installs preserve and own the README and license.
- Same-version checks use only the requested installation prefix.
- Document verification does not borrow files from another installation
  channel.
- The release version is v0.1.46 across tracked source and scaffold locks.
- The version scan rejects malformed tracked locks without scanning unrelated
  nested dependencies.
- Changelog generation preserves older release sections.
- RRC7 removed dead verifier code and stale API constants.

The repairs include focused regressions. Bash, Dash where applicable,
ShellCheck, version, archive, package, OCI, installer, and Homebrew checks pass.

## Routed Public Work

The distribution plan owns two remaining public operations:

- D2 apt is in progress. Builders exist, but public Pages or custom-domain
  cutover and a fresh public `apt install` proof do not exist.
- D3 COPR is in progress. SRPM and package builders exist, but live COPR
  publication and a fresh public `dnf copr enable` proof do not exist.

Those proofs require a Nimbus product publication. The active release plan
explicitly excludes product publication. RRC7 did not change credentials or a
public package channel.

## RRC7 Decision

Candidate binding: Nimbus
`7d0ca18a709d1b78e087bc4a69c8a96bee6f32b9`, Deno
`95413e012ee9f73e7f652e1e7b1ad9e351b9a8df`, and upstream baseline
`b57a2d680891de852d5576e65ccaea787b005431`.

`release_archives` is `pass`. `oci_image` is `pass`. `install_channels`
remains `blocked` only on the public apt and COPR proofs.

The exact candidate has no verified local archive, OCI, DEB, RPM, installer,
or upgrade defect. RRC7 is terminal and blocked on external publication state,
not on more local implementation or testing.
