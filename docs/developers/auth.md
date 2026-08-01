---
title: Authenticate users
description: Wire an identity provider into Nimbus so functions can verify who is calling them.
sidebar:
  order: 3
---

Nimbus verifies JWTs from Auth0, Clerk, Firebase Auth, Keycloak, and other
identity providers. The provider must support OIDC or publish a JSON Web Key
Set (JWKS). Nimbus exposes the verified identity through `ctx.auth`. This guide
wires a provider into a Convex-style project.

## 1. Declare the provider

Create exactly one config file: `convex/auth.config.ts` or
`convex/auth.config.js`. Add a default export that lists your providers.

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
  issues. Nimbus rejects tokens with multiple audiences.

For a provider that is not a full OIDC issuer but publishes a JWKS:

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

Codegen evaluates the config statically. It resolves `process.env.*` reads
against the codegen environment. You can therefore keep issuer URLs out of
source:

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

`nimbus dev` loads the config on the next codegen pass. A running dev loop
starts that pass when you save the file.

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
under the provider. Outside React, set the token directly on the client. Use
either a string or an async fetcher that runs again when the token needs a
refresh. Clear the token on sign-out:

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

`tokenIdentifier` has the form `issuer|subject` and is stable for each user
and provider. Use it as the foreign key for user records. Your code owns
authorization. Check the identity and decide what the caller may do. Nimbus
has no separate rules language.

## What Nimbus verifies

For each request token, Nimbus checks the signature against the provider's
published keys and the issuer against your config. When you set
`applicationID`, Nimbus also checks the audience. A request without a token
runs your function, and `getUserIdentity()` returns `null`. Nimbus rejects an
invalid or expired token before your function runs.

## Next steps

- [Compatibility reference](/reference/convex/compatibility/): the full
  authentication surface.
- [Build with the Convex API](/developers/convex/): functions, schema, and
  the dev loop.
