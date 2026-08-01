---
title: DynamoDB example apps
description: A runnable DynamoDB app on Nimbus — the shared tasks list driven by the stock AWS SDK against the Nimbus DynamoDB endpoint.
sidebar:
  label: Examples
  order: 2
---

A runnable DynamoDB app that builds on Nimbus with the stock
`@aws-sdk/client-dynamodb` client. The `@nimbus/dynamodb` helper supplies the
endpoint, region, and credentials. Everything else is the unchanged AWS SDK API.
Read the [overview](/developers/dynamodb/) first if you have not pointed an AWS
SDK at Nimbus yet.

The example lives under
[`examples/dynamodb/`](https://github.com/nimbus/nimbus/tree/main/examples/dynamodb)
in the source repository. Run it in place from a checkout.

## Run the example

```bash
nimbus dev
```

`nimbus dev` starts a local server with the DynamoDB listener on a dedicated
endpoint. The default is `127.0.0.1:8000`. When it detects the AWS SDK
dependency, the command writes the `NIMBUS_DYNAMODB_*` connection variables to
`.env.local`. The access key ID selects the Nimbus tenant.

Then run the example app from its directory. Its README gives the command. The
app reads those variables and drives the DynamoDB endpoint directly. It has no
Nimbus function bundle to deploy.

## The app

**[Tasks](https://github.com/nimbus/nimbus/tree/main/examples/dynamodb/tasks)**
is a headless task list against the DynamoDB endpoint. It creates a `tasks`
table with a single string partition key, `id`, then drives the shared
[tasks](https://github.com/nimbus/nimbus/blob/main/examples/specs/tasks.md) spec
through standard AWS SDK commands:

- `PutItem` creates an incomplete task with a stable id and creation time.
- `Scan` reads every task. Because a scan is unordered, the app sorts the
  results client-side by `createdAt`, newest first.
- `UpdateItem` toggles a task's `completed` flag.
- `DeleteItem` removes a task.

## Live updates are polled

DynamoDB has no live-query view on this surface. The app cannot meet the
spec's no-polling subscription behavior. It satisfies the live-update flow by
re-scanning `tasks` until a later read observes the change. This is polling,
not a live subscription. The example records that gap and does not emulate a
subscription.
