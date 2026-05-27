# Gate 62: Install And Package Surfaces

Date: 2026-05-25

Plan gate: `BJD5` in `docs/plans/archive/bun-jsc-distribution-and-release-plan.md`

## Result

`BJD5` is complete for the current supported distribution shape:

- Linux package builders can stage `nimbus-bun-jsc-adapter` as a separate
  optional package from a verified
  `nimbus-bun-jsc-adapter-linux-x86_64.tar.gz` release archive.
- The package installs the adapter under
  `/usr/libexec/nimbus/runtime/bun-jsc/<adapter_version>/` and points
  `/usr/libexec/nimbus/runtime/bun-jsc/current` at that version.
- The optional package depends on `nimbus`, but `nimbus` does not depend on the
  optional adapter package. Default installs therefore keep the Bun/JSC lane in
  `not_linked` unless the operator explicitly opts in.
- The release-driven Linux mirror has an explicit
  `include_bun_jsc_adapter` control. Release events use the repository
  variable `NIMBUS_INCLUDE_BUN_JSC_ADAPTER_PACKAGES=true`; manual dispatches
  use the matching workflow input.
- The apt repository workflow can include the optional amd64 adapter package
  when the release asset and adapter checksum file exist.
- `scripts/install.sh --with-bun-jsc` installs the Linux x86_64 adapter from
  the matching Nimbus release asset, verifies the release checksum, verifies
  GitHub artifact attestation when `gh` is available or required, rejects
  unsafe/unexpected tar entries, verifies the archive's internal checksums, and
  installs the packaged discovery layout without requiring
  `NIMBUS_BUN_EMBED_SHARED_LIBRARY`.
- macOS Homebrew/cask does not install the adapter yet. The installer warns
  when `--with-bun-jsc` is requested on macOS and points operators at the
  release asset/package lane for the same tag. Runtime discovery already has
  the Homebrew layout reserved for the future package/cask payload.
- `scripts/verify-install.sh` reports optional Bun/JSC adapter state on Linux
  and macOS so operators can distinguish "absent optional" from a broken
  installed adapter.

## Files

- `.github/workflows/linux-packages.yml`
- `.github/workflows/apt-repo.yml`
- `.github/workflows/linux-distribution-release.yml`
- `.github/workflows/ci.yml`
- `Makefile`
- `scripts/build-linux-release-packages.sh`
- `scripts/install.sh`
- `scripts/verify-build-linux-release-packages-helper.sh`
- `scripts/verify-install.sh`
- `scripts/verify-install-helper.sh`
- `docs/operating/ci-modernization.md`
- `docs/plans/distribution-plan.md`

## Verification

```sh
bash -n scripts/build-linux-release-packages.sh \
  scripts/verify-build-linux-release-packages-helper.sh \
  scripts/install.sh \
  scripts/verify-install.sh \
  scripts/verify-install-helper.sh \
  scripts/package-bun-jsc-adapter.sh \
  scripts/verify-bun-jsc-adapter-package.sh \
  scripts/verify-bun-jsc-release-assets.sh
```

Passed with no output.

```sh
dash -n scripts/install.sh
```

Passed with no output.

```sh
ruby -e 'require "yaml"; %w[.github/workflows/linux-packages.yml .github/workflows/apt-repo.yml .github/workflows/linux-distribution-release.yml .github/workflows/ci.yml].each { |path| YAML.load_file(path) }; puts "yaml-ok"'
```

Output:

```text
yaml-ok
```

```sh
bash scripts/verify-build-linux-release-packages-helper.sh
```

Output:

```text
verified: linux package builder rendered deterministic nimbus/nimbus-libkrun/nimbus-crun/nimbus-bun-jsc-adapter deb/rpm manifests (nfpm not installed; package build skipped)
```

```sh
bash scripts/verify-install-helper.sh
```

Output:

```text
verified: install script helper passed 32 tests
```

```sh
bash scripts/verify-bun-jsc-release-assets-helper.sh
```

Output:

```text
verified: Bun/JSC release asset helper accepts absent-optional and good assets, rejects missing required assets, bad release checksums, unknown platforms, and tampered adapter packages
```

```sh
make proof-helpers
```

The first sandboxed run reached the existing Homebrew cask proof helper and
failed when the helper tried to use its local Unix socket:

```text
PermissionError: [Errno 1] Operation not permitted
```

The command was rerun with approved sandbox escalation and passed. The final
output included:

```text
verified: nimbus homebrew cask proof helper captures the packaged macOS release-asset contract deterministically
verified: Bun/JSC adapter package helper accepts a good fixture with SBOM/provenance and rejects missing library, bad checksum, missing evidence, bad evidence checksum, wrong provenance subject, bad manifest, wrong exports, and native leaks
verified: Bun/JSC release asset helper accepts absent-optional and good assets with SBOM/provenance, rejects missing required assets, bad release checksums, unknown platforms, and tampered adapter packages
verified: linux package builder rendered deterministic nimbus/nimbus-libkrun/nimbus-crun/nimbus-bun-jsc-adapter deb/rpm manifests (nfpm not installed; package build skipped)
verified: install script helper passed 32 tests
```

## Open Follow-Up

- macOS cask packaging still intentionally uses the existing release asset lane.
  A future Homebrew-specific slice can stage the adapter under
  `$(brew --prefix)/opt/nimbus/libexec/runtime/bun-jsc/current/` once we are
  ready to make Bun/JSC packaging available through the tap.
- COPR/SRPM publication does not yet publish a separate adapter SRPM. The
  shared package builder can render RPM manifests for the adapter, but the live
  Fedora channel should get its own proof before we claim public `dnf install`
  adapter support.
