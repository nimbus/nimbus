"use node";

import crypto from "node:crypto";

import { v } from "convex/values";

import { internal } from "./_generated/api.js";
import { action } from "./_generated/server.js";

// Node runtime: the "use node" directive above must be the first line of
// the module, and a "use node" module may contain only actions. It runs on
// Nimbus's Node-compatible runtime with full access to Node builtins —
// here, node:crypto's synchronous digest API, which the default runtime in
// digests.ts does not expose.
export const hashWithNodeRuntime = action({
  args: { text: v.string() },
  returns: v.id("digests"),
  handler: async (ctx, { text }) => {
    const output = crypto.createHash("sha256").update(text, "utf8").digest("hex");
    return await ctx.runMutation(internal.digests.store, {
      runtime: "node",
      algorithm: "SHA-256 (node:crypto createHash)",
      input: text,
      output,
    });
  },
});
