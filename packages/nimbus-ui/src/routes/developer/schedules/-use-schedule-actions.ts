import { useCallback, useState } from "react";
import { toast } from "sonner";

import {
  type ScheduleMutation,
  schedules as scheduleApi,
} from "../../../lib/api-mutations";
import { useUiStore } from "../../../store/ui-store";
import { jobIdFromDocumentId } from "./-job-ids";
import type { CronJobDoc, ScheduledJobDoc } from "./-types";

export type ScheduleActionKind = "run" | "cancel" | "delete";

export type ScheduleActions = {
  // The in-flight action per row id (job document id or cron name).
  pending: Record<string, ScheduleActionKind>;
  runJob: (job: ScheduledJobDoc) => Promise<void>;
  runCron: (cron: CronJobDoc) => Promise<void>;
  cancelJob: (job: ScheduledJobDoc) => Promise<boolean>;
  deleteCron: (cron: CronJobDoc) => Promise<boolean>;
};

function isMutation(value: unknown): value is ScheduleMutation {
  if (typeof value !== "object" || value === null) return false;
  const type = (value as { type?: unknown }).type;
  return type === "insert" || type === "update" || type === "delete";
}

// Owns the scheduler side effects for the Schedules page: run a job's
// mutation now, cancel a pending job, and remove a cron. The lists are
// live queries, so a settled request needs no reload; the toast is the
// operator's receipt.
export function useScheduleActions(): ScheduleActions {
  const activeTenant = useUiStore((s) => s.activeTenant);
  const [pending, setPending] = useState<Record<string, ScheduleActionKind>>(
    {},
  );

  const track = useCallback(
    async <T>(
      key: string,
      kind: ScheduleActionKind,
      work: () => Promise<T>,
    ): Promise<T> => {
      setPending((prev) => ({ ...prev, [key]: kind }));
      try {
        return await work();
      } finally {
        setPending((prev) => {
          const next = { ...prev };
          delete next[key];
          return next;
        });
      }
    },
    [],
  );

  const runJob = useCallback(
    async (job: ScheduledJobDoc) => {
      const tenant = job.tenantId ?? activeTenant;
      const label = job.functionPath ?? job._id;
      if (!tenant) {
        toast.error(`Cannot run ${label}: no tenant is selected`);
        return;
      }
      if (!isMutation(job.args)) {
        toast.error(`Cannot run ${label}: the job carries no mutation`);
        return;
      }
      const mutation = job.args;
      await track(job._id, "run", async () => {
        const result = await scheduleApi.runNow(tenant, mutation);
        if (result.ok) toast.success(`Queued ${label} to run now`);
        else
          toast.error(`Run now refused for ${label}`, {
            description: result.error,
          });
      });
    },
    [activeTenant, track],
  );

  // The cron row does not hold its mutation; the crons route does.
  const runCron = useCallback(
    async (cron: CronJobDoc) => {
      const tenant = cron.tenantId ?? activeTenant;
      const label = cron.name ?? cron._id;
      if (!tenant || !cron.name) {
        toast.error(`Cannot run ${label}: no tenant is selected`);
        return;
      }
      const name = cron.name;
      await track(name, "run", async () => {
        const listed = await scheduleApi.listCrons(tenant);
        if (!listed.ok) {
          toast.error(`Run now refused for ${label}`, {
            description: listed.error,
          });
          return;
        }
        const entry = listed.data.crons?.find((c) => c.name === name);
        if (!entry) {
          toast.error(`Cannot run ${label}: the scheduler no longer lists it`);
          return;
        }
        const result = await scheduleApi.runNow(tenant, entry.mutation);
        if (result.ok) toast.success(`Queued ${label} to run now`);
        else
          toast.error(`Run now refused for ${label}`, {
            description: result.error,
          });
      });
    },
    [activeTenant, track],
  );

  const cancelJob = useCallback(
    async (job: ScheduledJobDoc) => {
      const tenant = job.tenantId ?? activeTenant;
      const label = job.functionPath ?? job._id;
      const jobId = jobIdFromDocumentId(job._id);
      if (!tenant || !jobId) {
        toast.error(`Cannot cancel ${label}: the job id is not addressable`);
        return false;
      }
      return track(job._id, "cancel", async () => {
        const result = await scheduleApi.cancel(tenant, jobId);
        if (result.ok) toast.success(`Cancelled ${label}`);
        else
          toast.error(`Cancel refused for ${label}`, {
            description: result.error,
          });
        return result.ok;
      });
    },
    [activeTenant, track],
  );

  const deleteCron = useCallback(
    async (cron: CronJobDoc) => {
      const tenant = cron.tenantId ?? activeTenant;
      const label = cron.name ?? cron._id;
      if (!tenant || !cron.name) {
        toast.error(`Cannot delete ${label}: no tenant is selected`);
        return false;
      }
      const name = cron.name;
      return track(name, "delete", async () => {
        const result = await scheduleApi.removeCron(tenant, name);
        if (result.ok) toast.success(`Deleted cron ${label}`);
        else
          toast.error(`Delete refused for ${label}`, {
            description: result.error,
          });
        return result.ok;
      });
    },
    [activeTenant, track],
  );

  return { pending, runJob, runCron, cancelJob, deleteCron };
}
