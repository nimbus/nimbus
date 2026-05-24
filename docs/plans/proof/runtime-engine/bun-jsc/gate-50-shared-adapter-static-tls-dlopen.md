# Gate 50: BJA4L6 Shared Adapter Static TLS Dlopen Proof

Date: 2026-05-24

## Purpose

BJA4L5 proved that the source-owned Bun/JSC shared adapter can build on
Debian 13 and export only the Nimbus C ABI. BJA4L6 must also prove that the
artifact can be loaded after Nimbus has already initialized the V8/Deno lane.

## Failed Proof

The first warm `minicloud` rerun used:

```text
Nimbus repo: /home/nimbus/src/github.com/nimbus/nimbus-worktrees/bja4l3-linux
Bun repo: /home/nimbus/src/github.com/nimbus/bun-worktrees/bun-v1.4.0-nimbus.2
Bun tag: bun-v1.4.0-nimbus.2
Bun revision: c0896b441c89402c8af0ade847f806f2fcc5fece
Shared adapter: /home/nimbus/.cache/nimbus-bun-proof/shared-adapter-tag2/libnimbus_bun_jsc_embedder.so
```

Passing evidence before failure:

- default no-link runtime contract passed.
- linked no-shared-library unit contract passed 10 tests.
- shared adapter build completed from warm cache.
- generated build graph rejected unsafe duplicate-symbol linker workarounds.
- export audit found exactly the 10 Nimbus C ABI exports.
- native leak audit found 0 leaked defined native symbols.
- simdutf namespace audit passed for Bun/WebKit and V8/rusty_v8 separation.

The linked same-process lane then failed:

```text
failed to load .../libnimbus_bun_jsc_embedder.so:
cannot allocate memory in static TLS block
```

ELF inspection showed:

```text
FLAGS: BIND_NOW STATIC_TLS
.tdata
.tbss
```

The generated Bun build graph had one explicit static TLS source:

```text
scripts/build/deps/mimalloc.ts -> -ftls-model=initial-exec
```

## Fix

The Nimbus Bun fork now uses `-ftls-model=local-dynamic` for mimalloc when
`--embedder-shared` is enabled, while preserving `initial-exec` for Bun's
normal static executable.

```text
Bun tag: bun-v1.4.0-nimbus.3
Bun revision: ed8d05f17ee2803520440a07bcc7f6f47f2f68b8
```

## Passing Proof

The `.3` source was rerun on Debian 13 `minicloud` with:

```text
Nimbus repo: /home/nimbus/src/github.com/nimbus/nimbus-worktrees/bja4l3-linux
Bun repo: /home/nimbus/src/github.com/nimbus/bun-worktrees/bun-v1.4.0-nimbus.3
Bun tag: bun-v1.4.0-nimbus.3
Bun revision: ed8d05f17ee2803520440a07bcc7f6f47f2f68b8
Shared adapter: /home/nimbus/.cache/nimbus-bun-proof/shared-adapter-tag3/libnimbus_bun_jsc_embedder.so
```

The linked gate passed.

Required evidence:

- default no-link runtime contract passed.
- linked no-shared-library unit contract passed 10 tests.
- Bun source export and Rust format checks passed.
- generated build graph safety policy passed, including no unsafe
  duplicate-symbol linker policy.
- generated build graph uses `-ftls-model=local-dynamic` for the shared
  embedder's mimalloc object and not `-ftls-model=initial-exec`.
- `readelf -d libnimbus_bun_jsc_embedder.so` has no `STATIC_TLS`.
- export audit found exactly the 10 Nimbus C ABI exports.
- native leak audit found 0 leaked defined native symbols.
- simdutf namespace audit still separates Bun/WebKit from V8/rusty_v8.
- linked same-process unit lane passed 10 tests.
- `tests/bun_jsc_linked_adapter.rs` passed 1 integration test proving V8 and
  linked Bun/JSC can be invoked in one Nimbus test process.
- Nimbus and Bun whitespace diff checks passed.

## Decision

`BJA4L6` is complete. The source-owned shared adapter remains the selected
in-process product path, and the verifier now treats static TLS as a hard
failure so this class of late-`dlopen` regression cannot quietly re-enter the
proof lane.
