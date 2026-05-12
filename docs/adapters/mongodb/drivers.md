# Compatible Drivers

Any MongoDB driver that supports the modern wire protocol (OP_MSG) and
`directConnection=true` works with Nimbus. This includes all current
official drivers maintained by MongoDB, Inc.

## Official Drivers

| Language | Driver | Install |
|---|---|---|
| JavaScript/TypeScript | [mongodb](https://www.mongodb.com/docs/drivers/node/current/) | `npm install mongodb` |
| Python | [pymongo](https://www.mongodb.com/docs/drivers/pymongo/) | `pip install pymongo` |
| Python (async) | [motor](https://www.mongodb.com/docs/drivers/motor/) | `pip install motor` |
| Go | [mongo-go-driver](https://www.mongodb.com/docs/drivers/go/current/) | `go get go.mongodb.org/mongo-driver` |
| Java | [mongodb-driver-sync](https://www.mongodb.com/docs/drivers/java/sync/current/) | Maven: `org.mongodb:mongodb-driver-sync` |
| Kotlin | [mongodb-driver-kotlin](https://www.mongodb.com/docs/drivers/kotlin/coroutine/current/) | Maven: `org.mongodb:mongodb-driver-kotlin-coroutine` |
| C#/.NET | [MongoDB.Driver](https://www.mongodb.com/docs/drivers/csharp/current/) | `dotnet add package MongoDB.Driver` |
| Rust | [mongodb](https://www.mongodb.com/docs/drivers/rust/current/) | `cargo add mongodb` |
| Ruby | [mongo](https://www.mongodb.com/docs/drivers/ruby/current/) | `gem install mongo` |
| PHP | [mongodb](https://www.mongodb.com/docs/drivers/php/current/) | `pecl install mongodb` + `composer require mongodb/mongodb` |
| Swift | [mongodb-vapor](https://www.mongodb.com/docs/drivers/swift/current/) | Swift Package Manager |
| C | [libmongoc](https://www.mongodb.com/docs/drivers/c/) | System package or source build |
| C++ | [mongocxx](https://www.mongodb.com/docs/drivers/cxx/) | System package or source build |

## Nimbus URI Helper

| Package | Install |
|---|---|
| [@nimbus/mongodb](../../../packages/mongodb/) | `npm install @nimbus/mongodb` |

Builds a `mongodb://` URI with `directConnection=true` and sensible defaults.
See the [Client Package](README.md#client-package) section for details.

## Tools

| Tool | Install |
|---|---|
| [mongosh](https://www.mongodb.com/docs/mongodb-shell/) (shell) | `npm install -g mongosh` |
| [MongoDB Compass](https://www.mongodb.com/products/tools/compass) (GUI) | Desktop installer |

## ODMs and ORMs

Popular ODMs and ORMs that sit on top of the official drivers should also
work since they delegate to the underlying driver for wire protocol
communication:

- **Mongoose** (Node.js) -- ODM with schema validation and middleware
- **Mongoid** (Ruby) -- ODM for Ruby on Rails
- **MongoEngine** (Python) -- document-object mapper for Python
- **Spring Data MongoDB** (Java/Kotlin) -- Spring framework integration

## Requirements

Always connect with `directConnection=true`. Nimbus is not a MongoDB replica
set, so topology discovery will fail without this flag. The `@nimbus/mongodb`
URI helper includes it automatically.

## FerretDB

[FerretDB](https://www.ferretdb.com) occupies the same architectural position
as Nimbus's MongoDB adapter -- it is a server that accepts MongoDB wire
protocol connections and stores data in a non-MongoDB backend
(Postgres/SQLite). FerretDB and Nimbus do not talk to each other.

From a client's perspective they are interchangeable: any app using a MongoDB
driver with `directConnection=true` can point at either one. The difference
is what's behind the protocol -- FerretDB proxies to Postgres, while Nimbus
routes through its own engine with pluggable storage, V8 runtime execution,
the Convex/Nimbus SDK surface, and multi-adapter support.
