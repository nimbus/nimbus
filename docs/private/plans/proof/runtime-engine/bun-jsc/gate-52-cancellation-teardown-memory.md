# Gate 52: BJA6 Cancellation, Teardown, And Memory Policy Proof

Date: 2026-05-24

## Purpose

`BJA6` proves that the optional in-process Bun/JSC backend preserves Nimbus'
untrusted-runtime lifecycle contract after HostBridge support lands. The gate
is about cancellation and teardown semantics around the linked adapter, plus
the memory-policy posture that keeps untrusted Bun/JSC workloads behind an
outer quota instead of claiming a JSC hard heap limit Nimbus has not proven.

## Source

The verified Bun source remains the Nimbus-owned fork:

```text
Repository: https://github.com/nimbus/bun
Branch: nimbus/bja4l2-simdutf-namespace
Tag: bun-v1.4.0-nimbus.4
Revision: 7c6dd4312e437c67a6c4c8cbb252f0d7ae898db8
```

No Bun source change was required for this gate. The `.4` source already
exports the lifecycle, cancellation, and memory probes:

- `nimbus_bun_embed_probe_timeout_and_cancel`
- `nimbus_bun_embed_probe_memory_behavior`
- `nimbus_bun_embed_probe_lifecycle_reuse_stress`

## Implementation

Nimbus now rejects a pre-cancelled Bun/JSC invocation before entering the
linked adapter. If the linked adapter returns `NimbusRuntimeError::Cancelled`,
the Bun/JSC pool records `CancelRequested` and still completes the normal
terminate, reset/discard, and teardown acknowledgements.

HostBridge cancellation is no longer flattened into a generic denial. The
linked callback maps `NimbusRuntimeError::Cancelled` into a structured guest
error with code `cancelled`, while preserving the no-token rule for guest code.

The loaded shared-adapter tests now cover:

- HostBridge cancellation visible to guest code without raw host tokens.
- Promise/microtask progress through the Bun/JSC invocation wrapper.
- Fresh VM/discard behavior for untrusted invocations by proving global state
  does not survive between two calls.

The memory stance remains explicit: product-selectable Bun/JSC requires the
fresh/discard pool with `outer_quota_required`. Bun's native memory probe
records JSC heap growth, GC/shrink behavior, and the fact that no hard JSC heap
limit has been proven, so Nimbus continues to rely on outer sandbox/resource
quota enforcement for untrusted Bun/JSC.

## Passing Proof

Local focused checks:

- `cargo fmt --all --check` passed.
- `cargo test -p nimbus-runtime --lib backends::bun_jsc` passed 9 tests.
- `cargo test -p nimbus-runtime --features bun-jsc-linked-adapter --lib backends::bun_jsc`
  passed 12 tests.
- `RUSTFLAGS=--cfg=nimbus_bun_jsc_shared_adapter cargo test -p nimbus-runtime
  --features bun-jsc-linked-adapter --test bun_jsc_linked_adapter --no-run`
  compiled the loaded shared-adapter integration tests.
- `git diff --check` passed.

The local Mac full linked verifier reached the default no-link contract and
linked no-shared-library unit gate, then stopped during Bun's native shared
adapter build because this machine does not have a local `vendor/WebKit`
checkout or `BUN_WEBKIT_PATH`. That run is not counted as BJA6 completion
evidence; the source-backed loaded-adapter proof is the Debian 13 run below.

Debian 13 `minicloud` checks used:

```text
Nimbus repo: /home/nimbus/src/github.com/nimbus/nimbus-worktrees/bja5-hostbridge
Bun repo: /home/nimbus/src/github.com/nimbus/bun-worktrees/bja5-hostbridge
Bun proof root: /home/nimbus/.cache/nimbus-bun-proof
```

The warmed final run of `bash scripts/verify-bun-jsc-linked-adapter.sh`
passed against the `.4` source revision after applying the BJA6 Nimbus patch.
Evidence:

- default no-link runtime contract passed.
- linked no-shared-library unit contract passed 12 tests, including
  pre-cancel rejection and cancelled-adapter teardown metrics.
- Bun source export check found all 11 required Nimbus ABI symbols.
- Bun Rust format passed.
- generated build graph safety policy passed.
- shared adapter export audit found exactly the 11 Nimbus C ABI exports.
- leaked native defined symbol count was 0.
- ELF audit found no `STATIC_TLS`.
- simdutf namespace audit still separates Bun/WebKit from V8/rusty_v8.
- linked same-process unit lane passed 12 tests.
- `tests/bun_jsc_linked_adapter.rs` passed 7 integration tests:
  same-process V8 plus Bun/JSC coexistence, HostBridge allow, HostBridge deny,
  forged tenant/context rejection, HostBridge cancellation, microtask progress,
  and fresh/discard state reset.
- Nimbus and Bun whitespace diff checks passed.

## Decision

`BJA6` is complete. Bun/JSC remains product-selectable only through the
fresh/discard, outer-quota-required policy. Cancellation is deterministic at
the Nimbus pool boundary, HostBridge cancellation is guest-visible without
exposing host tokens, and the linked adapter tears down or discards untrusted
state after each invocation.

Next gate: `BJA7` must finish the product metadata and diagnostics promotion
now that pure invocation, HostBridge behavior, cancellation, teardown, and
fresh/discard lifecycle are proven through the linked adapter.
