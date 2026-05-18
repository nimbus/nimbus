import {
  createFileRoute,
  Link,
  useNavigate,
  useSearch,
} from "@tanstack/react-router";
import { useQuery } from "nimbus/react";
import { useMemo } from "react";

import { api } from "../../../convex/_generated/api";
import { StateChip } from "../../components/state-chip";
import { RelativeTime } from "../../components/time";
import { cn } from "../../lib/cn";
import { formatDuration, shortId } from "../../lib/format";
import {
  type SubDrawerSpec,
  useContributeSubDrawer,
  useSubDrawerSearch,
} from "../../shell/sub-drawer";
import { useUiStore } from "../../store/ui-store";

type Section = "scheduled" | "cron";

const SECTIONS: Array<{ id: Section; label: string }> = [
  { id: "scheduled", label: "Scheduled" },
  { id: "cron", label: "Cron" },
];

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

function SchedulesPage() {
  const search = useSearch({ from: "/app/schedules" });
  const navigate = useNavigate();
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

  const setSection = (next: Section) =>
    navigate({
      to: "/app/schedules",
      search: { section: next },
      replace: true,
    });

  const spec = useMemo<SubDrawerSpec>(
    () => ({
      kind: "dynamic",
      title: "Schedules",
      search: { placeholder: "Filter schedules" },
      children: (
        <SchedulesSubDrawer
          scheduled={scheduled}
          cron={cron}
          activeSection={section}
        />
      ),
    }),
    [scheduled, cron, section],
  );
  useContributeSubDrawer(spec);

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

      <nav
        aria-label="Schedule sections"
        className="flex shrink-0 gap-px border-b border-app bg-surface-2"
        data-testid="schedules-tabs"
      >
        {SECTIONS.map((s) => {
          const isActive = section === s.id;
          return (
            <button
              key={s.id}
              type="button"
              onClick={() => setSection(s.id)}
              aria-current={isActive ? "page" : undefined}
              data-testid={`schedules-tab-${s.id}`}
              className={cn(
                "flex items-center px-3 py-2 font-mono text-xs uppercase tracking-wide",
                isActive
                  ? "border-b-2 border-[color:var(--color-brand)] text-default"
                  : "text-muted hover:text-default",
              )}
            >
              {s.label}
            </button>
          );
        })}
      </nav>

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

function SchedulesSubDrawer({
  scheduled,
  cron,
  activeSection,
}: {
  scheduled: ScheduledJobDoc[] | undefined;
  cron: CronJobDoc[] | undefined;
  activeSection: Section;
}) {
  const filter = useSubDrawerSearch().trim().toLowerCase();

  return (
    <div className="flex flex-col gap-2 px-2 py-2">
      <SectionLink id="scheduled" label="Scheduled" active={activeSection} />
      <SectionLink id="cron" label="Cron" active={activeSection} />
      <div className="mt-2 border-t border-app pt-2">
        <div className="px-2 pb-1 font-mono text-[10px] uppercase tracking-[0.18em] text-muted">
          {activeSection === "scheduled"
            ? "Recent scheduled"
            : "Recent cron"}
        </div>
        {activeSection === "scheduled" ? (
          <ScheduledList jobs={scheduled} filter={filter} />
        ) : (
          <CronList jobs={cron} filter={filter} />
        )}
      </div>
    </div>
  );
}

function SectionLink({
  id,
  label,
  active,
}: {
  id: Section;
  label: string;
  active: Section;
}) {
  const isActive = id === active;
  return (
    <Link
      to="/app/schedules"
      search={{ section: id }}
      data-testid={`sub-drawer-schedules-${id}`}
      className={cn(
        "flex h-8 items-center gap-2 rounded-md px-2 text-sm",
        isActive
          ? "bg-surface-2 text-default"
          : "text-muted hover:bg-surface-2 hover:text-default",
      )}
    >
      <span className="flex-1 truncate font-mono text-xs uppercase tracking-wide">
        {label}
      </span>
    </Link>
  );
}

function ScheduledList({
  jobs,
  filter,
}: {
  jobs: ScheduledJobDoc[] | undefined;
  filter: string;
}) {
  if (jobs === undefined) {
    return (
      <div className="px-2 py-2 text-xs text-muted">
        <span aria-hidden>·</span>
        <span className="sr-only">loading</span>
      </div>
    );
  }
  const filtered = filter
    ? jobs.filter(
        (j) =>
          (j.functionPath ?? "").toLowerCase().includes(filter) ||
          (j.status ?? "").toLowerCase().includes(filter),
      )
    : jobs;
  if (filtered.length === 0) {
    return (
      <div className="px-2 py-2 text-xs text-muted">
        {jobs.length === 0 ? "No scheduled jobs." : "No matches."}
      </div>
    );
  }
  return (
    <ul className="flex flex-col gap-px">
      {filtered.slice(0, 30).map((job) => (
        <li key={job._id}>
          <div className="flex h-7 items-center gap-2 rounded-md px-2 text-sm text-muted">
            <span className="flex-1 truncate font-mono text-xs">
              {job.functionPath ?? shortId(job._id, 12)}
            </span>
            {job.status ? (
              <span className="tabular font-mono text-[10px] uppercase tracking-[0.18em] text-muted">
                {job.status}
              </span>
            ) : null}
          </div>
        </li>
      ))}
    </ul>
  );
}

function CronList({
  jobs,
  filter,
}: {
  jobs: CronJobDoc[] | undefined;
  filter: string;
}) {
  if (jobs === undefined) {
    return (
      <div className="px-2 py-2 text-xs text-muted">
        <span aria-hidden>·</span>
        <span className="sr-only">loading</span>
      </div>
    );
  }
  const filtered = filter
    ? jobs.filter(
        (j) =>
          (j.name ?? "").toLowerCase().includes(filter) ||
          (j.functionPath ?? "").toLowerCase().includes(filter) ||
          (j.cron ?? "").toLowerCase().includes(filter),
      )
    : jobs;
  if (filtered.length === 0) {
    return (
      <div className="px-2 py-2 text-xs text-muted">
        {jobs.length === 0 ? "No cron jobs." : "No matches."}
      </div>
    );
  }
  return (
    <ul className="flex flex-col gap-px">
      {filtered.slice(0, 30).map((job) => (
        <li key={job._id}>
          <div className="flex h-7 items-center gap-2 rounded-md px-2 text-sm text-muted">
            <span className="flex-1 truncate font-mono text-xs">
              {job.name ?? job.functionPath ?? shortId(job._id, 12)}
            </span>
            {job.status ? (
              <span className="tabular font-mono text-[10px] uppercase tracking-[0.18em] text-muted">
                {job.status}
              </span>
            ) : null}
          </div>
        </li>
      ))}
    </ul>
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
