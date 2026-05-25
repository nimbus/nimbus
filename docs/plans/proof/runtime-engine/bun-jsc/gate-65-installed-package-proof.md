# Gate 65: Installed-Package Proof

Date: 2026-05-25

## Decision

The Bun/JSC adapter can be discovered from package-manager-owned install
layouts without development override environment variables.

`scripts/verify-bun-jsc-installed-package-proof.sh` is now the repeatable gate
for that contract. It verifies the adapter archive, stages a proof-owned
package layout, runs linked Bun/JSC execution through packaged manifest
discovery, removes the package layout, proves no-link fallback, and reruns the
default Bun/JSC runtime contract.

The proof keeps both override variables unset:

```text
NIMBUS_BUN_EMBED_SHARED_LIBRARY
NIMBUS_BUN_JSC_ADAPTER_MANIFEST
```

This means a passing proof demonstrates installed manifest discovery, not the
development override path.

## Code Changes

- `scripts/verify-bun-jsc-installed-package-proof.sh` stages and cleans
  package-owned layouts for macOS and Linux.
- `Makefile` exposes `verify-bun-jsc-installed-package-proof` and syntax-checks
  the helper in `proof-helpers`.
- `crates/nimbus-runtime/tests/bun_jsc_linked_adapter.rs` adds a literal
  `"use bun";` invocation test for packaged-discovery proof.
- `scripts/package-bun-jsc-adapter.sh` sets `umask 022` so generated package
  files are not group/other writable on hosts with permissive user umasks.
- `scripts/verify-bun-jsc-adapter-package.sh` preserves archive modes during
  extraction and rejects group/other writable adapter files, manifests,
  checksums, README files, SBOMs, and SLSA evidence.
- `scripts/verify-bun-jsc-adapter-package-helper.sh` now includes an
  `unsafe-mode` fixture to prove that package mode check.

## macOS Proof

Host:

```text
target_triple=aarch64-apple-darwin
platform=darwin
installed_manifest=/opt/homebrew/opt/nimbus/libexec/runtime/bun-jsc/current/nimbus-bun-jsc-adapter.json
```

Archive:

```text
path=/private/tmp/nimbus-bun-jsc-bjd8-macos-rerun/nimbus-bun-jsc-adapter-darwin-arm64.tar.gz
archive_sha256=f61ae60fe6190d95816969630b16cb03064bdb85db986d8e7f1106d2c27e2882
library_sha256=601971d856bfc6cbe067875ab8cf8f644bc04fe53c8a0a076d929dc632897d06
```

Commands:

```sh
bash scripts/package-bun-jsc-adapter.sh \
  --output-dir /private/tmp/nimbus-bun-jsc-bjd8-macos-rerun \
  --shared-library /Users/jack/.cache/nimbus-bun-proof/shared-adapter-release-local-namespaced/libnimbus_bun_jsc_embedder.dylib \
  --nimbus-version v0.1.31-403-g4d478710-bjd8-proof2 \
  --target-triple aarch64-apple-darwin

bash scripts/verify-bun-jsc-installed-package-proof.sh \
  --archive /private/tmp/nimbus-bun-jsc-bjd8-macos-rerun/nimbus-bun-jsc-adapter-darwin-arm64.tar.gz \
  --target-triple aarch64-apple-darwin
```

Result:

- adapter archive verifier passed
- packaged discovery executed `bun_shared_adapter_executes_use_bun_directive_program_wrapper`: 1 passed
- same-process V8 plus Bun/JSC integration test: 1 passed
- same-process V8 plus Bun/JSC unit test: 1 passed
- server diagnostics linked-state test: 1 passed
- no-link fallback after package removal: 1 passed
- default runtime contract passed:
  - runtime policy and memory semantics: 11 passed
  - Bun/JSC pool scaffold: 10 passed
  - Convex runtime lane registry: 15 passed
  - runtime diagnostics API: 2 passed
  - tenant admission: 1 passed
  - operator UI diagnostics: 2 files / 5 tests passed

Cleanup check:

```sh
ls -ld /opt/homebrew/opt/nimbus /opt/homebrew/opt/nimbus/libexec/runtime/bun-jsc/current 2>/dev/null || true
```

Result: no output. The proof-owned Homebrew opt link was removed.

## Debian 13 minicloud Proof

Host:

```text
ssh_target=nimbus@192.168.4.29
hostname=minicloud
os=Debian 13
kernel=Linux minicloud 6.12.88+deb13-amd64
target_triple=x86_64-unknown-linux-gnu
platform=linux
rustc=1.93.1
installed_manifest=/usr/libexec/nimbus/runtime/bun-jsc/current/nimbus-bun-jsc-adapter.json
```

Archive:

```text
path=/home/nimbus/.cache/nimbus-bun-proof/bjd8-linux-package/nimbus-bun-jsc-adapter-linux-x86_64.tar.gz
archive_sha256=0862f8e2a87a87e5a9f215ad5aff0edf1cf7c010c4482c56238cf0a9340787de
library_sha256=a8a9d0af77758716eed0f0f8a5813b9321d400bd9924760210575ac87c2d600a
```

Commands:

```sh
PATH=$HOME/.cargo/bin:$PATH \
TMPDIR=$HOME/.cache/nimbus-bun-proof/tmp \
bash scripts/package-bun-jsc-adapter.sh \
  --output-dir $HOME/.cache/nimbus-bun-proof/bjd8-linux-package \
  --shared-library $HOME/.cache/nimbus-bun-proof/shared-adapter-release-local-namespaced/libnimbus_bun_jsc_embedder.so \
  --nimbus-version v0.1.31-403-g4d478710-bjd8-proof \
  --target-triple x86_64-unknown-linux-gnu

PATH=$HOME/.cargo/bin:$PATH \
TMPDIR=$HOME/.cache/nimbus-bun-proof/tmp \
bash scripts/verify-bun-jsc-installed-package-proof.sh \
  --archive $HOME/.cache/nimbus-bun-proof/bjd8-linux-package/nimbus-bun-jsc-adapter-linux-x86_64.tar.gz \
  --target-triple x86_64-unknown-linux-gnu
```

Result:

- adapter archive verifier passed
- packaged discovery executed `bun_shared_adapter_executes_use_bun_directive_program_wrapper`: 1 passed
- same-process V8 plus Bun/JSC integration test: 1 passed
- same-process V8 plus Bun/JSC unit test: 1 passed
- server diagnostics linked-state test: 1 passed
- no-link fallback after package removal: 1 passed
- default runtime contract passed:
  - runtime policy and memory semantics: 11 passed
  - Bun/JSC pool scaffold: 10 passed
  - Convex runtime lane registry: 15 passed
  - runtime diagnostics API: 2 passed
  - tenant admission: 1 passed
  - operator UI diagnostics: 2 files / 5 tests passed

Cleanup check:

```sh
ssh nimbus@192.168.4.29 \
  'ls -ld /usr/libexec/nimbus/runtime/bun-jsc /usr/libexec/nimbus/runtime/bun-jsc/current 2>/dev/null || true'
```

Result: no output. The proof-owned `/usr/libexec/nimbus/runtime/bun-jsc`
layout was removed.

## Issues Found And Fixed

The installed-package proof caught three product-contract issues before
closeout:

- package file modes inherited a permissive remote `umask`; packaging now uses
  `umask 022`, and verification rejects group/other writable files
- archive verification originally discarded mode bits during extraction;
  verification now extracts with `tar -xpzf`
- direct server diagnostics tests needed UI artifacts; the installed-package
  proof now runs `make build-ui` before the server diagnostics lane

## Verification

Passed locally:

```text
bash -n scripts/verify-bun-jsc-installed-package-proof.sh
cargo fmt --all --check
bash scripts/verify-bun-jsc-adapter-package-helper.sh
bash scripts/verify-bun-jsc-installed-package-proof.sh --archive /private/tmp/nimbus-bun-jsc-bjd8-macos-rerun/nimbus-bun-jsc-adapter-darwin-arm64.tar.gz --target-triple aarch64-apple-darwin
```

Passed on Debian 13 `minicloud`:

```text
bash scripts/verify-bun-jsc-installed-package-proof.sh --archive /home/nimbus/.cache/nimbus-bun-proof/bjd8-linux-package/nimbus-bun-jsc-adapter-linux-x86_64.tar.gz --target-triple x86_64-unknown-linux-gnu
```

The final `BJD9` broad repository gate remains next.
