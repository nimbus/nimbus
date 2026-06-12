# @nimbus/dynamodb

Tiny connection helpers for pointing the official AWS SDK
(`@aws-sdk/client-dynamodb`) at a [Nimbus](../../README.md) DynamoDB-compatible
endpoint.

This package is intentionally minimal: Nimbus speaks the DynamoDB wire
protocol, so you use the real AWS SDK for all data operations. All this package
does is build the right `endpoint` / `region` / `credentials` config so you do
not hand-assemble local URLs and tenant credentials.

## Usage

`@aws-sdk/client-dynamodb` is an optional peer dependency — install it
alongside this package:

```bash
npm install @aws-sdk/client-dynamodb
```

```ts
import { DynamoDBClient, ListTablesCommand } from "@aws-sdk/client-dynamodb";
import { clientConfig } from "@nimbus/dynamodb";

// Defaults to http://127.0.0.1:8000, region us-east-1.
const client = new DynamoDBClient(clientConfig({ accessKeyId: "AKIAACME" }));

const { TableNames } = await client.send(new ListTablesCommand({}));
```

The **access key id selects the tenant** — Nimbus binds each access key to a
tenant server-side, so two clients with different keys are isolated. Strict
SigV4 verification is the default, so the secret must match the key's
registered signing secret; only the embedding-only `insecure_dev_auth()`
lookup mode (which the server refuses to bind to a non-loopback address)
accepts any non-empty secret.

## API

### `clientConfig(options?)`

Returns a `NimbusDynamoConfig` that is a drop-in for `new DynamoDBClient(config)`.

### `endpoint(options?)`

Returns just the endpoint URL string. An explicit `endpoint` wins; otherwise
it is `http://<host>:<port>`.

### `NimbusDynamoOptions`

| Field | Default | Notes |
| --- | --- | --- |
| `endpoint` | — | Full endpoint URL; overrides `host`/`port` |
| `host` | `127.0.0.1` | Nimbus DynamoDB listener host |
| `port` | `8000` | Listener port (the DynamoDB Local default) |
| `region` | `us-east-1` | Credential-scope region |
| `accessKeyId` | `nimbus` | Selects the Nimbus tenant |
| `secretAccessKey` | `nimbus` | Only checked in strict SigV4 mode |

## Scripts

```bash
npm run build --workspace @nimbus/dynamodb      # build-only selftest pass
npm run test --workspace @nimbus/dynamodb       # selftest suite
npm run typecheck --workspace @nimbus/dynamodb   # type-only selftest pass
```

## Related

- [DynamoDB SDK compatibility](../../docs/reference/dynamodb/sdk-compatibility.md)
- [DynamoDB feature coverage](../../docs/reference/dynamodb/feature-coverage.md)
- [Known divergences](../../docs/reference/dynamodb/divergences.md)
