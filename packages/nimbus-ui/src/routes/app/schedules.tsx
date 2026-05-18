import { createFileRoute, useSearch } from "@tanstack/react-router";
import { useQuery } from "nimbus/react";

import { api } from "../../../convex/_generated/api";
import { StateChip } from "../../components/state-chip";
import { RelativeTime } from "../../components/time";
import { cn } from "../../lib/cn";
import { formatDuration, shortId } from "../../lib/format";
import {
  type SubDrawerSpec,
  useContributeSubDrawer,
} from "../../shell/sub-drawer";
import { useUiStore } from "../../store/ui-store";

type Section = "scheduled" | "cron";

type SchedulesSearch = {
  section?: Section;
};

type ScheduledJobDoc = {
  _id: string;
  tenantId?: string;
  functionPath?: string;
  status?: string;
  scheduledTime?: number;
  startedAt?: number;
  completedAt?: number;
};

type CronJobDoc = {
  _id: string;
  tenantId?: string;
  name?: string;
  cron?: string;
  schedule?: string;
  functionPath?: string;
  nextRunAt?: number;
  lastRunAt?: number;
  status?: string;
};

export const Route = createFileRoute("/app/schedules")({
  validateSearch: (search: Record<string, unknown>): SchedulesSearch => ({
    section: isSection(search.section) ? search.section : undefined,
  }),
  component: SchedulesPage,
});

function isSection(value: unknown): value is Section {
  return value === "scheduled" || value === "cron";
}

export const SCHEDULES_SUB_DRAWER: SubDrawerSpec = {
  kind: "static",
  title: "Schedules",
  items: [
    {
      id: "scheduled",
      label: "Scheduled",
      to: "/app/schedules",
      search: { section: "scheduled" },
    },
    {
      id: "cron",
      label: "Cron",
      to: "/app/schedules",
      search: { section: "cron" },
    },
  ],
};

function SchedulesPage() {
  useContributeSubDrawer(SCHEDULES_SUB_DRAWER);
  const search = useSearch({ from: "/app/schedules" });
  const section: Section = search.section ?? "scheduled";
  const activeTenant = useUiStore((s) => s.activeTenant);

  const scheduled = useQuery(api.scheduled_jobs.list, {
    tenantId: activeTenant,
    status: null,
    limit: 200,
  }) as ScheduledJobDoc[] | undefined;

  const cron = useQuery(api.cron_jobs.list, {
    tenantId: activeTenant,
    status: null,
    limit: 200,
  }) as CronJobDoc[] | undefined;

  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-schedules"
    >
      <header className="flex items-baseline justify-between">
        <div>
          <h1
            className="text-xl text-default"
            style={{ fontSize: "var(--text-xl)" }}
          >
            Schedules
          </h1>
          <p className="text-sm text-muted">
            Scheduler-driven and cron-driven invocations for this tenant.
            Switch between one-shot scheduled jobs and recurring cron entries.
          </p>
        </div>
      </header>

      <div className="min-h-0 flex-1 overflow-hidden rounded-md border border-app bg-surface">
        {section === "scheduled" ? (
          <ScheduledTable jobs={scheduled} />
        ) : (
          <CronTable jobs={cron} />
        )}
      </div>
    </section>
  );
}

function ScheduledTable({ jobs }: { jobs: ScheduledJobDoc[] | undefined }) {
  if (jobs === undefined) return <Loading label="Loading scheduled jobs…" />;
  if (jobs.length === 0) {
    return (
      <Empty
        title="No scheduled jobs"
        detail="Scheduler-driven invocations appear here. The list updates as jobs are enqueued."
      />
    );
  }
  return (
    <div className="overflow-auto">
      <table
        className="w-full border-collapse text-sm"
        data-testid="schedules-scheduled-table"
      >
        <thead className="sticky top-0 bg-surface-2 text-[10px] uppercase tracking-[0.14em] text-muted">
          <tr>
            <Th>Function</Th>
            <Th>Status</Th>
            <Th>Tenant</Th>
            <Th>Scheduled</Th>
            <Th>Duration</Th>
          </tr>
        </thead>
        <tbody>
          {jobs.map((job) => {
            const duration =
              typeof job.completedAt === "number" &&
              typeof job.startedAt === "number"
                ? job.completedAt - job.startedAt
                : null;
            return (
              <tr
                key={job._id}
                className="border-t border-app hover:bg-surface-2"
                data-testid={`schedules-scheduled-${job._id}`}
              >
                <Td>
                  <span className="font-mono text-default">
                    {job.functionPath ?? shortId(job._id, 12)}
                  </span>
                </Td>
                <Td>
                  <StateChip state={job.status} />
                </Td>
                <Td>
                  <span className="font-mono text-xs text-default">
                    {job.tenantId ?? "—"}
                  </span>
                </Td>
                <Td>
                  {typeof job.scheduledTime === "number" ? (
                    <RelativeTime epochMs={job.scheduledTime} />
                  ) : (
                    <span className="tabular text-muted">—</span>
                  )}
                </Td>
                <Td>
                  {duration !== null ? (
                    <span className="tabular font-mono text-xs text-default">
                      {formatDuration(duration)}
                    </span>
                  ) : (
                    <span className="tabular text-muted">—</span>
                  )}
                </Td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function CronTable({ jobs }: { jobs: CronJobDoc[] | undefined }) {
  if (jobs === undefined) return <Loading label="Loading cron jobs…" />;
  if (jobs.length === 0) {
    return (
      <Empty
        title="No cron jobs"
        detail="Cron-scheduled functions appear here with their schedule and next-run time."
      />
    );
  }
  return (
    <div className="overflow-auto">
      <table
        className="w-full border-collapse text-sm"
        data-testid="schedules-cron-table"
      >
        <thead className="sticky top-0 bg-surface-2 text-[10px] uppercase tracking-[0.14em] text-muted">
          <tr>
            <Th>Name</Th>
            <Th>Function</Th>
            <Th>Schedule</Th>
            <Th>Status</Th>
            <Th>Next run</Th>
            <Th>Last run</Th>
          </tr>
        </thead>
        <tbody>
          {jobs.map((job) => (
            <tr
              key={job._id}
              className="border-t border-app hover:bg-surface-2"
              data-testid={`schedules-cron-${job.name ?? job._id}`}
            >
              <Td>
                <span className="font-mono text-default">
                  {job.name ?? shortId(job._id, 12)}
                </span>
              </Td>
              <Td>
                <span className="font-mono text-xs text-default">
                  {job.functionPath ?? "—"}
                </span>
              </Td>
              <Td>
                <span className="font-mono text-xs text-default">
                  {job.cron ?? job.schedule ?? "—"}
                </span>
              </Td>
              <Td>
                <StateChip state={job.status} />
              </Td>
              <Td>
                {typeof job.nextRunAt === "number" ? (
                  <RelativeTime epochMs={job.nextRunAt} />
                ) : (
                  <span className="tabular text-muted">—</span>
                )}
              </Td>
              <Td>
                {typeof job.lastRunAt === "number" ? (
                  <RelativeTime epochMs={job.lastRunAt} />
                ) : (
                  <span className="tabular text-muted">never</span>
                )}
              </Td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function Th({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <th
      className={cn(
        "border-b border-app px-3 py-2 text-left font-normal",
        className,
      )}
    >
      {children}
    </th>
  );
}

function Td({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <td className={cn("px-3 py-2 align-middle", className)}>{children}</td>
  );
}

function Loading({ label }: { label: string }) {
  return (
    <div className="flex h-32 items-center justify-center text-xs text-muted">
      {label}
    </div>
  );
}

function Empty({ title, detail }: { title: string; detail: string }) {
  return (
    <div className="flex h-32 flex-col items-center justify-center gap-1 text-center">
      <span className="font-mono text-sm text-default">{title}</span>
      <span className="max-w-md text-xs text-muted">{detail}</span>
    </div>
  );
}
