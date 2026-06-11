# @nimbus/mongodb

A connection-string helper for pointing the official MongoDB Node.js driver
(`mongodb`) at a [Nimbus](../../README.md) MongoDB-compatible endpoint.

Nimbus speaks the MongoDB wire protocol, so you use the real `mongodb` driver
for all data operations. This package only builds a correct connection URI —
including `directConnection=true` (Nimbus is a single endpoint, not a replica
set) and URL-encoded credentials — so you do not hand-assemble it.

## Usage

```ts
import { MongoClient } from "mongodb";
import { mongoUri } from "@nimbus/mongodb";

const client = new MongoClient(mongoUri({
  database: "app",
  username: "app-user",
  password: "app-secret",
}));

await client.connect();
const docs = await client.db("app").collection("messages").find().toArray();
```

With credentials:

```ts
const client = new MongoClient(
  mongoUri({ host: "db.internal", port: 27017, database: "app", username: "svc", password: "p@ss/word" }),
);
// → mongodb://svc:p%40ss%2Fword@db.internal:27017/app?directConnection=true
```

## API

### `mongoUri(options?)`

Returns a `mongodb://…` connection string. Username/password are only included
when **both** are provided, and are URL-encoded for you. Nimbus MongoDB
listeners require the explicit SCRAM credentials configured by the embedding
server.

### `MongoUriOptions`

| Field | Default | Notes |
| --- | --- | --- |
| `host` | `127.0.0.1` | Nimbus MongoDB listener host |
| `port` | `27017` | Listener port (the MongoDB default) |
| `database` | `default` | Default database in the URI path |
| `username` | — | Included only with `password` |
| `password` | — | Included only with `username` |

## Scripts

```bash
npm run build --workspace @nimbus/mongodb      # build-only selftest pass
npm run test --workspace @nimbus/mongodb       # selftest suite
npm run typecheck --workspace @nimbus/mongodb   # type-only selftest pass
```

## Related

- [Supported MongoDB drivers](../../docs/private/adapters/mongodb/drivers.md)
- [Supported operations](../../docs/private/adapters/mongodb/operations.md)
- [Usage examples](../../docs/private/adapters/mongodb/examples.md)
- [Tenant isolation](../../docs/private/adapters/mongodb/tenant-isolation.md)
