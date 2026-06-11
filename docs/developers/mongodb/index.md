---
title: Connect a MongoDB driver
description: Run a Nimbus server with the MongoDB endpoint enabled and talk to it with the official Node.js driver.
sidebar:
  label: Overview
  order: 1
---

Nimbus speaks the MongoDB wire protocol natively. In this tutorial you start
a Nimbus server with the MongoDB endpoint enabled, connect the official
Node.js `mongodb` driver to it, insert a document, and query it back. No
codegen, no schema files — the stock driver is the client.

You enable the MongoDB endpoint when you start the server — a CLI flag on
`nimbus start`, or the embedding API if you run Nimbus as a library (see
[the embedding alternative](#embedding-alternative) below). Everything the
driver sees afterwards is plain MongoDB.

## What you need

- The `nimbus` binary ([install](/get-started/quickstart/#1-install-nimbus)).
- Node.js 22 or later for the client side.

## 1. Start the server

The endpoint requires SCRAM-SHA-256 credentials; the password is env-only
so it never appears in process listings:

```bash
NIMBUS_MONGODB_PASSWORD=app-secret \
  nimbus start --mongodb-port 27017 --mongodb-username app-user
```

The server logs both listeners — the HTTP API on `127.0.0.1:8080` and the
MongoDB endpoint on `127.0.0.1:27017`.

The MongoDB endpoint always binds to loopback. The server refuses to bind
it to a network-reachable address — front it with a TLS-terminating proxy
for remote access.

## 2. Create the client

In a separate directory, set up a Node project with the official driver:

```bash
mkdir mongo-client && cd mongo-client
npm init -y
npm install mongodb
```

Create `index.mjs`:

```javascript
import { MongoClient } from "mongodb";

const client = new MongoClient(
  "mongodb://app-user:app-secret@127.0.0.1:27017/myapp?directConnection=true",
);
await client.connect();

const messages = client.db("myapp").collection("messages");

await messages.insertOne({ author: "Ada", body: "Hello from Nimbus" });
const docs = await messages.find().toArray();
console.log(docs);

await client.close();
```

Two parts of that connection string matter:

- `app-user:app-secret` are the SCRAM-SHA-256 credentials the server was
  started with. Every data operation requires them.
- `directConnection=true` is required. Nimbus is a single endpoint, not a
  replica set; without this flag the driver attempts topology discovery and
  times out.

## 3. Run it

```bash
node index.mjs
```

You see your document come back, with the `_id` the server assigned:

```text
[ { _id: ..., author: 'Ada', body: 'Hello from Nimbus' } ]
```

## Embedding alternative

Running Nimbus as a Rust library? Enable the same endpoint with
`ServeOptions`:

```rust
use nimbus_server::{MongoDbAuthConfig, MongoDbConfig, ServeOptions};

let auth = MongoDbAuthConfig::new("app-user".into(), "app-secret".into());
let options = ServeOptions::new(engine)
    .with_mongodb(MongoDbConfig::localhost(27017, auth));
```

## What just happened

- Connecting to the database `myapp` routed you to the Nimbus tenant
  `myapp`. The tenant was created automatically on first write — see
  [tenant isolation](/reference/mongodb/tenant-isolation/) for the mapping
  rules.
- The collection `messages` was created on first insert. Schema is
  optional: a collection accepts any document shape.
- The write went through the same engine as Nimbus's HTTP API. A document
  inserted through the MongoDB driver is immediately visible to every other
  Nimbus surface in the same tenant, and vice versa.

## Next steps

- [Driver examples](/developers/mongodb/examples/) — CRUD, aggregation,
  transactions, and other languages.
- [Supported drivers](/reference/mongodb/drivers/) — what works beyond
  Node.js.
- [Supported operations](/reference/mongodb/operations/) — the exact
  command, filter, and update surface.
