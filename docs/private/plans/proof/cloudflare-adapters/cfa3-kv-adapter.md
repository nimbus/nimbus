# CFA3 — Workers KV Adapter Proof

Captured 2026-06-28 on branch `codex/nkv-cloudflare-foundation`.

## Scope landed

- Added `crates/nimbus-server/src/adapters/cloudflare/kv/` and mounted it from
  `build_cloudflare_router`.
- Implemented the Cloudflare KV REST routes over the existing NKV0 F2
  `TenantKvStore` primitive:
  - `GET` / `PUT` / `DELETE` values.
  - `GET` metadata.
  - `GET` key listing with `result_info.cursor` and `list_complete`.
- Stored values under a Cloudflare-specific ordered prefix and stored metadata
  in the KV metadata plane.
- Enforced the CFA3 contract bounds:
  - key length <= 512 bytes
  - value length <= 25 MiB
  - metadata JSON <= 1024 bytes
  - `expiration` and `expiration_ttl` are mutually exclusive
  - `expiration_ttl` must be at least 60 seconds
  - list limit <= 1000
- Added `HostCallOperation::CfKvGet`, `CfKvPut`, `CfKvDelete`, and `CfKvList`
  payloads in `crates/nimbus-runtime/src/host.rs` for the CFA4 Workers runtime
  bridge.
- Added explicit Convex bridge rejection for `CfKv*` operations so the
  adapter-owned Cloudflare surface cannot accidentally route through Convex.
- Extended the `CloudflareConfig` credential mapping with a per-credential
  tenant binding for the REST front door.

## Deliberate boundaries

- This is the redb-backed CFA wedge over NKV0 F2. NKV0 F2 landed
  `TenantKvStore` for redb; cross-backend KV coverage remains the NKV1/follow-on
  storage completeness work.
- Nimbus returns strongly consistent reads from the local durable store. That is
  a compatible superset of Cloudflare KV's eventual-consistency window, not an
  attempt to reproduce edge propagation delay.
- CFA3 proves the REST front door and host-call ABI shape. The in-Worker
  `env.NS` proof is CFA5 after CFA4 lands the Workers runtime slice.

## Verification

- `cargo fmt --all --check`
  - passed.
- `cargo test -p nimbus-server adapters::cloudflare::kv -- --nocapture`
  - `2 passed; 0 failed; 0 ignored; 485 filtered out`
  - integration test binaries also ran zero selected tests:
    `dynamodb_spec` 0/27, `mongodb_spec` 0/23, `reactive_loop` 0/32.
- `cargo test -p nimbus-runtime host_call -- --nocapture`
  - `16 passed; 0 failed; 5 ignored; 1216 filtered out`
  - integration test binaries ran zero selected tests for this selector.
- `cargo test -p nimbus-convex host_bridge -- --nocapture`
  - `4 passed; 0 failed; 0 ignored; 19 filtered out`.
- `cargo test -p nimbus-bin start::adapters -- --nocapture`
  - `20 passed; 0 failed; 0 ignored; 771 filtered out`
  - `server_discovery_serde` ran zero selected tests: 0/2.
- `bash scripts/verify-cloudflare-adapters.sh`
  - `6 passed, 6 failed`
  - CFA3 is green.
  - Remaining failures are expected future rows: CFA4, CFA5, CFA6, CFA7+CFA8,
    CFA9, and the later security posture gate.
