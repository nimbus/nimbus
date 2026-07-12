# Cloud Functions examples

Nimbus runs covered `firebase-functions/v2` handlers without changing their
imports. HTTP and callable exports serve from the main Nimbus port, while
Firestore document triggers use durable, at-least-once delivery with retry.

Docs: [Cloud Functions](../../docs/developers/cloud-functions/index.md).

## Examples

- **[`tasks/`](tasks/)** — a Firebase v2 functions bundle with a directly
  callable HTTP handler and an `onDocumentCreated` trigger that writes a
  per-task derived document.

## `tasks` spec support

Cloud Functions is handler code, not a task data client, so CRUD and live-query
columns do not apply to this surface.

| Spec role | Supported | Observable behavior |
| --- | --- | --- |
| `tasks.create` trigger | via Firestore trigger | `onDocumentCreated` writes an idempotent derived document keyed by the source task id. |
| CRUD client | n/a | The smoke uses plain Firestore REST requests to create test data externally. |
| `tasks.live-update` | n/a | Durable trigger delivery replaces a client subscription on this surface. |
| HTTP handler | yes | `taskDetails` reads a named task and returns its current fields as JSON. |

This is the Cloud Functions row of the shared [`tasks`](../specs/tasks.md)
spec: a derived write is observed after `tasks.create`, rather than treating
the functions bundle as a CRUD client.

## Running

```bash
nimbus dev
nimbus deploy [TARGET]
```

Run either command from an example app directory. `TARGET` is a URL or a
configured target name; omit it to use the local target.
