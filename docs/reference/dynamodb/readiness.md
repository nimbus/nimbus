---
title: DynamoDB readiness
description: Security posture, isolation guarantees, and operational limits of the Nimbus DynamoDB endpoint.
sidebar:
  order: 4
---

A factual summary for teams evaluating the Nimbus DynamoDB endpoint as a
DynamoDB replacement: what is implemented, how it fails, how tenants are
isolated, and which limits apply in operation.

## Surface

The full operation tier set T0–T7 — control plane, single-item, query and
scan, batch and transactions, secondary indexes, streams, TTL and tagging,
and authentication — is implemented and continuously verified through the
official AWS Rust SDK. The exact operation list is in
[feature coverage](/reference/dynamodb/feature-coverage/); the recorded
behavioral differences from AWS are in
[divergences](/reference/dynamodb/divergences/); per-SDK status is in
[SDK compatibility](/reference/dynamodb/sdk-compatibility/).

## Failure posture

The endpoint fails closed. Malformed input, unknown operations, invalid or
oversize keys, missing or unregistered credentials, failed condition
expressions, cancelled transactions, and bad signatures all map to modeled
DynamoDB 4xx errors — the same exception names and shapes the AWS SDKs
already handle. There are no partial-success envelopes: a transactional
write either commits in full or reports a cancellation with per-item
reasons.

## Storage guarantees

DynamoDB writes lower through the same engine and storage path as every
other Nimbus surface. A committed write updates the document, its index
effects, and the commit journal in one storage transaction. Stream records
are derived from that durable history, not from a side log, so a record you
read from a stream corresponds to a write that committed.

## Tenant and authentication isolation

- Each access key ID is bound to exactly one tenant. Two keys bound to two
  tenants cannot read, write, list, tag, TTL-configure, or otherwise
  observe each other's tables — even when table names are identical.
- Table catalogs, TTL configuration, tags, and stream state are stored
  per tenant. Only the access-key registry is global, and it maps each key
  to a single tenant.
- Access keys can never be bound to a reserved Nimbus-internal tenant
  (`_nimbus_*`); both the key-registration path and the request-resolution
  path refuse it.
- Listing access keys returns a redacted view. The secret access key is
  never readable back through any listing surface.

## Transport and request authentication

| Property | Posture |
| --- | --- |
| Default auth mode | Strict SigV4: full canonical-request verification, derived signing key, constant-time comparison |
| Clock skew | Requests with a timestamp outside ±15 minutes are rejected |
| Body integrity | The body hash is signature-bound (`x-amz-content-sha256` must match the body); `UNSIGNED-PAYLOAD` is refused |
| Development mode | Lookup-only mode skips signature verification and is refused on any non-loopback address |
| Transport encryption | The listener speaks plain HTTP. SigV4 authenticates requests but does not encrypt them — terminate TLS in front of the listener for any non-loopback deployment |
| Replay bound | SigV4 has no per-request nonce; the ±15-minute window is the replay bound, backstopped by TLS and your network boundary |
| Request size | Bodies over 16 MiB are rejected with `413 Payload Too Large` before buffering, so oversized payloads cannot force a large pre-authentication allocation |

## Data at rest

Access-key records and table data are stored like all other Nimbus data
and are covered by Nimbus's at-rest encryption when it is enabled. The
endpoint adds no separate secret cipher of its own. Enable at-rest
encryption in production, or run on an external database backend that
provides its own — see [Operators](/operators/).

## Operational limits

- **No provisioned-capacity throttling.** Requests are never throttled
  against configured capacity units.
- **Key size.** The combined primary key supports roughly 1,100 raw bytes
  (DDB-DIV-001), below AWS's 3,072.
- **Streams.** One shard per stream; the 24-hour record retention is
  enforced at read time, and storage for expired records is reclaimed on
  the next poll of the stream.
- **TTL sweeper.** Expired items are reclaimed by a background sweeper,
  every 60 seconds by default; the cadence is configurable, and the
  sweeper can be disabled.
- **Table status.** Tables are `ACTIVE` immediately; there is no
  asynchronous `CREATING` phase to wait out.
