# CFA1 — Adapter Skeleton, Config, and Wiring

Captured 2026-06-28 on branch `codex/nkv-cloudflare-foundation`.

## Scope landed

- Added `crates/nimbus-server/src/adapters/cloudflare/`.
- `mod.rs` exposes `CloudflareConfig` and `build_cloudflare_router`.
- `config.rs` parses `wrangler.jsonc`, `wrangler.json`, and `wrangler.toml`
  binding declarations into a typed registry:
  - `kv_namespaces`
  - `durable_objects.bindings`
  - `d1_databases`
  - `r2_buckets`
- Registered `pub mod cloudflare;` in `crates/nimbus-server/src/adapters/mod.rs`.
- Threaded `CloudflareConfig` through:
  - `ServeOptions::with_cloudflare`
  - `RouterOptions::with_cloudflare_config`
  - `RouterBuildConfig::with_cloudflare`
  - `AppStateConfig` / `DeploymentState`
  - system listener-state recording as adapter `cloudflare`, protocol `http`
- Added `--no-cloudflare` as a default-on `nimbus start` adapter toggle.
- `crates/nimbus-bin/src/start/adapters.rs` now resolves the Cloudflare
  config from the app dir's `wrangler.*` file when present.

No Cloudflare data behavior landed in CFA1. The router merge is intentionally
inert until CFA3/CFA6 add KV and Durable Object routes.

## Verification

- `cargo test -p nimbus-server adapters::cloudflare::config -- --nocapture`
  - `2 passed; 0 failed; 0 ignored; 483 filtered out`
  - integration test binaries also ran zero selected tests:
    `dynamodb_spec` 0/27, `mongodb_spec` 0/23, `reactive_loop` 0/32.
- `cargo test -p nimbus-bin start::adapters -- --nocapture`
  - `20 passed; 0 failed; 0 ignored; 771 filtered out`
  - `server_discovery_serde` ran zero selected tests: 0/2.
- `cargo test -p nimbus-server router_prepare_system_tenant_records_enabled_adapter_listeners -- --nocapture`
  - `1 passed; 0 failed; 0 ignored; 484 filtered out`
  - integration test binaries also ran zero selected tests:
    `dynamodb_spec` 0/27, `mongodb_spec` 0/23, `reactive_loop` 0/32.
- `cargo test -p nimbus-bin cli_adapters_serve_store_backed_by_default_and_opt_outs_disable -- --nocapture`
  - `1 passed; 0 failed; 0 ignored; 790 filtered out`
  - `server_discovery_serde` ran zero selected tests: 0/2.
- `cargo fmt --all --check`
  - passed after rustfmt line wrapping.
- `bash scripts/verify-cloudflare-adapters.sh`
  - `5 passed, 7 failed`
  - CFA1 is green.
  - Remaining failures are expected future rows: CFA3, CFA4, CFA5, CFA6,
    CFA7+CFA8, CFA9, and the later security posture gate.

## Notes

The active plan text still mentions `crates/nimbus-server/src/start/adapters.rs`
for the start toggle, but the live code and verifier use
`crates/nimbus-bin/src/start/adapters.rs`. CFA1 was implemented against the live
CLI crate path.
