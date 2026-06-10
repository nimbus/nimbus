# Gate 49: BJA4L5 Shared Adapter Build And Export Audit

Date: 2026-05-24

## Purpose

Gate 48 selected a source-owned shared in-process Bun/JSC adapter because the
static same-binary link collided with Rust runtime symbols, Highway, Bun's V8
shim, and earlier `simdutf` symbols. This gate proves the selected BJA4L5
build shape on Debian 13 `minicloud`.

## Source Patch

The proof ran against the Nimbus-owned Bun fork:

```text
Repo: /home/nimbus/src/github.com/nimbus/bun
Source tag: bun-v1.4.0-nimbus.2
Source revision: c0896b441c89402c8af0ade847f806f2fcc5fece
```

The source checkpoint adds:

- `--embedder-shared` as an explicit Unix-only build mode.
- fail-closed validation that `--embedder-shared` requires local WebKit.
- PIC propagation through Bun C/C++, nested dependency builds, and WebKit/JSC.
- a `libnimbus_bun_jsc_embedder.so` target plus `bun-embed-shared` and
  `check-bun-embed-shared` phony targets.
- a Linux version script that exports only the Nimbus Bun/JSC C ABI.
- Rust `-Crelocation-model=pic` for `--embedder-shared`.

Source ownership is now reproducible from the Nimbus-owned `nimbus/bun`
branch `nimbus/bja4l2-simdutf-namespace` and tag
`bun-v1.4.0-nimbus.2`. `git ls-remote nimbus
refs/heads/nimbus/bja4l2-simdutf-namespace
refs/tags/bun-v1.4.0-nimbus.2^{}` resolved both refs to
`c0896b441c89402c8af0ade847f806f2fcc5fece`.

## Build Result

The proof used home-backed paths on Debian 13:

```text
Build dir: /home/nimbus/.cache/nimbus-bun-proof/shared-adapter-configure
Cache dir: /home/nimbus/.cache/nimbus-bun-proof/cache-shared
WebKit: /home/nimbus/src/github.com/oven-sh/WebKit
Target: check-bun-embed-shared
```

Configure-only generated the expected shared adapter target:

```text
features: webkit:local, simdutf:nimbus_bun_simdutf, embedder:shared
target: libnimbus_bun_jsc_embedder.so
```

The first full build failed at the shared-object link:

```text
ld.lld: error: relocation R_X86_64_64 cannot be used against local symbol;
recompile with -fPIC
referenced by std::sys::args::unix::imp::ARGV_INIT_ARRAY
archive: rust-target/x86_64-unknown-linux-gnu/release/libbun_embed_probe.a
```

Root cause: `scripts/build/rust.ts` still emitted
`-Crelocation-model=static` for Linux release builds. That is correct for
Bun's normal ET_EXEC executable, but incorrect when the Rust staticlib feeds a
shared object. The proof patch now emits:

```text
-Crelocation-model=pic
```

for `cfg.embedderShared`.

The second full build linked the shared object but exported no defined dynamic
symbols. Root cause: `-Wl,--exclude-libs,ALL` localized symbols from the Rust
static archive before the version script could expose the Nimbus ABI. The proof
patch removes that flag from the shared adapter target and relies on the
version script's `local: *` rule to hide everything except the explicit ABI.

After both fixes:

```text
ninja -C /home/nimbus/.cache/nimbus-bun-proof/shared-adapter-configure -j4 check-bun-embed-shared
```

passed with exit status 0.

## Export Audit

The produced artifact is:

```text
/home/nimbus/.cache/nimbus-bun-proof/shared-adapter-configure/libnimbus_bun_jsc_embedder.so
ELF 64-bit LSB shared object, x86-64
SONAME: libnimbus_bun_jsc_embedder.so
FLAGS: BIND_NOW STATIC_TLS
TEXTREL: none reported
```

Defined dynamic exports:

```text
nimbus_bun_embed_invoke_program_wrapper_json@@NIMBUS_BUN_JSC_EMBEDDER_1.0
nimbus_bun_embed_probe_async_host_call@@NIMBUS_BUN_JSC_EMBEDDER_1.0
nimbus_bun_embed_probe_construct_and_destroy_vm@@NIMBUS_BUN_JSC_EMBEDDER_1.0
nimbus_bun_embed_probe_lifecycle_reuse_stress@@NIMBUS_BUN_JSC_EMBEDDER_1.0
nimbus_bun_embed_probe_memory_behavior@@NIMBUS_BUN_JSC_EMBEDDER_1.0
nimbus_bun_embed_probe_package_module_policy@@NIMBUS_BUN_JSC_EMBEDDER_1.0
nimbus_bun_embed_probe_permission_surface_inventory@@NIMBUS_BUN_JSC_EMBEDDER_1.0
nimbus_bun_embed_probe_program_bundle_host_calls@@NIMBUS_BUN_JSC_EMBEDDER_1.0
nimbus_bun_embed_probe_sync_host_call@@NIMBUS_BUN_JSC_EMBEDDER_1.0
nimbus_bun_embed_probe_timeout_and_cancel@@NIMBUS_BUN_JSC_EMBEDDER_1.0
```

Counts:

```text
defined dynamic exports: 10
defined leaked native symbols: 0
```

The leak audit checked defined dynamic symbols for:

```text
v8::
hwy::
rust_eh_personality
simdutf::
simdutf__
nimbus_bun_simdutf::
nimbus_bun_simdutf__
```

None were exported.

## Decision

The source-owned shared adapter shape is viable on Debian 13. BJA4L5 has build
and export evidence, and source ownership is closed by the Nimbus-owned
`bun-v1.4.0-nimbus.2` tag.

This is a build/export decision only. Gate 50 later found that the `.2`
artifact was not late-`dlopen` safe because it carried ELF `STATIC_TLS`; the
runtime source contract is therefore superseded by `bun-v1.4.0-nimbus.3` for
BJA4L6 and later.

## Next

- Add the Nimbus dynamic loader path behind `BunJscExecutionAdapter`.
- Prove BJA4L6: V8 and Bun/JSC coexist in one process by loading the shared
  adapter with local symbol scope and invoking both lanes.
