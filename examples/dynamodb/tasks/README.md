# DynamoDB tasks

A headless task-list example using the stock `@aws-sdk/client-dynamodb` client
against Nimbus's DynamoDB wire-protocol listener. `@nimbus/dynamodb` supplies
the endpoint and credentials; the app creates a `tasks` table, then creates,
lists, toggles, and deletes items through standard AWS SDK commands.

The table has a single string partition key, `id`. Because DynamoDB does not
order a `Scan`, the app sorts scanned items client-side by the numeric
`createdAt` field, newest first. Items implement the DynamoDB subset of the
shared [`tasks` spec](../../specs/tasks.md).

## Spec subset

| Flow anchor | Supported | Observable behavior |
| --- | --- | --- |
| `tasks.create` | yes | `PutItem` creates an incomplete task with a stable id and creation time. |
| `tasks.list` | yes | `Scan` reads all tasks and the client sorts them newest-first by `createdAt`. |
| `tasks.toggle` | yes | `UpdateItem` persists `completed: true`. |
| `tasks.delete` | yes | `DeleteItem` removes the selected task. |
| `tasks.live-update` | polled | Repeated `tasks.list` scans observe changes. This is polling, not a live subscription. |

DynamoDB has no live-query view on this surface. The example records that gap
and satisfies `tasks.live-update` by polling every 200 milliseconds.

## Running

```bash
nimbus dev
nimbus deploy [TARGET]
```

`TARGET` is a URL or a configured target name; omit it to use the local target.
When `nimbus dev` sees the AWS SDK dependency, it writes the
`NIMBUS_DYNAMODB_*` connection variables to `.env.local`. Without those
variables the app uses `clientConfig()`'s local defaults: `127.0.0.1:8000`,
`us-east-1`, and `nimbus` credentials. The table becomes `ACTIVE`
synchronously, so the app does not use a waiter; a repeated run reuses the
existing table.

Requires Node.js >=22 <25 (`script.ts`/`smoke.ts` run via
`--experimental-strip-types`).

## Smoke verification

With Nimbus running and the matching `NIMBUS_DYNAMODB_*` credentials set:

```bash
npm run smoke -w dynamodb-tasks
```

The smoke creates or reuses the `tasks` table, clears its items, prints one
`PASS` line per flow anchor, and removes its test data afterward. The
`tasks.live-update` assertion performs an initial list read, inserts a task,
then polls until a later read observes it.
