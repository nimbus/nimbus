# Gate 42: BJA4L Dynamic PIC Feasibility

Date: 2026-05-24

## Purpose

Gate 41 selected the in-process dynamic Bun adapter as the next feasibility
lane, with a clear caveat: the current Bun/WebKit Linux release objects are
non-PIC. This gate verifies whether the existing BJA4 static embed manifest can
be repurposed into a dynamic artifact after removing obvious executable-only
flags.

## Command

The probe ran on Debian 13 `minicloud` against the same BJA4 artifacts:

```sh
ROOT=/home/nimbus/.cache/nimbus-bun-proof
MANIFEST=$ROOT/linked-adapter-release/nimbus-bun-embed-link-args.txt
RESP=$ROOT/dynamic-feasibility.args
OUT=$ROOT/libnimbus_bun_jsc_embedder_feasibility.so
LOG=$ROOT/dynamic-feasibility.log

grep -v -x -- "-fno-pic" "$MANIFEST" \
  | grep -v -x -- "-Wl,-no-pie" \
  | grep -v -x -- "-rdynamic" > "$RESP"

clang++-21 -shared @"$RESP" -o "$OUT" > "$LOG" 2>&1
```

## Result

The link failed with status `1`. The first actionable errors are non-PIC TLS
relocations from WebKit's bmalloc archive:

```text
ld.lld: error: relocation R_X86_64_TPOFF32 against pas_thread_local_cache_pointer cannot be used with -shared
>>> defined in libbmalloc.a(pas_thread_local_cache.c.o)
>>> referenced by pas_thread_local_cache.h
>>> pas_local_allocator_scavenger_data.c.o in libbmalloc.a

ld.lld: error: relocation R_X86_64_TPOFF32 against pas_thread_local_cache_is_exiting cannot be used with -shared
>>> defined in libbmalloc.a(pas_thread_local_cache.c.o)
>>> referenced by pas_deallocate.c
>>> pas_deallocate.c.o in libbmalloc.a
```

The same relocation family repeats through bmalloc/libpas allocator and JIT
heap objects. This is not a missing flag on the final link. It is baked into
the current release objects.

## Decision

The dynamic adapter is not the next implementation lane unless Nimbus chooses
to produce and own a PIC WebKit/Bun artifact. That is a larger fork/release
surface than the dynamic path was meant to avoid.

Given Nimbus' stated preference for single-binary simplicity, BJA4L2 rolls back
to the static namespace-isolation path:

```text
selected BJA4L2 path
  = Nimbus-owned Bun/WebKit namespace isolation
  + same-binary static link
  + explicit export audit
```

The implementation may still use a deterministic symbol-rewrite experiment as
a diagnostic, but the product path must be source-owned by a Nimbus Bun/WebKit
fork or an equivalent reproducible source/tagged artifact. A binary-only
post-processing trick is not enough to close BJA8.

## Next

- Audit where `simdutf` enters the Bun/WebKit prebuilt or local WebKit source
  build.
- Add a source-owned namespace mechanism for Bun/WebKit's `simdutf::` and
  Bun's `simdutf__` wrappers.
- Update the Bun embed manifest/gate to assert that no global Bun-side
  `simdutf::` or `simdutf__` symbols collide with V8.
