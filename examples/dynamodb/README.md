# DynamoDB examples

Nimbus exposes a DynamoDB wire-protocol listener, so the stock
`@aws-sdk/client-dynamodb` client runs data operations against Nimbus with an
endpoint override. The `@nimbus/dynamodb` helper supplies that endpoint,
region, and credential configuration while leaving the AWS SDK API unchanged.

Docs: [DynamoDB](../../docs/developers/dynamodb/index.md).

## Examples

- **[`tasks/`](tasks/)** — a headless Node app using the stock AWS SDK client
  and `@nimbus/dynamodb` for table creation and task CRUD against the dedicated
  DynamoDB listener.

## `tasks` spec support

| Flow anchor | Supported | Observable behavior |
| --- | --- | --- |
| `tasks.create` | yes | `PutItem` creates an incomplete task with a stable id and creation time. |
| `tasks.list` | yes | `Scan` reads all tasks and the client sorts them newest-first by `createdAt`. |
| `tasks.toggle` | yes | `UpdateItem` persists `completed: true`. |
| `tasks.delete` | yes | `DeleteItem` removes the selected task. |
| `tasks.live-update` | polled | Repeated `Scan` reads observe changes. This is polling, not a live subscription. |

DynamoDB has no live-query view on this surface, so the example cannot meet the
shared [`tasks`](../specs/tasks.md) spec's no-polling subscription behavior. It
satisfies `tasks.live-update` by polling `tasks.list` instead.

## Running

```bash
nimbus dev
nimbus deploy [TARGET]
```

`TARGET` is a URL or a configured target name; omit it to target your local
server. The DynamoDB listener uses its own endpoint (`127.0.0.1:8000` by
default), and the access key id selects the Nimbus tenant.
