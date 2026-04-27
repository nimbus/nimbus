# MongoDB Adapter

## Overview

The MongoDB adapter is a wire protocol listener built into the Neovex server.
It speaks the MongoDB binary protocol (OP_MSG) natively, so any standard
MongoDB client -- `mongosh`, Compass, the `mongodb` npm package, `pymongo`,
the Go driver, etc. -- can connect to Neovex as if it were a MongoDB instance.

**Data never touches MongoDB.** Every operation flows through the Neovex
engine and lands in whichever storage backend you configured (redb, SQLite,
Postgres, MySQL, or libsql). The adapter translates between BSON on the wire
and Neovex's internal document model; the storage backend is completely
transparent to the client.

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
│        Neovex Server                 │
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

The MongoDB adapter shares the exact same engine mutation path as Neovex's
HTTP and WebSocket interfaces. There is no separate code path -- a document
inserted via `mongosh` is immediately visible through the HTTP API and
vice versa.

## Quick Start

```bash
# 1. Start Neovex with the MongoDB listener enabled
neovex start --port 8080

# 2. Connect with mongosh (port 27017 is the default)
mongosh mongodb://127.0.0.1:27017/default?directConnection=true

# 3. Try it out
db.messages.insertOne({ author: "Alice", body: "Hello from mongosh" })
db.messages.find()
```

> **Note:** The MongoDB wire protocol listener is currently configured at the
> server library level. A `--mongodb-port` CLI flag is planned. Check the
> [CLI reference](../../reference/cli.md) for the current state.

## Client Package

`@neovex/mongodb` is a URI builder that produces a correct `mongodb://`
connection string for Neovex. It does not wrap the MongoDB driver -- you
create and manage the `MongoClient` yourself.

```bash
npm install @neovex/mongodb mongodb
```

```typescript
import { MongoClient } from "mongodb";
import { uri } from "@neovex/mongodb";

const client = new MongoClient(uri({ database: "myapp" }));
await client.connect();
const db = client.db("myapp");
```

The helper exists because Neovex is not a MongoDB replica set. Without
`directConnection=true`, drivers attempt topology discovery and fail with a
confusing timeout. `uri()` always includes it, along with sensible defaults
(`127.0.0.1:27017`, database `"default"`).

You do not need the helper. The stock `mongodb` driver works directly as long
as you include `?directConnection=true` in your connection string.

## Project Layout

No special directory structure is required. The MongoDB adapter does not use
a `.neovex` directory, codegen, or schema files. You bring your own project
layout and use the standard MongoDB driver API.

```
my-mongo-app/
├── src/
│   └── index.ts        # your application code
├── package.json        # depends on "mongodb" or "@neovex/mongodb"
└── tsconfig.json
```

No special directory structure is needed. Schema is optional -- the adapter
accepts any document shape and auto-creates collections on first write.

## Further Reading

- [Compatible Drivers](drivers.md) -- every tested MongoDB driver by language
- [Examples](examples.md) -- CRUD, aggregation, transactions, authentication
- [Tenant Isolation](tenant-isolation.md) -- how database names map to tenants
- [Operations & Configuration](operations.md) -- supported commands, auth, limitations
