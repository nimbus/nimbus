---
title: Use DynamoDB SDKs with Nimbus
description: Point official AWS SDKs at the Nimbus DynamoDB-compatible endpoint — served by default on every server, with nimbus dev writing endpoint and credentials to .env.local.
sidebar:
  label: Overview
  order: 1
---

Nimbus serves the DynamoDB wire protocol on a dedicated HTTP listener. The
official AWS SDKs connect to it the same way they connect to DynamoDB
Local: override the endpoint URL, keep everything else stock. You do not need
a Nimbus SDK. Only client construction changes.

Every Nimbus server serves the endpoint by default. This applies to
`nimbus start` and `nimbus dev`. Use `--no-dynamodb` to switch it off.
Embedders opt in through the API described in
[the embedding alternative](#embedding-alternative).

In an app directory,
`nimbus dev` also detects an `@aws-sdk/client-dynamodb` dependency in your
`package.json`. It writes the endpoint and a generated access key to
your app's `.env.local` as
`NIMBUS_DYNAMODB_*` keys. This guide talks to the endpoint with the AWS
SDK for JavaScript v3.

## How the endpoint works

**Dedicated port.** The listener uses `127.0.0.1:8000` by default, which is
the DynamoDB Local convention. It is separate from the main Nimbus HTTP API.

**DynamoDB JSON protocol.** The listener accepts `POST /` requests with an
`X-Amz-Target` header and an `application/x-amz-json-1.0` body. Each AWS SDK
sends this format. Any SDK with an endpoint override therefore works.

**Tenant binding.** Nimbus binds each AWS access key ID to one tenant.
Requests with that key see only the bound tenant's tables.

## 1. Start the server

```bash
nimbus start
```

The DynamoDB listener starts on `127.0.0.1:8000` with the rest of the server.
If another process holds `8000`, Nimbus skips the listener and logs a warning.
Pass `--dynamodb-port` to set a different port. Nimbus treats a busy explicit
port as a hard error.

Every request authenticates. Without access-key flags, the server binds a
generated access key to the `default` tenant. It stores the key in
`wire-credentials.json` in its data directory. This owner-only file has mode
`0600`. `nimbus dev` gives that key to your app through `.env.local`. To
choose your bindings, map an AWS access key ID to a tenant. Include its SigV4
signing secret:

```bash
nimbus start --dynamodb-access-key AKIAACME:acme-secret:acme
```

Repeat `--dynamodb-access-key` for more tenants, or set the
comma-separated `NIMBUS_DYNAMODB_ACCESS_KEYS` environment variable.
Explicit bindings replace the generated default. Requests signed with an
unregistered access key receive `UnrecognizedClientException`. The registry
fails closed.

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

The embedding API also offers an `insecure_dev_auth()` lookup mode for local
development. This mode skips signature verification and accepts any signature
for a registered key. The server therefore refuses to bind this mode to a
non-loopback address.

## 2. Point the AWS SDK at it

If your project uses the Nimbus CLI, provision the helper package:

```bash
nimbus packages install dynamodb
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

**The access key ID selects the tenant.** Clients with different registered
keys use different table namespaces and data. Nimbus isolates them from each
other.

**Strict SigV4 is the default.** Nimbus verifies each request signature
against the registered secret. It accepts the standard ±15-minute clock-skew
window. Nimbus rejects unsigned or incorrectly signed requests.

**Lookup mode is for loopback development only.** `insecure_dev_auth()` skips
signature verification. The server prevents this mode from binding to a
network-reachable address.

## Next steps

- [Example app](/developers/dynamodb/examples/): the shared tasks list
  driven by the stock AWS SDK.
- [Feature coverage](/reference/dynamodb/feature-coverage/): every
  supported operation, by tier.
- [Divergences](/reference/dynamodb/divergences/): the documented
  behavioral differences from AWS DynamoDB.
- [SDK compatibility](/reference/dynamodb/sdk-compatibility/): per-SDK
  verification status.
- [Readiness](/reference/dynamodb/readiness/): security posture and
  operational limits.
