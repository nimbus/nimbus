---
title: Use DynamoDB SDKs with Nimbus
description: Enable the Nimbus DynamoDB-compatible endpoint and point official AWS SDKs at it with an endpoint override.
sidebar:
  label: Overview
  order: 1
---

Nimbus serves the DynamoDB wire protocol on a dedicated HTTP listener. The
official AWS SDKs connect to it the same way they connect to DynamoDB
Local: override the endpoint URL, keep everything else stock. No Nimbus
SDK, no code changes beyond client construction.

The endpoint is part of the Nimbus server library, and you enable it when
you configure the server. The `nimbus start` CLI does not currently expose
a flag for it, so this guide enables it through a small Rust program, then
talks to it with the AWS SDK for JavaScript v3.

## How the endpoint works

- It listens on its own port — `127.0.0.1:8000` by default, the DynamoDB
  Local convention — separate from the main Nimbus HTTP API.
- It accepts the standard DynamoDB JSON protocol: `POST /` with an
  `X-Amz-Target` header and an `application/x-amz-json-1.0` body. That is
  what every AWS SDK sends, so any SDK with an endpoint override works.
- Each AWS access key ID is bound server-side to one Nimbus tenant.
  Requests authenticated with that key see only that tenant's tables.

## 1. Enable the endpoint

Create a Rust project next to your Nimbus checkout and point it at the
Nimbus crates. In `Cargo.toml`:

```toml
[dependencies]
nimbus-core = { path = "../nimbus/crates/nimbus-core" }
nimbus-dynamodb = { path = "../nimbus/crates/nimbus-dynamodb" }
nimbus-engine = { path = "../nimbus/crates/nimbus-engine" }
nimbus-server = { path = "../nimbus/crates/nimbus-server" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

In `src/main.rs`:

```rust
use std::sync::Arc;

use nimbus_core::TenantId;
use nimbus_dynamodb::DynamoDbConfig;
use nimbus_engine::Engine;
use nimbus_server::{ServeOptions, serve};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Persists to ./data with the embedded SQLite backend.
    let engine = Arc::new(Engine::new("./data")?);

    // The main HTTP API. The DynamoDB endpoint runs as a sibling listener.
    let http = tokio::net::TcpListener::bind("127.0.0.1:8080").await?;

    // Binds 127.0.0.1:8000 and maps the access key AKIAACME to the
    // tenant "acme", verified with full SigV4 against this secret.
    let dynamodb = DynamoDbConfig::default().with_signed_access_key(
        "AKIAACME",
        TenantId::new("acme")?,
        "acme-secret",
    );

    let options = ServeOptions::new(engine).with_dynamodb(dynamodb);
    serve(http, options).await?;
    Ok(())
}
```

Run it with `cargo run`. Requests signed with an access key you did not
register are rejected with `UnrecognizedClientException` — the registry
starts empty and fails closed.

For local development you can skip signature verification:

```rust
let dynamodb = DynamoDbConfig::default()
    .with_access_key("dev-key", TenantId::new("dev")?)
    .insecure_dev_auth();
```

In this mode any signature is accepted for a registered key, so the server
refuses to bind it to anything but a loopback address.

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

- [Feature coverage](/reference/dynamodb/feature-coverage/) — every
  supported operation, by tier.
- [Divergences](/reference/dynamodb/divergences/) — the documented
  behavioral differences from AWS DynamoDB.
- [SDK compatibility](/reference/dynamodb/sdk-compatibility/) — per-SDK
  verification status.
- [Readiness](/reference/dynamodb/readiness/) — security posture and
  operational limits.
