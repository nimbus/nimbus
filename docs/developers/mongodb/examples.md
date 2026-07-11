---
title: MongoDB driver examples
description: Task-oriented recipes for the Nimbus MongoDB endpoint — connection strings, CRUD, aggregation, transactions, and other languages.
sidebar:
  order: 2
---

Recipes for common tasks against the Nimbus MongoDB endpoint, using stock
MongoDB drivers. Each section is independent — jump to the task you need.
If you haven't connected a driver yet, start with the
[tutorial](/developers/mongodb/).

All examples assume a Nimbus server serving the MongoDB endpoint on
`127.0.0.1:27017` (the default) with SCRAM-SHA-256 credentials
`app-user` / `app-secret`, as set up in the tutorial.

## Build a connection string

The shape Nimbus expects:

```text
mongodb://<username>:<password>@<host>:<port>/<database>
```

- Credentials are your server's configured SCRAM-SHA-256 username and
  password, URL-encoded if they contain special characters.
- The database name selects the [Nimbus tenant](/reference/mongodb/tenant-isolation/).
  If you omit it, you land in the tenant `default`.
- Nimbus is a single endpoint, not a replica set — never pass a
  `replicaSet` option. With a single host the driver connects directly;
  appending `?directConnection=true` is harmless but not required.

If your project uses the Nimbus CLI, the `@nimbus/mongodb` helper package
builds the string for you. Provision it into your project:

```bash
nimbus packages provision mongodb
```

Then:

```javascript
import { mongoUri } from "@nimbus/mongodb";

const uri = mongoUri({
  username: "app-user",
  password: "app-secret",
  database: "myapp",
});
// mongodb://app-user:app-secret@127.0.0.1:27017/myapp?directConnection=true
```

`mongoUri()` defaults to `127.0.0.1`, port `27017`, and database `default`,
URL-encodes the credentials, and always appends `directConnection=true`.

## Basic CRUD

```javascript
import { MongoClient } from "mongodb";

const client = new MongoClient(
  "mongodb://app-user:app-secret@127.0.0.1:27017/myapp?directConnection=true",
);
await client.connect();
const tasks = client.db("myapp").collection("tasks");

// Create
await tasks.insertOne({ title: "Write docs", done: false, priority: 2 });
await tasks.insertMany([
  { title: "Review PR", done: false, priority: 1 },
  { title: "Ship release", done: true, priority: 3 },
]);

// Read — implicit equality and comparison operators
const open = await tasks.find({ done: false }).toArray();
const urgent = await tasks.find({ priority: { $lte: 2 } }).toArray();

// Update
await tasks.updateOne({ title: "Write docs" }, { $set: { done: true } });
await tasks.updateMany({ done: true }, { $inc: { priority: -1 } });

// Delete
await tasks.deleteOne({ title: "Ship release" });

await client.close();
```

Filters support implicit equality plus `$eq`, `$ne`, `$gt`, `$gte`, `$lt`,
and `$lte`. Other filter operators (`$in`, `$or`, `$regex`, `$exists`, ...)
are rejected with a `BadValue` error — see
[supported operations](/reference/mongodb/operations/) for the full surface.

## Upserts and findAndModify

```javascript
// Insert-or-update keyed on a field
await tasks.updateOne(
  { title: "Triage inbox" },
  { $set: { done: false }, $setOnInsert: { priority: 5 } },
  { upsert: true },
);

// Atomically claim a task and get the updated document back
const claimed = await tasks.findOneAndUpdate(
  { done: false },
  { $set: { owner: "ada" } },
  { returnDocument: "after" },
);
```

## Count and distinct

```javascript
const openCount = await tasks.countDocuments({ done: false });
const owners = await tasks.distinct("owner");
```

## Aggregate

The pipeline stages `$match`, `$sort`, `$limit`, `$skip`, `$project`,
`$addFields`, `$count`, `$group`, and `$unwind` are supported:

```javascript
const byOwner = await tasks
  .aggregate([
    { $match: { done: false } },
    { $group: { _id: "$owner", open: { $sum: 1 } } },
    { $sort: { open: -1 } },
  ])
  .toArray();
```

Unsupported stages are rejected with an error naming the stage, so a
pipeline never silently degrades.

## Use transactions

Multi-document transactions work through standard driver sessions. Writes
inside the transaction are isolated until commit; if a concurrent write
conflicts, the commit fails with a `WriteConflict` error and your
application decides whether to retry:

```javascript
const session = client.startSession();
try {
  await session.withTransaction(async () => {
    const db = client.db("myapp");
    await db.collection("accounts").updateOne(
      { name: "checking" },
      { $inc: { balance: -100 } },
      { session },
    );
    await db.collection("accounts").updateOne(
      { name: "savings" },
      { $inc: { balance: 100 } },
      { session },
    );
  });
} finally {
  await session.endSession();
}
```

A transaction stays within one database — one Nimbus tenant. Start separate
transactions for separate tenants.

## Connect from Python

The same connection string works with `pymongo`:

```python
from pymongo import MongoClient

client = MongoClient(
    "mongodb://app-user:app-secret@127.0.0.1:27017/myapp?directConnection=true"
)
tasks = client["myapp"]["tasks"]

tasks.insert_one({"title": "Write docs", "done": False})
for doc in tasks.find({"done": False}):
    print(doc)
```

## Connect from Go

With the official Go driver:

```go
package main

import (
	"context"
	"fmt"

	"go.mongodb.org/mongo-driver/v2/bson"
	"go.mongodb.org/mongo-driver/v2/mongo"
	"go.mongodb.org/mongo-driver/v2/mongo/options"
)

func main() {
	uri := "mongodb://app-user:app-secret@127.0.0.1:27017/myapp?directConnection=true"
	client, err := mongo.Connect(options.Client().ApplyURI(uri))
	if err != nil {
		panic(err)
	}
	defer client.Disconnect(context.Background())

	tasks := client.Database("myapp").Collection("tasks")
	if _, err := tasks.InsertOne(context.Background(),
		bson.M{"title": "Write docs", "done": false}); err != nil {
		panic(err)
	}

	cur, err := tasks.Find(context.Background(), bson.M{"done": false})
	if err != nil {
		panic(err)
	}
	var results []bson.M
	if err := cur.All(context.Background(), &results); err != nil {
		panic(err)
	}
	fmt.Println(results)
}
```

## Work with multiple tenants

Each database name is its own isolated Nimbus tenant. One client can address
several:

```javascript
const acme = client.db("acme").collection("orders");
const globex = client.db("globex").collection("orders");

await acme.insertOne({ sku: "A-1" });
// Invisible to globex — different tenant, different storage namespace.
const none = await globex.find({ sku: "A-1" }).toArray(); // []
```

Tenants are created automatically on first access. Database names must be
valid tenant IDs: ASCII letters, digits, `_`, and `-`, up to 128 characters.

## Change streams are not supported

`collection.watch()` fails: the server rejects `$changeStream` pipelines
with a `CommandNotSupported` error rather than emulating a partial feature.
Poll with `find` where you need to observe changes through the MongoDB
endpoint.

## Runnable example apps

Two complete apps live under
[`examples/mongodb/`](https://github.com/nimbus/nimbus/tree/main/examples/mongodb)
in the source repository. Run them in place from a checkout, against a Nimbus
server serving the MongoDB endpoint:

- **[Node (`node`)](https://github.com/nimbus/nimbus/tree/main/examples/mongodb/node)**
  — a script using the stock `mongodb` driver and the `@nimbus/mongodb` URI
  helper to run create, read, update, and read-again over a `messages`
  collection.
- **[Tasks](https://github.com/nimbus/nimbus/tree/main/examples/mongodb/tasks)**
  — the shared [tasks](https://github.com/nimbus/nimbus/blob/main/examples/specs/tasks.md)
  task list: create, list, toggle, and delete over a `tasks` collection, sorted
  newest-first by `createdAt`. Because change streams are unavailable on this
  surface, the live-update flow degrades to polling — the example re-reads the
  list until a change appears rather than emulating a subscription.

Both apps set `NIMBUS_MONGODB_USERNAME` and `NIMBUS_MONGODB_PASSWORD` (host and
port default to `127.0.0.1:27017`); see each README for the exact run command.

## Related pages

- [Supported operations](/reference/mongodb/operations/) — the exact
  command, filter, update, and aggregation surface.
- [Supported drivers](/reference/mongodb/drivers/) — driver and tool
  compatibility.
- [Tenant isolation](/reference/mongodb/tenant-isolation/) — how database
  names map to tenants.
