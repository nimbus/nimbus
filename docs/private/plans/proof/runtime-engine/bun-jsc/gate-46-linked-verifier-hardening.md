# Gate 46: BJA4L3 Linked Verifier Hardening

Date: 2026-05-24

## Purpose

BJA4L2 gave Nimbus a source-owned Bun tag with isolated simdutf symbols. This
gate starts BJA4L3 by moving that evidence into the reusable verifier instead
of leaving it as manual proof notes.

## Changes

`scripts/verify-bun-jsc-linked-adapter.sh` now defaults to the Nimbus-owned
Bun source:

```text
Bun repo: /Users/jack/src/github.com/nimbus/bun
Bun ref:  bun-v1.4.0-nimbus.1
Bun rev:  5ba54ccecdfabd857a7ca362c14c0f614d25b21b
```

The verifier checks that the expected ref resolves to the expected commit and
that the checkout `HEAD` is the same commit.

On Linux, the verifier defaults to:

```text
profile: release-local
simdutf namespace: nimbus_bun_simdutf
symbol audit: required
```

On non-Linux hosts, the verifier keeps the prebuilt release path and skips the
Linux-only symbol audit unless explicitly requested.

The verifier now audits:

- Bun/WebKit `libWTF.a` contains private `nimbus_bun_simdutf::` definitions
  and no plain `simdutf::` definitions.
- Bun's `bun-simdutf.cpp.o` contains private `nimbus_bun_simdutf__*`
  definitions and no plain `simdutf__*` definitions.
- Existing V8/rusty_v8 artifacts do not own any `nimbus_bun_simdutf::` or
  `nimbus_bun_simdutf__*` symbols.

It also rejects unsafe link policy from both:

- `RUSTFLAGS` / `CARGO_ENCODED_RUSTFLAGS`;
- the generated Bun embed link manifest.

`crates/nimbus-runtime/build.rs` now rejects unsafe manifest entries even when
the verifier is bypassed.

`BunJscLinkedAdapterSourceContract` now records:

```text
repository:        https://github.com/nimbus/bun
source_ref:        bun-v1.4.0-nimbus.1
git_revision:      5ba54ccecdfabd857a7ca362c14c0f614d25b21b
proof_target:      check-bun-embed-probe
simdutf_namespace: nimbus_bun_simdutf
```

## Local Verification

Rust contract:

```sh
cargo test -p nimbus-runtime --features bun-jsc-linked-adapter --lib backends::bun_jsc
```

Result:

```text
10 passed; 0 failed; 501 filtered out
```

Local linked verifier:

```sh
bash scripts/verify-bun-jsc-linked-adapter.sh
```

Result:

```text
Bun/JSC runtime contract gate: pass
linked feature tests: 10 passed
Bun proof source exports: all 10 present
Bun native embed probe and link manifest: pass
Link manifest safety policy: pass
Bun/WebKit/V8 simdutf symbol audit: skipped for host aarch64-apple-darwin
linked pure invocation: 1 passed
Nimbus whitespace diff check: pass
Bun whitespace diff check: pass
Bun/JSC linked adapter gate: pass
```

Unsafe `RUSTFLAGS` rejection:

```sh
env RUSTFLAGS='-Wl,--allow-multiple-definition' \
  bash scripts/verify-bun-jsc-linked-adapter.sh
```

Result:

```text
unsafe linker policy detected in RUSTFLAGS/CARGO_ENCODED_RUSTFLAGS
```

Unsafe link manifest rejection:

```sh
NIMBUS_BUN_EMBED_LINK_ARGS=/private/tmp/nimbus-bun-unsafe-link-manifest.txt \
  cargo check -p nimbus-runtime --features bun-jsc-linked-adapter
```

Result:

```text
unsafe Bun/JSC link argument `-Wl,--allow-multiple-definition` ...
```

Quick hygiene:

```sh
bash -n scripts/verify-bun-jsc-linked-adapter.sh
cargo fmt --all --check
git diff --check
```

All passed.

## Decision

The verifier is hardened locally. BJA4L3 is not complete until the same script
passes on Debian 13 `minicloud` with the default Linux source-namespaced build
and required symbol audit.
