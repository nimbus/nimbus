# CFA5 - env.NS Worker KV End-to-End Proof

Captured 2026-06-28 on branch `codex/nkv-cloudflare-foundation`.

## Scope landed

- Added `crates/nimbus-server/src/adapters/cloudflare/host_bridge.rs` with a
  tenant-scoped `CloudflareHostBridge`.
- The bridge handles `HostCallPayload::CfKvGet`, `CfKvPut`, `CfKvDelete`, and
  `CfKvList`, using the same Cloudflare KV storage-key, metadata, TTL, cursor,
  and namespace-resolution helpers as the REST adapter.
- The bridge validates the payload tenant against the invocation tenant before
  touching storage. A forged Worker KV host call for another tenant fails before
  storage access.
- Worker KV reads return `value_base64` plus metadata to the CFA4 Worker shim,
  which decodes `text`, `json`, and `arrayBuffer` values for `env.NS.get()` and
  `env.NS.getWithMetadata()`.
- Worker KV lists return the Workers binding shape:
  `{ keys, list_complete, cursor }`.

## Worker source

The e2e test runs this real ES-module Worker on `nimbus-runtime`:

```js
export default {
  async fetch(_request, env) {
    await env.NS.put("greeting", JSON.stringify({ text: "hello" }), {
      metadata: { lang: "en" },
      expirationTtl: 120,
    });
    const value = await env.NS.get("greeting", "json");
    const withMetadata = await env.NS.getWithMetadata("greeting", "json");
    const list = await env.NS.list({ prefix: "g", limit: 10 });
    return new Response(JSON.stringify({ value, withMetadata, list }), {
      headers: { "content-type": "application/json" },
    });
  },
};
```

Invocation binds:

```json
{
  "env": {
    "NS": {
      "type": "kv_namespace",
      "tenant_id": "tenant-a",
      "namespace": "namespace-prod"
    }
  }
}
```

The configured `CloudflareBindingRegistry` maps `namespace-prod` to the binding
name `NS`; storage lands under the same `cloudflare-kv\0<namespace>\0<key>`
ordered prefix used by the REST adapter.

## Assertions

`cloudflare_worker_env_ns_e2e_round_trips_kv` asserts:

- `env.NS.put()` stores a JSON value with metadata and a valid
  `expirationTtl`.
- `env.NS.get("greeting", "json")` returns `{ "text": "hello" }`.
- `env.NS.getWithMetadata("greeting", "json")` returns the same JSON value
  plus `{ "lang": "en" }`.
- `env.NS.list({ prefix: "g", limit: 10 })` returns `greeting`, marks the list
  complete, and includes the stored metadata.

## Verification

- `cargo fmt --all`
  - passed.
- `cargo test -p nimbus-server env_ns -- --nocapture`
  - `1 passed; 0 failed; 0 ignored; 487 filtered out`.
  - integration binaries selected zero additional tests:
    `dynamodb_spec` 0/27, `mongodb_spec` 0/23, `reactive_loop` 0/32.
- `cargo test -p nimbus-server adapters::cloudflare -- --nocapture`
  - `5 passed; 0 failed; 0 ignored; 483 filtered out`.
  - integration binaries selected zero additional tests:
    `dynamodb_spec` 0/27, `mongodb_spec` 0/23, `reactive_loop` 0/32.
- `bash scripts/verify-cloudflare-adapters.sh`
  - `8 passed, 4 failed`.
  - CFA5 is green.
  - Remaining failures are expected future rows: CFA6, CFA7+CFA8, CFA9, and
    the later security posture gate.
