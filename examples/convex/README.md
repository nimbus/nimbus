# Convex examples

Nimbus speaks the Convex surface: you author functions with
`convex/_generated/server`, `convex/values`, and a `convex/schema.ts`, and run
`convex/react` and `convex/browser` clients against Nimbus unchanged. These
examples are Nimbus ports and adaptations of common Convex shapes, not the
official Convex demos running as-is.

Docs: [Convex](../../docs/developers/convex/index.md). Contributor status on the
compiled/runtime subset these apps exercise lives in [`DEVNOTES.md`](DEVNOTES.md).

## Examples

- **[`html/`](html/)** — a React app using `convex/react` and generated
  `_generated/api.ts` over Nimbus's Convex transport, exercising live inserts,
  scheduled writes, patch/delete, and paginated queries.
- **[`http/`](http/)** — a browser app using `convex/browser` and generated refs
  without React, including compiled `httpAction` routes and an action that
  delegates to an internal mutation.
- **[`node/`](node/)** — a Node app using generated refs, an injected WebSocket
  implementation, point-in-time reads, and live subscriptions.
- **[`showcase/`](showcase/)** — a small app used to exercise the developer
  console's function source visibility (source view, symbol navigation, and
  type-hover). See its [README](showcase/README.md).

## `tasks` spec support

| Create / List / Toggle / Delete | Live view |
| --- | --- |
| yes | yes (reactive query) |

Full [`tasks`](../specs/tasks.md) spec via `convex/react` / `convex/browser`.

## Running

```bash
nimbus dev
nimbus deploy [TARGET]
```

`TARGET` is a URL or a configured target name; omit it to target your local
server. The individual apps also have standalone dev-server scripts —
`npm run convex:demo:html`, `convex:demo:http`, `convex:demo:node`, each paired
with the matching `npm run convex:server:*` support server.
