# Gate 48: BJA4L4 Global Collision Audit And Linkage Decision

Date: 2026-05-24

## Purpose

Gate 47 proved that `nimbus_bun_simdutf` fixed only the first Linux co-link
collision family. This gate completes BJA4L4 by auditing the broader symbol
surface and selecting the next product linkage shape for in-process Bun/JSC.

## Linux Artifact Audit

The audit ran read-only on Debian 13 `minicloud` against the existing
source-owned build:

```text
Bun manifest:
  /home/nimbus/.cache/nimbus-bun-proof/configure-namespaced/nimbus-bun-embed-link-args.txt

Bun staticlib:
  /home/nimbus/.cache/nimbus-bun-proof/configure-namespaced/rust-target/x86_64-unknown-linux-gnu/release/libbun_embed_probe.a

Nimbus V8 target:
  /home/nimbus/src/github.com/nimbus/nimbus/target
```

### Rust Staticlib Runtime Symbols

`libbun_embed_probe.a` is a Rust `staticlib` with two archive members and one
global Rust personality symbol:

```text
rust_eh_personality definitions: 1
```

That is expected for a Rust staticlib. It is not acceptable for the product
shape because the host Nimbus test/binary is also Rust and already links Rust
std/runtime symbols.

### Highway

V8/rusty_v8 currently owns Highway globals:

```text
libv8-*.rlib hwy:: definitions: 30
librusty_v8.a hwy:: definitions: 30
```

Bun's embed manifest owns Highway objects directly:

```text
abort.cc.o             hwy:: definitions: 6
aligned_allocator.cc.o hwy:: definitions: 3
nanobenchmark.cc.o     hwy:: definitions: 4
per_target.cc.o        hwy:: definitions: 5
perf_counters.cc.o     hwy:: definitions: 6
print.cc.o             hwy:: definitions: 3
profiler.cc.o          hwy:: definitions: 1
targets.cc.o           hwy:: definitions: 4
timer.cc.o             hwy:: definitions: 7
```

Total Bun-side audited Highway definitions: 39.

### Bun V8 Shim

The source-owned Bun build includes two unified V8 shim objects:

```text
obj/unified/UnifiedSource-src_jsc_bindings_v8-0.cpp.o
obj/unified/UnifiedSource-src_jsc_bindings_v8_shim-0.cpp.o
```

Those objects define 170 `v8::` symbols, including:

```text
v8::HandleScope::CreateHandle(...)
v8::ObjectTemplate::New(...)
v8::FunctionTemplate::New(...)
v8::EscapableHandleScope::EscapableHandleScope(...)
```

Real V8 from rusty_v8 defines the same namespace and overlapping APIs,
including:

```text
v8::EscapableHandleScopeBase::EscapeSlot(...)
v8::Array::New(...)
v8::Function::New(...)
v8::Function::Call(...)
v8::Boolean::Value() const
```

This is the strongest signal that a static same-binary merge is the wrong
default product shape. Bun's V8 shim intentionally presents a V8-compatible
ABI over JSC; Nimbus also links real V8. Those two implementations should not
share one executable-global C++ namespace.

### Simdutf

Gate 45 remains valid. The source-owned `nimbus/bun` tag
`bun-v1.4.0-nimbus.1` moved Bun/WebKit `simdutf` into the
`nimbus_bun_simdutf` namespace:

```text
libWTF.a nimbus_bun_simdutf:: definitions: 526
libWTF.a plain simdutf:: definitions: 0
bun-simdutf.cpp.o nimbus_bun_simdutf__ definitions: 60
bun-simdutf.cpp.o plain simdutf__ definitions: 0
```

That repair should stay in the fork until the final linkage shape proves it no
longer needs the namespace.

## Options

### Option A: Static Same-Binary Namespace/Hiding

This preserves the strongest single-binary story, but it requires Nimbus to
own every global collision family:

- change Rust packaging so the Bun adapter does not bundle colliding Rust
  runtime symbols;
- namespace or hide Highway for Bun without breaking WebKit/JSC users;
- remove, namespace, or hide Bun's V8 shim symbols even though their public
  names intentionally live under `v8::`;
- keep the `nimbus_bun_simdutf` namespace;
- repeat the process for future native dependency collisions.

This is high-maintenance and easy to make accidentally fragile. The Bun V8
shim is especially poor fit for static co-linking beside real V8 because its
purpose is to emulate a real V8 ABI.

### Option B: Source-Owned Shared In-Process Adapter

This keeps Bun/JSC in the Nimbus process while avoiding one merged executable
global symbol table:

```text
Nimbus binary
  -> linked Bun/JSC adapter seam
  -> load verified libnimbus_bun_jsc_embedder.so/dylib with local scope
  -> call nimbus_bun_embed_* C ABI
  -> Bun/JSC symbols remain private to the adapter artifact
```

The required Bun fork work is larger than Gate 42's failed attempt, but better
bounded:

- build a dedicated Bun/JSC embedder shared library from source with PIC;
- compile WebKit/JSC, bmalloc/libpas, direct deps, and Bun C++ in PIC mode;
- remove executable-only flags such as `-fno-pic`, `-Wl,-no-pie`, and
  `-rdynamic` from the shared adapter path;
- export only the Nimbus C ABI via an exported-symbol list or version script;
- hide bundled native dependencies and prevent accidental global interposition;
- keep a native smoke test that loads the library with local symbol scope and
  calls the same probe exports.

Nimbus then loads the adapter explicitly, records the loaded source/ABI in
diagnostics, and keeps the default no-link build fail-closed.

## Decision

Select Option B for BJA4L5: a source-owned shared in-process Bun/JSC adapter.

This is the more canonical shape for embedding two large native runtimes that
both carry their own implementations of V8-compatible APIs, Highway, Rust, and
other native dependencies. It preserves the product requirement that Bun/JSC
run in-process, while avoiding a brittle, open-ended campaign to make every
Bun-native global symbol coexist with real V8 in the executable namespace.

The default Nimbus binary remains simple and fail-closed. Product Bun/JSC
support becomes an optional, verified runtime artifact. If a single-file UX is
required later, Nimbus can package the shared adapter as a content-addressed
embedded asset that is extracted to a verified cache before loading; that is a
distribution concern, not a reason to force unsafe static co-linking.

## Rejected Paths

- Unsafe duplicate-symbol linker policy remains forbidden. The earlier
  `--allow-multiple-definition` proof linked and crashed.
- Repurposing current release objects as a shared object remains rejected.
  Gate 42 showed non-PIC bmalloc/libpas TLS relocations. BJA4L5 must build a
  PIC adapter from source.
- External process or microVM execution is not the target of this plan. Nimbus
  already has OCI/microVM sandbox workload support for that model; this plan is
  specifically about the optional in-process runtime backend.
- Static same-binary co-linking is retained only as a fallback if the shared
  adapter path proves impossible from source. That fallback must solve every
  audited collision family before product support can be claimed.

## BJA4L5 Implementation Contract

BJA4L5 should make the chosen shape real:

- Bun fork: add a dedicated shared embedder target, for example
  `libnimbus_bun_jsc_embedder.so` on Linux and
  `libnimbus_bun_jsc_embedder.dylib` on macOS.
- Bun fork: add an explicit shared/PIC build mode instead of mutating the
  normal Bun executable profile.
- Bun fork: export only `nimbus_bun_embed_*` symbols and audit that `v8::`,
  `hwy::`, Rust runtime, and plain `simdutf` symbols are not globally exported
  from the adapter.
- Nimbus: replace the static link-manifest product path with an explicit
  dynamic adapter load path behind the existing `BunJscExecutionAdapter` seam.
- Nimbus: keep no-link builds `not_linked`, and report `linked` only after the
  shared adapter is present, loaded, ABI-checked, and callable.
- Verifier: prove same-process V8 before/after Bun/JSC on Debian 13
  `minicloud` without unsafe linker flags or crashes.

## Status

BJA4L4 is complete. BJA4L5 is now the active implementation slice.
