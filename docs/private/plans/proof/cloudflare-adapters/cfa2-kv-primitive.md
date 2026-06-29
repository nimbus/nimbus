# CFA2 — KV Primitive Prerequisite Gate

Captured 2026-06-28 on branch `codex/nkv-cloudflare-foundation`.

## Decision

CFA2 is a prerequisite gate, not duplicate Cloudflare code. The flat
`TenantKvStore` capability is owned by the `nimbus-kv` foundation plan and was
landed through NKV0/F2 before CFA3.

## Evidence

- `bash scripts/verify-cloudflare-adapters.sh`
  - condition 5 passed:
    `TenantKvStore trait + kv_* methods present in nimbus-storage`
  - total after CFA1/CFA2 checkpoint: `5 passed, 7 failed`.
- `bash scripts/verify-nimbus-kv-foundation.sh`
  - final NKV F5 closeout result: `9 passed, 0 failed`.
- Latest branch workflow checkpoint for commit
  `18647e3de5f919e47ef13bb29a1d475ad2cf66c9`:
  - CI run `28331361216`: completed success, 29 jobs; the only non-success job
    was the intentionally skipped nightly matrix.
  - Nimbus KV Conformance run `28331361209`: completed success, 1 job.

## Handoff to CFA3

CFA3 must consume the existing `TenantKvStore` seam for Workers KV behavior.
It must not introduce a second flat KV persistence trait or a standalone
Cloudflare-owned KV substrate.
