# CFA7/CFA8 - Durable Objects Storage, Lifecycle, Alarms, and Hibernation Proof

Captured 2026-06-28 on branch `codex/nkv-cloudflare-foundation`.

## Scope landed

- Added `crates/nimbus-server/src/adapters/cloudflare/durable_objects/`.
- Added `DurableObjectSubstrate` and `DurableObjectStub` over the CFA6
  `(tenant_id, do_namespace, do_id)` directory key.
- Added per-instance concurrency lanes keyed by `DurableObjectInstanceKey`.
  `block_concurrency_while` holds only the target DO lane, so unrelated DOs in
  the same tenant can continue making write progress.
- Added activation claims with monotonically increasing `lease_epoch`.
  Stale activations are rejected before writes commit and before queued output
  messages are released.
- Added per-instance storage transactions through the engine-backed
  `TenantKvStore::kv_apply_batch` path. This keeps DO state in Nimbus storage
  and avoids a side store.
- Added a minimal `sql_exec` proof cursor for the CFA7 SQLite-backed API seam.
  Full SQL compatibility remains the Worker-facing API expansion; the storage
  substrate now has the cursor shape and per-instance authority boundary.
- Added alarm persistence (`set_alarm`, `get_alarm`, `delete_alarm`) fenced to
  the current activation lease.
- Added WebSocket hibernation records:
  `accept_web_socket`, `serialize_attachment`, `deserialize_attachment`,
  `get_web_sockets`, and `set_web_socket_auto_response`.
- Made Cloudflare startup use the shared non-loopback bind guard explicitly.

## Trust boundaries

- Tenant isolation is enforced by deriving all storage keys from
  `DurableObjectInstanceKey`, whose lead component is `tenant_id`.
- `idFromString` parses a 64-hex object id, but it does not carry tenant
  authority. A caller can only construct a stub under its authenticated tenant
  and namespace.
- A lease from one tenant/namespace/object cannot authorize a different stub.
- Output-gate messages are returned only after the storage transaction
  succeeds. Stale-epoch failures return an error and release no queued output.

## Tests added

`crates/nimbus-server/src/adapters/cloudflare/durable_objects/mod.rs` covers:

- per-instance storage transaction round-trip and delete;
- `sql_exec("select 1")` cursor proof;
- cross-tenant `idFromString` denial for the same 64-hex DO id;
- independent per-DO lanes with one DO stalled in `blockConcurrencyWhile`;
- stale activation epoch rejecting loser writes and discarding queued output;
- alarm set/get/delete round-trip;
- WebSocket hibernation attachment and auto-response round-trip.

`crates/nimbus-bin/src/start/tests/adapters.rs` covers:

- Cloudflare routes refusing a non-loopback main listener bind without
  `--allow-network`.

## Verification

- `cargo fmt --all --check`
  - passed.
- `cargo test -p nimbus-server durable_object -- --nocapture`
  - `5 passed; 0 failed; 0 ignored; 0 measured; 488 filtered out`.
  - integration binaries selected zero additional tests:
    `dynamodb_spec` 0/27, `mongodb_spec` 0/23, `reactive_loop` 0/32.
- `cargo test -p nimbus-bin cloudflare_routes_refuse_non_loopback -- --nocapture`
  - `1 passed; 0 failed; 0 ignored; 0 measured; 791 filtered out`.
  - integration binary selected zero additional tests:
    `server_discovery_serde` 0/2.
- `bash scripts/verify-cloudflare-adapters.sh`
  - `11 passed, 1 failed`.
  - CFA7/CFA8 and security posture are green.
  - Remaining failure is expected until CFA9: operator doc + all ledger rows
    done + final PR closeout.
