# Convex examples

Nimbus speaks the Convex surface: you author functions with
`convex/_generated/server`, `convex/values`, and a `convex/schema.ts`, and run
`convex/react` and `convex/browser` clients against Nimbus unchanged. These
examples are Nimbus ports and adaptations of common Convex shapes, not the
official Convex demos running as-is.

Docs: [Convex](../../docs/developers/convex/index.md). Contributor status on the
compiled/runtime subset these apps exercise lives in [`DEVNOTES.md`](DEVNOTES.md).

> **⚠️ Monorepo-only — do not copy this directory out of the repo yet.**
> These apps import the workspace `convex` package, which is Nimbus's Convex
> compatibility package. It deliberately takes the official `convex` name and
> `convex` bin so your code runs unchanged — but that also means if you copy an
> app out of this monorepo and run `npm install`, npm silently pulls the **real
> Convex Cloud** `convex` package instead of Nimbus's, and your app quietly
> talks to Convex's hosted service rather than your Nimbus server. There is no
> error; the substitution is invisible. Until the `nimbus init --example`
> scaffolder ships (it rewrites workspace deps to published pins), run these
> examples in place, from a checkout of this repository. The other adapter
> examples import stock upstream clients (`firebase`, `mongodb`, the AWS SDK)
> and have no such name collision — a copied-out app there fails loudly with an
> unresolved workspace dependency instead.

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

| CRUD anchors | `tasks.live-update` |
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
