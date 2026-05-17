import { createFileRoute, Link } from "@tanstack/react-router";
import { useQuery } from "nimbus/react";

import { api } from "../../../convex/_generated/api";
import { CopyChip } from "../../components/copy-chip";
import { StateChip } from "../../components/state-chip";
import { RelativeTime, Uptime } from "../../components/time";
import { formatDuration, shortId } from "../../lib/format";

export const Route = createFileRoute("/app/")({
  component: OverviewPage,
});

type SystemStatusDoc = {
  _id?: string;
  name?: string;
  version?: string;
  health?: string;
  startedAt?: number;
  updatedAt?: number;
  details?: Record<string, unknown> | null;
} | null;

type AnyDoc = Record<string, unknown> & { _id?: string };

function OverviewPage() {
  const status = useQuery(api.system.status, {}) as SystemStatusDoc | undefined;
  const machines = useQuery(api.machines.list, {
    state: null,
    provider: null,
    limit: 200,
  }) as AnyDoc[] | undefined;
  const services = useQuery(api.services.list, {
    tenantId: null,
    machineId: null,
    state: null,
    limit: 200,
  }) as AnyDoc[] | undefined;
  const tables = useQuery(api.tables.list, {
    tenantId: null,
    limit: 200,
  }) as AnyDoc[] | undefined;
  const functions = useQuery(api.functions.list, {
    bundleId: null,
    kind: null,
    limit: 200,
  }) as AnyDoc[] | undefined;
  const runs = useQuery(api.runs.recent, {
    bundleId: null,
    functionPath: null,
    status: null,
    limit: 20,
  }) as AnyDoc[] | undefined;
  const events = useQuery(api.events.recent, {
    source: null,
    level: null,
    category: null,
    correlationId: null,
    limit: 20,
  }) as AnyDoc[] | undefined;

  const tenantIdSet = new Set<string>();
  for (const doc of services ?? []) {
    if (typeof doc.tenantId === "string") tenantIdSet.add(doc.tenantId);
  }
  for (const doc of tables ?? []) {
    if (typeof doc.tenantId === "string") tenantIdSet.add(doc.tenantId);
  }

  return (
    <section
      className="flex h-full flex-col gap-4 overflow-y-auto px-6 py-5"
      data-testid="page-overview"
    >
      <header className="flex items-baseline justify-between">
        <div>
          <h1
            className="text-xl text-default"
            style={{ fontSize: "var(--text-xl)" }}
          >
            Overview
          </h1>
          <p className="text-sm text-muted">
            Deployment health, recent activity, and live resource counts.
          </p>
        </div>
      </header>

      <TopStrip status={status} />

      <ResourceCountsGrid
        machines={machines}
        services={services}
        functions={functions}
        tables={tables}
        runs={runs}
        tenantCount={tenantIdSet.size}
      />

      <div
        className="grid grid-cols-1 gap-3 lg:grid-cols-2"
        data-testid="overview-activity"
      >
        <EventsFeed events={events} />
        <RecentRuns runs={runs} />
      </div>
    </section>
  );
}

function TopStrip({ status }: { status: SystemStatusDoc | undefined }) {
  const details = (status?.details ?? {}) as Record<string, unknown>;
  const storageBackend =
    typeof details.storageBackend === "string"
      ? details.storageBackend
      : typeof details.storage === "string"
        ? details.storage
        : "—";
  const license =
    typeof details.license === "string"
      ? details.license
      : typeof details.licensePosture === "string"
        ? details.licensePosture
        : "developer";
  const version = status?.version ?? "—";
  const health = status?.health ?? (status === null ? "unknown" : "—");
  const startedAt =
    typeof status?.startedAt === "number" ? status.startedAt : null;
  return (
    <div
      data-testid="overview-top-strip"
      className="grid grid-cols-2 gap-px overflow-hidden rounded-md border border-app bg-surface-2 md:grid-cols-4"
    >
      <Cell label="Server">
        <StateChip state={health} />
      </Cell>
      <Cell label="Version">
        <CopyChip label="version" value={version} testid="overview-version" />
      </Cell>
      <Cell label="Uptime">
        {startedAt ? (
          <Uptime startedAtMs={startedAt} />
        ) : (
          <span className="tabular text-muted">—</span>
        )}
      </Cell>
      <Cell label="Storage">
        <span className="font-mono text-xs text-default">{storageBackend}</span>
      </Cell>
      <Cell label="License">
        <span className="font-mono text-xs text-default">{license}</span>
      </Cell>
      <Cell label="Started">
        {startedAt ? (
          <RelativeTime epochMs={startedAt} />
        ) : (
          <span className="tabular text-muted">—</span>
        )}
      </Cell>
      <Cell label="Updated">
        {typeof status?.updatedAt === "number" ? (
          <RelativeTime epochMs={status.updatedAt} />
        ) : (
          <span className="tabular text-muted">—</span>
        )}
      </Cell>
      <Cell label="Tenant">
        <CopyChip
          label="active tenant"
          value="_nimbus"
          testid="overview-tenant"
        />
      </Cell>
    </div>
  );
}

function Cell({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1 bg-surface px-3 py-2">
      <span className="text-[10px] uppercase tracking-[0.14em] text-muted">
        {label}
      </span>
      <span className="text-sm">{children}</span>
    </div>
  );
}

function ResourceCountsGrid({
  machines,
  services,
  functions,
  tables,
  runs,
  tenantCount,
}: {
  machines: AnyDoc[] | undefined;
  services: AnyDoc[] | undefined;
  functions: AnyDoc[] | undefined;
  tables: AnyDoc[] | undefined;
  runs: AnyDoc[] | undefined;
  tenantCount: number;
}) {
  return (
    <div
      className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3"
      data-testid="overview-counts"
    >
      <CountPanel
        title="Machines"
        testid="overview-count-machines"
        docs={machines}
        groupBy="state"
        to="/admin/machines"
      />
      <CountPanel
        title="Services"
        testid="overview-count-services"
        docs={services}
        groupBy="state"
        to="/app/compute"
      />
      <CountPanel
        title="Tenants"
        testid="overview-count-tenants"
        docs={undefined}
        explicitTotal={tenantCount}
        groupBy={null}
        to="/app/storage"
      />
      <CountPanel
        title="Tables"
        testid="overview-count-tables"
        docs={tables}
        groupBy={null}
        to="/app/storage"
      />
      <CountPanel
        title="Functions"
        testid="overview-count-functions"
        docs={functions}
        groupBy="kind"
        to="/app/compute"
      />
      <CountPanel
        title="Recent runs"
        testid="overview-count-runs"
        docs={runs}
        groupBy="status"
        to="/app/observability"
      />
    </div>
  );
}

function CountPanel({
  title,
  testid,
  docs,
  groupBy,
  to,
  explicitTotal,
}: {
  title: string;
  testid: string;
  docs: AnyDoc[] | undefined;
  groupBy: string | null;
  to:
    | "/admin/machines"
    | "/app/compute"
    | "/app/storage"
    | "/app/observability";
  explicitTotal?: number;
}) {
  const loading = docs === undefined && explicitTotal === undefined;
  const total = explicitTotal ?? docs?.length ?? 0;
  const breakdown = groupBy && docs ? groupCount(docs, groupBy) : [];
  return (
    <Link
      to={to}
      data-testid={testid}
      className="group flex flex-col gap-2 rounded-md border border-app bg-surface p-3 hover:border-strong"
    >
      <div className="flex items-baseline justify-between">
        <span className="text-xs uppercase tracking-[0.14em] text-muted">
          {title}
        </span>
        <span
          className="tabular font-mono text-lg text-default"
          data-testid={`${testid}-total`}
        >
          {loading ? "·" : total}
        </span>
      </div>
      {loading ? (
        <span className="text-xs text-muted">Loading…</span>
      ) : breakdown.length === 0 ? (
        <span className="text-xs text-muted">No state breakdown</span>
      ) : (
        <ul className="flex flex-wrap gap-1.5">
          {breakdown.map(([key, count]) => (
            <li key={key} className="inline-flex items-center gap-1">
              <StateChip state={key} />
              <span className="tabular font-mono text-xs text-default">
                {count}
              </span>
            </li>
          ))}
        </ul>
      )}
    </Link>
  );
}

function groupCount(docs: AnyDoc[], field: string): Array<[string, number]> {
  const map = new Map<string, number>();
  for (const doc of docs) {
    const raw = doc[field];
    const key = typeof raw === "string" && raw.length > 0 ? raw : "unknown";
    map.set(key, (map.get(key) ?? 0) + 1);
  }
  return Array.from(map.entries()).sort((a, b) => b[1] - a[1]);
}

function EventsFeed({ events }: { events: AnyDoc[] | undefined }) {
  return (
    <section
      data-testid="overview-events"
      className="flex flex-col rounded-md border border-app bg-surface"
    >
      <header className="flex items-baseline justify-between border-b border-app px-3 py-2">
        <h2 className="text-xs uppercase tracking-[0.14em] text-muted">
          Recent events
        </h2>
        <Link
          to="/app/observability"
          className="text-xs text-link hover:underline"
        >
          View all
        </Link>
      </header>
      {events === undefined ? (
        <p className="px-3 py-4 text-xs text-muted">Loading…</p>
      ) : events.length === 0 ? (
        <p className="px-3 py-4 text-xs text-muted">
          No events recorded yet — the feed updates live.
        </p>
      ) : (
        <ul className="divide-y divide-app">
          {events.slice(0, 20).map((event) => (
            <EventRow key={String(event._id)} event={event} />
          ))}
        </ul>
      )}
    </section>
  );
}

function EventRow({ event }: { event: AnyDoc }) {
  const level = typeof event.level === "string" ? event.level : "info";
  const source = typeof event.source === "string" ? event.source : "—";
  const message = typeof event.message === "string" ? event.message : "";
  const createdAt =
    typeof event.createdAt === "number" ? event.createdAt : null;
  const correlationId =
    typeof event.correlationId === "string"
      ? event.correlationId
      : typeof event._id === "string"
        ? event._id
        : null;
  return (
    <li className="group flex flex-col gap-1 px-3 py-2 hover:bg-surface-2">
      <div className="flex items-center gap-2">
        <StateChip state={level} />
        <span className="font-mono text-xs text-muted">{source}</span>
        {correlationId ? (
          <CopyChip
            label="event id"
            value={correlationId}
            hideUntilHover
            className="text-muted"
            testid="event-id"
          >
            {shortId(correlationId)}
          </CopyChip>
        ) : null}
        <span className="ml-auto text-xs">
          {createdAt ? <RelativeTime epochMs={createdAt} /> : null}
        </span>
      </div>
      <p className="truncate text-xs text-default">{message}</p>
    </li>
  );
}

function RecentRuns({ runs }: { runs: AnyDoc[] | undefined }) {
  return (
    <section
      data-testid="overview-runs"
      className="flex flex-col rounded-md border border-app bg-surface"
    >
      <header className="flex items-baseline justify-between border-b border-app px-3 py-2">
        <h2 className="text-xs uppercase tracking-[0.14em] text-muted">
          Recent runs
        </h2>
        <Link
          to="/app/observability"
          className="text-xs text-link hover:underline"
        >
          View all
        </Link>
      </header>
      {runs === undefined ? (
        <p className="px-3 py-4 text-xs text-muted">Loading…</p>
      ) : runs.length === 0 ? (
        <p className="px-3 py-4 text-xs text-muted">
          No runs yet — invoke a function to populate this list.
        </p>
      ) : (
        <ul className="divide-y divide-app">
          {runs.slice(0, 10).map((run) => (
            <RunRow key={String(run._id)} run={run} />
          ))}
        </ul>
      )}
    </section>
  );
}

function RunRow({ run }: { run: AnyDoc }) {
  const status = typeof run.status === "string" ? run.status : "unknown";
  const functionPath =
    typeof run.functionPath === "string" ? run.functionPath : "—";
  const durationMs =
    typeof run.durationMs === "number" ? run.durationMs : undefined;
  const startedAt = typeof run.startedAt === "number" ? run.startedAt : null;
  const runId = typeof run._id === "string" ? run._id : null;
  return (
    <li className="group flex items-center gap-2 px-3 py-2 hover:bg-surface-2">
      <StateChip state={status} />
      <span className="truncate font-mono text-xs text-default">
        {functionPath}
      </span>
      {runId ? (
        <CopyChip
          label="run id"
          value={runId}
          hideUntilHover
          className="text-muted"
          testid="run-id"
        >
          {shortId(runId)}
        </CopyChip>
      ) : null}
      <span className="ml-auto tabular font-mono text-xs text-muted">
        {formatDuration(durationMs)}
      </span>
      <span className="text-xs">
        {startedAt ? <RelativeTime epochMs={startedAt} /> : null}
      </span>
    </li>
  );
}
