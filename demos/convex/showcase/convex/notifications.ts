import { v } from "convex/values";

import { api, internal } from "./_generated/api";
import { mutation } from "./_generated/server";

// Cross-references other functions via `api.*` / `internal.*`, so the console
// code-navigation (FSV7) shows CALLS edges: announce -> users:touch,
// messages:send, messages:list.
export const announce = mutation({
  args: {
    channel: v.string(),
    email: v.string(),
    name: v.string(),
    body: v.string(),
  },
  handler: async (ctx, { channel, email, name, body }) => {
    await ctx.runMutation(internal.users.touch, { email, name });
    await ctx.runMutation(api.messages.send, { channel, author: name, body });
    const recent = await ctx.runQuery(api.messages.list, { channel });
    return recent.length;
  },
});
