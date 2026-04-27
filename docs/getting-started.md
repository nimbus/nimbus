# Getting Started

Install Neovex, start the server, and run your first queries in under 5 minutes.

## Install

```bash
# macOS / Linux via Homebrew
brew install agentstation/tap/neovex
```

Or download a binary from [GitHub Releases](https://github.com/agentstation/neovex/releases/latest). See the root [README](../README.md#install) for all platforms and build-from-source instructions.

## Start the server

```bash
neovex start --port 8080 --data-dir ./data
```

Storage, compute, and networking are now running on `http://localhost:8080`.

## Create a tenant and insert data

```bash
# Create a tenant
curl -s -X POST http://localhost:8080/api/tenants \
  -H "Content-Type: application/json" \
  -d '{"id": "demo"}'

# Insert a document
curl -s -X POST http://localhost:8080/api/tenants/demo/documents \
  -H "Content-Type: application/json" \
  -d '{"table": "messages", "fields": {"text": "hello world", "author": "you"}}'

# Query it back
curl -s -X POST http://localhost:8080/api/tenants/demo/query \
  -H "Content-Type: application/json" \
  -d '{"table": "messages", "filters": []}'
```

## Pick an adapter

Neovex speaks the protocols of platforms you already use. Choose the one that matches your existing client code:

| Adapter | When to use it | Guide |
|---------|---------------|-------|
| **MongoDB** | You have a MongoDB app or want a familiar document API | [adapters/mongodb/](adapters/mongodb/) |
| **Convex** | You're migrating from Convex or want reactive queries + React hooks | [adapters/convex/](adapters/convex/) |
| **Firebase** | You're migrating from Firestore | [adapters/firebase/](adapters/firebase/) |
| **Cloud Functions** | You have Firebase v2 or Functions Framework handlers | [adapters/cloud-functions/](adapters/cloud-functions/) |
| **Native** | You want the direct REST and WebSocket API | [adapters/native/](adapters/native/) |

## Example: MongoDB adapter

The MongoDB adapter runs on port 27017 by default. Connect with any standard MongoDB driver:

```bash
# Connect with mongosh
mongosh mongodb://127.0.0.1:27017/default?directConnection=true

# Insert and query
db.messages.insertOne({ author: "Alice", body: "Hello from mongosh" })
db.messages.find()
```

See the [MongoDB adapter guide](adapters/mongodb/) for TypeScript, Python, and Go examples.

## Next steps

- [Storage backends](operating/storage-backends.md) -- configure Postgres, MySQL, or other backends
- [CLI reference](operating/cli.md) -- all server flags and commands
- [Current capabilities](current-capabilities.md) -- what's implemented today
- [Demos](../demos/README.md) -- working example applications
