---
title: Cloudflare compatibility
description: The Cloudflare-compatible surface Nimbus serves today — the Workers KV REST data plane, its wrangler binding registry, and the binding families that are parsed but not served.
sidebar:
  order: 1
---

This reference records the Cloudflare-compatible surface Nimbus serves
today. It is intentionally narrower than a generic "Cloudflare-compatible"
claim: only the Workers KV REST data plane is served. Anything not listed as
served should be assumed unserved.

Cloudflare routes are served by default on the main HTTP listener; opt out
with `--no-cloudflare`. The routes are gated on a Cloudflare configuration
being present, which it is by default. A non-loopback `--host` is refused
unless `--allow-network` is also set, matching the network-bind posture of
the other adapters.

This Cloudflare KV data plane — the `GET`/`PUT`/`DELETE` REST API under
`/client/v4/accounts/...` — is distinct from the RESP `nimbus kv` surface
documented at [Nimbus KV compatibility](/reference/kv/compatibility/). They
are separate protocols backed by separate wire formats.

## Maturity

The Cloudflare surface is default-on but early and partial. Workers KV over
REST is the only Cloudflare data plane served today. Durable Objects, D1,
and R2 are not served: Durable Objects has a storage substrate that no HTTP
front door mounts, and D1 and R2 exist only as parsed wrangler bindings in
the configuration registry. The [Not served](#not-served-today) section
lists each unserved family.

## Served: Workers KV REST

The KV REST API is mounted on the main listener. Routes follow the
Cloudflare account-scoped path shape.

| Method | Path | Behavior |
| --- | --- | --- |
| `GET` | `/client/v4/accounts/{account_id}/storage/kv/namespaces/{namespace_id}/values/{key}` | Reads a value. Responds `application/octet-stream`; `404` when the key is absent. |
| `PUT` | `/client/v4/accounts/{account_id}/storage/kv/namespaces/{namespace_id}/values/{key}` | Writes a value with optional `expiration`, `expirationTtl`, and `metadata` query parameters. |
| `DELETE` | `/client/v4/accounts/{account_id}/storage/kv/namespaces/{namespace_id}/values/{key}` | Deletes a key. Deleting an absent key succeeds. |
| `GET` | `/client/v4/accounts/{account_id}/storage/kv/namespaces/{namespace_id}/metadata/{key}` | Returns the JSON metadata stored with a key. |
| `GET` | `/client/v4/accounts/{account_id}/storage/kv/namespaces/{namespace_id}/keys` | Lists keys with optional `prefix`, `cursor`, and `limit` query parameters. |

Metadata, list, write-acknowledgement, and error responses use the
Cloudflare API envelope shape (`success`, `errors`, `messages`, `result`);
value reads (`GET .../values/{key}`) return the raw stored bytes as
`application/octet-stream`. Errors carry a numeric code and message. List
responses add `result_info` with a `cursor` and a `list_complete` flag —
`list_complete` is `false` and a `cursor` is returned when a full page is
read.

### Write and list parameters

| Parameter | Applies to | Behavior |
| --- | --- | --- |
| `expiration` | `PUT` value | Absolute expiry in seconds since the Unix epoch. |
| `expirationTtl` | `PUT` value | Relative time-to-live in seconds; must be at least 60. |
| `metadata` | `PUT` value | JSON string stored alongside the value and returned by the metadata and list routes. |
| `prefix` | `GET` keys | Restricts the listing to keys under a prefix. |
| `cursor` | `GET` keys | Opaque continuation token from a prior list page. |
| `limit` | `GET` keys | Page size; defaults to 1000 and may not exceed 1000. |

`expiration` and `expirationTtl` are mutually exclusive; supplying both is
rejected. Listed keys report their `expiration` (in seconds) and `metadata`
when present.

### Limits

| Limit | Value |
| --- | --- |
| Maximum key length | 512 bytes |
| Maximum value size | 25 MiB |
| Maximum metadata size | 1024 bytes |
| Minimum `expirationTtl` | 60 seconds |
| Default and maximum list `limit` | 1000 |

Keys must be non-empty and must not contain NUL. Values and metadata over
their limits are rejected with a `400`.

### Authentication

Every KV REST request authenticates with an `Authorization: Bearer` header
whose token is `ACCESS_KEY_ID:SECRET`. The id and secret resolve to a
tenant through the adapter's signed-access-key registry. By default this is
the same generated wire access key the DynamoDB listener uses, bound to the
tenant `default`; the credential is persisted with the other wire
credentials in the data directory. A missing header, a non-Bearer scheme, a
token without the `id:secret` shape, an unrecognized id, or a mismatched
secret is rejected with `401`.

### Namespace resolution

The `{namespace_id}` path segment resolves against the KV namespace
bindings in the active configuration. It matches a binding by its `binding`
name, its `id`, or its `preview_id`. When no KV namespace bindings are
configured, the `{namespace_id}` value is used directly as the namespace.
When bindings are configured and none matches, the request is rejected with
`404`. Stored keys are namespaced, so keys in one namespace are not visible
through another.

## Binding registry

At startup the adapter reads a wrangler configuration from the app
directory when one is present, checking `wrangler.jsonc`, `wrangler.json`,
then `wrangler.toml`. JSONC comments and trailing commas are stripped before
parsing. The parsed bindings populate a configuration registry with four
families: KV namespaces, Durable Objects, D1 databases, and R2 buckets.

Only the KV namespace bindings drive a served data plane (namespace
resolution, above). The Durable Object, D1, and R2 bindings are parsed and
held in the registry but back no HTTP surface.

## Not served today

| Cloudflare family | State |
| --- | --- |
| Durable Objects | A storage substrate exists (per-instance activation leases, transactional storage, alarms, hibernated-WebSocket records) but no HTTP front door mounts it. There is no served Durable Object data plane. |
| D1 | Parsed as wrangler `d1_databases` bindings into the configuration registry only. No D1 HTTP data plane. |
| R2 | Parsed as wrangler `r2_buckets` bindings into the configuration registry only. No R2 HTTP data plane. |
| Workers script execution | Not served. Nimbus does not run Worker scripts against these bindings. |