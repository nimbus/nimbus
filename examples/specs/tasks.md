# Spec: `tasks`

The industry hello-world: a task list with create, read, update, and delete,
plus a live view that updates without re-polling. Every adapter's `tasks`
example implements this spec for its supported subset and its smoke script
asserts the flows below.

## Schema

One collection, `tasks`, with documents shaped:

| Field | Type | Notes |
| --- | --- | --- |
| `text` | string | The task description. Required, non-empty. |
| `completed` | boolean | Whether the task is done. Defaults to `false`. |
| `createdAt` | number | Creation time in epoch milliseconds. Set on insert. |

The adapter's native id field (`_id`, `id`, document key, etc.) identifies a
task. Examples never assume a specific id encoding beyond "stable and unique".

## Flows

1. **Create** — insert a task with `text` and `completed: false`. The created
   task is retrievable and carries a stable id and a `createdAt`.
2. **List** — read all tasks, newest first by `createdAt`.
3. **Toggle** — set `completed` to the opposite of its current value for one
   task by id. The change is visible on the next read.
4. **Delete** — remove one task by id. It no longer appears in the list.
5. **Live** *(surfaces that support subscriptions)* — an open subscription to
   the task list receives the create/toggle/delete above as pushed updates,
   with no polling.

## Observable assertions (smoke-checkable)

A smoke script must assert behavior, not just that code ran:

- After **Create**, a **List** contains exactly one task with the inserted
  `text` and `completed === false`.
- After **Toggle** on that task, a **List** shows `completed === true`.
- After a second **Create**, **List** returns both tasks ordered newest-first.
- After **Delete** of the first task, **List** no longer contains it.
- **Live**: with a subscription open before **Create**, the subscription
  delivers the new task without an explicit re-read. Adapters that do not
  support live queries assert this by polling **List** instead and record the
  gap in their supported-subset table.

## Supported subset by adapter

| Adapter | Create / List / Toggle / Delete | Live view | Notes |
| --- | --- | --- | --- |
| Native (`@nimbus/nimbus`) | yes | yes (WebSocket subscription) | Full spec. |
| Convex | yes | yes (reactive query) | Full spec via `convex/react` / `convex/browser`. |
| Firebase / Firestore | yes | yes (`onSnapshot`) | Full spec; live view uses the `Listen` bridge. |
| MongoDB | yes | no | Stock driver CRUD. Change streams are not supported, so the live view degrades to polling **List**. |
| DynamoDB | yes | no | Stock AWS SDK CRUD. No live view; polls **List**. |
| Cloud Functions | via triggers | n/a | Not a client; an `onDocumentCreated` trigger performs a derived write, asserted as an observable side effect after **Create**. |

Each example's README repeats its own row and links back to this spec.
