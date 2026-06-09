import { createFileRoute, Link } from "@tanstack/react-router";
import { useNimbusConnectionState, useQuery } from "@nimbus/nimbus/react";

import { api } from "../../../convex/_generated/api";
import { CopyChip } from "../../components/copy-chip";
import { LoadingCell } from "../../components/loading-cell";
import { StateChip } from "../../components/state-chip";
import { RelativeTime, Uptime } from "../../components/time";
import { formatDuration, shortId } from "../../lib/format";
import {
  type ConnectionSnapshot,
  type LoadingValue,
  toLoadingValue,
} from "../../shell/loading-value";

export const Route = createFileRoute("/developer/")({
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
  const conn = useConnSnapshot();
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
        machines={toLoadingValue(machines, conn)}
        services={toLoadingValue(services, conn)}
        functions={toLoadingValue(functions, conn)}
        tables={toLoadingValue(tables, conn)}
        runs={toLoadingValue(runs, conn)}
        tenantCount={tenantIdSet.size}
      />

      <div
        className="grid grid-cols-1 gap-3 lg:grid-cols-2"
        data-testid="overview-activity"
      >
        <EventsFeed events={toLoadingValue(events, conn)} />
        <RecentRuns runs={toLoadingValue(runs, conn)} />
      </div>
    </section>
  );
}

function useConnSnapshot(): ConnectionSnapshot {
  const conn = useNimbusConnectionState();
  return {
    isWebSocketConnected: conn.isWebSocketConnected,
    hasEverConnected: conn.hasEverConnected,
  };
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
  machines: LoadingValue<AnyDoc[]>;
  services: LoadingValue<AnyDoc[]>;
  functions: LoadingValue<AnyDoc[]>;
  tables: LoadingValue<AnyDoc[]>;
  runs: LoadingValue<AnyDoc[]>;
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
        to="/operator/machines"
      />
      <CountPanel
        title="Services"
        testid="overview-count-services"
        docs={services}
        groupBy="state"
        to="/developer/compute"
      />
      <CountPanel
        title="Tenants"
        testid="overview-count-tenants"
        docs={{ kind: "ok", value: [] }}
        explicitTotal={tenantCount}
        groupBy={null}
        to="/developer/storage"
      />
      <CountPanel
        title="Tables"
        testid="overview-count-tables"
        docs={tables}
        groupBy={null}
        to="/developer/storage"
      />
      <CountPanel
        title="Functions"
        testid="overview-count-functions"
        docs={functions}
        groupBy="kind"
        to="/developer/compute"
      />
      <CountPanel
        title="Recent runs"
        testid="overview-count-runs"
        docs={runs}
        groupBy="status"
        to="/developer/observability"
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
  docs: LoadingValue<AnyDoc[]>;
  groupBy: string | null;
  to:
    | "/operator/machines"
    | "/developer/compute"
    | "/developer/storage"
    | "/developer/observability";
  explicitTotal?: number;
}) {
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
          {explicitTotal !== undefined ? (
            explicitTotal
          ) : (
            <LoadingCell value={docs} testid={`${testid}-total`}>
              {(items) => items.length}
            </LoadingCell>
          )}
        </span>
      </div>
      <CountPanelBreakdown docs={docs} groupBy={groupBy} />
    </Link>
  );
}

function CountPanelBreakdown({
  docs,
  groupBy,
}: {
  docs: LoadingValue<AnyDoc[]>;
  groupBy: string | null;
}) {
  if (docs.kind === "loading") {
    return <span className="text-xs text-muted">Loading…</span>;
  }
  if (docs.kind === "offline") {
    return (
      <span className="text-xs text-muted" title="Disconnected">
        offline · last value shown elsewhere
      </span>
    );
  }
  if (docs.kind === "error") {
    return (
      <span className="text-xs text-danger" title={docs.message}>
        {docs.message}
      </span>
    );
  }
  const breakdown = groupBy ? groupCount(docs.value, groupBy) : [];
  if (breakdown.length === 0) {
    return <span className="text-xs text-muted">No state breakdown</span>;
  }
  return (
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

function EventsFeed({ events }: { events: LoadingValue<AnyDoc[]> }) {
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
          to="/developer/observability"
          className="text-xs text-link hover:underline"
        >
          View all
        </Link>
      </header>
      <FeedBody
        value={events}
        emptyMessage="No events recorded yet — the feed updates live."
        renderItems={(items) => (
          <ul className="divide-y divide-app">
            {items.slice(0, 20).map((event) => (
              <EventRow key={String(event._id)} event={event} />
            ))}
          </ul>
        )}
      />
    </section>
  );
}

function FeedBody({
  value,
  emptyMessage,
  renderItems,
}: {
  value: LoadingValue<AnyDoc[]>;
  emptyMessage: string;
  renderItems: (items: AnyDoc[]) => React.ReactNode;
}) {
  if (value.kind === "loading") {
    return <p className="px-3 py-4 text-xs text-muted">Loading…</p>;
  }
  if (value.kind === "offline") {
    return (
      <p
        className="px-3 py-4 text-xs text-muted"
        title="Disconnected — stream resumes on reconnect"
      >
        offline · live feed paused
      </p>
    );
  }
  if (value.kind === "error") {
    return (
      <p className="px-3 py-4 text-xs text-danger" title={value.message}>
        {value.message}
      </p>
    );
  }
  if (value.value.length === 0) {
    return <p className="px-3 py-4 text-xs text-muted">{emptyMessage}</p>;
  }
  return <>{renderItems(value.value)}</>;
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

function RecentRuns({ runs }: { runs: LoadingValue<AnyDoc[]> }) {
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
          to="/developer/observability"
          className="text-xs text-link hover:underline"
        >
          View all
        </Link>
      </header>
      <FeedBody
        value={runs}
        emptyMessage="No runs yet — invoke a function to populate this list."
        renderItems={(items) => (
          <ul className="divide-y divide-app">
            {items.slice(0, 10).map((run) => (
              <RunRow key={String(run._id)} run={run} />
            ))}
          </ul>
        )}
      />
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
