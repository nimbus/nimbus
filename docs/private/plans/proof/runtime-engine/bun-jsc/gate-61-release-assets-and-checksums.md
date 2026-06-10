# Gate 61: Bun/JSC Release Assets And Checksums

Date: 2026-05-25

## Scope

This proof closes `BJD4` of
`docs/plans/archive/bun-jsc-distribution-and-release-plan.md`.

The goal is to integrate optional Bun/JSC adapter archives into release asset
verification and publication without making every default Nimbus tag release
build Bun/WebKit.

## Release Shape

The default `Release` workflow remains the single-binary release owner. It now
runs `scripts/verify-bun-jsc-release-assets.sh --artifacts-dir artifacts`
before checksums are generated. When no adapter archives are present, the
verifier records that optional Bun/JSC assets are absent by policy. If adapter
archives are staged into the release artifact directory later, the same
verifier validates each archive before release creation.

The release checksum step now includes any staged
`nimbus-bun-jsc-adapter-*.tar.gz` files in `checksums-sha256.txt` when they are
present.

The heavy adapter workflow remains separate:

- `.github/workflows/bun-jsc-adapter.yml`
- `publish_release_assets=false` by default
- `publish_release_assets=true` downloads the Linux x86_64 and macOS arm64
  adapter artifacts from the workflow run
- the publish job generates
  `nimbus-bun-jsc-adapter-checksums-sha256.txt`
- `scripts/verify-bun-jsc-release-assets.sh` requires both platforms and
  verifies the generated checksum file before upload
- `actions/attest@v4` attests the adapter archives and adapter checksum file
- `gh release upload --clobber` publishes the optional adapter assets to the
  selected `v*` release tag

This gives operators an explicit same-tag adapter release path while keeping
the default release fast and deterministic.

## Verifier Contract

`scripts/verify-bun-jsc-release-assets.sh` owns release-asset validation:

- accepts absent optional assets when no platform is required
- rejects a missing required platform
- maps archive names to target triples
- rejects unknown adapter platforms
- delegates archive validation to
  `scripts/verify-bun-jsc-adapter-package.sh`
- verifies release-level checksums when a checksum file is provided

`scripts/verify-bun-jsc-release-assets-helper.sh` proves the contract with
fixture archives and a fake `nm` command.

## Verification

Recorded local checks:

```text
bash -n scripts/verify-bun-jsc-release-assets.sh \
  scripts/verify-bun-jsc-release-assets-helper.sh \
  scripts/build-bun-jsc-adapter-artifacts.sh \
  scripts/verify-bun-jsc-linked-adapter.sh

make verify-bun-jsc-release-assets

ruby -e 'require "yaml"; YAML.load_file(".github/workflows/bun-jsc-adapter.yml"); YAML.load_file(".github/workflows/release.yml"); YAML.load_file(".github/workflows/ci.yml"); puts "yaml-ok"'

git diff --check
```
