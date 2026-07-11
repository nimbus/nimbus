import { internalScheduledFunctions } from "./_generated/scheduled_functions";
import { internalMutation, mutation, query } from "./_generated/server";
import { v } from "@nimbus/nimbus/values";

// Raw reads from ctx.db are returned as-is; formatting and field access on
// query results happen client-side (in src/App.tsx and smoke.ts), where the
// generated Doc<TableName> types apply. See docs/private/plans/
// examples-and-target-resolution-plan.md EX3.2 for why: handler-body access
// to a field on a ctx.db read result is a recorded product gap, not something
// this app works around by fabricating a type.

export const list = query({
  args: { conversationId: v.string() },
  handler: async (ctx, { conversationId }) =>
    await ctx.db
      .query("messages")
      .withIndex("by_conversation", (q) => q.eq("conversationId", conversationId))
      .collect(),
});

export const listMemory = query({
  args: { conversationId: v.string() },
  handler: async (ctx, { conversationId }) =>
    await ctx.db
      .query("agentMemory")
      .withIndex("by_conversation", (q) => q.eq("conversationId", conversationId))
      .collect(),
});

export const send = mutation({
  args: {
    conversationId: v.string(),
    text: v.string(),
  },
  returns: v.object({
    tool: v.union(v.string(), v.null()),
  }),
  handler: async (ctx, { conversationId, text }) => {
    const trimmed = text.trim();
    if (!trimmed) {
      throw new Error("Message text must not be empty");
    }

    await ctx.db.insert("messages", {
      conversationId,
      role: "user",
      text: trimmed,
      createdAt: Date.now(),
    });

    const lower = trimmed.toLowerCase();
    const remindMatch = trimmed.match(/^remind me in (\d+)ms:\s*(.+)$/i);

    const outcome = await (async () => {
      if (lower.startsWith("remember:")) {
        const fact = trimmed.slice("remember:".length).trim();
        if (!fact) {
          return {
            tool: "remember",
            replyText: 'Tell me what to remember, e.g. "remember: my favorite color is teal".',
          };
        }
        await ctx.db.insert("agentMemory", {
          conversationId,
          text: fact,
          createdAt: Date.now(),
        });
        return { tool: "remember", replyText: `Got it — I'll remember: "${fact}".` };
      }

      if (lower.includes("what do you remember")) {
        const facts = await ctx.db
          .query("agentMemory")
          .withIndex("by_conversation", (q) => q.eq("conversationId", conversationId))
          .collect();
        return {
          tool: "recall",
          replyText:
            facts.length === 0
              ? "I don't have anything remembered for this conversation yet."
              : `I have ${facts.length} thing${facts.length === 1 ? "" : "s"} remembered for this conversation — see the memory panel.`,
        };
      }

      if (remindMatch) {
        const delayMs = Number(remindMatch[1]);
        const reminderText = remindMatch[2].trim();
        await ctx.scheduler.runAfter(
          delayMs,
          internalScheduledFunctions.agent.deliverReminder,
          {
            conversationId,
            text: `Reminder: ${reminderText}`,
            createdAt: Date.now() + delayMs,
          },
        );
        return {
          tool: "remind",
          replyText: `Okay — I'll remind you in ${delayMs}ms: "${reminderText}".`,
        };
      }

      return { tool: null, replyText: `Got your message: "${trimmed}".` };
    })();

    await ctx.db.insert("messages", {
      conversationId,
      role: "assistant",
      text: outcome.replyText,
      tool: outcome.tool ?? undefined,
      createdAt: Date.now(),
    });

    return { tool: outcome.tool };
  },
});

// deliverReminder is scheduled via ctx.scheduler.runAfter, which requires a
// statically-analyzable single-operation plan (see docs/private/plans/
// examples-and-target-resolution-plan.md EX4.1): exactly one ctx.db call,
// whose result is returned directly. The reminder text and the delivery-time
// createdAt are pre-computed by the caller (send, an unrestricted runtime
// handler) and passed in as plain args — this handler does no formatting or
// Date.now() of its own, since either would either fail plan compilation or
// (for Date.now()) get frozen into the plan at codegen time.
export const deliverReminder = internalMutation({
  args: {
    conversationId: v.string(),
    text: v.string(),
    createdAt: v.number(),
  },
  handler: async (ctx, { conversationId, text, createdAt }) =>
    await ctx.db.insert("messages", {
      conversationId,
      role: "assistant",
      text,
      tool: "remind-delivery",
      createdAt,
    }),
});
