# Spec: `tasks`

The industry hello-world: a task list with create, read, update, and delete,
plus a live view that updates without re-polling. Every adapter's `tasks`
example implements this spec for its supported subset and its smoke script
asserts the flows below.

> **Target state, not current coverage.** This spec is the contract the `tasks`
> apps and their smoke scripts will meet — the per-surface `tasks` apps and the
> anchor-asserting smokes are still being built. The apps checked in under
> `examples/<adapter>/` today are different per-surface demos (they operate on a
> `messages` collection), and no smoke references the `tasks.*` anchors yet.
> Everything below — the flows, the anchors, and the supported-subset table —
> describes what those deliverables will satisfy, so treat it as the spec to
> build against, not a claim of what ships today.

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

Each flow has a stable **anchor** (`tasks.create`, `tasks.list`, …). The anchor
is the contract key: a smoke script references the flow it exercises by anchor
name, so a checker can confirm every anchor is covered and flag spec/smoke
drift mechanically. Anchors are append-only — never renamed or reused.

| Anchor | Flow | Observable assertion (what the smoke asserts) |
| --- | --- | --- |
| `tasks.create` | Insert a task with `text` and `completed: false`. | The created task is retrievable, carries a stable id and a `createdAt`, and a `tasks.list` right after contains exactly one task with the inserted `text` and `completed === false`. |
| `tasks.list` | Read all tasks. | After a second `tasks.create`, the read returns both tasks ordered newest-first by `createdAt`. |
| `tasks.toggle` | Set `completed` to the opposite value for one task by id. | A `tasks.list` after the toggle shows that task's `completed === true`. |
| `tasks.delete` | Remove one task by id. | A `tasks.list` after the delete no longer contains that task. |
| `tasks.live-update` | Open a subscription to the task list before `tasks.create`. | The subscription delivers the new task with no explicit re-read (no polling). Adapters without live queries satisfy this anchor by polling `tasks.list` instead and record the gap in the supported-subset table below. |

A smoke script must assert the observable outcome in the third column, not just
that the flow's code ran. Every anchor above is asserted by every adapter's
`tasks` smoke for its supported subset.

## Supported subset by adapter

This table is the target subset each `tasks` app will cover once built (see the
target-state note above); it is not a report of current app coverage. The CRUD
column covers `tasks.create` / `tasks.list` / `tasks.toggle` / `tasks.delete`;
the live column is the `tasks.live-update` anchor.

| Adapter | CRUD anchors | `tasks.live-update` | Notes |
| --- | --- | --- | --- |
| Native (`@nimbus/nimbus`) | yes | yes (WebSocket subscription) | Full spec. |
| Convex | yes | yes (reactive query) | Full spec via `convex/react` / `convex/browser`. |
| Firebase / Firestore | yes | yes (`onSnapshot`) | Full spec; live view uses the `Listen` bridge. |
| MongoDB | yes | polled | Stock driver CRUD. Change streams are not supported, so `tasks.live-update` is satisfied by polling `tasks.list`. |
| DynamoDB | yes | polled | Stock AWS SDK CRUD. No live view; `tasks.live-update` polls `tasks.list`. |
| Cloud Functions | via triggers | n/a | Not a client; an `onDocumentCreated` trigger performs a derived write, asserted as an observable side effect after `tasks.create`. |

Each example's README repeats its own row and links back to this spec.
