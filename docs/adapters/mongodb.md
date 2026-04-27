# MongoDB Adapter

## Overview

MongoDB wire protocol listener that lets stock MongoDB drivers (`mongodb` npm package, `mongosh`, Compass) connect directly to Neovex. Applications use the standard MongoDB driver -- Neovex speaks the wire protocol natively.

## Client Package

`@neovex/mongodb` -- a thin helper that builds a connection string and calls `MongoClient.connect()`.

You can also use the stock `mongodb` driver directly without the helper package.

## Quick Start

```bash
# Start Neovex (MongoDB wire protocol is available alongside HTTP)
neovex start --port 8080

# Connect with mongosh
mongosh mongodb://127.0.0.1:27017/default?directConnection=true

# Or connect from application code
```

> **Note:** The MongoDB wire protocol listener is currently configured at the server library level. A `--mongodb-port` CLI flag is planned. Check the [CLI reference](../reference/cli.md) for the current state.

## Project Layout

```
my-mongo-app/
├── src/
│   └── index.ts
├── package.json
└── tsconfig.json
```

No special directory structure needed.

## Example Code

### Using `@neovex/mongodb` helper

```typescript
import { connectNeovex } from "@neovex/mongodb";

const client = await connectNeovex({ host: "127.0.0.1", port: 27017, database: "default" });
const db = client.db("default");
const messages = db.collection("messages");

await messages.insertOne({ author: "Alice", body: "Hello from MongoDB" });
const docs = await messages.find({ author: "Alice" }).toArray();
console.log("Messages:", docs);

await messages.updateOne({ author: "Alice" }, { $set: { body: "Updated" } });

const results = await messages.aggregate([
  { $match: { author: "Alice" } },
  { $sort: { _id: -1 } },
  { $limit: 10 },
]).toArray();
```

### Using stock `mongodb` driver directly

```typescript
import { MongoClient } from "mongodb";

const client = new MongoClient("mongodb://127.0.0.1:27017/default?directConnection=true");
await client.connect();
const db = client.db("default");
// ... same API as above
```

## Configuration

- Tenant mapping: MongoDB database name maps to Neovex tenant ID
- Authentication: SCRAM-SHA-256 with configurable credentials
- The adapter auto-creates tenants on first access (the `ensure_tenant` pattern)

## Supported Surface

Wire protocol commands including handshake, CRUD (`insert`, `find`, `update`, `delete`), cursor management, `aggregate` pipeline, index operations, change streams, and admin commands.

## Known Limitations

See [MongoDB adapter hardening plan](../plans/mongodb-adapter-hardening-plan.md) for the current coverage and planned work.

## Related Docs

- [MongoDB adapter hardening plan](../plans/mongodb-adapter-hardening-plan.md)
- [Demo: mongodb/node](../../demos/mongodb/node/) (Node.js example)
