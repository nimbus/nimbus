import { NimbusClient, NimbusHttpClient } from "@nimbus/nimbus/browser";

import { api } from "./nimbus/_generated/api.ts";
import type { Doc } from "./nimbus/_generated/dataModel.d.ts";

type Job = Doc<"jobs">;

const nativeUrl = process.env.NIMBUS_NATIVE_URL ?? "http://localhost:8080";
const adminToken = process.env.NIMBUS_ADMIN_TOKEN;
const tenantId = process.env.NIMBUS_TENANT_ID ?? "demo";
const nimbusUrl = process.env.NIMBUS_URL ?? `${nativeUrl}/convex/${tenantId}`;
const intervalMs = Number(process.env.NIMBUS_WORKER_INTERVAL_MS ?? "750");
const jobLabels = ["fetch report", "compress archive", "send digest"];

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
    throw new Error(`failed to ensure demo tenant: ${response.status}`);
  }
}

async function main() {
  await ensureTenant();

  const http = new NimbusHttpClient(nimbusUrl);
  const live = new NimbusClient(nimbusUrl, { webSocket: globalThis.WebSocket });

  const shutdown = () => {
    live.close();
    process.exit(0);
  };
  process.once("SIGINT", shutdown);
  process.once("SIGTERM", shutdown);

  const jobIds: string[] = [];
  for (const label of jobLabels) {
    const id = await http.mutation(api.worker.enqueue, { label });
    console.log(`Enqueued: ${label} (${id})`);
    jobIds.push(id);
  }

  console.log(
    `Kicking off the headless worker: ${jobIds.length} jobs, ${intervalMs}ms apart. ` +
      "No further client action is needed — completion is entirely scheduler-driven.",
  );
  await http.mutation(api.worker.runWorker, { jobIds, intervalMs });

  // Track completion only for the jobs this run enqueued — the jobs table is
  // shared demo state, so a prior run's rows may still be sitting around.
  await new Promise<void>((resolve, reject) => {
    const unsubscribe = live.onUpdate(
      api.worker.list,
      {},
      (jobs) => {
        const ours = jobIds
          .map((id) => jobs.find((job: Job) => job._id === id))
          .filter((job): job is Job => job !== undefined);
        const doneCount = ours.filter((job) => job.status === "done").length;
        console.log(`Live: ${doneCount}/${jobIds.length} jobs done.`);
        if (ours.length === jobIds.length && doneCount === jobIds.length) {
          unsubscribe();
          resolve();
        }
      },
      (error) => {
        unsubscribe();
        reject(error);
      },
    );
  });

  console.log("All jobs completed with no client polling.");
  live.close();
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
