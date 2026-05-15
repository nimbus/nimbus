import { createFileRoute, Link } from "@tanstack/react-router";
import { useQuery } from "nimbus/react";
import { useMemo, useState } from "react";

import { api } from "../../convex/_generated/api";
import { CopyChip } from "../components/copy-chip";
import { StateChip } from "../components/state-chip";
import { RelativeTime } from "../components/time";
import { cn } from "../lib/cn";
import { formatDuration, shortId } from "../lib/format";

export const Route = createFileRoute("/compute")({
  component: ComputePage,
});

type ServiceDoc = {
  _id: string;
  _updateTime?: number;
  name?: string;
  state?: string;
  tenantId?: string;
  machineId?: string;
  endpoint?: string;
  meta?: Record<string, unknown> | null;
};

type FunctionDoc = {
  _id: string;
  _updateTime?: number;
  path?: string;
  kind?: string;
  adapter?: string;
  bundleId?: string;
  source?: string;
  argsSchema?: unknown;
  returnsSchema?: unknown;
  lastStatus?: string;
  lastRunAt?: number;
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

type BundleDoc = {
  _id: string;
  sha256?: string;
  status?: string;
  sourceRef?: string;
  _creationTime?: number;
};

type ComputeSection = "services" | "functions" | "scheduled" | "cron";

const SECTIONS: Array<{ id: ComputeSection; label: string }> = [
  { id: "services", label: "Services" },
  { id: "functions", label: "Functions" },
  { id: "scheduled", label: "Scheduled" },
  { id: "cron", label: "Cron" },
];

function ComputePage() {
  const [active, setActive] = useState<ComputeSection>("services");

  const services = useQuery(api.services.list, {
    tenantId: null,
    machineId: null,
    state: null,
    limit: 200,
  }) as ServiceDoc[] | undefined;
  const functions = useQuery(api.functions.list, {
    bundleId: null,
    kind: null,
    limit: 200,
  }) as FunctionDoc[] | undefined;
  const scheduled = useQuery(api.scheduled_jobs.list, {
    tenantId: null,
    status: null,
    limit: 200,
  }) as ScheduledJobDoc[] | undefined;
  const cron = useQuery(api.cron_jobs.list, {
    tenantId: null,
    status: null,
    limit: 200,
  }) as CronJobDoc[] | undefined;
  const bundles = useQuery(api.bundles.list, {
    status: null,
    limit: 50,
  }) as BundleDoc[] | undefined;

  const counts: Record<ComputeSection, number | undefined> = {
    services: services?.length,
    functions: functions?.length,
    scheduled: scheduled?.length,
    cron: cron?.length,
  };

  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-compute"
    >
      <header className="flex items-baseline justify-between">
        <div>
          <h1
            className="text-xl text-default"
            style={{ fontSize: "var(--text-xl)" }}
          >
            Compute
          </h1>
          <p className="text-sm text-muted">
            Services, functions, scheduled and cron jobs. Reads stream from the
            <code className="px-1 font-mono text-xs text-default">_nimbus</code>
            system tenant.
          </p>
        </div>
        <div className="flex items-center gap-3">
          <BundleHint bundles={bundles} />
          <Link
            to="/compute/runner"
            data-testid="compute-open-runner"
            className="rounded border border-app px-2 py-0.5 font-mono text-[11px] uppercase tracking-wide text-muted hover:bg-surface hover:text-default"
          >
            runner →
          </Link>
        </div>
      </header>

      <nav
        aria-label="Compute sections"
        className="flex gap-px overflow-hidden rounded-md border border-app bg-surface-2"
        data-testid="compute-tabs"
      >
        {SECTIONS.map((section) => {
          const isActive = active === section.id;
          const count = counts[section.id];
          return (
            <button
              key={section.id}
              type="button"
              onClick={() => setActive(section.id)}
              aria-current={isActive ? "page" : undefined}
              data-testid={`compute-tab-${section.id}`}
              className={cn(
                "flex items-center gap-2 px-3 py-1.5 font-mono text-xs uppercase tracking-wide",
                isActive
                  ? "bg-surface text-default"
                  : "text-muted hover:bg-surface hover:text-default",
              )}
            >
              <span>{section.label}</span>
              <span
                className="tabular text-[10px] text-muted"
                data-testid={`compute-tab-${section.id}-count`}
              >
                {count === undefined ? "—" : count}
              </span>
            </button>
          );
        })}
      </nav>

      <div className="min-h-0 flex-1 overflow-hidden rounded-md border border-app bg-surface">
        {active === "services" ? <ServicesTable services={services} /> : null}
        {active === "functions" ? (
          <FunctionsTable functions={functions} bundles={bundles} />
        ) : null}
        {active === "scheduled" ? <ScheduledTable jobs={scheduled} /> : null}
        {active === "cron" ? <CronTable jobs={cron} /> : null}
      </div>
    </section>
  );
}

function BundleHint({ bundles }: { bundles: BundleDoc[] | undefined }) {
  if (bundles === undefined) {
    return (
      <span
        className="font-mono text-[11px] text-muted"
        data-testid="compute-bundles-loading"
      >
        bundles: loading…
      </span>
    );
  }
  const active = bundles.filter((b) => b.status === "active").length;
  return (
    <span
      className="font-mono text-[11px] text-muted"
      data-testid="compute-bundles"
    >
      {bundles.length} bundle{bundles.length === 1 ? "" : "s"}
      {active > 0 ? ` · ${active} active` : ""}
    </span>
  );
}

function ServicesTable({ services }: { services: ServiceDoc[] | undefined }) {
  if (services === undefined) return <Loading label="Loading services…" />;
  if (services.length === 0) {
    return (
      <Empty
        title="No services"
        detail="Services published by adapters and runtime hosts appear here in real time."
      />
    );
  }
  return (
    <div className="overflow-auto">
      <table
        className="w-full border-collapse text-sm"
        data-testid="compute-services-table"
      >
        <thead className="sticky top-0 bg-surface-2 text-[10px] uppercase tracking-[0.14em] text-muted">
          <tr>
            <Th>Name</Th>
            <Th>State</Th>
            <Th>Tenant</Th>
            <Th>Machine</Th>
            <Th>Endpoint</Th>
            <Th>Updated</Th>
          </tr>
        </thead>
        <tbody>
          {services.map((svc) => (
            <tr
              key={svc._id}
              className="border-t border-app hover:bg-surface-2"
              data-testid={`compute-service-${svc.name ?? svc._id}`}
            >
              <Td>
                <span className="font-mono text-default">
                  {svc.name ?? shortId(svc._id, 12)}
                </span>
              </Td>
              <Td>
                <StateChip state={svc.state} />
              </Td>
              <Td>
                <span className="font-mono text-xs text-default">
                  {svc.tenantId ?? "—"}
                </span>
              </Td>
              <Td>
                <span className="font-mono text-xs text-default">
                  {svc.machineId ?? "—"}
                </span>
              </Td>
              <Td>
                <span className="font-mono text-xs text-muted">
                  {svc.endpoint ?? "—"}
                </span>
              </Td>
              <Td>
                {typeof svc._updateTime === "number" ? (
                  <RelativeTime epochMs={svc._updateTime} />
                ) : (
                  <span className="tabular text-muted">—</span>
                )}
              </Td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function FunctionsTable({
  functions,
  bundles,
}: {
  functions: FunctionDoc[] | undefined;
  bundles: BundleDoc[] | undefined;
}) {
  const bundleLookup = useMemo(() => {
    const map = new Map<string, BundleDoc>();
    bundles?.forEach((b) => {
      map.set(b._id, b);
    });
    return map;
  }, [bundles]);
  if (functions === undefined) return <Loading label="Loading functions…" />;
  if (functions.length === 0) {
    return (
      <Empty
        title="No functions registered"
        detail="Deploy a Convex, Nimbus, or Cloud Functions app to populate the function inventory."
      />
    );
  }
  return (
    <div className="overflow-auto">
      <table
        className="w-full border-collapse text-sm"
        data-testid="compute-functions-table"
      >
        <thead className="sticky top-0 bg-surface-2 text-[10px] uppercase tracking-[0.14em] text-muted">
          <tr>
            <Th>Path</Th>
            <Th>Kind</Th>
            <Th>Adapter</Th>
            <Th>Bundle</Th>
            <Th>Last status</Th>
            <Th>Last run</Th>
            <Th>Action</Th>
          </tr>
        </thead>
        <tbody>
          {functions.map((fn) => {
            const bundle = fn.bundleId
              ? bundleLookup.get(fn.bundleId)
              : undefined;
            return (
              <tr
                key={fn._id}
                className="border-t border-app hover:bg-surface-2"
                data-testid={`compute-function-${fn.path ?? fn._id}`}
              >
                <Td>
                  <span className="font-mono text-default">
                    {fn.path ?? "—"}
                  </span>
                </Td>
                <Td>
                  <span className="font-mono text-xs uppercase tracking-wide text-muted">
                    {fn.kind ?? "—"}
                  </span>
                </Td>
                <Td>
                  <span className="font-mono text-xs text-default">
                    {fn.adapter ?? "—"}
                  </span>
                </Td>
                <Td>
                  {bundle?.sha256 ? (
                    <CopyChip
                      label="bundle sha256"
                      value={bundle.sha256}
                      testid={`compute-function-bundle-${fn.path ?? fn._id}`}
                    >
                      {shortId(bundle.sha256, 12)}
                    </CopyChip>
                  ) : (
                    <span className="tabular text-muted">—</span>
                  )}
                </Td>
                <Td>
                  <StateChip state={fn.lastStatus ?? "idle"} />
                </Td>
                <Td>
                  {typeof fn.lastRunAt === "number" ? (
                    <RelativeTime epochMs={fn.lastRunAt} />
                  ) : (
                    <span className="tabular text-muted">never</span>
                  )}
                </Td>
                <Td>
                  {fn.path ? (
                    <Link
                      to="/compute/runner"
                      search={{ fn: fn.path }}
                      data-testid={`compute-function-run-${fn.path}`}
                      className="rounded border border-app px-1.5 py-0.5 font-mono text-[11px] uppercase tracking-wide text-muted hover:bg-surface hover:text-default"
                    >
                      run
                    </Link>
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
        data-testid="compute-scheduled-table"
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
                data-testid={`compute-scheduled-${job._id}`}
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
        data-testid="compute-cron-table"
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
              data-testid={`compute-cron-${job.name ?? job._id}`}
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
