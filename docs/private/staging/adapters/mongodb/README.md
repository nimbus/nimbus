# MongoDB Adapter

```bash
nimbus start --port 8080
mongosh mongodb://app-user:app-secret@127.0.0.1:27017/default?directConnection=true
```

```javascript
db.messages.insertOne({ author: "Alice", body: "Hello from Nimbus" })
db.messages.find()
```

That's it. Stock MongoDB drivers, any language, no codegen, no schema files.
Data lives in Nimbus (SQLite by default), not MongoDB. ~2 minutes from install to query.

## How it works

The adapter speaks the MongoDB binary protocol (OP_MSG) natively. Any standard
MongoDB client connects to Nimbus as if it were a MongoDB instance.
See [Compatible Drivers](drivers.md) for the full list by language.

```
┌──────────────────────────────────────┐
│          Your Application            │
│                                      │
│  const client = new MongoClient(...) │
│  db.collection("users").insertOne()  │
│  db.collection("users").find()       │
└───────────────┬──────────────────────┘
                │  MongoDB wire protocol
                │  (BSON over TCP, port 27017)
                ▼
┌──────────────────────────────────────┐
│        Nimbus Server                 │
│                                      │
│  ┌──────────┐    ┌────────────────┐  │
│  │ Wire     │    │ BSON Bridge    │  │
│  │ Protocol ├───►│ BSON <-> JSON  │  │
│  │ Parser   │    │ (lossless      │  │
│  │ (OpMsg)  │    │  round-trip)   │  │
│  └──────────┘    └───────┬────────┘  │
│                          │           │
│  ┌───────────────────────▼────────┐  │
│  │  Engine (Service)              │  │
│  │  Schema validation, atomics,   │  │
│  │  query planning, transactions  │  │
│  └───────────────┬────────────────┘  │
│                  │                   │
│  ┌───────────────▼────────────────┐  │
│  │  Storage Backend (pluggable)   │  │
│  │  redb │ SQLite │ Postgres      │  │
│  │  MySQL │ libsql                │  │
│  └────────────────────────────────┘  │
└──────────────────────────────────────┘
```

The adapter shares the same engine mutation path as Nimbus's HTTP and
WebSocket interfaces. A document inserted via `mongosh` is immediately
visible through the HTTP API and vice versa.

## Quick start by language

### mongosh (zero install)

```bash
nimbus start --port 8080
mongosh mongodb://app-user:app-secret@127.0.0.1:27017/default?directConnection=true
```

```javascript
db.messages.insertOne({ author: "Alice", body: "Hello from mongosh" })
db.messages.find()
```

### TypeScript

```bash
mkdir my-app && cd my-app
npm init -y
npm install mongodb                 # the official driver (developer-supplied)
nimbus packages provision mongodb   # the @nimbus/mongodb helper (from the binary)
```

```typescript
import { MongoClient } from "mongodb";
import { mongoUri } from "@nimbus/mongodb";

const client = new MongoClient(mongoUri({
  username: "app-user",
  password: "app-secret",
}));
await client.connect();

const db = client.db("default");
await db.collection("messages").insertOne({ author: "Alice", text: "Hello from Nimbus" });
console.log(await db.collection("messages").find().toArray());

await client.close();
```

```bash
npx tsx index.ts
```

### Python

```bash
pip install pymongo
```

```python
from pymongo import MongoClient

client = MongoClient("mongodb://app-user:app-secret@127.0.0.1:27017/default?directConnection=true")
db = client["default"]

db.messages.insert_one({"author": "Bob", "text": "Hello from Python"})
print(list(db.messages.find()))
```

> **Note:** The MongoDB wire protocol listener and its SCRAM credentials are
> currently configured at the server library level. A `--mongodb-port` CLI flag
> is planned. Check the [CLI reference](../../staging/operating/cli.md) for the current
> state.

## Client Package

`@nimbus/mongodb` is a URI builder that produces a correct `mongodb://`
connection string for Nimbus. It does not wrap the MongoDB driver -- you
create and manage the `MongoClient` yourself.

```bash
# @nimbus/mongodb (the URI helper) is provisioned by the nimbus binary:
nimbus packages provision mongodb
# the official MongoDB driver is developer-supplied:
npm install mongodb
```

```typescript
import { MongoClient } from "mongodb";
import { mongoUri } from "@nimbus/mongodb";

const client = new MongoClient(mongoUri({
  database: "myapp",
  username: "app-user",
  password: "app-secret",
}));
await client.connect();
const db = client.db("myapp");
```

The helper exists because Nimbus is not a MongoDB replica set. Without
`directConnection=true`, drivers attempt topology discovery and fail with a
confusing timeout. `mongoUri()` always includes it, along with sensible defaults
for the host, port, and database (`127.0.0.1:27017`, database `"default"`).

You do not need the helper. The stock `mongodb` driver works directly as long
as you include `?directConnection=true` in your connection string. See
[Examples](examples.md) for CRUD, aggregation, transactions, and
authentication patterns in multiple languages.

## Project Layout

No special directory structure is required. The MongoDB adapter does not use
a `.nimbus` directory, codegen, or schema files. You bring your own project
layout and use the standard MongoDB driver API.

```
my-mongo-app/
├── src/
│   └── index.ts        # your application code
├── package.json        # depends on "mongodb" or "@nimbus/mongodb"
└── tsconfig.json
```

No special directory structure is needed. Schema is optional -- the adapter
accepts any document shape and auto-creates collections on first write.

## Further Reading

- [Compatible Drivers](drivers.md) -- every tested MongoDB driver by language
- [Examples](examples.md) -- CRUD, aggregation, transactions, authentication
- [Tenant Isolation](tenant-isolation.md) -- how database names map to tenants
- [Operations & Configuration](operations.md) -- supported commands, auth, limitations
