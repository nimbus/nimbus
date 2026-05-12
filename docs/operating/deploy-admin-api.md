# Deploy Admin API

The deploy admin API is the server-side contract behind `nimbus deploy`. It is
available at a stable route, but it is disabled unless the server was started
with `NIMBUS_DEPLOY_TOKEN`.

## Route

| Method | Path | Purpose |
| --- | --- | --- |
| `POST` | `/api/admin/deploy` | validate, diff, and optionally activate app artifacts |

Clients must send:

```http
Authorization: Bearer <NIMBUS_DEPLOY_TOKEN>
Content-Type: application/json
```

If `NIMBUS_DEPLOY_TOKEN` was not present when the server started, the route
returns `401` and no deploy is possible through this API. Invalid or missing
bearer tokens also return `401`.

## Request

```json
{
  "dry_run": false,
  "artifacts": {
    "functions_json": { "functions": [] },
    "http_routes_json": { "routes": [] },
    "schema_json": { "tables": {} },
    "auth_config_json": {},
    "bundle_mjs": "export const value = 1;\n",
    "bundle_sha256": "64-character lowercase sha256 hex"
  }
}
```

`functions_json` is required. `http_routes_json`, `schema_json`,
`auth_config_json`, `bundle_mjs`, and `bundle_sha256` are optional. Runtime
bundles are a pair: if either `bundle_mjs` or `bundle_sha256` is supplied, both
must be supplied.

This request shape is the **current Convex-compatible artifact family**. The
Cloud Functions plan keeps the same staging, integrity, and generation-swap
guarantees, but uses a sibling internal artifact family under
`.nimbus/firebase/` with its own manifest envelope rather than forcing Cloud
Functions metadata through the Convex manifest schema. See
[Cloud Functions artifact contract](cloud-functions-artifact-contract.md) and
[Cloud Functions target binding contract](cloud-functions-target-binding-contract.md).

## Server Behavior

The server stages uploaded artifacts into a temporary app directory and loads
them through the same Convex registry path used by `nimbus start --app-dir`.
That staging step validates manifest readability, optional HTTP routes, schema
and index definitions, auth config readability, and runtime bundle integrity.

Dry-runs validate and diff without changing the active generation. Non-dry-run
deploys activate only after staging and validation succeed. Activation swaps the
active app generation atomically: in-flight requests keep the generation they
already captured, while new requests observe the new generation after the swap.

Activation-time rollback is internal only in v1. If staging or validation
fails, the previous generation remains live. There is no user-facing rollback
command until Nimbus defines retained generation history and operator intent.

## Response

```json
{
  "dry_run": false,
  "activated": true,
  "generation": 2,
  "previous_generation": 1,
  "diff": {
    "functions": {
      "added": [{ "name": "messages:list", "kind": "query" }],
      "changed": [],
      "removed": []
    },
    "http_routes": {
      "added": [{ "key": "GET /healthz" }],
      "changed": [],
      "removed": []
    },
    "schema_changed": true,
    "indexes_changed": true,
    "runtime_bundle_changed": true
  }
}
```

The generation counter is process-local. A dry-run returns the current
generation and `activated: false`; a successful activation returns the new
generation.
