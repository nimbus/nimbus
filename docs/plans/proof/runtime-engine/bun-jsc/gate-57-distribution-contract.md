# Gate 57: Bun/JSC Distribution Contract

Date: 2026-05-25

## Baseline

- Nimbus HEAD at plan start: `34a819fa` (`Plan Bun JSC adapter distribution`)
- Linked-adapter proof baseline: `dec70418` (`Close Bun JSC linked adapter plan`)
- Bun source baseline: `nimbus/bun` tag `bun-v1.4.0-nimbus.5`
- Bun source revision: `ad0e1d2bbc6690651e04f10eaf1dcdf8a6c0de57`

## Contract Decisions

The Bun/JSC adapter is an optional Nimbus runtime artifact, not a system Bun
installation and not a microVM replacement.

Default installs keep the existing single-binary behavior:

```text
nimbus binary present
Bun/JSC adapter absent
/debug/runtime/metrics reports bun_jsc not_linked
"use bun"; invocations fail closed with an install/linking diagnostic
```

Optional installs use a packaged adapter manifest:

```text
/usr/libexec/nimbus/runtime/bun-jsc/<adapter_version>/
  libnimbus_bun_jsc_embedder.so
  nimbus-bun-jsc-adapter.json
  checksums-sha256.txt
  README.md

/usr/libexec/nimbus/runtime/bun-jsc/current/
  nimbus-bun-jsc-adapter.json
```

On macOS/Homebrew the equivalent root is:

```text
<Homebrew prefix>/libexec/runtime/bun-jsc/<adapter_version>/
<Homebrew prefix>/libexec/runtime/bun-jsc/current/
```

The `current` entry may be a symlink or package-managed pointer to the selected
adapter version. Nimbus canonicalizes the manifest and library paths before
validation.

Discovery order:

1. `NIMBUS_BUN_EMBED_SHARED_LIBRARY` for development/source-proof runs
2. `NIMBUS_BUN_JSC_ADAPTER_MANIFEST` for explicit operator/test manifest runs
3. `/usr/libexec/nimbus/runtime/bun-jsc/current/nimbus-bun-jsc-adapter.json`
4. `/opt/homebrew/opt/nimbus/libexec/runtime/bun-jsc/current/nimbus-bun-jsc-adapter.json`
5. `/usr/local/opt/nimbus/libexec/runtime/bun-jsc/current/nimbus-bun-jsc-adapter.json`
6. no adapter found: report `not_linked` and fail closed

Manifest validation must reject:

- unknown schema versions or unknown fields
- wrong kind
- wrong Nimbus-supported Bun source repository, ref, or revision
- wrong target triple or platform
- wrong ABI name, version, or required export list
- unsupported memory or lifecycle policy
- missing library
- library paths that are not a single relative filename beside the manifest
- group/other-writable packaged manifest directories or files on Unix
- SHA-256 mismatches

The direct `NIMBUS_BUN_EMBED_SHARED_LIBRARY` override intentionally remains a
development/source-proof escape hatch so `make verify-bun-jsc-linked-adapter`
can continue to load freshly built Bun artifacts without first packaging them.
Packaged installs use the manifest path.

## Touched Surface Inventory

Current implementation surfaces:

- `crates/nimbus-runtime/src/backends/bun_jsc/manifest.rs`
  - strict manifest schema, checksum validation, source/ABI policy, packaged
    path discovery, and focused tests
- `crates/nimbus-runtime/src/backends/bun_jsc/linked.rs`
  - source contract moved into the manifest module and linked loading now
    resolves through manifest-aware discovery
- `crates/nimbus-runtime/src/backends/bun_jsc/mod.rs`
  - feature-gated manifest module ownership
- `crates/nimbus-runtime/build.rs`
  - tracks both direct library and manifest override env vars for linked test
    cfg

Future BJD surfaces:

- `scripts/package-bun-jsc-adapter.sh`
- `scripts/verify-bun-jsc-adapter-package.sh`
- `scripts/verify-bun-jsc-linked-adapter.sh`
- `Makefile`
- `.github/workflows/ci.yml`
- `.github/workflows/release.yml`
- `.github/workflows/linux-packages.yml`
- `scripts/build-linux-release-packages.sh`
- `scripts/install.sh`
- `docs/adapters/native/http-api.md`
- `docs/operating/ci-modernization.md`
- `docs/plans/distribution-plan.md`

## Rejected Alternatives

| Alternative | Rejection reason |
| --- | --- |
| Bundle Bun/JSC into every default Nimbus binary. | Violates the default single-binary/no-link simplicity and would make every release depend on the heavy WebKit/Bun artifact path. |
| Keep requiring `NIMBUS_BUN_EMBED_SHARED_LIBRARY` for packaged installs. | Good for source proofs, poor operator DX, and too easy to drift from package-manager-owned artifacts. |
| Search version directories dynamically. | Adds nondeterminism and upgrade ambiguity. The package manager should own the `current` pointer. |
| Load first and validate manifest later. | Weakens fail-closed behavior; source/ref/ABI/checksum mismatches must be rejected before `dlopen`. |
| Accept absolute library paths inside the manifest. | Allows manifests to redirect loading outside the package-owned adapter directory. |

## Verification

Commands run:

```sh
cargo fmt --all --check
cargo test -p nimbus-runtime --features bun-jsc-linked-adapter --lib backends::bun_jsc -- --nocapture
git diff --check
```

Results:

- `cargo fmt --all --check`: passed
- `cargo test -p nimbus-runtime --features bun-jsc-linked-adapter --lib backends::bun_jsc -- --nocapture`: passed, 25 tests
- `git diff --check`: passed

