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

The MongoDB endpoint is part of the Nimbus server library, and you enable it
when you configure the server. The `nimbus start` CLI does not currently
expose a flag for it, so this tutorial runs the server through a small Rust
program. Everything the driver sees afterwards is plain MongoDB.

## What you need

- A checkout of the Nimbus repository, with its build prerequisites
  installed (Rust toolchain and Node.js — see the repository README).
- Node.js 22 or later for the client side.

## 1. Create the server program

Create a new Rust project next to your Nimbus checkout:

```bash
cargo new nimbus-mongo-server
cd nimbus-mongo-server
```

Point it at the Nimbus crates in your checkout. In `Cargo.toml`:

```toml
[dependencies]
nimbus-engine = { path = "../nimbus/crates/nimbus-engine" }
nimbus-server = { path = "../nimbus/crates/nimbus-server" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Replace `src/main.rs` with:

```rust
use std::sync::Arc;

use nimbus_engine::Engine;
use nimbus_server::{MongoDbAuthConfig, MongoDbConfig, ServeOptions, serve};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Persists to ./data with the embedded SQLite backend.
    let engine = Arc::new(Engine::new("./data")?);

    // The main HTTP API listens here. The MongoDB endpoint runs as a
    // sibling listener on its own port.
    let http = tokio::net::TcpListener::bind("127.0.0.1:8080").await?;

    // SCRAM-SHA-256 credentials every MongoDB client must present.
    let auth = MongoDbAuthConfig::new("app-user".into(), "app-secret".into());

    let options = ServeOptions::new(engine)
        .with_mongodb(MongoDbConfig::localhost(27017, auth));

    serve(http, options).await?;
    Ok(())
}
```

## 2. Start the server

```bash
cargo run
```

The first build compiles the Nimbus workspace, so it takes a while. When the
server is up it logs both listeners — the HTTP API on `127.0.0.1:8080` and
the MongoDB endpoint on `127.0.0.1:27017`.

The MongoDB endpoint always binds to loopback. The server refuses to bind it
to a network-reachable address.

## 3. Create the client

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

- `app-user:app-secret` are the SCRAM-SHA-256 credentials your server
  program configured. Every data operation requires them.
- `directConnection=true` is required. Nimbus is a single endpoint, not a
  replica set; without this flag the driver attempts topology discovery and
  times out.

## 4. Run it

```bash
node index.mjs
```

You see your document come back, with the `_id` the server assigned:

```text
[ { _id: ..., author: 'Ada', body: 'Hello from Nimbus' } ]
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
