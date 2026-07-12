---
title: Use DynamoDB SDKs with Nimbus
description: Point official AWS SDKs at the Nimbus DynamoDB-compatible endpoint — served by default on every server, with nimbus dev writing endpoint and credentials to .env.local.
sidebar:
  label: Overview
  order: 1
---

Nimbus serves the DynamoDB wire protocol on a dedicated HTTP listener. The
official AWS SDKs connect to it the same way they connect to DynamoDB
Local: override the endpoint URL, keep everything else stock. No Nimbus
SDK, no code changes beyond client construction.

Every Nimbus server serves the endpoint by default — `nimbus start` and
`nimbus dev` alike (`--no-dynamodb` switches it off; embedders opt in
through the API — see
[the embedding alternative](#embedding-alternative) below). In an app
directory, `nimbus dev` goes further: when it sees an
`@aws-sdk/client-dynamodb` dependency in your `package.json`, it writes
the endpoint and a generated access key to your app's `.env.local` as
`NIMBUS_DYNAMODB_*` keys. This guide talks to the endpoint with the AWS
SDK for JavaScript v3.

## How the endpoint works

- It listens on its own port — `127.0.0.1:8000` by default, the DynamoDB
  Local convention — separate from the main Nimbus HTTP API.
- It accepts the standard DynamoDB JSON protocol: `POST /` with an
  `X-Amz-Target` header and an `application/x-amz-json-1.0` body. That is
  what every AWS SDK sends, so any SDK with an endpoint override works.
- Each AWS access key ID is bound server-side to one Nimbus tenant.
  Requests authenticated with that key see only that tenant's tables.

## 1. Start the server

```bash
nimbus start
```

The DynamoDB listener comes up on `127.0.0.1:8000` with the rest of the
server. If another process already holds `8000`, the listener is skipped
with a warning — pass `--dynamodb-port` to pin a different port (a busy
explicit port is a hard error instead of a skip).

Every request authenticates. With no access-key flags, the server binds a
generated access key — persisted at `wire-credentials.json` (owner-only,
`0600`) in its data directory — to the tenant `default`; `nimbus dev` is
the shape that hands that key to your app via `.env.local`. To choose
your own bindings, map an AWS access key ID (with its SigV4 signing
secret) to a tenant explicitly:

```bash
nimbus start --dynamodb-access-key AKIAACME:acme-secret:acme
```

Repeat `--dynamodb-access-key` for more tenants, or set the
comma-separated `NIMBUS_DYNAMODB_ACCESS_KEYS` environment variable.
Explicit bindings replace the generated default. Requests signed with an
access key that is not registered are rejected with
`UnrecognizedClientException` — the registry fails closed.

### Embedding alternative

Running Nimbus as a Rust library? Configure the same listener with
`ServeOptions`:

```rust
use nimbus_core::TenantId;
use nimbus_server::{DynamoDbConfig, ServeOptions};

let dynamodb = DynamoDbConfig::default().with_signed_access_key(
    "AKIAACME",
    TenantId::new("acme")?,
    "acme-secret",
);
let options = ServeOptions::new(engine).with_dynamodb(dynamodb);
```

The embedding API additionally offers a signature-skipping
`insecure_dev_auth()` lookup mode for local development — any signature is
accepted for a registered key, so the server refuses to bind it to a
non-loopback address.

## 2. Point the AWS SDK at it

If your project uses the Nimbus CLI, provision the helper package:

```bash
nimbus packages provision dynamodb
```

`@nimbus/dynamodb` exports `clientConfig()`, a drop-in configuration for
`DynamoDBClient` with the local defaults (endpoint `http://127.0.0.1:8000`,
region `us-east-1`):

```javascript
import { DynamoDBClient } from "@aws-sdk/client-dynamodb";
import { clientConfig } from "@nimbus/dynamodb";

const client = new DynamoDBClient(
  clientConfig({ accessKeyId: "AKIAACME", secretAccessKey: "acme-secret" }),
);
```

Without the helper, the plain SDK configuration is just as short:

```javascript
const client = new DynamoDBClient({
  endpoint: "http://127.0.0.1:8000",
  region: "us-east-1",
  credentials: { accessKeyId: "AKIAACME", secretAccessKey: "acme-secret" },
});
```

In an app that `nimbus dev` wired, skip the literals and read the
Nimbus-owned `.env.local` keys instead:

```javascript
const client = new DynamoDBClient({
  endpoint: process.env.NIMBUS_DYNAMODB_ENDPOINT,
  region: "us-east-1",
  credentials: {
    accessKeyId: process.env.NIMBUS_DYNAMODB_ACCESS_KEY_ID,
    secretAccessKey: process.env.NIMBUS_DYNAMODB_SECRET_ACCESS_KEY,
  },
});
```

## 3. Create a table and use it

```javascript
import {
  CreateTableCommand,
  PutItemCommand,
  GetItemCommand,
} from "@aws-sdk/client-dynamodb";

await client.send(
  new CreateTableCommand({
    TableName: "orders",
    AttributeDefinitions: [{ AttributeName: "pk", AttributeType: "S" }],
    KeySchema: [{ AttributeName: "pk", KeyType: "HASH" }],
    BillingMode: "PAY_PER_REQUEST",
  }),
);
// The table is ACTIVE immediately — no waiter needed.

await client.send(
  new PutItemCommand({
    TableName: "orders",
    Item: { pk: { S: "order-1" }, total: { N: "42" } },
  }),
);

const { Item } = await client.send(
  new GetItemCommand({
    TableName: "orders",
    Key: { pk: { S: "order-1" } },
  }),
);
console.log(Item); // { pk: { S: "order-1" }, total: { N: "42" } }
```

Tables transition to `ACTIVE` synchronously, so the create-wait-use dance
from AWS deployments collapses to create-use.

## Credentials and tenants

- **The access key ID selects the tenant.** Two clients with different
  registered keys are fully isolated from each other — different table
  namespaces, different data.
- **Strict SigV4 is the default.** Each request's signature is verified
  against the registered secret, with the standard ±15-minute clock-skew
  window. Unsigned or wrongly signed requests are rejected.
- **Lookup mode is for loopback development only.** `insecure_dev_auth()`
  skips signature verification; the server enforces that this mode never
  binds to a network-reachable address.

## Next steps

- [Example app](/developers/dynamodb/examples/) — the shared tasks list
  driven by the stock AWS SDK.
- [Feature coverage](/reference/dynamodb/feature-coverage/) — every
  supported operation, by tier.
- [Divergences](/reference/dynamodb/divergences/) — the documented
  behavioral differences from AWS DynamoDB.
- [SDK compatibility](/reference/dynamodb/sdk-compatibility/) — per-SDK
  verification status.
- [Readiness](/reference/dynamodb/readiness/) — security posture and
  operational limits.
