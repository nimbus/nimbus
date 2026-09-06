# Firebase / Firestore tasks

A focused task list built with stock `firebase/app` and
`firebase/firestore` imports against Nimbus. Firestore CRUD calls create,
toggle, and delete tasks; an `onSnapshot` query keeps the newest-first list
current without polling.

The app uses REST for unary calls and Nimbus's WebSocket `Listen` bridge for
live updates. It implements the full shared [`tasks` spec](../../specs/tasks.md).

## Spec subset

| Flow anchor | Supported | Observable behavior |
| --- | --- | --- |
| `tasks.create` | yes | A new incomplete task has a stable document id and creation time. |
| `tasks.list` | yes | Tasks render newest-first by `createdAt`. |
| `tasks.toggle` | yes | Toggling a task persists its completed state. |
| `tasks.delete` | yes | Deleting a task removes it from the list. |
| `tasks.live-update` | yes | `onSnapshot` pushes list changes through the Listen bridge without polling. |

## Running

```bash
npm run build -w firebase-tasks
nimbus dev --app-dir examples/firebase/tasks --no-open
nimbus deploy [TARGET]
```

`TARGET` is a URL or configured target name; omit it to use the local target.
The Firestore project id is `demo`, which maps directly to the same-named
Nimbus tenant.

Open `http://127.0.0.1:3210/examples/firebase/tasks/dist/` to run the built
browser app. It defaults to its page origin. Add `?server=<url>` only when
Nimbus is at another origin. The browser's emulator token is accepted only by
the loopback local-development policy that `nimbus dev` selects; production
startup remains fail-closed without a configured application auth provider.

`smoke.ts` requires Node.js >=22 <25 — it runs via `--experimental-strip-types
--experimental-transform-types`, and Node 25 dropped `transform-types`.

## Smoke verification

With Nimbus running at `http://localhost:8080`:

```bash
npm run smoke -w firebase-tasks
```

Set `NIMBUS_FIRESTORE_URL` to exercise another Nimbus URL and
`NIMBUS_FIRESTORE_PROJECT_ID` when it does not use the default `demo` project.
The smoke clears that project's `tasks` collection, then prints one `PASS` line
for every flow anchor, including a real `onSnapshot` push for
`tasks.live-update`.
