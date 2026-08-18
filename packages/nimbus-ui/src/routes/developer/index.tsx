import { createFileRoute, Link } from "@tanstack/react-router";
import { useNimbusConnectionState, useQuery } from "@nimbus/nimbus/react";

import { api } from "../../../convex/_generated/api";
import { CategoryChip } from "../../components/category-chip";
import { CopyChip } from "../../components/copy-chip";
import { LoadingCell } from "../../components/loading-cell";
import { PageHeader } from "../../components/page-header";
import { StateChip } from "../../components/state-chip";
import { RelativeTime, Uptime } from "../../components/time";
import { formatDuration, shortId } from "../../lib/format";
import {
  type ConnectionSnapshot,
  type LoadingValue,
  toLoadingValue,
} from "../../shell/loading-value";
import { useUiStore } from "../../store/ui-store";

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

  const serviceTenantIds = distinctTenantIds(services);
  const tableTenantIds = distinctTenantIds(tables);
  const tenantIdSet = new Set([...serviceTenantIds, ...tableTenantIds]);

  return (
    <section
      className="flex h-full flex-col gap-4 overflow-y-auto px-6 py-5"
      data-testid="page-overview"
    >
      <PageHeader
        title="Overview"
        subtitle="Deployment health, recent activity, and live resource counts."
      />

      <TopStrip status={status} />

      <ResourceCountsGrid
        machines={toLoadingValue(machines, conn)}
        services={toLoadingValue(services, conn)}
        functions={toLoadingValue(functions, conn)}
        tables={toLoadingValue(tables, conn)}
        runs={toLoadingValue(runs, conn)}
        tenantCount={tenantIdSet.size}
        tenantSubline={
          services === undefined
            ? undefined
            : `${serviceTenantIds.size} with services`
        }
        tableSubline={
          tables === undefined
            ? undefined
            : `across ${tableTenantIds.size} ${tableTenantIds.size === 1 ? "tenant" : "tenants"}`
        }
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

function distinctTenantIds(docs: AnyDoc[] | undefined): Set<string> {
  const ids = new Set<string>();
  for (const doc of docs ?? []) {
    if (typeof doc.tenantId === "string") ids.add(doc.tenantId);
  }
  return ids;
}

function useConnSnapshot(): ConnectionSnapshot {
  const conn = useNimbusConnectionState();
  return {
    isWebSocketConnected: conn.isWebSocketConnected,
    hasEverConnected: conn.hasEverConnected,
  };
}

function TopStrip({ status }: { status: SystemStatusDoc | undefined }) {
  const activeTenant = useUiStore((s) => s.activeTenant);
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
  // `shrink-0` below is load-bearing. This grid is a flex child of a column
  // that overflows once the activity feeds fill up, and `overflow-hidden`
  // means flexbox can shrink it to a hairline without leaving any visual cue
  // that the health header was removed. The page container already scrolls
  // (`overflow-y-auto`); let it, rather than eating the strip.
  return (
    <div
      data-testid="overview-top-strip"
      className="grid shrink-0 grid-cols-2 gap-px overflow-hidden rounded-md border border-app bg-surface-2 md:grid-cols-4"
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
          value={activeTenant ?? "—"}
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
      <span className="text-xs uppercase tracking-[0.14em] text-muted">
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
  tenantSubline,
  tableSubline,
}: {
  machines: LoadingValue<AnyDoc[]>;
  services: LoadingValue<AnyDoc[]>;
  functions: LoadingValue<AnyDoc[]>;
  tables: LoadingValue<AnyDoc[]>;
  runs: LoadingValue<AnyDoc[]>;
  tenantCount: number;
  tenantSubline?: React.ReactNode;
  tableSubline?: React.ReactNode;
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
        to="/developer/services"
      />
      <CountPanel
        title="Tenants"
        testid="overview-count-tenants"
        docs={{ kind: "ok", value: [] }}
        explicitTotal={tenantCount}
        subline={tenantSubline}
        to="/developer/storage"
      />
      <CountPanel
        title="Tables"
        testid="overview-count-tables"
        docs={tables}
        subline={tableSubline}
        to="/developer/storage"
      />
      <CountPanel
        title="Functions"
        testid="overview-count-functions"
        docs={functions}
        groupBy="kind"
        groupKind="category"
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

/**
 * One count tile. The second line is either a breakdown of the counted
 * documents (`groupBy`) or a caller-supplied fact (`subline`) — never a
 * sentence apologising for the absence of a breakdown a tile was never
 * built to have. Tenants and tables have no lifecycle state, so "No state
 * breakdown" was reporting the absence of something that cannot exist.
 *
 * `groupKind` decides the badge. A state gets a labeled dot from the
 * DESIGN.md token table; a category (function kind, adapter, backend) gets
 * a filled pill. Routing a category through `StateChip` is what made the
 * landing page read "? QUERY 3   ? MUTATION 3".
 */
function CountPanel({
  title,
  testid,
  docs,
  groupBy,
  groupKind = "state",
  subline,
  to,
  explicitTotal,
}: {
  title: string;
  testid: string;
  docs: LoadingValue<AnyDoc[]>;
  groupBy?: string;
  groupKind?: "state" | "category";
  subline?: React.ReactNode;
  to:
    | "/operator/machines"
    | "/developer/compute"
    | "/developer/services"
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
      {/* The slot keeps its line whatever it holds — dash, sentence, dots,
          or the slightly taller category pills — so all six tiles and both
          grid rows stay the same height. */}
      <div
        className="flex min-h-5 items-center"
        data-testid={`${testid}-subline`}
      >
        <CountPanelSubline
          docs={docs}
          groupBy={groupBy}
          groupKind={groupKind}
          subline={subline}
        />
      </div>
    </Link>
  );
}

function CountPanelSubline({
  docs,
  groupBy,
  groupKind,
  subline,
}: {
  docs: LoadingValue<AnyDoc[]>;
  groupBy?: string;
  groupKind: "state" | "category";
  subline?: React.ReactNode;
}) {
  // Connection state outranks both paths: a tile whose query has not landed
  // must not present a stale breakdown or a confident subline.
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
  if (groupBy === undefined) {
    if (subline === undefined) return <Dash />;
    return <span className="text-xs text-muted">{subline}</span>;
  }
  const breakdown = groupCount(docs.value, groupBy);
  // Groupable but genuinely empty. The count already says zero; DESIGN.md
  // allows the em dash at row scope, and it costs no reading.
  if (breakdown.length === 0) return <Dash />;
  return (
    <ul className="flex flex-wrap gap-1.5">
      {breakdown.map(([key, count]) => (
        <li key={key} className="inline-flex items-center gap-1">
          {groupKind === "category" ? (
            <CategoryChip value={key} />
          ) : (
            <StateChip state={key} />
          )}
          <span className="tabular font-mono text-xs text-default">
            {count}
          </span>
        </li>
      ))}
    </ul>
  );
}

function Dash() {
  return (
    <span className="text-xs text-muted" aria-hidden="true">
      —
    </span>
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
      className="flex min-h-[200px] flex-col rounded-md border border-app bg-surface"
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
        testid="overview-events"
        empty={{
          title: "No events recorded yet",
          body: "Server, scheduler, and function activity streams here live.",
          action: { label: "Open Compute", to: "/developer/compute" },
        }}
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

type FeedEmpty = {
  title: string;
  body: string;
  action: { label: string; to: "/developer/compute" };
};

/**
 * The two feeds sit in a stretched two-column grid, so the shorter card is
 * handed the taller one's height whether it has content or not. Rather than
 * un-stretching the grid — which only moves the ragged edge, since the events
 * feed shows 20 rows and the runs feed 10 — the non-list branches fill the
 * frame they are given and centre in it. A centred two-line message with a
 * next action reads as a deliberate empty state; the same words pinned to the
 * top-left of a tall empty box read as a list that failed to load.
 */
function FeedBody({
  value,
  empty,
  testid,
  renderItems,
}: {
  value: LoadingValue<AnyDoc[]>;
  empty: FeedEmpty;
  testid: string;
  renderItems: (items: AnyDoc[]) => React.ReactNode;
}) {
  if (value.kind === "loading") {
    return <FeedNotice>Loading…</FeedNotice>;
  }
  if (value.kind === "offline") {
    return (
      <FeedNotice title="Disconnected — stream resumes on reconnect">
        offline · live feed paused
      </FeedNotice>
    );
  }
  if (value.kind === "error") {
    return (
      <FeedNotice title={value.message} tone="danger">
        {value.message}
      </FeedNotice>
    );
  }
  if (value.value.length === 0) {
    return (
      <div
        className="flex flex-1 flex-col items-center justify-center gap-1 px-3 py-6 text-center"
        data-testid={`${testid}-empty`}
      >
        <p className="text-xs text-default">{empty.title}</p>
        <p className="max-w-[40ch] text-xs text-muted">{empty.body}</p>
        <Link
          to={empty.action.to}
          className="mt-2 rounded border border-app px-3 py-1 font-mono text-xs uppercase tracking-wide text-muted hover:bg-surface-2 hover:text-default"
          data-testid={`${testid}-empty-cta`}
        >
          {empty.action.label}
        </Link>
      </div>
    );
  }
  return <>{renderItems(value.value)}</>;
}

function FeedNotice({
  children,
  title,
  tone = "muted",
}: {
  children: React.ReactNode;
  title?: string;
  tone?: "muted" | "danger";
}) {
  return (
    <p
      className={`flex flex-1 items-center justify-center px-3 py-6 text-center text-xs ${
        tone === "danger" ? "text-danger" : "text-muted"
      }`}
      title={title}
    >
      {children}
    </p>
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

function RecentRuns({ runs }: { runs: LoadingValue<AnyDoc[]> }) {
  return (
    <section
      data-testid="overview-runs"
      className="flex min-h-[200px] flex-col rounded-md border border-app bg-surface"
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
        testid="overview-runs"
        empty={{
          title: "No runs yet",
          body: "A run is recorded each time a query, mutation, or action executes.",
          action: { label: "Open Compute", to: "/developer/compute" },
        }}
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
