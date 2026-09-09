import type { ScheduleMutation } from "../../../lib/api-mutations";

// A row of `scheduled_jobs` as the system tenant writes it
// (crates/nimbus-system/src/records/scheduler.rs): the job id lives in the
// document id, the mutation in `args`, and the outcome in `result`.
export type ScheduledJobDoc = {
  _id: string;
  tenantId?: string;
  functionPath?: string;
  status?: string;
  scheduledTime?: number;
  args?: ScheduleMutation | Record<string, unknown>;
  result?: {
    finishedAt?: number;
    outcome?: string;
    error?: string;
  };
};

// A row of `cron_jobs`: the name is the key, the schedule a compact
// `interval:{n}s` string, and the mutation lives with the scheduler, not
// in the row.
export type CronJobDoc = {
  _id: string;
  tenantId?: string;
  name?: string;
  schedule?: string;
  functionPath?: string;
  nextRunAt?: number;
  lastRunAt?: number;
  status?: string;
};

export type ScheduleSection = "scheduled" | "cron";

export type SchedulesSearch = {
  section?: ScheduleSection;
  job?: string;
  cron?: string;
};

export function isSection(value: unknown): value is ScheduleSection {
  return value === "scheduled" || value === "cron";
}

export function parseSchedulesSearch(
  search: Record<string, unknown>,
): SchedulesSearch {
  return {
    section: isSection(search.section) ? search.section : undefined,
    job: typeof search.job === "string" && search.job ? search.job : undefined,
    cron:
      typeof search.cron === "string" && search.cron ? search.cron : undefined,
  };
}

// `interval:30s` is what the scheduler writes; the console says it the way
// an operator would.
export function formatSchedule(schedule: string | undefined): string {
  if (!schedule) return "—";
  const match = /^interval:(\d+)s$/.exec(schedule);
  if (!match) return schedule;
  const seconds = Number(match[1]);
  if (seconds % 3600 === 0) return `every ${seconds / 3600}h`;
  if (seconds % 60 === 0) return `every ${seconds / 60}m`;
  return `every ${seconds}s`;
}
