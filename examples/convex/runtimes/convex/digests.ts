import { v } from "convex/values";

import { internal } from "./_generated/api.js";
import { action, internalMutation, query } from "./_generated/server.js";

export const list = query({
  args: {},
  handler: async (ctx, _args) =>
    await ctx.db.query("digests").withIndex("by_created_at").order("desc").collect(),
});

export const store = internalMutation({
  args: {
    runtime: v.union(v.literal("default"), v.literal("node")),
    algorithm: v.string(),
    input: v.string(),
    output: v.string(),
  },
  returns: v.id("digests"),
  handler: async (ctx, { runtime, algorithm, input, output }) =>
    await ctx.db.insert("digests", {
      runtime,
      algorithm,
      input,
      output,
      createdAt: Date.now(),
    }),
});

// Default runtime: no "use node" directive at the top of this module, so it
// runs on Nimbus's V8-based default runtime — the web-standard global
// surface (fetch, TextEncoder, crypto.subtle, ...), not Node's built-in
// modules. See nodeDigests.ts for the "use node" counterpart.
export const hashWithDefaultRuntime = action({
  args: { text: v.string() },
  returns: v.id("digests"),
  handler: async (ctx, { text }) => {
    const encoded = new TextEncoder().encode(text);
    const digestBytes = await crypto.subtle.digest("SHA-256", encoded);
    const output = Array.from(new Uint8Array(digestBytes))
      .map((byte) => byte.toString(16).padStart(2, "0"))
      .join("");
    return await ctx.runMutation(internal.digests.store, {
      runtime: "default",
      algorithm: "SHA-256 (Web Crypto SubtleCrypto)",
      input: text,
      output,
    });
  },
});
