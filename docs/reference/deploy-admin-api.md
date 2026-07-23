---
title: Deploy & admin API
description: The staging, diff, and activation contract behind nimbus deploy — endpoint, credentials, request and response schemas, and error catalog.
sidebar:
  label: Deploy & admin API
  order: 4
---

The deploy admin API is the server-side contract behind `nimbus deploy`:
one endpoint that validates uploaded app artifacts, reports a diff against
the active app generation, and (unless the request is a dry run) atomically
activates the new generation.

## Endpoint

| Method | Path | Purpose |
| --- | --- | --- |
| `POST` | `/api/admin/deploy` | Validate, diff, and optionally activate app artifacts |

Requests and responses are JSON (`Content-Type: application/json`).

## Enablement

| Condition | Effect |
| --- | --- |
| `NIMBUS_DEPLOY_TOKEN` set in the server's environment at startup | Endpoint enabled; the value is the expected deploy bearer token |
| `NIMBUS_DEPLOY_TOKEN` not set at startup | Every request returns `401` (`deploy admin API is disabled; set NIMBUS_DEPLOY_TOKEN before starting the server`) |

## Authentication

Two independent credentials gate the endpoint:

| Credential | Header | Required when |
| --- | --- | --- |
| Deploy token | `Authorization: Bearer <NIMBUS_DEPLOY_TOKEN>` | Always |
| Local admin token | `X-Nimbus-Admin-Token: <token>` | Whenever the server runs with local security — always the case for `nimbus start` |

Notes:

- The deploy bearer comparison is constant-time; a wrong or
  prefix-matching token returns `401` (`invalid deploy admin token`).
- The local admin gate accepts only the `X-Nimbus-Admin-Token` header on
  this route. Operator session cookies and `Authorization`-bearer admin
  tokens, which other admin routes accept, are not valid here — the
  `Authorization` header is reserved for the deploy token.
- The local admin token file lives at
  `~/.local/share/nimbus/auth/token` (Linux),
  `~/Library/Application Support/nimbus/auth/token` (macOS), or
  `%LOCALAPPDATA%\nimbus\auth\token.json` (Windows).
- Requests carrying a browser `Origin` header are restricted to loopback
  HTTP origins on the server's port; others return `403`.

## Request schema

```json
{
  "dry_run": false,
  "convex_silo": "demo",
  "artifacts": {
    "convex": {
      "functions_json": { "functions": [] },
      "http_routes_json": { "routes": [] },
      "schema_json": { "tables": {} },
      "auth_config_json": {},
      "bundle_mjs": "export const value = 1;\n",
      "bundle_sha256": "<64-character lowercase sha256 hex>"
    },
    "cloud_functions": {
      "artifact_json": { },
      "targets_json": { },
      "bundle_mjs": "export const value = 1;\n",
      "bundle_sha256": "<64-character lowercase sha256 hex>"
    }
  }
}
```

| Field | Type | Required | Meaning |
| --- | --- | --- | --- |
| `dry_run` | boolean | no (default `false`) | Validate and diff without activating |
| `convex_silo` | string | with `artifacts.convex` | Trusted silo that receives this Convex deployment's auth verifier |
| `artifacts.convex` | object | at least one family | Convex-compatible artifact family |
| `artifacts.cloud_functions` | object | at least one family | Cloud Functions artifact family |

### `artifacts.convex`

| Field | Type | Required |
| --- | --- | --- |
| `functions_json` | JSON value | yes |
| `http_routes_json` | JSON value | no |
| `schema_json` | JSON value | no |
| `auth_config_json` | JSON value | no |
| `bundle_mjs` | string | paired |
| `bundle_sha256` | string | paired |

`bundle_mjs` and `bundle_sha256` are a pair: supplying one without the
other is a `400`.

### `artifacts.cloud_functions`

| Field | Type | Required |
| --- | --- | --- |
| `artifact_json` | JSON value | yes |
| `targets_json` | JSON value | yes |
| `bundle_mjs` | string | yes |
| `bundle_sha256` | string | yes |

## Server behavior

| Fact | Detail |
| --- | --- |
| Staging | Artifacts are written to a private (`0700`) randomized temporary directory and loaded through the same registry path as a server started directly from an app directory |
| Validation | Manifest readability, optional HTTP routes, schema and index definitions, auth config readability, and runtime-bundle SHA-256 integrity are all checked during staging |
| Activation | Non-dry-run deploys activate only after staging and validation succeed; the swap is atomic — in-flight requests keep the generation they captured, new requests observe the new one |
| Failure | If staging or validation fails, the previous generation remains active |
| Generation counter | Process-local: `0` on a server started without an app, `1` when started with one, incremented per activation; it resets on restart |
| Rollback | There is no rollback endpoint and no retained generation history |
| Partial families | A deploy that includes only one artifact family keeps the other family's active registry unchanged |
| Convex auth scope | A Convex deploy replaces the verifier only for `convex_silo`; existing verifier bindings for other silos remain unchanged, and an unprovisioned silo never falls back to another verifier |
| Diff scope | The `diff` object is computed from Convex artifacts only; Cloud Functions changes do not appear in it |

## Response schema

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

| Field | Type | Meaning |
| --- | --- | --- |
| `dry_run` | boolean | Echoes the request |
| `activated` | boolean | `true` exactly when `dry_run` is `false` |
| `generation` | integer | New generation after activation; the current (unchanged) generation on a dry run |
| `previous_generation` | integer | Generation active before the request |
| `diff.functions` | object | `added` / `changed` / `removed` arrays of `{name, kind}` |
| `diff.http_routes` | object | `added` / `changed` / `removed` arrays of `{key}` |
| `diff.schema_changed` | boolean | Schema fingerprint differs |
| `diff.indexes_changed` | boolean | Index fingerprint differs |
| `diff.runtime_bundle_changed` | boolean | Runtime bundle fingerprint differs |

Function and route changes are detected by fingerprint comparison against
the previously active Convex registry.

## Errors

All errors use the standard [error envelope](/reference/native/errors/).

| Status | Code | Condition |
| --- | --- | --- |
| `401` | `auth.unauthorized` | `NIMBUS_DEPLOY_TOKEN` was not set at server startup |
| `401` | `auth.unauthorized` | Missing or non-Bearer `Authorization` header, or wrong deploy token |
| `401` | `auth.unauthorized` | Missing or invalid `X-Nimbus-Admin-Token` on a server with local security |
| `403` | `auth.forbidden` | Non-loopback browser `Origin` header |
| `400` | `op.invalid_input` | Neither `artifacts.convex` nor `artifacts.cloud_functions` present |
| `400` | `op.invalid_input` | Convex artifacts are present without `convex_silo`, or the silo id is invalid |
| `400` | `op.invalid_input` | `bundle_mjs` supplied without `bundle_sha256`, or vice versa |
| `400` | `op.invalid_input` | Artifact staging or manifest/schema/bundle validation failed |
| `500` | `service.internal` | Unexpected server-side failure while staging artifacts or loading the registry |
