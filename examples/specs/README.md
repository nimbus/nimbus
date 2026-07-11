# Example specs

A spec is one behavior contract for a canonical app, written once and shared by
every adapter that implements that app. It fixes the schema, the flows, and the
observable assertions so the same behavior can be compared across surfaces
(Convex, Firebase, MongoDB, DynamoDB, Cloud Functions, native).

Each spec is the unit an example's smoke script asserts against: the smoke seeds
data, drives the flows, and checks the observable assertions — never just that
the code compiled. A spec also carries a per-adapter supported-subset table, so
a surface that cannot do part of the spec (for example a driver with no live
queries) says so explicitly instead of pretending.

## Specs

- [`tasks.md`](tasks.md) — CRUD plus a live task list; the hello-world every
  adapter implements.

More canonical apps (chat, agent-chat, agent-worker, filedrop, jobs) get their
own spec files as their examples land.
