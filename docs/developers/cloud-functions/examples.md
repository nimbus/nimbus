---
title: Cloud Functions example apps
description: A runnable firebase-functions/v2 bundle on Nimbus — a Firestore document trigger and a plain HTTP handler, with at-least-once delivery.
sidebar:
  label: Examples
  order: 3
---

A runnable `firebase-functions/v2` bundle that runs on Nimbus without changing
its imports. HTTP and callable exports serve from the main Nimbus port, and
Firestore document triggers get at-least-once delivery with durable retry. Read
the [overview](/developers/cloud-functions/) first if you have not run a
functions bundle on Nimbus yet.

The example lives under
[`examples/cloud-functions/`](https://github.com/nimbus/nimbus/tree/main/examples/cloud-functions)
in the source repository. Run it in place from a checkout.

## Run the example

```bash
nimbus dev
nimbus deploy [TARGET]
```

Run from the example directory. `TARGET` is a URL or a configured target name.
Omit it to target your local server. Nimbus generates the functions bundle
under `.nimbus/firebase/`.

## The app

**[Tasks](https://github.com/nimbus/nimbus/tree/main/examples/cloud-functions/tasks)**
reacts to the shared `tasks` collection and exposes a plain HTTP handler:

- `deriveTask` is an `onDocumentCreated` trigger. When a client inserts a task
  document, the trigger writes a derived record to `taskDerivations`.
- `taskDetails` is an HTTP handler. It reads a task and its derived record by id
  and returns their current fields as JSON.

Once a task exists, call the HTTP export on the main Nimbus port:

```bash
curl "http://localhost:8080/taskDetails?taskId=TASK_DOCUMENT_ID"
```

## A trigger, not a CRUD client

Cloud Functions is handler code, not a task data client, so this surface
implements the [tasks](https://github.com/nimbus/nimbus/blob/main/examples/specs/tasks.md)
spec through a trigger rather than through create/read/update/delete calls. The
observable behavior is a derived write that appears after a client creates a
task. It is not a client subscription.

Nimbus delivers Firestore triggers at least once, so the trigger is
idempotent. The derived document uses the source task's id as its own id.
`set()` writes the complete value. A retry therefore overwrites the same
document instead of duplicating it or double-counting.
