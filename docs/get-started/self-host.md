---
title: Self-host quickstart
description: Run the Nimbus server and make your first requests with curl in about two minutes.
sidebar:
  order: 3
---

Run Nimbus as a server and talk to it over plain HTTP. No codegen, no schema
files, no Node.js — `curl` is enough.

## 1. Install Nimbus

```bash
brew install nimbus/tap/nimbus
```

Other platforms ship via the install script and release binaries — see the
[install options](https://github.com/nimbus/nimbus#install).

## 2. Start the server

```bash
nimbus start --port 8080 --data-dir ./data
```

The server binds to localhost by default and persists to `./data` with the
embedded SQLite backend.

## 3. Grab the admin token

The native API is protected by a local admin token, created on first boot
and stored as a JSON file:

```bash
# Linux
export NIMBUS_TOKEN=$(jq -r .token ~/.local/share/nimbus/auth/token)

# macOS
export NIMBUS_TOKEN=$(jq -r .token "$HOME/Library/Application Support/nimbus/auth/token")
```

On Windows the file is `%LOCALAPPDATA%\nimbus\auth\token.json`. Requests
without the token get a `401`.

## 4. Create a tenant

```bash
curl -s -X POST http://localhost:8080/api/tenants \
  -H "Authorization: Bearer $NIMBUS_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"id": "demo"}'
```

## 5. Insert a document

```bash
curl -s -X POST http://localhost:8080/api/tenants/demo/documents \
  -H "Authorization: Bearer $NIMBUS_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"table": "messages", "fields": {"text": "hello world", "author": "you"}}'
```

## 6. Query it back

```bash
curl -s -X POST http://localhost:8080/api/tenants/demo/query \
  -H "Authorization: Bearer $NIMBUS_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"table": "messages", "filters": []}'
```

`nimbus start` runs the same engine as `nimbus dev` without codegen. The
native HTTP/WebSocket API is the front door here, and the protocol adapters
are served alongside it by default — the Firestore-compatible routes on this
same listener, the MongoDB endpoint on `127.0.0.1:27017`, and the DynamoDB
endpoint on `127.0.0.1:8000` (`--no-firestore`, `--no-mongodb`, and
`--no-dynamodb` switch them off). See the per-adapter guides under
[Developers](/developers/).

## Next steps

- [Native API guide](/developers/native/) — tenants, documents, queries,
  and live subscriptions over WebSocket, from any language.
- [Operators](/operators/) — production deployment, tenants, storage
  backends (Postgres, MySQL, libSQL, redb), encryption at rest, networking,
  and observability.
- [Reference](/reference/) — every `nimbus start` flag and the native
  HTTP/WebSocket API.
