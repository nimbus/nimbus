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

## 3. Create a tenant

```bash
curl -s -X POST http://localhost:8080/api/tenants \
  -H "Content-Type: application/json" \
  -d '{"id": "demo"}'
```

## 4. Insert a document

```bash
curl -s -X POST http://localhost:8080/api/tenants/demo/documents \
  -H "Content-Type: application/json" \
  -d '{"table": "messages", "fields": {"text": "hello world", "author": "you"}}'
```

## 5. Query it back

```bash
curl -s -X POST http://localhost:8080/api/tenants/demo/query \
  -H "Content-Type: application/json" \
  -d '{"table": "messages", "filters": []}'
```

`nimbus start` runs the same engine as `nimbus dev` without codegen — stock
MongoDB drivers, Firestore SDKs, DynamoDB SDKs, or any HTTP client connect to
the same data.

## Next steps

- [Operators](/operators/) — production deployment, tenants, storage
  backends (Postgres, MySQL, libSQL, redb), encryption at rest, networking,
  and observability.
- [Reference](/reference/) — every `nimbus start` flag and the native
  HTTP/WebSocket API.
