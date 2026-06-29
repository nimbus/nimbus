# NKV0 F1 Proof - RESP Server

Captured 2026-06-27 on branch `codex/nkv-cloudflare-foundation`.

## What Landed

- New workspace crate `crates/nimbus-kv/`.
- RESP2/RESP3 decode/encode uses `redis-protocol` `6.0.0`.
- Public server entrypoints: `run_listener` and `serve`.
- Root CLI entrypoint: `nimbus kv --bind 127.0.0.1:6380 --tenant demo`.
- Shared pure bind guard: `nimbus_core::refuse_non_loopback_bind`.
- Dev credential registry binds each username/password to exactly one
  `TenantId`.
- Minimal F1 command surface: `AUTH`, `HELLO`, `PING`, `ECHO`, `QUIT`,
  `COMMAND`, `CLIENT SETINFO`, and fail-closed `SELECT`.

## Behavioral Proof

- `cargo fmt --all --check` - passed.
- `cargo test -p nimbus-core net::` - 3 passed, 0 failed, 134 filtered out.
- `cargo test -p nimbus-kv` - 5 integration tests passed, 0 failed:
  - `redis_rs_client_connects_and_ping_echo_round_trip`
  - `hello_3_negotiates_resp3`
  - `listener_rejects_non_loopback_bind`
  - `unauthenticated_command_is_rejected`
  - `tenant_a_credential_cannot_read_tenant_b_keys`
- `cargo test -p nimbus-bin kv_root_command_parses` - 1 passed, 0 failed,
  784 filtered out.
- `bash scripts/verify-nimbus-kv-foundation.sh` - 5 passed, 4 failed. The
  remaining failures are the expected F2-F5 gates.

## JS Prerequisites Used For CLI Proof

Fresh worktree direct `cargo test -p nimbus-bin kv_root_command_parses` first
hit the documented `nimbus-assets` build-contract prerequisites. Resolved by:

- `npm ci` - installed 557 packages; npm reported existing audit findings
  (2 low, 3 high), no install failure.
- `npm run build -w nimbus-ui` - passed; emitted existing route-file warnings
  and Vite chunk-size warnings.
- `npm run build:embedded-packages` - passed; staged 8 embedded packages.

The generated UI/package artifacts are ignored; only Rust/source changes and
`Cargo.lock` are part of the tracked F1 diff.
