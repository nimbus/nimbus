# Examples

## Using `@nimbus/mongodb` URI helper

```typescript
import { MongoClient } from "mongodb";
import { uri } from "@nimbus/mongodb";

const client = new MongoClient(uri({ database: "myapp" }));
await client.connect();

const db = client.db("myapp");
const messages = db.collection("messages");

// Insert
await messages.insertOne({ author: "Alice", body: "Hello" });

// Query
const docs = await messages.find({ author: "Alice" }).toArray();

// Update
await messages.updateOne({ author: "Alice" }, { $set: { body: "Updated" } });

// Aggregate
const results = await messages.aggregate([
  { $match: { author: "Alice" } },
  { $sort: { _id: -1 } },
  { $limit: 10 },
]).toArray();

// Transactions
const session = client.startSession();
session.startTransaction();
await messages.insertOne({ author: "Bob", body: "In a transaction" }, { session });
await messages.updateOne({ author: "Alice" }, { $set: { peer: "Bob" } }, { session });
await session.commitTransaction();

await client.close();
```

## Using stock `mongodb` driver directly

```typescript
import { MongoClient } from "mongodb";

const client = new MongoClient(
  "mongodb://127.0.0.1:27017/myapp?directConnection=true"
);
await client.connect();

const db = client.db("myapp");
const messages = db.collection("messages");

await messages.insertOne({ author: "Alice", body: "Hello" });
const docs = await messages.find({ author: "Alice" }).toArray();
```

## With authentication

```typescript
import { MongoClient } from "mongodb";
import { uri } from "@nimbus/mongodb";

const client = new MongoClient(uri({
  database: "myapp",
  username: "admin",
  password: "secret",
}));
await client.connect();
```

Or with the stock driver:

```typescript
const client = new MongoClient(
  "mongodb://admin:secret@127.0.0.1:27017/myapp?directConnection=true"
);
```

## Python (pymongo)

```python
from pymongo import MongoClient

client = MongoClient(
    "mongodb://127.0.0.1:27017/myapp?directConnection=true"
)
db = client["myapp"]
messages = db["messages"]

messages.insert_one({"author": "Alice", "body": "Hello from Python"})
docs = list(messages.find({"author": "Alice"}))
```

## Go

```go
package main

import (
    "context"

    "go.mongodb.org/mongo-driver/v2/bson"
    "go.mongodb.org/mongo-driver/v2/mongo"
    "go.mongodb.org/mongo-driver/v2/mongo/options"
)

func main() {
    uri := "mongodb://127.0.0.1:27017/myapp?directConnection=true"
    client, err := mongo.Connect(options.Client().ApplyURI(uri))
    if err != nil {
        panic(err)
    }
    defer client.Disconnect(context.Background())

    coll := client.Database("myapp").Collection("messages")
    _, err = coll.InsertOne(context.Background(), bson.M{
        "author": "Alice",
        "body":   "Hello from Go",
    })
}
```

## Change Streams

```typescript
import { MongoClient } from "mongodb";
import { uri } from "@nimbus/mongodb";

const client = new MongoClient(uri({ database: "myapp" }));
await client.connect();

const messages = client.db("myapp").collection("messages");

const stream = messages.watch();
stream.on("change", (change) => {
  console.log("Change:", change.operationType, change.fullDocument);
});

// Insert triggers the change stream
await messages.insertOne({ author: "Alice", body: "Hello" });
```

## Demo App

See the [mongodb/node demo](../../../demos/mongodb/node/) for a runnable
Node.js example.
