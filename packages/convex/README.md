# convex (Nimbus compatibility package)

A drop-in `convex` package that points an existing
[Convex](https://convex.dev) app at a [Nimbus](../../README.md) backend with
no source changes. It exposes the familiar `convex/react`, `convex/browser`,
`convex/server`, and `convex/values` import surfaces and the `convex codegen`
CLI.

Under the hood this is a thin compatibility wrapper over the first-party
[`nimbus`](../nimbus/README.md) SDK — `ConvexReactClient` extends
`NimbusReactClient`, `ConvexProvider` wraps `NimbusProvider`, and so on.
Behavior matches Nimbus; the names match Convex.

> The compatibility surface is intentionally partial and evolving. See the
> [Convex compatibility reference](../../docs/adapters/convex/compatibility.md)
> for the precise support matrix, and **always read**
> [`docs/adapters/convex/ai-guidelines.md`](../../docs/adapters/convex/ai-guidelines.md)
> before working on Convex-compatible code.

## Entry points

| Import | Use it for |
| --- | --- |
| `convex/react` | `ConvexProvider`, `ConvexProviderWithAuth`, `useQuery`, `useMutation`, `useAction`, `usePaginatedQuery`, `useConvexAuth` |
| `convex/browser` | `ConvexClient`, `ConvexReactClient`, `ConvexHttpClient`, `anyApi` |
| `convex/server` | `query`, `mutation`, `action`, schema/validator types for authoring functions |
| `convex/values` | The validator builder `v`, `GenericId`, `Validator`, `Infer` |

## Usage

### React

```tsx
import { ConvexReactClient } from "convex/browser";
import { ConvexProvider, useQuery } from "convex/react";
import { api } from "../convex/_generated/api";

const client = new ConvexReactClient(import.meta.env.VITE_CONVEX_URL);

function App() {
  return (
    <ConvexProvider client={client}>
      <Messages />
    </ConvexProvider>
  );
}

function Messages() {
  const messages = useQuery(api.messages.list, { channel: "general" });
  return <p>{messages === undefined ? "Loading…" : messages.length}</p>;
}
```

Point `VITE_CONVEX_URL` / `NEXT_PUBLIC_CONVEX_URL` at your Nimbus deployment
URL instead of a Convex cloud URL — everything else stays the same.

### Authoring functions

```ts
import { query } from "convex/server";
import { v } from "convex/values";

export const list = query({
  args: { channel: v.string() },
  handler: async (ctx, { channel }) =>
    ctx.db.query("messages").withIndex("by_channel", (q) => q.eq("channel", channel)).collect(),
});
```

## CLI

```bash
convex codegen --app <dir>   # generate _generated/* and the runtime bundle
convex help
```

`convex codegen` delegates to [`@nimbus/codegen`](../codegen/README.md). It uses
the `nimbus/` source root when present, otherwise `convex/`.

## Scripts

```bash
npm run build --workspace convex             # bundle dist/
npm run test --workspace convex              # selftest suite
npm run typecheck --workspace convex         # type-only selftest pass
npm run test:differential --workspace convex # compare behavior against upstream convex
```

## Related

- [`nimbus`](../nimbus/README.md) — the native SDK this package wraps
- [`@nimbus/codegen`](../codegen/README.md) — the codegen engine behind `convex codegen`
- [Convex adapter docs](../../docs/adapters/convex/README.md)
