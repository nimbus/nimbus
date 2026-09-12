import { useNimbusConnectionState, useQuery } from "@nimbus/nimbus/react";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { useEffect, useState } from "react";

import { api } from "../../../convex/_generated/api";
import type { Doc } from "../../../convex/_generated/dataModel";
import { CopyChip } from "../../components/copy-chip";
import { DataTable, dataColumns } from "../../components/data-table";
import { LoadingCell } from "../../components/loading-cell";
import { Mascot, type MascotState } from "../../components/mascot";
import { StatePill } from "../../components/pill";
import { resolveStateKind, StateDot } from "../../components/state-dot";
import { RelativeTime, Uptime } from "../../components/time";
import { loadTenantList } from "../../hooks/use-tenant-list";
import { formatCount, shortHash } from "../../lib/format";
import { adapterSummary, stateSummary } from "../../lib/inventory-summary";
import {
  type ConnectionSnapshot,
  type LoadingValue,
  toLoadingValue,
} from "../../shell/loading-value";

export const Route = createFileRoute("/operator/")({
  component: NodesPage,
});

type SystemStatusDoc = {
  version?: string;
  buildHash?: string;
  health?: string;
  startedAt?: number;
  updatedAt?: number;
  details?: Record<string, unknown> | null;
} | null;

type SystemStatus = NonNullable<SystemStatusDoc>;
type MachineDoc = Doc<"machines">;
type ServiceDoc = Doc<"services">;
type ListenerDoc = Doc<"listeners">;
type EventDoc = Doc<"events">;

export type EventRow = {
  id: string;
  level: string;
  source: string;
  message: string;
  correlationId: string | null;
  createdAt: number | null;
};

const RECENT_EVENT_ROWS = 5;

// A "node" is a host running the Nimbus binary (the `nimbus node` lifecycle).
// Multi-node clustering is not wired yet, so this deployment is exactly one
// node: the local host, sourced from system status. The page answers "is
// this node fine" first, then what the node is, what it hosts, and what
// happened on it last. This is distinct from a "machine" (the outer dev VM
// under Operator → Machines).
function NodesPage() {
  const conn = useConnSnapshot();
  const status = useQuery(api.system.status, {}) as SystemStatusDoc | undefined;
  const machines = useQuery(api.machines.list, {
    state: null,
    provider: null,
    limit: 500,
  }) as MachineDoc[] | undefined;
  const services = useQuery(api.services.list, {
    tenantId: null,
    machineId: null,
    state: null,
    limit: 500,
  }) as ServiceDoc[] | undefined;
  const listeners = useQuery(api.listeners.list, {
    adapter: null,
    observedPhase: null,
    limit: 100,
  }) as ListenerDoc[] | undefined;
  const events = useQuery(api.events.recent, {
    source: null,
    level: null,
    category: null,
    correlationId: null,
    tenantId: null,
    limit: RECENT_EVENT_ROWS,
  }) as EventDoc[] | undefined;

  const tenantsLv = useTenantCount();
  const statusLv = toStatusValue(status, conn);
  const hosted = toHosted(machines, services, listeners, conn);
  const eventsLv = toLoadingValue(events, conn);

  return (
    <section
      className="flex h-full flex-col gap-6 overflow-y-auto px-6 py-5"
      data-testid="page-operator-nodes"
    >
      <Headline status={statusLv} hosted={hosted} />
      <NodeCard status={statusLv} />
      <HostedRow tenants={tenantsLv} hosted={hosted} />
      <RecentEvents events={eventsLv} />
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

/**
 * `useQuery` resolves to `null` when the server has no status row. That is
 * an answer, so it must not go through `toLoadingValue`, whose null branch
 * means "loading": the headline would sit on the working face for the life
 * of the page on a deployment that has no status document.
 */
function toStatusValue(
  status: SystemStatusDoc | undefined,
  conn: ConnectionSnapshot,
): LoadingValue<SystemStatus> {
  if (status === null) return { kind: "ok", value: {} };
  return toLoadingValue(status, conn);
}

type Hosted = {
  machines: ReadonlyArray<MachineDoc>;
  services: ReadonlyArray<ServiceDoc>;
  listeners: ReadonlyArray<ListenerDoc>;
};

// The three socket-borne lists load as one value so the headline and the
// tiles change shape once. The tenant count is a REST read with its own
// lifecycle (see useTenantCount) and stays outside.
function toHosted(
  machines: MachineDoc[] | undefined,
  services: ServiceDoc[] | undefined,
  listeners: ListenerDoc[] | undefined,
  conn: ConnectionSnapshot,
): LoadingValue<Hosted> {
  const m = toLoadingValue(machines, conn);
  const s = toLoadingValue(services, conn);
  const l = toLoadingValue(listeners, conn);
  for (const v of [m, s, l]) {
    if (v.kind !== "ok") return v as LoadingValue<Hosted>;
  }
  if (m.kind !== "ok" || s.kind !== "ok" || l.kind !== "ok") {
    return { kind: "loading" };
  }
  return {
    kind: "ok",
    value: { machines: m.value, services: s.value, listeners: l.value },
  };
}

const UP_KINDS = new Set(["ok", "healthy", "ready", "running", "active"]);

function isUp(state: string | null | undefined): boolean {
  return UP_KINDS.has(resolveStateKind(state));
}

function isFailing(state: string | null | undefined): boolean {
  const kind = resolveStateKind(state);
  return kind === "error" || kind === "failed" || kind === "crashed";
}

function plural(n: number, noun: string): string {
  return `${formatCount(n)} ${n === 1 ? noun : `${noun}s`}`;
}

// ---------------------------------------------------------------------------
// Headline

type HeadlineReading = {
  mascot: MascotState;
  sentence: string;
};

// readNodeHeadline turns the status document and the hosted inventory into
// the one sentence at the top of the page. The order matters: a lost
// connection beats a healthy status document, because the document is
// stale; a failing service beats a healthy node, because the node is the
// thing the services run on.
export function readNodeHeadline(
  status: LoadingValue<SystemStatus>,
  hosted: LoadingValue<Hosted>,
): HeadlineReading {
  if (status.kind === "offline" || hosted.kind === "offline") {
    return {
      mascot: "error",
      sentence: "The connection to the node dropped. Stale data is shown.",
    };
  }
  if (status.kind === "error") {
    return { mascot: "error", sentence: status.message };
  }
  if (hosted.kind === "error") {
    return { mascot: "error", sentence: hosted.message };
  }
  if (status.kind === "loading" || hosted.kind === "loading") {
    return { mascot: "working", sentence: "Reading the node status." };
  }
  const health = status.value.health;
  if (health && !isUp(health)) {
    return {
      mascot: "error",
      sentence: `The node reports its health as ${health}.`,
    };
  }
  const services = hosted.value.services;
  const failing = services.filter((svc) => isFailing(svc.state)).length;
  if (failing > 0) {
    return {
      mascot: "error",
      sentence: `The node is up. ${plural(failing, "service")} ${failing === 1 ? "is" : "are"} failing.`,
    };
  }
  if (services.length === 0) {
    return {
      mascot: "idle",
      sentence: "The node is up. No services are placed on it yet.",
    };
  }
  const running = services.filter((svc) => isUp(svc.state)).length;
  if (running === services.length) {
    return {
      mascot: "idle",
      sentence: "The node is up and every service is running.",
    };
  }
  return {
    mascot: "idle",
    sentence: `The node is up. ${formatCount(running)} of ${plural(services.length, "service")} ${running === 1 ? "is" : "are"} running.`,
  };
}

function Headline({
  status,
  hosted,
}: {
  status: LoadingValue<SystemStatus>;
  hosted: LoadingValue<Hosted>;
}) {
  const reading = readNodeHeadline(status, hosted);
  const facts = status.kind === "ok" ? readFacts(status.value) : null;
  return (
    <header className="flex flex-col gap-2" data-testid="nodes-headline">
      <div className="flex items-center gap-3">
        <Mascot
          size={40}
          state={reading.mascot}
          decorative
          className="shrink-0 text-text-1"
          data-testid="nodes-mascot"
          data-state={reading.mascot}
        />
        <h1
          className="text-xl text-text-1"
          style={{ fontSize: "var(--text-xl)" }}
          data-testid="nodes-sentence"
        >
          {reading.sentence}
        </h1>
      </div>
      {/* One mono line of facts. A fact the server has not reported is left
          out rather than shown as a dash, so the line never lists what it
          does not know. */}
      <p
        className="flex flex-wrap items-center gap-x-2 gap-y-1 font-mono text-xs text-text-3"
        data-testid="nodes-facts"
      >
        <LoadingCell value={status} testid="nodes-facts">
          {() =>
            facts ? (
              <>
                {facts.listenAddress ? (
                  <CopyChip
                    label="listen address"
                    value={facts.listenAddress}
                    testid="nodes-fact-address"
                    className="text-text-2"
                  />
                ) : null}
                {facts.version ? (
                  <>
                    {facts.listenAddress ? <Separator /> : null}
                    <CopyChip
                      label="version"
                      value={facts.version}
                      testid="nodes-fact-version"
                      className="text-text-2"
                    >
                      v{facts.version}
                    </CopyChip>
                  </>
                ) : null}
                {facts.startedAt !== null ? (
                  <>
                    {facts.listenAddress || facts.version ? (
                      <Separator />
                    ) : null}
                    <span data-testid="nodes-fact-uptime">
                      up{" "}
                      <Uptime
                        startedAtMs={facts.startedAt}
                        className="text-text-2"
                      />
                    </span>
                  </>
                ) : null}
                {facts.dataDir ? (
                  <>
                    {facts.listenAddress ||
                    facts.version ||
                    facts.startedAt !== null ? (
                      <Separator />
                    ) : null}
                    <CopyChip
                      label="data directory"
                      value={facts.dataDir}
                      testid="nodes-fact-data-dir"
                      className="text-text-2"
                    />
                  </>
                ) : null}
              </>
            ) : null
          }
        </LoadingCell>
      </p>
    </header>
  );
}

type Facts = {
  listenAddress: string | null;
  version: string | null;
  startedAt: number | null;
  dataDir: string | null;
};

export function readFacts(status: SystemStatus): Facts {
  const details = status.details ?? {};
  const listenAddress =
    typeof details.listenAddress === "string"
      ? details.listenAddress
      : typeof details.address === "string"
        ? details.address
        : null;
  return {
    listenAddress,
    version: status.version ?? null,
    startedAt: typeof status.startedAt === "number" ? status.startedAt : null,
    dataDir: typeof details.dataDir === "string" ? details.dataDir : null,
  };
}

function Separator() {
  return (
    <span aria-hidden className="text-border-2">
      ·
    </span>
  );
}

// ---------------------------------------------------------------------------
// Node card

// The one row of the future node list. Identity and address sit in the
// facts line above, so the card carries what is left: health, role, and
// the timestamps.
function NodeCard({ status }: { status: LoadingValue<SystemStatus> }) {
  return (
    <article
      className="flex flex-col overflow-hidden rounded-md border border-border-2 bg-bg-panel"
      data-testid="node-row"
    >
      <header className="flex items-center justify-between gap-3 border-b border-border-2 px-4 py-3">
        <div className="flex flex-col gap-0.5">
          <span
            className="font-mono text-sm text-text-1"
            data-testid="node-name"
          >
            local node
          </span>
          <span className="text-xs text-text-3">
            standalone · clustering is not active
          </span>
        </div>
        <span data-testid="node-health">
          <LoadingCell value={status} testid="node-health">
            {(s) => <StatePill state={s.health ?? "unknown"} />}
          </LoadingCell>
        </span>
      </header>
      <div className="grid grid-cols-3 gap-px bg-bg-raised">
        <Cell label="Started" testid="node-started">
          <LoadingCell value={status} testid="node-started">
            {(s) =>
              typeof s.startedAt === "number" ? (
                <RelativeTime epochMs={s.startedAt} className="text-text-1" />
              ) : (
                "—"
              )
            }
          </LoadingCell>
        </Cell>
        <Cell label="Updated" testid="node-updated">
          <LoadingCell value={status} testid="node-updated">
            {(s) =>
              typeof s.updatedAt === "number" ? (
                <RelativeTime epochMs={s.updatedAt} className="text-text-1" />
              ) : (
                "—"
              )
            }
          </LoadingCell>
        </Cell>
        <Cell label="Build" testid="node-build">
          <LoadingCell value={status} testid="node-build">
            {(s) => (s.buildHash ? shortHash(s.buildHash, 7) : "—")}
          </LoadingCell>
        </Cell>
      </div>
    </article>
  );
}

function Cell({
  label,
  testid,
  children,
}: {
  label: string;
  testid: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1 bg-bg-panel px-3 py-2">
      <span className="text-xs font-medium text-text-3">{label}</span>
      <span
        className="font-mono text-sm tabular text-text-1"
        data-testid={testid}
      >
        {children}
      </span>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Hosted on this node

type HostedTo =
  | "/operator/tenants"
  | "/operator/machines"
  | "/operator/services"
  | "/operator/network";

function HostedRow({
  tenants,
  hosted,
}: {
  tenants: LoadingValue<number>;
  hosted: LoadingValue<Hosted>;
}) {
  const value = hosted.kind === "ok" ? hosted.value : null;
  return (
    <section
      className="flex flex-col gap-2"
      aria-labelledby="nodes-hosted-title"
    >
      <h2 id="nodes-hosted-title" className="text-sm font-medium text-text-1">
        Hosted on this node
      </h2>
      <div
        className="grid grid-cols-2 gap-3 md:grid-cols-4"
        data-testid="nodes-hosted"
      >
        <Tile
          label="Tenants"
          testid="nodes-hosted-tenants"
          to="/operator/tenants"
          value={tenants}
          render={(n) => ({ count: n, subline: null })}
        />
        <Tile
          label="Machines"
          testid="nodes-hosted-machines"
          to="/operator/machines"
          value={hosted}
          render={() => ({
            count: value?.machines.length ?? 0,
            subline: stateSummary(value?.machines ?? []),
          })}
        />
        <Tile
          label="Services"
          testid="nodes-hosted-services"
          to="/operator/services"
          value={hosted}
          render={() => ({
            count: value?.services.length ?? 0,
            subline: stateSummary(value?.services ?? []),
          })}
        />
        <Tile
          label="Listeners"
          testid="nodes-hosted-listeners"
          to="/operator/network"
          value={hosted}
          render={() => ({
            count: value?.listeners.length ?? 0,
            subline: adapterSummary(value?.listeners ?? []),
          })}
        />
      </div>
    </section>
  );
}

function Tile<T>({
  label,
  testid,
  to,
  value,
  render,
}: {
  label: string;
  testid: string;
  to: HostedTo;
  value: LoadingValue<T>;
  render: (value: T) => { count: number; subline: string | null };
}) {
  return (
    <Link
      to={to}
      data-testid={testid}
      className="flex min-w-0 flex-col gap-2 rounded-md border border-border-2 bg-bg-panel px-4 py-3 transition-colors hover:border-border-3"
    >
      <span className="text-xs font-medium text-text-3">{label}</span>
      <LoadingCell value={value} testid={testid}>
        {(v) => {
          const { count, subline } = render(v);
          return (
            <span className="flex min-w-0 flex-col gap-1">
              <span
                className="tabular text-2xl leading-none text-text-1"
                data-testid={`${testid}-count`}
              >
                {formatCount(count)}
              </span>
              {/* The subline sits under the count and wraps, so a
                  four-adapter list reads in full at the tile width. */}
              <span
                className="min-h-4 break-words text-xs text-text-3"
                data-testid={`${testid}-subline`}
              >
                {subline ?? "\u00a0"}
              </span>
            </span>
          );
        }}
      </LoadingCell>
    </Link>
  );
}

// ---------------------------------------------------------------------------
// Recent events

export function toEventRow(event: EventDoc, index: number): EventRow {
  return {
    id: typeof event._id === "string" ? event._id : `event-${index}`,
    level: event.level ?? "info",
    source: event.source ?? "—",
    message: event.message ?? event.category ?? "(event)",
    correlationId: event.correlationId ?? null,
    createdAt:
      typeof event.createdAt === "number"
        ? event.createdAt
        : typeof event._creationTime === "number"
          ? event._creationTime
          : null,
  };
}

const eventCol = dataColumns<EventRow>();
const EVENT_COLUMNS = [
  eventCol.accessor("level", {
    header: "Level",
    size: 96,
    cell: (ctx) => (
      <span className="flex items-center gap-2 font-mono text-xs text-text-1">
        <StateDot state={ctx.getValue()} />
        {ctx.getValue()}
      </span>
    ),
  }),
  eventCol.accessor("source", {
    header: "Source",
    size: 112,
    cell: (ctx) => (
      <span className="font-mono text-xs text-text-3">{ctx.getValue()}</span>
    ),
  }),
  eventCol.accessor("message", {
    header: "Message",
    cell: (ctx) => (
      <span className="truncate font-mono text-xs text-text-1">
        {ctx.getValue()}
      </span>
    ),
  }),
  eventCol.accessor("createdAt", {
    header: "When",
    size: 120,
    cell: (ctx) => (
      <span className="block text-right text-xs">
        <RelativeTime epochMs={ctx.getValue()} />
      </span>
    ),
  }),
];

function RecentEvents({ events }: { events: LoadingValue<EventDoc[]> }) {
  const navigate = useNavigate();
  const rows =
    events.kind === "ok"
      ? events.value.slice(0, RECENT_EVENT_ROWS).map(toEventRow)
      : [];
  return (
    <section
      aria-labelledby="nodes-events-title"
      className="flex flex-col gap-2"
      data-testid="nodes-events"
    >
      <div className="flex items-baseline justify-between gap-3">
        <h2 id="nodes-events-title" className="text-sm font-medium text-text-1">
          Recent events
        </h2>
        <Link
          to="/operator/observability"
          search={{ tab: "logs" }}
          className="text-xs text-text-3 hover:text-text-1"
          data-testid="nodes-events-all"
        >
          View all logs
        </Link>
      </div>
      {events.kind === "ok" && rows.length === 0 ? (
        <p
          className="rounded-md border border-border-2 bg-bg-panel px-4 py-3 text-xs text-text-3"
          data-testid="nodes-events-empty"
        >
          No events recorded yet. The node writes one for every start, tenant
          change, and machine action.
        </p>
      ) : events.kind === "ok" ? (
        <DataTable
          columns={EVENT_COLUMNS}
          data={rows}
          getRowId={(row) => row.id}
          ariaLabel="Recent events"
          virtual={false}
          onRowActivate={(row) =>
            navigate({
              to: "/operator/observability",
              search: row.correlationId
                ? { tab: "logs", correlationId: row.correlationId }
                : { tab: "logs" },
            })
          }
          rowTestid={(row) => `nodes-event-row-${row.id}`}
          testid="nodes-events-table"
        />
      ) : (
        <div className="rounded-md border border-border-2 bg-bg-panel px-4 py-3 text-xs text-text-3">
          <LoadingCell value={events} testid="nodes-events">
            {() => null}
          </LoadingCell>
        </div>
      )}
    </section>
  );
}

/**
 * The tenant count is the one tile on this page that does not ride the
 * WebSocket: it is a REST read of `/api/tenants`, so it reports its own
 * outcome rather than being folded through `toLoadingValue`, whose
 * undefined-while-connected branch means "loading" and has no way back out.
 * A failed read used to leave the tile on the loading marker for the life of
 * the page, which is the worst of the three states: it promises the number is
 * still coming.
 *
 * `loadTenantList` rather than `fetchTenants` because it rejects with the
 * error-envelope message instead of collapsing a non-OK response to `null`,
 * and an error state has to be able to say what failed. The tile links to
 * Operator → Tenants, which carries the same failure with a Retry.
 */
function useTenantCount(): LoadingValue<number> {
  const [value, setValue] = useState<LoadingValue<number>>({ kind: "loading" });
  useEffect(() => {
    const controller = new AbortController();
    loadTenantList(controller.signal)
      .then((tenants) => {
        if (controller.signal.aborted) return;
        setValue({ kind: "ok", value: tenants.length });
      })
      .catch((err) => {
        if (controller.signal.aborted) return;
        setValue({
          kind: "error",
          message: err instanceof Error ? err.message : String(err),
        });
      });
    return () => controller.abort();
  }, []);
  return value;
}
