# MongoDB tasks

A headless task-list example using the stock `mongodb` driver against Nimbus's
MongoDB wire-protocol listener. It creates, lists, toggles, and deletes
documents in a `tasks` collection. The list is sorted client-side by the
`createdAt` epoch-millisecond field, newest first.

The app implements the MongoDB subset of the shared
[`tasks` spec](../../specs/tasks.md).

## Spec subset

| Flow anchor | Supported | Observable behavior |
| --- | --- | --- |
| `tasks.create` | yes | A new incomplete task has a stable id and creation time. |
| `tasks.list` | yes | Tasks are returned newest-first by `createdAt`. |
| `tasks.toggle` | yes | Updating a task persists `completed: true`. |
| `tasks.delete` | yes | Deleting a task removes it from the list. |
| `tasks.live-update` | polled | Repeated `tasks.list` reads observe changes. This is polling, not a live subscription. |

MongoDB change streams are not supported by Nimbus, so this surface cannot
meet the spec's no-polling live-subscription behavior. The example records that
gap directly and satisfies `tasks.live-update` by polling `find().toArray()`.

## Running

```bash
nimbus dev
nimbus deploy [TARGET]
```

`TARGET` is a URL or configured target name; omit it to use the local target.

## Run the script directly (development loop)

Set `NIMBUS_MONGODB_USERNAME` and `NIMBUS_MONGODB_PASSWORD`. Host and port
default to `127.0.0.1:27017`; override them with `NIMBUS_MONGODB_HOST` and
`NIMBUS_MONGODB_PORT`.

```bash
node --experimental-strip-types ./examples/mongodb/tasks/script.ts
```

## Smoke verification

With Nimbus running and the same credential variables set:

```bash
npm run smoke -w mongodb-tasks
```

The smoke clears the `demo` tenant's `tasks` collection, prints one `PASS` line
per flow anchor, and removes its test data afterward. The
`tasks.live-update` check performs an initial list read before insertion, then
polls every 200 milliseconds until a later read observes the new task.
