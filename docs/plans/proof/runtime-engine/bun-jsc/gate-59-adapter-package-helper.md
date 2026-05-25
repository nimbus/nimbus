# Gate 59: Bun/JSC Adapter Package Helper

Date: 2026-05-25

## What Changed

BJD2 added a deterministic local packaging and verification helper for the
optional Bun/JSC shared adapter artifact.

New scripts:

- `scripts/bun-jsc-adapter-contract.sh`
  - shared shell-side source/ref/revision, ABI, export, memory, lifecycle, and
    file-name contract
- `scripts/package-bun-jsc-adapter.sh`
  - packages an existing `libnimbus_bun_jsc_embedder.{so,dylib}` into the
    release archive layout with `nimbus-bun-jsc-adapter.json`,
    `checksums-sha256.txt`, and `README.md`
  - does not build Bun/WebKit
  - optionally includes SBOM and SLSA/in-toto files when provided
- `scripts/verify-bun-jsc-adapter-package.sh`
  - extracts an adapter archive and verifies manifest schema, source contract,
    target/platform, library SHA-256, checksums, exact dynamic exports, and
    native symbol leak policy
- `scripts/verify-bun-jsc-adapter-package-helper.sh`
  - deterministic fixture gate using a fake `nm` command so CI can verify the
    package helper without compiling WebKit/Bun

`scripts/verify-bun-jsc-linked-adapter.sh` now sources the shared contract
instead of owning its own independent export/source list.

`Makefile` exposes:

```sh
make verify-bun-jsc-adapter-package
```

## Fixture Coverage

The helper gate proves that a good package is accepted and these failure modes
are rejected:

- missing library
- tampered library / bad checksum
- bad manifest with updated checksum
- wrong dynamic export set
- leaked native implementation symbol

The fixture uses a text-file library and fake `nm` output. Real release lanes
still audit actual shared objects with the system `nm` and, on Linux, `readelf`
for `TEXTREL` and `STATIC_TLS`.

## Verification

Commands run:

```sh
bash -n scripts/bun-jsc-adapter-contract.sh scripts/package-bun-jsc-adapter.sh scripts/verify-bun-jsc-adapter-package.sh scripts/verify-bun-jsc-adapter-package-helper.sh scripts/verify-bun-jsc-linked-adapter.sh
make verify-bun-jsc-adapter-package
git diff --check
```

Results:

- shell syntax check: passed
- `make verify-bun-jsc-adapter-package`: passed
- `git diff --check`: passed

