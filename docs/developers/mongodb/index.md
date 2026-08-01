---
title: Connect a MongoDB driver
description: Talk to Nimbus with the official Node.js MongoDB driver — every server serves the endpoint by default, and nimbus dev writes the connection URL to .env.local.
sidebar:
  label: Overview
  order: 1
---

Nimbus speaks the MongoDB wire protocol natively. In this tutorial you run
a Nimbus server, connect the official Node.js `mongodb` driver to it,
insert a document, and query it back. You do not need codegen or schema files.
The stock driver is the client.

Every Nimbus server serves the MongoDB endpoint by default. This applies to
`nimbus start` and `nimbus dev`. Use `--no-mongodb` to switch it off. Embedders
opt in through [the embedding alternative](#embedding-alternative).

In an app directory, `nimbus dev` detects a `mongodb` dependency in
`package.json`. It writes a ready-to-use connection URL to `.env.local` as
`NIMBUS_MONGODB_URL`. The client can then use
`new MongoClient(process.env.NIMBUS_MONGODB_URL)`. The steps below use
`nimbus start` with explicit credentials for a long-lived server. The driver
sees plain MongoDB.

## What you need

- The `nimbus` binary ([install](/get-started/quickstart/#1-install-nimbus)).
- Node.js 22 or later for the client side.

## 1. Start the server

The endpoint requires SCRAM-SHA-256 credentials. The password is env-only
so it never appears in process listings:

```bash
NIMBUS_MONGODB_PASSWORD=app-secret \
  nimbus start --mongodb-username app-user
```

The server logs both listeners. The HTTP API uses `127.0.0.1:8080`, and the
MongoDB endpoint uses `127.0.0.1:27017`. If another process holds `27017`,
Nimbus skips the listener and logs a warning. Pass `--mongodb-port` to set a
different port. Nimbus treats a busy explicit port as a hard error.

Without credential flags, the server still serves the endpoint. It generates
a credential pair on the first boot. It stores the pair in
`wire-credentials.json` in its data directory. This owner-only file has mode
`0600`. `nimbus dev` gives those generated credentials to your app through
`.env.local`.

The MongoDB endpoint always binds to loopback. The server refuses to bind
it to a network-reachable address. For remote access, put a TLS-terminating
proxy in front of the endpoint.

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
  "mongodb://app-user:app-secret@127.0.0.1:27017/myapp",
);
await client.connect();

const messages = client.db("myapp").collection("messages");

await messages.insertOne({ author: "Ada", body: "Hello from Nimbus" });
const docs = await messages.find().toArray();
console.log(docs);

await client.close();
```

Two parts of that connection string matter:

- `app-user:app-secret` are the server's SCRAM-SHA-256 startup credentials.
  Every data operation requires them.
- The string names exactly one host. Nimbus is a single endpoint, not a
  replica set. Do not pass a `replicaSet` option. With a single host, the
  driver connects directly. Adding `directConnection=true` is harmless but
  not required.

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

**Tenant route.** Connecting to the database `myapp` routed you to the Nimbus
tenant `myapp`. Nimbus created the tenant automatically on the first write.
See [tenant isolation](/reference/mongodb/tenant-isolation/) for the mapping
rules.

**Collection creation.** Nimbus created the `messages` collection on the first
insert. Schema is optional, so a collection accepts any document shape.

**Shared engine.** The write used the same engine as Nimbus's HTTP API. Each
Nimbus surface in the same tenant can immediately read a document inserted
through the MongoDB driver. The reverse also applies.

## Next steps

- [Driver examples](/developers/mongodb/examples/): CRUD, aggregation,
  transactions, and other languages.
- [Supported drivers](/reference/mongodb/drivers/): what works beyond
  Node.js.
- [Supported operations](/reference/mongodb/operations/): the exact
  command, filter, and update surface.
