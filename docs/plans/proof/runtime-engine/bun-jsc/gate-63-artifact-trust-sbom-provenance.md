# Gate 63: Artifact Trust, SBOM, And Provenance Evidence

Date: 2026-05-25

Plan gate: `BJD6` in `docs/plans/archive/bun-jsc-distribution-and-release-plan.md`

## Result

`BJD6` is complete for the current adapter artifact contract.

The Bun/JSC adapter archive now includes, by default:

- `checksums-sha256.txt`
- `nimbus-bun-jsc-adapter.sbom.cdx.json`
- `nimbus-bun-jsc-adapter.intoto.jsonl`

The generated SBOM is a minimal CycloneDX document that identifies the adapter
shared library, its SHA-256, the Bun source repository/ref/revision, the Nimbus
version, and the target triple. It is evidence of artifact contents and source
coordinates, not a vulnerability, license, or dependency-completeness claim.

The generated provenance file is a deterministic in-toto statement using the
SLSA provenance v1 predicate. It binds the adapter shared-library SHA-256 to
the Bun source coordinates and the `bun-jsc-adapter.yml` builder identity. It
is release-attested by GitHub Actions when published. Nimbus still does not
implement DSSE, Fulcio/Rekor, SLSA, or signature cryptography; those remain in
the artifact-provenance/Cosign/SLSA verifier seams.

## Enforcement

Production-enforced now:

- Package verifier rejects unsafe archive entries, duplicates, missing evidence,
  unknown manifest fields, checksum mismatches, malformed SBOM shape, malformed
  SLSA statement shape, wrong SLSA subject digest, wrong export set, native
  symbol leaks, `TEXTREL`, and `STATIC_TLS`.
- Runtime packaged-manifest discovery requires provenance metadata, requires
  the checksum file beside the manifest, requires SBOM and SLSA evidence files,
  verifies their SHA-256 entries, and still verifies the shared-library SHA-256
  before loading.
- Linux package staging and direct installer preserve the SBOM/provenance files
  beside `nimbus-bun-jsc-adapter.json`.
- Direct Linux install verifies release checksums/attestation, rejects unsafe
  tar layouts, verifies archive-internal checksums, and installs the evidence
  files into the packaged discovery directory.

Release-attested now:

- `bun-jsc-adapter.yml` attests the adapter archives and adapter checksum file
  before GitHub Release upload.
- The adapter archive contains the minimal SBOM and in-toto/SLSA statement that
  the package verifier checks before upload or package consumption.

Deferred to the artifact-provenance plan/tooling lanes:

- Cosign signature verification of adapter archives as standalone release
  assets.
- SLSA verifier validation of an externally signed statement or DSSE envelope.
- SBOM completeness, license policy, and vulnerability policy.

## Verification

```sh
cargo fmt --all --check
```

Passed with no output.

```sh
bash -n scripts/build-bun-jsc-adapter-artifacts.sh \
  scripts/package-bun-jsc-adapter.sh \
  scripts/verify-bun-jsc-adapter-package.sh \
  scripts/verify-bun-jsc-adapter-package-helper.sh \
  scripts/verify-bun-jsc-release-assets.sh \
  scripts/verify-bun-jsc-release-assets-helper.sh \
  scripts/build-linux-release-packages.sh \
  scripts/verify-build-linux-release-packages-helper.sh \
  scripts/install.sh \
  scripts/verify-install-helper.sh
```

Passed with no output.

```sh
cargo test -p nimbus-runtime --features bun-jsc-linked-adapter --lib backends::bun_jsc -- --nocapture
```

Output summary:

```text
test result: ok. 29 passed; 0 failed; 0 ignored; 0 measured; 501 filtered out
```

```sh
bash scripts/verify-bun-jsc-adapter-package-helper.sh
```

Output:

```text
verified: Bun/JSC adapter package helper accepts a good fixture with SBOM/provenance and rejects missing library, bad checksum, missing evidence, bad evidence checksum, wrong provenance subject, bad manifest, wrong exports, and native leaks
```

```sh
bash scripts/verify-bun-jsc-release-assets-helper.sh
```

Output:

```text
verified: Bun/JSC release asset helper accepts absent-optional and good assets with SBOM/provenance, rejects missing required assets, bad release checksums, unknown platforms, and tampered adapter packages
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
bash scripts/verify-artifact-provenance.sh
```

Output summary:

```text
artifact provenance verification gate: pass
```

The gate reported 41 artifact provenance fixtures, 1 runtime invocation
provenance fixture, 14 image admission fixtures, 1 operator SBOM policy hook
fixture, and 6 production Compose admission fixtures.

```sh
make proof-helpers
```

Run with approved sandbox escalation because the existing Homebrew/machine
helper uses a local Unix socket. Output included:

```text
verified: Bun/JSC adapter package helper accepts a good fixture with SBOM/provenance and rejects missing library, bad checksum, missing evidence, bad evidence checksum, wrong provenance subject, bad manifest, wrong exports, and native leaks
verified: Bun/JSC release asset helper accepts absent-optional and good assets with SBOM/provenance, rejects missing required assets, bad release checksums, unknown platforms, and tampered adapter packages
verified: linux package builder rendered deterministic nimbus/nimbus-libkrun/nimbus-crun/nimbus-bun-jsc-adapter deb/rpm manifests (nfpm not installed; package build skipped)
verified: install script helper passed 32 tests
```

```sh
make verify-bun-jsc-runtime-contract
```

The sandboxed run reached the runtime metrics API slice and failed because the
server fixture could not bind a local listener:

```text
listener should bind: Os { code: 1, kind: PermissionDenied, message: "Operation not permitted" }
```

The command was rerun with approved sandbox escalation and passed:

```text
Bun/JSC runtime contract gate: pass
```
