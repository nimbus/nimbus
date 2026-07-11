import { NimbusClient, NimbusHttpClient } from "@nimbus/nimbus/browser";

import { api } from "./nimbus/_generated/api.ts";
import type { Doc } from "./nimbus/_generated/dataModel.d.ts";

type Job = Doc<"jobs">;

const nativeUrl = process.env.NIMBUS_NATIVE_URL ?? "http://localhost:8080";
const adminToken = process.env.NIMBUS_ADMIN_TOKEN;
const tenantId = process.env.NIMBUS_TENANT_ID ?? "demo";
const nimbusUrl = process.env.NIMBUS_URL ?? `${nativeUrl}/convex/${tenantId}`;
let activeAnchor = "setup";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function pass(anchor: string) {
  console.log(`PASS ${anchor}`);
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

async function withTimeout<T>(promise: Promise<T>, message: string): Promise<T> {
  let timeoutId: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timeoutId = setTimeout(() => reject(new Error(message)), 5_000);
  });
  return await Promise.race([promise, timeout]).finally(() => clearTimeout(timeoutId));
}

async function ensureTenant() {
  const response = await fetch(`${nativeUrl}/api/tenants`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      ...(adminToken ? { Authorization: `Bearer ${adminToken}` } : {}),
    },
    body: JSON.stringify({ id: tenantId }),
  });
  if (!response.ok && response.status !== 409) {
    throw new Error(`failed to ensure smoke tenant: ${response.status}`);
  }
}

function findJob(jobs: Job[], id: string): Job | undefined {
  return jobs.find((job) => job._id === id);
}

async function main() {
  await ensureTenant();

  const http = new NimbusHttpClient(nimbusUrl);
  const runPrefix = `smoke-${Date.now()}`;

  // agent-worker.enqueue
  activeAnchor = "agent-worker.enqueue";
  const labels = [`${runPrefix}-a`, `${runPrefix}-b`, `${runPrefix}-c`];
  const jobIds: string[] = [];
  for (const label of labels) {
    const id = await http.mutation(api.worker.enqueue, { label });
    assert(typeof id === "string" && id.length > 0, "enqueue must return a stable job id");
    jobIds.push(id);
  }
  const afterEnqueue = await http.query(api.worker.list, {});
  for (let i = 0; i < jobIds.length; i++) {
    const job = findJob(afterEnqueue, jobIds[i]);
    assert(job !== undefined, "agent-worker.enqueue must persist every enqueued job");
    assert(job.label === labels[i], "agent-worker.enqueue must preserve the job label");
    assert(job.status === "pending", "agent-worker.enqueue must leave new jobs pending");
    assert(Number.isFinite(job.createdAt), "agent-worker.enqueue job must have a finite createdAt");
    assert(job.completedAt === undefined, "agent-worker.enqueue job must not yet be completed");
  }
  pass("agent-worker.enqueue");

  // agent-worker.schedule
  activeAnchor = "agent-worker.schedule";
  const live = new NimbusClient(nimbusUrl, { webSocket: globalThis.WebSocket });
  const initial = deferred<Job[]>();
  const allDone = deferred<Job[]>();
  let watchForCompletion = false;
  const unsubscribe = live.onUpdate(
    api.worker.list,
    {},
    (jobs) => {
      initial.resolve(jobs);
      if (watchForCompletion && jobIds.every((id) => findJob(jobs, id)?.status === "done")) {
        allDone.resolve(jobs);
      }
    },
    (error) => {
      initial.reject(error);
      allDone.reject(error);
    },
  );

  try {
    await withTimeout(
      initial.promise,
      "agent-worker.schedule timed out waiting for the initial subscription result",
    );
    watchForCompletion = true;
    const scheduleResult = await http.mutation(api.worker.runWorker, {
      jobIds,
      intervalMs: 150,
    });
    assert(
      scheduleResult.scheduled === jobIds.length,
      `agent-worker.schedule must schedule exactly one hop per job, got ${scheduleResult.scheduled}`,
    );
    const doneJobs = await withTimeout(
      allDone.promise,
      "agent-worker.schedule timed out waiting for every job to complete with no client action",
    );
    for (const id of jobIds) {
      const job = findJob(doneJobs, id);
      assert(job !== undefined, "agent-worker.schedule completed job must still be present");
      assert(job.status === "done", "agent-worker.schedule must mark every job done");
      assert(Number.isFinite(job.completedAt), "agent-worker.schedule must record a finite completedAt");
    }
    pass("agent-worker.schedule");
  } finally {
    unsubscribe();
    live.close();
  }
}

main().catch((error) => {
  console.error(`FAIL ${activeAnchor}: ${(error as Error).message}`);
  process.exitCode = 1;
});
