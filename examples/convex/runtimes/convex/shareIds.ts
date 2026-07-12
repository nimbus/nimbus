import { v } from "convex/values";
import { nanoid } from "nanoid";

import { mutation, query } from "./_generated/server.js";

// Default runtime: no "use node" directive at the top of this module, so
// this runs on Nimbus's default V8 isolate, same as digests.ts. `nanoid` is
// a real, browser-compatible npm package (no Node builtins, works in any JS
// runtime) -- proving a third-party package can be imported directly into a
// default-runtime Convex function, not just a "use node" one. It must be
// externalized in convex.json (see node.externalPackages) the same way a
// "use node" module's external packages are; Nimbus never implicitly
// bundles npm packages into either lane's runtime artifact.
export const create = mutation({
  args: {},
  returns: v.id("shareIds"),
  handler: async (ctx) =>
    await ctx.db.insert("shareIds", {
      id: nanoid(),
      createdAt: Date.now(),
    }),
});

export const list = query({
  args: {},
  handler: async (ctx) =>
    await ctx.db.query("shareIds").withIndex("by_created_at").order("desc").collect(),
});
