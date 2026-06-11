---
title: Authenticate users
description: Wire an identity provider into Nimbus so functions can verify who is calling them.
sidebar:
  order: 3
---

Nimbus verifies JWTs issued by your identity provider — Auth0, Clerk,
Firebase Auth, Keycloak, or anything that speaks OIDC or publishes a JWKS —
and exposes the verified identity to your functions through `ctx.auth`. This
guide wires a provider into a Convex-style project.

## 1. Declare the provider

Create `convex/auth.config.ts` (or `.js` — exactly one of the two) with a
default export listing your providers.

For an OIDC provider:

```typescript
// convex/auth.config.ts
export default {
  providers: [
    {
      domain: "https://your-tenant.us.auth0.com",
      applicationID: "your-client-id",
    },
  ],
};
```

- `domain` is the token issuer. Nimbus discovers the signing keys from
  `{domain}/.well-known/openid-configuration`.
- `applicationID` must equal the `aud` claim of the tokens your provider
  issues. Tokens with multiple audiences are rejected.

For a provider that isn't a full OIDC issuer but publishes a JWKS:

```typescript
// convex/auth.config.ts
export default {
  providers: [
    {
      type: "customJwt",
      issuer: "https://auth.example.com",
      jwks: "https://auth.example.com/.well-known/jwks.json",
      algorithm: "RS256", // or "ES256"
      applicationID: "your-client-id", // optional
    },
  ],
};
```

The config is evaluated statically at codegen time. `process.env.*` reads
are supported and resolve against the environment of the codegen run, so
you can keep issuer URLs out of source:

```typescript
export default {
  providers: [
    {
      domain: process.env.AUTH_ISSUER!,
      applicationID: process.env.AUTH_CLIENT_ID!,
    },
  ],
};
```

`nimbus dev` picks the config up on the next codegen pass; with a running
dev loop that happens as soon as you save the file.

## 2. Send tokens from the client

In React, wrap your app in `ConvexProviderWithAuth` and supply a `useAuth`
hook that bridges your auth library:

```tsx
import { ConvexProviderWithAuth, ConvexReactClient } from "convex/react";

const client = new ConvexReactClient("http://localhost:3210/convex/demo");

function useAuthFromMyProvider() {
  // Adapt your auth library to this shape.
  return {
    isLoading: false,
    isAuthenticated: true,
    fetchAccessToken: async ({ forceRefreshToken }) =>
      await myAuthLibrary.getToken({ ignoreCache: forceRefreshToken }),
  };
}

export function App({ children }) {
  return (
    <ConvexProviderWithAuth client={client} useAuth={useAuthFromMyProvider}>
      {children}
    </ConvexProviderWithAuth>
  );
}
```

`useConvexAuth()` reports `{ isLoading, isAuthenticated }` from anywhere
under the provider. Outside React, set the token directly on the client —
either a string or an async fetcher that is re-invoked when the token needs
refreshing — and clear it on sign-out:

```typescript
client.setAuth(async () => await getFreshToken());
// ...later
client.clearAuth();
```

## 3. Read the identity in functions

Inside any query, mutation, or action, `ctx.auth.getUserIdentity()` returns
the verified identity, or `null` when the request carries no valid token:

```typescript
import { mutation } from "./_generated/server";
import { v } from "convex/values";

export const send = mutation({
  args: { body: v.string() },
  handler: async (ctx, { body }) => {
    const identity = await ctx.auth.getUserIdentity();
    if (identity === null) {
      throw new Error("Not signed in");
    }
    await ctx.db.insert("messages", {
      author: identity.tokenIdentifier,
      body,
    });
  },
});
```

`tokenIdentifier` has the form `issuer|subject` and is stable per user per
provider — use it as the foreign key for user records. Authorization is
your code: check the identity and decide what the caller may do; there is
no separate rules language.

## What Nimbus verifies

For each request token, Nimbus checks the signature against the provider's
published keys, the issuer against your config, and — when `applicationID`
is set — the audience. A request with no token runs your function with
`getUserIdentity()` returning `null`; a request carrying an invalid or
expired token is rejected with an authentication error before your function
runs.

## Next steps

- [Compatibility reference](/reference/convex/compatibility/) — the full
  authentication surface.
- [Build with the Convex API](/developers/convex/) — functions, schema, and
  the dev loop.
