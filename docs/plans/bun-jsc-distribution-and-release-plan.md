# Plan: Bun/JSC Adapter Distribution And Release

This plan owns the productization wave after
`docs/plans/bun-jsc-linked-adapter-plan.md`.

The linked adapter wave proved that Bun/JSC can run as an optional in-process
runtime backend beside the existing Deno/V8/Node-compatible backend. This plan
turns that proof into a distributable, verifiable, operator-understandable
optional runtime artifact while preserving Nimbus' default single-binary
experience.

## Status

- **Status:** active; `BJD0` through `BJD4` complete, `BJD5` next
- **Primary owner:** this plan
- **Nimbus baseline:** `dec70418` (`Close Bun JSC linked adapter plan`)
- **Bun source baseline:** `nimbus/bun` tag `bun-v1.4.0-nimbus.5`
  (`ad0e1d2bbc6690651e04f10eaf1dcdf8a6c0de57`)
- **Predecessor:** `docs/plans/bun-jsc-linked-adapter-plan.md`
- **Parent distribution plan:** `docs/plans/distribution-plan.md`
- **Provenance parent plan:** `docs/plans/artifact-provenance-verification-plan.md`
- **Default posture:** the normal Nimbus binary remains usable without Bun/JSC
  and reports the Bun/JSC lane as `not_linked` unless a verified shared adapter
  artifact is installed and discovered

Progress state is this plan's phase ledger, proof docs under
`docs/plans/proof/runtime-engine/bun-jsc/`, and focused local git commits.
Update this plan before any context loss, stop, or commit that changes BJD
scope.

## Goal

Ship Bun/JSC as an optional runtime backend that can be installed, discovered,
audited, and verified like the rest of the Nimbus release graph.

The final product shape should be:

```text
default Nimbus install
  -> nimbus binary works without Bun/JSC artifact
  -> /debug/runtime/metrics reports bun_jsc not_linked

Nimbus install with Bun/JSC adapter artifact
  -> packaged adapter manifest is discovered from a canonical location
  -> shared adapter is loaded explicitly and locally
  -> Bun/JSC lane reports linked
  -> "use bun"; functions execute through the Bun/JSC pool
  -> HostBridge, tenant identity, cancellation, teardown, and memory policy
     retain the BJA guarantees
```

This is not "Bun in a microVM." Bun in an OCI image remains a sandbox workload
mode already covered by the sandbox service stack. This plan is only for the
in-process optional shared adapter path.

## Non-Goals

- Do not make Bun/JSC the default JavaScript runtime.
- Do not require every developer, default PR, or default release build to build
  WebKit/Bun.
- Do not make the default binary depend on an unverified local Bun checkout.
- Do not silently load arbitrary shared libraries from tenant-controlled paths.
- Do not solve Windows support in this wave unless a separate proof lane is
  explicitly promoted.
- Do not duplicate the distribution plan's ownership of install script, Linux
  packages, Homebrew/cask, apt, COPR, or release archives. This plan supplies
  the Bun/JSC adapter artifact contract consumed by those channels.
- Do not hand-roll cryptographic signature, SLSA, SBOM, or Sigstore logic.
  Reuse the artifact-provenance seams and battle-tested tooling choices from
  `docs/plans/artifact-provenance-verification-plan.md`.

## Baseline Facts

- `make verify-bun-jsc-linked-adapter` already verifies:
  - the default no-link contract
  - the opt-in `nimbus-runtime/bun-jsc-linked-adapter` feature
  - exact Bun source ref and revision
  - the source-owned shared adapter build
  - exported Nimbus C ABI symbols
  - no leaked native defined symbols
  - Linux simdutf namespace separation
  - same-process V8 plus Bun/JSC tests
  - HostBridge allow, deny, forged-context, cancellation, teardown, and
    diagnostics tests
- `.github/workflows/ci.yml` runs the lightweight `make
  verify-bun-jsc-runtime-contract` lane by default and syntax-checks the heavy
  Bun proof scripts.
- `.github/workflows/release.yml` already owns release archive creation,
  checksums, GitHub artifact attestation, and upload.
- `scripts/verify-release-archive-layout.sh` already asserts the released
  archive layout.
- `scripts/build-linux-release-packages.sh` already stages Linux package
  payloads under `/usr/bin`, `/usr/libexec/nimbus`, and `/usr/share/doc`.
- The distribution plan already owns install script, Linux packages, Homebrew,
  apt/COPR mirrors, binary archives, container images, and cloud VM images.

## Product Contract

### Artifact Naming

The adapter is a Nimbus-owned optional runtime artifact, not a system Bun
installation. The release archive names should be stable and platform-scoped:

```text
nimbus-bun-jsc-adapter-linux-x86_64.tar.gz
nimbus-bun-jsc-adapter-darwin-arm64.tar.gz
```

The installed layout should be package-manager friendly and avoid global
library paths:

```text
/usr/libexec/nimbus/runtime/bun-jsc/<adapter_version>/
  libnimbus_bun_jsc_embedder.so
  nimbus-bun-jsc-adapter.json
  checksums-sha256.txt
  README.md
/usr/libexec/nimbus/runtime/bun-jsc/current/
  nimbus-bun-jsc-adapter.json

<Homebrew prefix>/libexec/runtime/bun-jsc/<adapter_version>/
  libnimbus_bun_jsc_embedder.dylib
  nimbus-bun-jsc-adapter.json
  checksums-sha256.txt
  README.md
<Homebrew prefix>/libexec/runtime/bun-jsc/current/
  nimbus-bun-jsc-adapter.json
```

Development can keep an explicit override environment variable, but packaged
installs must not require users to set it by hand. The `current` entry is a
package-manager-owned pointer to the selected adapter version.

### Manifest Shape

Each adapter archive must include a machine-readable manifest. The manifest
must be strict enough that Nimbus can reject unsupported or mismatched
artifacts before `dlopen`:

```json
{
  "schema_version": 1,
  "kind": "nimbus.bun_jsc.adapter",
  "adapter_version": "v0.1.0-bun-v1.4.0-nimbus.5",
  "nimbus_version": "v0.1.0",
  "bun_source_repository": "https://github.com/nimbus/bun",
  "bun_source_ref": "bun-v1.4.0-nimbus.5",
  "bun_source_revision": "ad0e1d2bbc6690651e04f10eaf1dcdf8a6c0de57",
  "target_triple": "x86_64-unknown-linux-gnu",
  "platform": "linux",
  "library": "libnimbus_bun_jsc_embedder.so",
  "library_sha256": "<sha256>",
  "abi": {
    "name": "nimbus-bun-jsc-embedder",
    "version": 1,
    "required_exports": [
      "nimbus_bun_embed_api_version",
      "nimbus_bun_embed_execute"
    ]
  },
  "memory_enforcement": "outer_quota_required",
  "lifecycle": "fresh_discard",
  "provenance": {
    "sbom": "nimbus-bun-jsc-adapter.sbom.cdx.json",
    "slsa": "nimbus-bun-jsc-adapter.intoto.jsonl",
    "checksum_file": "checksums-sha256.txt"
  }
}
```

The exact required export list should be generated from the current BJA gate,
not maintained in two unrelated places.

### Discovery And Trust

Runtime discovery must be deterministic:

1. explicit development override, currently `NIMBUS_BUN_EMBED_SHARED_LIBRARY`
2. explicit manifest override for tests or operators
3. packaged system location
4. packaged Homebrew location
5. no adapter found, report `not_linked` and fail closed on Bun/JSC invocation

Nimbus must reject:

- manifests with unknown schema versions
- unsupported platform or target triples
- source refs/revisions that do not match the compiled Nimbus contract
- missing or mismatched SHA-256 checksums
- missing required exports
- extra exported native symbols that violate the BJA leak policy
- artifacts writable by tenant-controlled users or located under
  tenant-controlled roots

Provenance and SBOM evidence must be wired through the artifact-provenance
seams where possible. The first implementation may use local fixture
verification for reproducibility, but the final release lane must have a clear
path to Sigstore/SLSA evidence and release attestation.

## Execution Phases

| Gate | Status | Goal | Verifiable success criteria |
| --- | --- | --- | --- |
| BJD0 | `done` | Freeze the release-artifact contract and inventory every touched distribution surface. | `docs/plans/proof/runtime-engine/bun-jsc/gate-57-distribution-contract.md` records exact archive names, installed paths, manifest fields, discovery order, owner file inventory, current HEAD, Bun tag/revision, and rejected alternatives. `cargo fmt --all --check`, `cargo test -p nimbus-runtime --features bun-jsc-linked-adapter --lib backends::bun_jsc -- --nocapture` (25 tests), and `git diff --check` passed. |
| BJD1 | `done` | Implement a typed adapter manifest parser and runtime discovery contract. | `crates/nimbus-runtime/src/backends/bun_jsc/manifest.rs` validates packaged manifests before shared-library loading. Tests cover valid packaged manifests, dev override behavior, manifest override behavior, packaged discovery, missing artifact fallback, schema mismatch, wrong Bun revision, wrong target triple, checksum mismatch, unsafe path ownership/location, unknown fields, and unsupported memory/lifecycle policy. Default builds still report `not_linked` without env vars. Evidence is recorded in `docs/plans/proof/runtime-engine/bun-jsc/gate-58-adapter-manifest-discovery.md`. |
| BJD2 | `done` | Add a deterministic local packaging helper for existing shared adapter artifacts. | `scripts/package-bun-jsc-adapter.sh` packages an existing `libnimbus_bun_jsc_embedder.{so,dylib}` into the archive layout with manifest, checksums, README, and optional SBOM/provenance files. `scripts/verify-bun-jsc-adapter-package.sh` verifies archive layout, manifest contract, checksums, exports, and native symbol leak policy. `scripts/verify-bun-jsc-adapter-package-helper.sh` accepts a good fixture and rejects missing library, bad checksum, bad manifest, wrong exports, and native symbol leaks without a full WebKit rebuild. Evidence is recorded in `docs/plans/proof/runtime-engine/bun-jsc/gate-59-adapter-package-helper.md`. |
| BJD3 | `done` | Add release/CI lanes for Linux and macOS adapter artifacts without slowing default PR CI. | CI keeps `make verify-bun-jsc-runtime-contract` in default PR lanes. `.github/workflows/bun-jsc-adapter.yml` adds a manual source-backed artifact lane for Linux x86_64 and macOS arm64 from `nimbus/bun` tag `bun-v1.4.0-nimbus.5`; each lane runs `scripts/build-bun-jsc-adapter-artifacts.sh` and uploads adapter archives, manifests, checksums, and proof logs. `.github/workflows/ci.yml` syntax-checks the Bun/JSC adapter scripts and runs `bash scripts/verify-bun-jsc-adapter-package-helper.sh` in default proof-helper CI. Evidence is recorded in `docs/plans/proof/runtime-engine/bun-jsc/gate-60-adapter-release-ci-lane.md`. |
| BJD4 | `done` | Integrate release archive checksums and GitHub release upload. | `.github/workflows/release.yml` now verifies optional Bun/JSC adapter assets when present and records absent assets as intentional by policy. Release checksum generation includes staged `nimbus-bun-jsc-adapter-*.tar.gz` assets when present. `.github/workflows/bun-jsc-adapter.yml` has an explicit `publish_release_assets` path that verifies Linux x86_64 and macOS arm64 adapter archives, generates `nimbus-bun-jsc-adapter-checksums-sha256.txt`, attests the assets, and uploads them to the selected `v*` GitHub Release. `scripts/verify-bun-jsc-release-assets.sh` and its helper prove good, absent, missing-required, bad-checksum, unknown-platform, and tampered-package behavior. Evidence is recorded in `docs/plans/proof/runtime-engine/bun-jsc/gate-61-release-assets-and-checksums.md`. |
| BJD5 | `pending` | Integrate Linux packages, Homebrew/cask, and install script behavior. | Linux package builders can stage the adapter under `/usr/libexec/nimbus/runtime/bun-jsc/...` as an optional package or optional payload with clear dependency semantics. Homebrew/cask can install the adapter under `libexec/runtime/bun-jsc/...` or document the separate artifact lane. The install script either installs the optional adapter when requested or explains how to add it. Packaged installs discover the adapter without manual env vars. |
| BJD6 | `pending` | Add artifact trust, SBOM, and provenance evidence. | Adapter archives include SHA-256 checksums and SBOM/provenance evidence or deterministic fixture placeholders accepted by the artifact-provenance verifier. Verification rejects checksum/provenance mismatches. Docs explain which evidence is production-enforced now and which evidence is release-attested by GitHub Actions or future Cosign/SLSA lanes. |
| BJD7 | `pending` | Update operator docs, diagnostics, and DX. | `docs/adapters/native/http-api.md`, runtime docs, install docs, and operator docs explain Bun/JSC states: `not_linked`, `linked`, missing artifact, checksum mismatch, unsupported platform, and `outer_quota_required`. `/debug/runtime/metrics` and UI diagnostics expose enough manifest/source information to debug installs without exposing host secrets or tenant-controlled paths. Tests lock the stable diagnostics shape. |
| BJD8 | `pending` | Prove installed-package behavior locally and on Debian 13 `minicloud`. | macOS proof shows a packaged adapter is discovered from the installed layout without `NIMBUS_BUN_EMBED_SHARED_LIBRARY`, and no-link fallback still works when the artifact is absent. Debian 13 `minicloud` proof shows the same for the Linux package/archive path using home-backed caches. Both proofs execute a `"use bun";` function and keep V8/Node lanes green. |
| BJD9 | `pending` | Close with broad verification, docs, and clean commits. | Final verification records `cargo fmt --all --check`, `make check`, `make clippy`, `npm run typecheck`, `npm run test`, `npm run build`, `make verify-bun-jsc-runtime-contract`, adapter package verifier, local linked adapter proof, `minicloud` linked/package proof, and `git diff --check`. If a docs reference validator exists, it passes; if not, the absence is recorded. The plan ledger, README entry, proof docs, and commits agree before the goal is marked complete. |

## Recommended Implementation Order

1. **Contract first (`BJD0-BJD1`)**
   - Define the artifact layout and manifest before touching installers.
   - Build the parser/discovery seam before CI or packaging starts depending
     on ad hoc environment variables.
   - Keep `not_linked` as a first-class state.

2. **Packaging helper second (`BJD2`)**
   - Make a local deterministic helper that can package a prebuilt shared
     adapter. This gives fast fixture tests and avoids making every validation
     run rebuild Bun/WebKit.

3. **Release lanes third (`BJD3-BJD4`)**
   - Add the heavy source-backed adapter build to manual/nightly/release lanes.
   - Keep default PR CI focused on syntax, fixtures, and the existing
     no-link runtime contract.

4. **Installers/packages fourth (`BJD5`)**
   - Wire the artifact into Linux/Homebrew/install paths after the archive
     shape and verifier are stable.
   - Prefer an optional package or explicit install option if bundling the
     adapter in every install would materially increase size or platform risk.

5. **Trust and evidence fifth (`BJD6`)**
   - Checksums are required immediately.
   - SBOM/provenance evidence should use the existing AP verifier seams and
     release attestation flow, not custom crypto.

6. **Operator proof and closeout (`BJD7-BJD9`)**
   - Make diagnostics and documentation real product contracts.
   - Finish with installed-package proofs on macOS and Debian 13 `minicloud`.

## Work Surfaces

Expected code and script surfaces:

- `crates/nimbus-runtime/src/backends/bun_jsc/`
  - manifest parsing, discovery, checksum and target validation, linked state
    diagnostics, and feature-gated loading
- `crates/nimbus-runtime/build.rs`
  - build-time env tracking for explicit development overrides
- `crates/nimbus-server/src/protocol.rs`
  - diagnostics serialization if manifest/source fields are added
- `crates/nimbus-server/src/tests/registry_and_license/runtime_metrics.rs`
  - diagnostics contract tests
- `scripts/verify-bun-jsc-linked-adapter.sh`
  - source-backed proof gate and export/leak audit source of truth
- new `scripts/package-bun-jsc-adapter.sh` or equivalent
  - deterministic archive/manifest/checksum builder
- new `scripts/verify-bun-jsc-adapter-package.sh` or equivalent
  - fixture verifier used by CI and release layout checks
- `.github/workflows/ci.yml`
  - default fixture/syntax gates only
- `.github/workflows/release.yml`
  - optional adapter archive publication, checksums, attestations
- `.github/workflows/linux-packages.yml`
  - optional package consumption if the adapter is split into a Linux package
- `scripts/build-linux-release-packages.sh`
  - Linux package staging if packaged with or beside `nimbus`
- `scripts/install.sh`
  - optional install/update UX
- Homebrew/cask release assets and docs
  - packaged discovery path for macOS
- `docs/adapters/native/http-api.md`
  - stable runtime diagnostics contract
- `docs/operating/ci-modernization.md`
  - CI lane shape after BJD3
- `docs/plans/distribution-plan.md`
  - distribution channel contract after BJD5

## Design Decisions To Close In BJD0

These are the decisions BJD0 must close before implementation proceeds:

| Decision | Preferred starting point | Why |
| --- | --- | --- |
| Adapter package shape | Optional release asset and optional package payload. | Preserves default binary size and no-link simplicity while making product installs deterministic. |
| Runtime lookup | Manifest-first packaged discovery with explicit dev override. | Avoids hand-set env vars for real installs while preserving focused local proof workflows. |
| Version compatibility | Manifest must match Nimbus-supported Bun source ref/revision and ABI version. | Prevents accidental loading of stale or locally patched adapters. |
| Checksums | SHA-256 manifest and release checksum required. | Aligns with existing release archive and install-script behavior. |
| Provenance | GitHub artifact attestation immediately, Cosign/SLSA through AP seams where available. | Keeps cryptographic verification battle-tested and avoids bespoke crypto code. |
| Linux packaging | Prefer separate optional `nimbus-bun-jsc-adapter` package unless size analysis says bundle. | Enterprise operators can opt in explicitly and audit runtime surface changes. |
| Homebrew packaging | Prefer optional cask/formula asset or formula option documented in install flow. | Keeps default macOS install light and avoids surprising WebKit payload growth. |
| CI lane | Default PR stays no-link plus fixture verifier; heavy linked build is manual/nightly/release. | Protects developer feedback loops while keeping release proof strong. |

## Verification Contract

Focused gates expected during this plan:

```sh
cargo fmt --all --check
make verify-bun-jsc-runtime-contract
make verify-bun-jsc-linked-adapter
bash scripts/verify-bun-jsc-adapter-package.sh
bash scripts/verify-release-archive-layout.sh --artifacts-dir <dir>
make check
make clippy
npm run typecheck
npm run test
npm run build
git diff --check
```

When source-backed Bun builds are needed, use the canonical Bun fork:

```sh
NIMBUS_BUN_REPO=$HOME/src/github.com/nimbus/bun \
NIMBUS_BUN_EXPECTED_REF=bun-v1.4.0-nimbus.5 \
NIMBUS_BUN_EXPECTED_REV=ad0e1d2bbc6690651e04f10eaf1dcdf8a6c0de57 \
make verify-bun-jsc-linked-adapter
```

For `minicloud`, avoid `/tmp` because it has filled during previous proof
runs. Use home-backed paths:

```sh
TMPDIR=$HOME/.cache/nimbus-bun-proof/tmp
NIMBUS_BUN_BUILD_DIR=$HOME/.cache/nimbus-bun-proof/embed-native
NIMBUS_BUN_CACHE_DIR=$HOME/.cache/nimbus-bun-proof/cache
NIMBUS_BUN_CARGO_TARGET_DIR=$HOME/.cache/nimbus-bun-proof/cargo-target
```

## Risk Register

| Risk | Required mitigation |
| --- | --- |
| Adapter payload is too large for default packages. | Keep it optional unless size measurements and user value justify bundling. |
| Packaged discovery loads an attacker-controlled library. | Only search fixed root-owned package paths and explicit development overrides; reject tenant roots and unsafe ownership. |
| Release source drifts from the proven Bun fork tag. | Manifest and verifier require exact source ref/revision or a new recorded superseding tag. |
| CI becomes too slow or expensive. | Keep heavy Bun/WebKit builds out of default PR CI; use fixture/package verifiers in PR and source-backed gates in nightly/release/manual lanes. |
| Provenance is treated as a checkbox. | Reuse AP verifier seams; clearly distinguish checksum, GitHub attestation, SBOM presence, and SLSA/Cosign verification. |
| Operator diagnostics expose host paths or secrets. | Redact sensitive paths, env vars, and credentials; expose source/version/state and safe basename/path class only. |
| Linux and macOS packaging diverge silently. | Use one manifest schema and shared verifier across platforms. |

## Completion Definition

This plan is complete only when:

- A release artifact contract and manifest are implemented and documented.
- Nimbus discovers packaged Bun/JSC adapters without requiring manual env vars.
- Default no-link installs remain fail-closed and operator-visible.
- Linux and macOS adapter archives are buildable or verifiable through release
  lanes from a recorded Nimbus Bun source tag.
- Release checksums, artifact layout validation, SBOM/provenance evidence, and
  package/install integration are implemented or explicitly gated by the parent
  distribution plan with a recorded reason.
- Operator diagnostics and docs explain every supported adapter state.
- Local macOS and Debian 13 `minicloud` installed-package proofs pass.
- Broad repository verification passes and is recorded in proof docs.

## Goal Prompt

Complete `docs/plans/bun-jsc-distribution-and-release-plan.md` from `BJD0`
through `BJD9` autonomously. Treat this plan file, proof docs under
`docs/plans/proof/runtime-engine/bun-jsc/`, and local git history as the
control plane. Keep unrelated dirty files out of commits.

Do not mark the goal complete until all of the following are true:

- the Bun/JSC shared adapter has a documented release artifact contract,
  installed layout, strict manifest schema, checksum policy, discovery order,
  and platform support matrix
- runtime discovery validates packaged manifests before loading shared
  libraries, rejects mismatches, and preserves default `not_linked`
  fail-closed behavior
- package/archive helper scripts can build and verify deterministic adapter
  archives without requiring a full Bun/WebKit rebuild for fixture tests
- release, CI, and package/install surfaces are updated so Linux and macOS
  adapter artifacts are either published as optional assets/packages or
  intentionally gated by the distribution plan with a proof-backed reason
- artifact checksum, SBOM, provenance, and release-attestation evidence are
  wired through existing Nimbus provenance seams or recorded as explicit
  production gates
- operator docs, HTTP diagnostics, and UI diagnostics explain `not_linked`,
  `linked`, missing artifact, checksum mismatch, unsupported platform, and
  `outer_quota_required` states
- installed-package proofs pass locally on macOS and on Debian 13 `minicloud`
  without requiring `NIMBUS_BUN_EMBED_SHARED_LIBRARY`
- `cargo fmt --all --check`, `make check`, `make clippy`, `npm run typecheck`,
  `npm run test`, `npm run build`, `make verify-bun-jsc-runtime-contract`,
  the adapter package verifier, local linked adapter proof, Debian/minicloud
  linked/package proof, and `git diff --check` pass, with any unavailable docs
  reference validation recorded explicitly
