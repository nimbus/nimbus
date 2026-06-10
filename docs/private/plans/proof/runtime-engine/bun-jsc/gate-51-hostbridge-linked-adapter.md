# Gate 51: BJA5 HostBridge Linked Adapter Proof

Date: 2026-05-24

## Purpose

`BJA5` proves that the optional in-process Bun/JSC backend can use the same
Nimbus `HostBridge` seam as the existing V8/Deno lane. This gate must preserve
the shared-adapter isolation properties from Gates 49-50 while adding
tenant-scoped host calls and denial behavior.

## Source

The verified Bun source is the Nimbus-owned fork:

```text
Repository: https://github.com/nimbus/bun
Branch: nimbus/bja4l2-simdutf-namespace
Tag: bun-v1.4.0-nimbus.4
Revision: 7c6dd4312e437c67a6c4c8cbb252f0d7ae898db8
```

`git ls-remote nimbus refs/heads/nimbus/bja4l2-simdutf-namespace
refs/tags/bun-v1.4.0-nimbus.4` returned the expected revision for both refs.

## Implementation

The Bun fork adds:

- `nimbus_bun_embed_invoke_program_wrapper_json_with_host_bridge`.
- `NimbusBunEmbedHostCallJsonFn`, a C ABI callback that receives serialized
  `HostCallRequest` JSON and returns structured allow/deny JSON.
- `__nimbusHostBridgeCallJson`, `__nimbusSyncHostValue`,
  `__nimbusAsyncHostValue`, and a minimal `__nimbusCreateContext` only inside
  the HostBridge-capable invocation entrypoint.

Nimbus loads the new symbol from `libnimbus_bun_jsc_embedder`, passes a stack
owned callback context containing `Arc<dyn HostBridge>` and
`HostCallCancellation`, and maps callback failures into structured guest-visible
errors. The existing pure invocation ABI remains required by the source
contract so export drift is still caught.

## Passing Proof

Local checks:

- `cargo check -p nimbus-runtime` passed.
- `cargo fmt --all --check` passed.
- `git diff --check` passed.
- In `/Users/jack/src/github.com/nimbus/bun`,
  `cargo fmt --all --check` and `git diff --check` passed.

Debian 13 `minicloud` checks used:

```text
Nimbus repo: /home/nimbus/src/github.com/nimbus/nimbus-worktrees/bja5-hostbridge
Bun repo: /home/nimbus/src/github.com/nimbus/bun-worktrees/bja5-hostbridge
Bun proof root: /home/nimbus/.cache/nimbus-bun-proof
```

The warmed final run of `bash scripts/verify-bun-jsc-linked-adapter.sh`
passed against the `.4` source revision. Evidence:

- default no-link runtime contract passed.
- linked no-shared-library unit contract passed 10 tests.
- Bun source export check found all 11 required Nimbus ABI symbols.
- Bun Rust format passed with no owned embed-probe deprecation warnings.
- generated build graph safety policy passed.
- shared adapter export audit found exactly the 11 Nimbus C ABI exports.
- leaked native defined symbol count was 0.
- ELF audit found no `STATIC_TLS`.
- simdutf namespace audit still separates Bun/WebKit from V8/rusty_v8.
- linked same-process unit lane passed 10 tests.
- `tests/bun_jsc_linked_adapter.rs` passed 4 integration tests:
  pure V8 plus Bun/JSC coexistence, HostBridge allow, HostBridge deny, and
  forged tenant/context rejection.
- Nimbus and Bun whitespace diff checks passed.

## Decision

`BJA5` is complete. Bun/JSC remains an optional linked runtime backend beside
the Deno/V8 lane, and HostBridge authority stays host-owned: guest code can
request host operations, but it receives no raw host token and cannot create
authority by forging tenant/context fields in payload JSON.

Next gate: `BJA6` must harden cancellation, teardown, fresh/discard lifecycle,
and outer memory policy for the HostBridge-capable linked adapter.
