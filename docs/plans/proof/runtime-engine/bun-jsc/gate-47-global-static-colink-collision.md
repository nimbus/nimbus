# Gate 47: BJA4L3 Global Static Co-Link Collision

Date: 2026-05-24

## Purpose

Gate 46 hardened the linked verifier and moved the Linux proof to the
Nimbus-owned Bun tag `bun-v1.4.0-nimbus.1`. That tag fixes the first known
Linux collision by moving Bun/WebKit `simdutf` symbols into
`nimbus_bun_simdutf`.

This gate records the next Debian 13 `minicloud` result. The important finding
is that `simdutf` isolation is necessary but not sufficient. A static
same-binary link of Rust/V8/Nimbus plus the current Bun/JSC embedder still has
global duplicate symbol families that cannot be accepted for product runtime
support.

## Command Shape

The focused same-process proof ran on Debian 13 `minicloud` using the
home-backed build/cache paths from the plan and the source-owned Bun manifest:

```sh
cd $HOME/src/github.com/nimbus/nimbus-worktrees/bja4l3-linux

LINK_ARGS=$HOME/.cache/nimbus-bun-proof/configure-namespaced/nimbus-bun-embed-link-args.txt

NIMBUS_BUN_EMBED_LINK_ARGS=$LINK_ARGS \
CARGO_TARGET_DIR=$HOME/src/github.com/nimbus/nimbus/target \
CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=clang++-21 \
CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C link-arg=-fuse-ld=lld" \
CARGO_BUILD_JOBS=1 \
cargo test -p nimbus-runtime \
  --features bun-jsc-linked-adapter \
  --test bun_jsc_linked_adapter \
  -- --nocapture
```

Using `lld` matters because the earlier `bfd` link failed with
`Resource temporarily unavailable (os error 11)`, which obscured the real
symbol errors. `lld` surfaced the deterministic duplicate-symbol set.

## Result

The command failed with status `101` during final linkage. The hard duplicate
families were:

| Family | Representative symbols | Meaning |
| --- | --- | --- |
| Rust staticlib/runtime | `rust_eh_personality` | Bun's embed probe is currently packaged as a Rust `staticlib`, which bundles Rust runtime/personality symbols. A Rust Nimbus test binary also links Rust std, so the two owners collide. |
| Highway | `hwy::platform::GetCpuString`, `hwy::DisableTargets` | V8/rusty_v8 and Bun/WebKit both expose global Highway symbols. |
| Bun V8 shim | `v8::Array::New`, `v8::Boolean::Value`, `v8::Function...`, `v8::EscapableHandleScopeBase...` | Bun ships `src/jsc/bindings/v8/*.cpp` compatibility shims that intentionally export `v8::` symbols. Those collide with real V8 from rusty_v8 in the same product process. |

The fixed `nimbus_bun_simdutf` family did not reappear as the first error
class. That confirms Gate 45 fixed one real issue, but the product shape still
fails on other global owners.

## Source Findings

Local Bun source explains why these families appear:

- `src/embed_probe/Cargo.toml` declares `crate-type = ["staticlib"]`, which is
  appropriate for Bun's standalone probe but unsafe to co-link into a Rust host
  binary without controlling duplicate Rust runtime symbols.
- `scripts/glob-sources.ts` includes `src/jsc/bindings/v8/*.cpp` and
  `src/jsc/bindings/v8/shim/*.cpp`, so the embedder link owns Bun's V8 shim
  objects unless the build surface excludes, hides, or namespaces them.
- The previous Gate 42 dynamic attempt only proved that the current non-PIC
  release objects cannot be repurposed into a shared object. It did not rule
  out a source-owned PIC/shared Bun adapter built for embedding.

## Decision

Do not mark BJA4L3 or BJA4L complete. Do not proceed to BJA5 HostBridge work
on top of the current static same-binary link.

The next batch is BJA4L4:

```text
complete collision audit
  -> choose source-owned PIC/shared in-process adapter
     OR deeper source-owned static namespace/hiding strategy
  -> implement selected shape
  -> prove V8 and Bun/JSC coexist in one product process
```

The preferred first question is whether Nimbus can produce a source-owned
shared Bun/JSC adapter that is built as PIC, exports only the Nimbus C ABI, and
is loaded with local symbol scope. That preserves the user's desired
in-process runtime model without requiring a single global symbol table to
merge real V8 with Bun's V8 shim and bundled native dependencies.

A static path remains possible only if the Nimbus Bun fork owns all required
changes:

- avoid or change Rust `staticlib` packaging so Rust runtime symbols do not
  collide with the host Rust binary;
- hide or namespace Highway symbols;
- remove, hide, or namespace Bun V8 shim symbols from the embedder link;
- keep the `nimbus_bun_simdutf` namespace audit.

Binary-only post-processing and unsafe linker workarounds are not acceptable
completion evidence for BJA8.

## Verification Status

Passing evidence before this blocker:

- Local macOS linked pure invocation passed.
- Local no-manifest linked feature tests passed.
- `nimbus/bun` tag `bun-v1.4.0-nimbus.1` is reproducible and clean.
- Debian 13 built and audited the source-owned `nimbus_bun_simdutf` artifacts.

Blocked evidence:

- Debian 13 same-process V8 plus Bun/JSC static co-link still fails under
  `lld` with duplicate global symbols.
- `make verify-bun-jsc-linked-adapter` cannot be considered product-green on
  Linux until BJA4L4-BJA4L6 resolve the global linkage contract.

## Next

- Extend the Linux symbol audit from `simdutf` to Rust runtime/staticlib,
  Highway, and Bun V8 shim families.
- Decide between a source-owned PIC/shared in-process adapter and deeper static
  namespace/hiding.
- Update the verifier so the selected path is the only passing linked path.
