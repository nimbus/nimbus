# Cloud Functions tasks

A Firebase Cloud Functions v2 bundle that reacts to the shared `tasks`
collection and exposes a plain HTTP handler. `deriveTask` listens for new task
documents and writes a derived record to `taskDerivations`; `taskDetails`
reads a task and its derived record by id and returns their current fields as
JSON.

The layout matches `nimbus init cloud-functions`: `firebase.json` points to the
nested `functions/` runtime package. The app root has a separate smoke package
because the smoke is an external Firestore client, not handler code.

## Spec subset

Cloud Functions is not a CRUD client, so its shared [`tasks`
spec](../../specs/tasks.md) row is intentionally framed around trigger behavior.

| Spec role | Supported | Observable behavior |
| --- | --- | --- |
| `tasks.create` trigger | via `onDocumentCreated` | A task insert produces a `taskDerivations/{taskId}` document. |
| CRUD client | n/a | Plain `fetch` calls to Nimbus's native document API create and read external test data. |
| `tasks.live-update` | n/a | This surface runs server-side handlers rather than a client subscription. |
| HTTP handler | yes | `GET /taskDetails?taskId=...` returns the current task and derived documents. |

Nimbus delivers Firestore triggers at least once. The derived document uses
the source task id as its own id and `set()` writes the complete deterministic
value. A retry therefore overwrites the same document instead of incrementing
a shared counter or creating a duplicate.

## Running

```bash
nimbus dev
nimbus deploy [TARGET]
```

Run from this directory. `TARGET` is a URL or configured target name; omit it
to use the local target. Nimbus generates the functions bundle under
`.nimbus/firebase/`.

`smoke.ts` requires Node.js >=22 <25 (runs via `--experimental-strip-types`).

Once a task exists, call the HTTP export on the main Nimbus port:

```bash
curl "http://localhost:8080/taskDetails?taskId=TASK_DOCUMENT_ID"
```

## Smoke verification

With this functions bundle served by Nimbus at `http://localhost:8080`:

```bash
NIMBUS_ADMIN_TOKEN="$(nimbus auth token)" npm run smoke -w cloud-functions-tasks
```

The smoke uses plain `fetch` against the Firestore REST commit endpoint for the
external task insert. This is the same wire path a Firestore client uses, but
keeps a data-client dependency out of the server-side example. It polls
`/taskDetails` until the source-keyed derived document lands, then makes a
second direct handler call and hard-asserts the JSON response. Set
`NIMBUS_CLOUD_FUNCTIONS_URL` or `NIMBUS_TENANT_ID` to override the defaults.
`nimbus dev` accepts the smoke's default emulator token; set
`NIMBUS_FIREBASE_AUTH_TOKEN` for a server with configured application auth. It
prints separate `PASS` or `FAIL` lines for the trigger side effect and HTTP
response required by EX3.6. A server that does not require local admin
authentication can omit `NIMBUS_ADMIN_TOKEN`.
