import { internalScheduledFunctions } from "./_generated/scheduled_functions.js";
import { internalMutation, mutation, query } from "./_generated/server.js";
import { v } from "@nimbus/nimbus/values";

// Raw reads from ctx.db are returned as-is; formatting and field access on
// query results happen client-side (run.ts, smoke.ts), where the generated
// Doc<TableName> types apply. See docs/private/plans/
// examples-and-target-resolution-plan.md EX3.2 for why: handler-body access
// to a field on a ctx.db read result is a recorded product gap, not something
// this app works around by fabricating a type. That is also why runWorker
// (below) takes the job ids to schedule as an explicit argument instead of
// re-querying "pending" jobs and reading their _id server-side.

export const list = query({
  args: {},
  handler: async (ctx) => await ctx.db.query("jobs").collect(),
});

export const enqueue = mutation({
  args: { label: v.string() },
  returns: v.id("jobs"),
  handler: async (ctx, { label }) =>
    await ctx.db.insert("jobs", { label, status: "pending", createdAt: Date.now() }),
});

// runWorker is the headless kickoff, and the only place this app calls
// ctx.scheduler.runAfter: given a batch of job ids, it schedules one
// processJob call per job, staggered by intervalMs — a fixed-cadence batch,
// not an open-ended self-chaining loop. That is a real constraint, not a
// stylistic choice: a scheduled target must compile to a single,
// statically-analyzable ctx.db operation whose result is returned directly
// (crates/nimbus-convex/src/registry/resolution/functions/writes.rs::
// resolve_scheduled_mutation_for_visibility deserializes the stored plan into
// nimbus_core::Mutation, an Insert/Update/Delete-only enum), and
// ctx.scheduler.runAfter itself is not representable in that plan — so no
// function that reschedules itself (or anything else) can ever be a valid
// scheduled target. This was discovered and fixed at the app level for
// examples/nimbus/agent-chat's deliverReminder (EX4.1); scheduling a whole
// batch up front, in the one unrestricted call that kicks the batch off, is
// the honest way to get autonomous, unattended, multi-step scheduler-driven
// work out of that same boundary — especially since EX0.3 confirmed there is
// no public cron API to fall back on either.
export const runWorker = mutation({
  args: { jobIds: v.array(v.id("jobs")), intervalMs: v.number() },
  returns: v.object({ scheduled: v.number() }),
  handler: async (ctx, { jobIds, intervalMs }) => {
    await Promise.all(
      jobIds.map((jobId, i) => {
        const delayMs = i * intervalMs;
        return ctx.scheduler.runAfter(delayMs, internalScheduledFunctions.worker.processJob, {
          jobId,
          completedAt: Date.now() + delayMs,
        });
      }),
    );
    return { scheduled: jobIds.length };
  },
});

// processJob is the scheduled target: exactly one ctx.db.patch, returning its
// result directly, with no Date.now() of its own — completedAt is
// precomputed by runWorker (Date.now() + delayMs, the same delivery-time
// approximation deliverReminder uses in EX4.1) because the plan compiler
// executes this handler body once, at codegen time; a live Date.now() call in
// here would freeze a single stale timestamp into the static plan forever.
export const processJob = internalMutation({
  args: { jobId: v.id("jobs"), completedAt: v.number() },
  returns: v.id("jobs"),
  handler: async (ctx, { jobId, completedAt }) =>
    await ctx.db.patch(jobId, { status: "done", completedAt }),
});
