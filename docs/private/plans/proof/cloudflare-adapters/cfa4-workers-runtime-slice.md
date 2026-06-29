# CFA4 - Workers Runtime Slice Proof

Captured 2026-06-28 on branch `codex/nkv-cloudflare-foundation`.

## Scope landed

- Added `InvocationKind::CloudflareWorkerFetch` so Worker `fetch` dispatch is
  explicit and does not hide under the Convex-shaped `Action` path.
- Added a Cloudflare Worker bootstrap source in `nimbus-runtime` that invokes
  real ES module Worker syntax:
  `export default { fetch(request, env, ctx) }`.
- Added a Worker `Request`/`Response` shim for the minimal slice:
  - request URL, method, headers, and buffered text/base64 request bodies;
  - response status, status text, headers, and buffered text body
    serialization;
  - `ctx.waitUntil` and `ctx.passThroughOnException`.
- Added `env` injection with KV namespace binding stubs. KV methods call the
  session-bound `CfKv*` host operations added in CFA3:
  `get`, `getWithMetadata`, `put`, `delete`, and `list`.
- Added Deno runtime ops for `op_nimbus_cf_kv_get`,
  `op_nimbus_cf_kv_put`, `op_nimbus_cf_kv_delete`, and
  `op_nimbus_cf_kv_list`.
- Added session binding to the `RuntimeAsyncCfKv*Payload` structs. Worker KV
  calls now carry `host_call_session_id:
  cloudflare_worker_fetch:<function_name>` and are enforced by the shared
  host-call session guard.
- Added explicit Convex fail-closed handling for `CloudflareWorkerFetch` so the
  Convex subscription/nested-runtime paths cannot treat it as a Convex query,
  mutation, or action.

## Deliberate boundaries

- This is the minimal module-worker slice only. It intentionally does not claim
  the full Workers runtime surface.
- Unsupported Worker APIs fail loudly by name. The tests cover `request.cf`;
  the shim also defines a named `caches.default` rejection and a named rejection
  for KV `stream` reads.
- Bodies are buffered for the CFA4 slice. Full WHATWG streaming fidelity remains
  a follow-on Workers-runtime-surface band.
- CFA4 proves runtime dispatch and binding injection. CFA5 is still the
  end-to-end `env.NS` conformance proof against the real Cloudflare KV adapter
  and storage primitive.

## Tests added

`crates/nimbus-runtime/src/runtime/tests/basic_invocation/cloudflare_workers.rs`
covers:

- A trivial Worker default export returning a `Response` with expected status,
  headers, body, `Request`, `env`, and `ctx.passThroughOnException` behavior.
- A KV namespace binding whose `put` and `list` methods use the session-bound
  `CfKv*` host operations and encode values as base64.
- A Worker referencing `request.cf` getting the named unsupported API error.
- A rejected `ctx.waitUntil` promise being classified by the post-response
  waitUntil drain.

## Verification

- `cargo fmt --all`
  - passed.
- `cargo test -p nimbus-runtime cloudflare_workers -- --nocapture`
  - `5 passed; 0 failed; 0 ignored; 1237 filtered out`.
  - integration/helper binaries selected zero additional tests:
    `build_node22_anchor_snapshot` 0/0,
    `bun_jsc_linked_adapter` 0/0, `embedded_anchor` 0/1,
    `engine_proofs` 0/1, `locker_smoke` 0/8.
- `cargo test -p nimbus-runtime host_call -- --nocapture`
  - `16 passed; 0 failed; 5 ignored; 1221 filtered out`.
  - integration/helper binaries selected zero additional tests:
    `build_node22_anchor_snapshot` 0/0,
    `bun_jsc_linked_adapter` 0/0, `embedded_anchor` 0/1,
    `engine_proofs` 0/1, `locker_smoke` 0/8.
- `cargo test -p nimbus-server adapters::cloudflare::kv -- --nocapture`
  - `2 passed; 0 failed; 0 ignored; 485 filtered out`.
  - integration binaries selected zero additional tests:
    `dynamodb_spec` 0/27, `mongodb_spec` 0/23, `reactive_loop` 0/32.
- `cargo test -p nimbus-convex host_bridge -- --nocapture`
  - `4 passed; 0 failed; 0 ignored; 19 filtered out`.
- `cargo test -p nimbus-server nested_runtime -- --nocapture`
  - `2 passed; 0 failed; 0 ignored; 485 filtered out` in the server unit
    binary.
  - integration binaries selected: `reactive_loop` 1/32 passed; DynamoDB 0/27
    and MongoDB 0/23 selected.
- `bash scripts/verify-cloudflare-adapters.sh`
  - `7 passed, 5 failed`.
  - CFA4 is green.
  - Remaining failures are expected future rows: CFA5, CFA6, CFA7+CFA8, CFA9,
    and the later security posture gate.
