import { useNimbusConnectionState, useQuery } from "@nimbus/nimbus/react";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { useMemo, useState } from "react";

import { api } from "../../../convex/_generated/api";
import { CopyButton } from "../../components/copy-button";
import { CopyChip } from "../../components/copy-chip";
import { DataTable, dataColumns } from "../../components/data-table";
import { LoadingCell } from "../../components/loading-cell";
import { Mascot, type MascotState } from "../../components/mascot";
import {
  FirstRun,
  firstRunComplete,
} from "../../components/onboarding/first-run";
import { StatePill } from "../../components/pill";
import { resolveStateKind } from "../../components/state-dot";
import { RelativeTime } from "../../components/time";
import { CHART, type ChartPoint, Sparkline } from "../../components/ui/chart";
import { Tabs, TabsList, TabsTrigger } from "../../components/ui/tabs";
import { useServerUrl } from "../../hooks/use-server-url";
import { formatCount, formatDuration } from "../../lib/format";
import { cn } from "../../lib/utils";
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

export type RunRow = {
  id: string;
  status: string;
  functionPath: string;
  durationMs: number | undefined;
  startedAt: number | null;
};

const DAY_MS = 24 * 60 * 60 * 1000;
const HOUR_MS = 60 * 60 * 1000;
const RECENT_RUN_ROWS = 5;

// The page answers one question first, "is my server fine", then the three
// things a developer comes here for: how to talk to it, what is on it, and
// what it did last. Anything without a value at that moment is not shown;
// a stat only gets a tile when the server has reported it.
function OverviewPage() {
  const conn = useConnSnapshot();
  const activeTenant = useUiStore((s) => s.activeTenant);
  const serverUrl = useServerUrl();
  const status = useQuery(api.system.status, {}) as SystemStatusDoc | undefined;
  const functions = useQuery(api.functions.list, {
    bundleId: null,
    kind: null,
    limit: 200,
  }) as AnyDoc[] | undefined;
  const tables = useQuery(api.tables.list, {
    tenantId: activeTenant ?? null,
    limit: 200,
  }) as AnyDoc[] | undefined;
  const runs = useQuery(api.runs.recent, {
    bundleId: null,
    functionPath: null,
    status: null,
    tenantId: null,
    limit: 200,
  }) as AnyDoc[] | undefined;

  const statusValue = toStatusValue(status, conn);
  const inventory = toInventory(functions, tables, runs, conn);
  const firstRun =
    inventory.kind === "ok" &&
    inventory.value.tables.length === 0 &&
    !firstRunComplete({
      functions: inventory.value.functions.length,
      runs: inventory.value.runs.length,
    });

  return (
    <section
      className="flex h-full flex-col gap-6 overflow-y-auto px-6 py-5"
      data-testid="page-overview"
    >
      <Headline
        status={statusValue}
        inventory={inventory}
        firstRun={firstRun}
        tenant={activeTenant}
        serverUrl={serverUrl}
      />

      {inventory.kind === "ok" && firstRun ? (
        <FirstRun
          progress={{
            functions: inventory.value.functions.length,
            runs: inventory.value.runs.length,
          }}
          testid="overview-onboarding"
        />
      ) : null}

      <ConnectPanel
        serverUrl={serverUrl}
        tenant={activeTenant}
        functionPath={firstFunctionPath(functions)}
        table={firstTableName(tables)}
      />

      {!firstRun ? (
        <>
          <StatsRow inventory={inventory} tenant={activeTenant} />
          <RecentRuns runs={inventory} />
        </>
      ) : null}
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

type SystemStatus = NonNullable<SystemStatusDoc>;

/**
 * `useQuery` resolves to `null` when the server has no status row. That is an
 * answer, so it must not go through `toLoadingValue`, whose null-or-undefined
 * branch means "loading" — the headline would then sit on the working face
 * for the life of the page on a deployment that simply has no status document.
 */
function toStatusValue(
  status: SystemStatusDoc | undefined,
  conn: ConnectionSnapshot,
): LoadingValue<SystemStatus> {
  if (status === null) return { kind: "ok", value: {} };
  return toLoadingValue(status, conn);
}

type Inventory = {
  functions: ReadonlyArray<AnyDoc>;
  tables: ReadonlyArray<AnyDoc>;
  runs: ReadonlyArray<RunRow>;
};

// The three lists load as one value so the page changes shape once, not
// three times: the onboarding decision needs all of them, and a stats row
// that appears tile by tile reads as broken rather than loading.
function toInventory(
  functions: AnyDoc[] | undefined,
  tables: AnyDoc[] | undefined,
  runs: AnyDoc[] | undefined,
  conn: ConnectionSnapshot,
): LoadingValue<Inventory> {
  const f = toLoadingValue(functions, conn);
  const t = toLoadingValue(tables, conn);
  const r = toLoadingValue(runs, conn);
  for (const v of [f, t, r]) {
    if (v.kind !== "ok") return v as LoadingValue<Inventory>;
  }
  if (f.kind !== "ok" || t.kind !== "ok" || r.kind !== "ok") {
    return { kind: "loading" };
  }
  return {
    kind: "ok",
    value: {
      functions: f.value,
      tables: t.value,
      runs: r.value.map(toRunRow),
    },
  };
}

function toRunRow(run: AnyDoc, index: number): RunRow {
  return {
    id: typeof run._id === "string" ? run._id : `run-${index}`,
    status: typeof run.status === "string" ? run.status : "unknown",
    functionPath: typeof run.functionPath === "string" ? run.functionPath : "—",
    durationMs: typeof run.durationMs === "number" ? run.durationMs : undefined,
    startedAt: typeof run.startedAt === "number" ? run.startedAt : null,
  };
}

function isFailedRun(run: RunRow): boolean {
  const kind = resolveStateKind(run.status);
  return kind === "error" || kind === "failed";
}

function firstFunctionPath(functions: AnyDoc[] | undefined): string | null {
  for (const fn of functions ?? []) {
    if (typeof fn.path === "string" && fn.path.length > 0) return fn.path;
  }
  return null;
}

function firstTableName(tables: AnyDoc[] | undefined): string | null {
  for (const table of tables ?? []) {
    if (typeof table.name === "string" && table.name.length > 0) {
      return table.name;
    }
  }
  return null;
}

// ---------------------------------------------------------------------------
// Headline

type HeadlineReading = {
  mascot: MascotState;
  sentence: string;
};

// readHeadline turns the status document and the run history into the one
// sentence at the top of the page. The order matters: a lost connection
// beats a healthy status document, because the document is stale.
export function readHeadline(
  status: LoadingValue<SystemStatus>,
  inventory: LoadingValue<Inventory>,
  firstRun: boolean,
  now = Date.now(),
): HeadlineReading {
  if (status.kind === "offline" || inventory.kind === "offline") {
    return {
      mascot: "error",
      sentence: "The connection to the server dropped. Stale data is shown.",
    };
  }
  if (status.kind === "error") {
    return { mascot: "error", sentence: status.message };
  }
  if (inventory.kind === "error") {
    return { mascot: "error", sentence: inventory.message };
  }
  if (status.kind === "loading" || inventory.kind === "loading") {
    return { mascot: "working", sentence: "Reading the server status." };
  }
  const health = status.value.health;
  const healthKind = resolveStateKind(health);
  if (
    health &&
    healthKind !== "ok" &&
    healthKind !== "healthy" &&
    healthKind !== "ready" &&
    healthKind !== "running"
  ) {
    return {
      mascot: "error",
      sentence: `The server reports its health as ${health}.`,
    };
  }
  if (firstRun) {
    return {
      mascot: "empty",
      sentence: "The server is up and waiting for its first app.",
    };
  }
  const failed = inventory.value.runs.filter(
    (run) =>
      isFailedRun(run) &&
      run.startedAt !== null &&
      now - run.startedAt <= DAY_MS,
  ).length;
  if (failed > 0) {
    return {
      mascot: "error",
      sentence: `The server is up. ${formatCount(failed)} ${failed === 1 ? "run" : "runs"} failed in the last 24 hours.`,
    };
  }
  return {
    mascot: "idle",
    sentence: "The server is up and every recent run succeeded.",
  };
}

function Headline({
  status,
  inventory,
  firstRun,
  tenant,
  serverUrl,
}: {
  status: LoadingValue<SystemStatus>;
  inventory: LoadingValue<Inventory>;
  firstRun: boolean;
  tenant: string | null;
  serverUrl: string;
}) {
  const reading = readHeadline(status, inventory, firstRun);
  const version = status.kind === "ok" ? status.value.version : undefined;
  return (
    <header className="flex flex-col gap-2" data-testid="overview-headline">
      <div className="flex items-center gap-3">
        <Mascot
          size={40}
          state={reading.mascot}
          variant="outline"
          decorative
          className="shrink-0 text-text-1"
          data-testid="overview-mascot"
          data-state={reading.mascot}
        />
        <h1
          className="text-xl text-text-1"
          style={{ fontSize: "var(--text-xl)" }}
          data-testid="overview-sentence"
        >
          {reading.sentence}
        </h1>
      </div>
      {/* One mono line of facts. A fact the server has not reported is left
          out rather than shown as a dash, so the line never lists what it
          does not know. */}
      <p
        className="flex flex-wrap items-center gap-x-2 gap-y-1 font-mono text-xs text-text-3"
        data-testid="overview-facts"
      >
        {tenant ? (
          <span data-testid="overview-fact-tenant">
            tenant <span className="text-text-2">{tenant}</span>
          </span>
        ) : null}
        {tenant && serverUrl ? <Separator /> : null}
        {serverUrl ? (
          <CopyChip
            label="server URL"
            value={serverUrl}
            testid="overview-fact-endpoint"
            className="text-text-2"
          />
        ) : null}
        <LoadingCell value={status} testid="overview-fact-version">
          {() =>
            version ? (
              <>
                <Separator />
                <CopyChip
                  label="version"
                  value={version}
                  testid="overview-fact-version"
                  className="text-text-2"
                >
                  v{version}
                </CopyChip>
              </>
            ) : null
          }
        </LoadingCell>
      </p>
    </header>
  );
}

function Separator() {
  return (
    <span aria-hidden className="text-border-2">
      ·
    </span>
  );
}

// ---------------------------------------------------------------------------
// Connect

type SnippetId = "curl" | "sdk" | "convex";

const SNIPPET_TABS: ReadonlyArray<{ id: SnippetId; label: string }> = [
  { id: "curl", label: "curl" },
  { id: "sdk", label: "TypeScript SDK" },
  { id: "convex", label: "Convex client" },
];

// connectSnippets writes the three ways into this server, addressed to this
// tenant, and using the first function and table the server actually has
// so the snippet runs as pasted. Without either it falls back to the names
// from the quick start.
export function connectSnippets({
  serverUrl,
  tenant,
  functionPath,
  table,
}: {
  serverUrl: string;
  tenant: string | null;
  functionPath: string | null;
  table: string | null;
}): Record<SnippetId, string> {
  const t = tenant ?? "demo";
  const url = serverUrl || "http://localhost:3210";
  const tableName = table ?? "messages";
  const apiRef = (functionPath ?? "messages:list").replace(/[:/]/g, ".");
  return {
    curl: [
      `curl -s -X POST ${url}/api/tenants/${t}/query \\`,
      `  -H "Authorization: Bearer $NIMBUS_TOKEN" \\`,
      `  -H "Content-Type: application/json" \\`,
      `  -d '{"table": "${tableName}", "filters": []}'`,
    ].join("\n"),
    sdk: [
      `import { NimbusClient } from "@nimbus/nimbus/browser";`,
      `import { api } from "./nimbus/_generated/api";`,
      ``,
      `const client = new NimbusClient("${url}/convex/${t}");`,
      `const rows = await client.query(api.${apiRef}, {});`,
    ].join("\n"),
    convex: [
      `import { ConvexReactClient } from "convex/react";`,
      ``,
      `const convex = new ConvexReactClient("${url}/convex/${t}");`,
      `// <ConvexProvider client={convex}>…</ConvexProvider>`,
    ].join("\n"),
  };
}

function ConnectPanel({
  serverUrl,
  tenant,
  functionPath,
  table,
}: {
  serverUrl: string;
  tenant: string | null;
  functionPath: string | null;
  table: string | null;
}) {
  const [active, setActive] = useState<SnippetId>("curl");
  const snippets = useMemo(
    () => connectSnippets({ serverUrl, tenant, functionPath, table }),
    [serverUrl, tenant, functionPath, table],
  );
  const snippet = snippets[active];
  return (
    <section
      aria-labelledby="overview-connect-title"
      className="flex flex-col gap-3"
      data-testid="overview-connect"
    >
      <PanelHeading id="overview-connect-title" title="Connect">
        {serverUrl ? (
          <CopyChip
            label="server URL"
            value={serverUrl}
            testid="overview-connect-url"
            className="font-mono text-xs text-text-2"
          />
        ) : null}
      </PanelHeading>
      <Tabs
        value={active}
        onValueChange={(value) => setActive(value as SnippetId)}
        className="gap-2"
      >
        <div className="flex items-center justify-between gap-3">
          <TabsList variant="line" data-testid="overview-connect-tabs">
            {SNIPPET_TABS.map((tab) => (
              <TabsTrigger
                key={tab.id}
                value={tab.id}
                data-testid={`overview-connect-tab-${tab.id}`}
              >
                {tab.label}
              </TabsTrigger>
            ))}
          </TabsList>
          <CopyButton
            text={snippet}
            label="snippet"
            testid="overview-connect-copy"
          />
        </div>
        {/* The copy control sits beside the tabs, not over the snippet:
            an overlay would cover the end of a long line once the block
            scrolls sideways on a narrow viewport. */}
        <pre
          className="overflow-x-auto rounded-md border border-border-2 bg-bg-panel px-4 py-3 font-mono text-xs leading-relaxed text-text-1"
          data-testid="overview-connect-snippet"
          data-snippet={active}
        >
          <code>{snippet}</code>
        </pre>
      </Tabs>
    </section>
  );
}

function PanelHeading({
  id,
  title,
  children,
}: {
  id: string;
  title: string;
  children?: React.ReactNode;
}) {
  return (
    <div className="flex items-baseline justify-between gap-3">
      <h2 id={id} className="text-sm font-medium text-text-1">
        {title}
      </h2>
      {children}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Stats

type Stat = {
  id: "functions" | "tables" | "runs" | "errors";
  label: string;
  value: number;
  subline: string | null;
  series: ReadonlyArray<ChartPoint> | null;
  color: string;
  to: string;
  search?: Record<string, string>;
};

// hourlyBuckets counts runs per hour over the last day, oldest first, so a
// sparkline reads left to right in time. Every hour is present, including
// empty ones, so a quiet hour is a dip and not a missing point.
export function hourlyBuckets(
  runs: ReadonlyArray<RunRow>,
  predicate: (run: RunRow) => boolean,
  now = Date.now(),
): ChartPoint[] {
  const points: ChartPoint[] = [];
  const start = now - DAY_MS;
  for (let i = 0; i < 24; i += 1) {
    points.push({ label: String(i), value: 0 });
  }
  for (const run of runs) {
    if (run.startedAt === null || !predicate(run)) continue;
    const age = run.startedAt - start;
    if (age < 0 || age > DAY_MS) continue;
    const index = Math.min(23, Math.floor(age / HOUR_MS));
    points[index].value += 1;
  }
  return points;
}

export function readStats(
  inventory: Inventory,
  tenant: string | null,
  now = Date.now(),
): Stat[] {
  const recent = inventory.runs.filter(
    (run) => run.startedAt !== null && now - run.startedAt <= DAY_MS,
  );
  const failed = recent.filter(isFailedRun);
  const kinds = new Map<string, number>();
  for (const fn of inventory.functions) {
    const kind = typeof fn.kind === "string" ? fn.kind : "other";
    kinds.set(kind, (kinds.get(kind) ?? 0) + 1);
  }
  const kindSummary = [...kinds.entries()]
    .sort((a, b) => b[1] - a[1])
    .slice(0, 3)
    .map(([kind, count]) => `${count} ${kind}`)
    .join(" · ");
  const stats: Stat[] = [
    {
      id: "functions",
      label: "Functions",
      value: inventory.functions.length,
      subline: kindSummary || null,
      series: null,
      color: CHART.neutral,
      to: "/developer/compute",
    },
    {
      id: "tables",
      label: "Tables",
      value: inventory.tables.length,
      subline: tenant ? `in ${tenant}` : null,
      series: null,
      color: CHART.neutral,
      to: "/developer/storage",
    },
    {
      id: "runs",
      label: "Runs, 24h",
      value: recent.length,
      subline: null,
      series: hourlyBuckets(inventory.runs, () => true, now),
      color: CHART.neutral,
      to: "/developer/observability",
      search: { tab: "runs" },
    },
    {
      id: "errors",
      label: "Errors, 24h",
      value: failed.length,
      subline: null,
      series: hourlyBuckets(inventory.runs, isFailedRun, now),
      color: failed.length > 0 ? CHART.error : CHART.neutral,
      to: "/developer/observability",
      search: { tab: "runs", status: "error" },
    },
  ];
  // A stat with nothing behind it is not shown. Runs and errors stay once
  // the server has ever run something, because zero in the last day is then
  // a reading; before that, the onboarding panel owns the page.
  return stats.filter((stat) => {
    if (stat.id === "runs" || stat.id === "errors") {
      return inventory.runs.length > 0;
    }
    return stat.value > 0;
  });
}

function StatsRow({
  inventory,
  tenant,
}: {
  inventory: LoadingValue<Inventory>;
  tenant: string | null;
}) {
  const stats =
    inventory.kind === "ok" ? readStats(inventory.value, tenant) : [];
  if (inventory.kind === "ok" && stats.length === 0) return null;
  return (
    <section
      aria-label="Stats"
      className="grid shrink-0 grid-cols-2 gap-3 lg:grid-cols-4"
      data-testid="overview-stats"
    >
      {inventory.kind === "ok" ? (
        stats.map((stat) => <StatTile key={stat.id} stat={stat} />)
      ) : (
        <div
          className="col-span-full rounded-md border border-border-2 bg-bg-panel px-4 py-3 text-xs text-text-3"
          data-testid="overview-stats-pending"
        >
          <LoadingCell value={inventory} testid="overview-stats">
            {() => null}
          </LoadingCell>
        </div>
      )}
    </section>
  );
}

function StatTile({ stat }: { stat: Stat }) {
  return (
    <Link
      to={stat.to}
      search={stat.search}
      className="group flex min-w-0 flex-col gap-2 rounded-md border border-border-2 bg-bg-panel px-4 py-3 transition-colors hover:border-border-3"
      data-testid={`overview-stat-${stat.id}`}
    >
      <span className="flex items-baseline justify-between gap-2">
        <span className="text-xs font-medium text-text-3">{stat.label}</span>
        {stat.subline ? (
          <span className="truncate text-xs text-text-3">{stat.subline}</span>
        ) : null}
      </span>
      <span className="flex items-end justify-between gap-3">
        <span
          className={cn(
            "tabular text-2xl leading-none text-text-1",
            stat.id === "errors" && stat.value > 0 && "text-error",
          )}
          data-testid={`overview-stat-${stat.id}-value`}
        >
          {formatCount(stat.value)}
        </span>
        {stat.series ? (
          <Sparkline
            data={stat.series}
            color={stat.color}
            height={28}
            ariaLabel={`${stat.label} by hour`}
            className="max-w-32"
          />
        ) : null}
      </span>
    </Link>
  );
}

// ---------------------------------------------------------------------------
// Recent runs

const runCol = dataColumns<RunRow>();
const RUN_COLUMNS = [
  runCol.accessor("status", {
    header: "Status",
    size: 96,
    cell: (ctx) => <StatePill state={ctx.getValue()} />,
  }),
  runCol.accessor("functionPath", {
    header: "Function",
    cell: (ctx) => (
      <span className="truncate font-mono text-xs text-text-1">
        {ctx.getValue()}
      </span>
    ),
  }),
  runCol.accessor("durationMs", {
    header: "Duration",
    size: 96,
    cell: (ctx) => (
      <span className="block text-right font-mono text-xs tabular text-text-3">
        {formatDuration(ctx.getValue())}
      </span>
    ),
  }),
  runCol.accessor("startedAt", {
    header: "Started",
    size: 120,
    cell: (ctx) => {
      const startedAt = ctx.getValue();
      return (
        <span className="block text-right text-xs">
          {startedAt ? <RelativeTime epochMs={startedAt} /> : null}
        </span>
      );
    },
  }),
];

function RecentRuns({ runs }: { runs: LoadingValue<Inventory> }) {
  const navigate = useNavigate();
  const rows =
    runs.kind === "ok" ? runs.value.runs.slice(0, RECENT_RUN_ROWS) : [];
  if (runs.kind === "ok" && runs.value.runs.length === 0) return null;
  return (
    <section
      aria-labelledby="overview-runs-title"
      className="flex flex-col gap-3"
      data-testid="overview-runs"
    >
      <PanelHeading id="overview-runs-title" title="Recent runs">
        <Link
          to="/developer/observability"
          search={{ tab: "runs" }}
          className="text-xs text-text-3 hover:text-text-1"
          data-testid="overview-runs-all"
        >
          View all runs
        </Link>
      </PanelHeading>
      {runs.kind === "ok" ? (
        <DataTable
          columns={RUN_COLUMNS}
          data={rows}
          getRowId={(row) => row.id}
          ariaLabel="Recent runs"
          virtual={false}
          onRowActivate={(row) =>
            navigate({
              to: "/developer/compute/runs/$runId",
              params: { runId: row.id },
            })
          }
          testid="overview-runs-table"
        />
      ) : (
        <div className="rounded-md border border-border-2 bg-bg-panel px-4 py-3 text-xs text-text-3">
          <LoadingCell value={runs} testid="overview-runs">
            {() => null}
          </LoadingCell>
        </div>
      )}
    </section>
  );
}
