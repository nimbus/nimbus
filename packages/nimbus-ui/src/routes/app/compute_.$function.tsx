import {
  createFileRoute,
  Link,
  useNavigate,
  useSearch,
} from "@tanstack/react-router";
import { useQuery } from "nimbus/react";
import { useMemo } from "react";

import { api } from "../../../convex/_generated/api";
import { Breadcrumb } from "../../components/breadcrumb";
import { CopyChip } from "../../components/copy-chip";
import { FunctionRunner } from "../../components/function-runner/function-runner";
import { StateChip } from "../../components/state-chip";
import { RelativeTime } from "../../components/time";
import { cn } from "../../lib/cn";
import { formatDuration, shortId } from "../../lib/format";
import { buildFunctionTree } from "../../shell/function-tree";
import { FunctionTreeView } from "../../shell/function-tree-view";
import {
  type SubDrawerSpec,
  useContributeSubDrawer,
  useSubDrawerSearch,
} from "../../shell/sub-drawer";

type DetailTab = "statistics" | "source" | "logs" | "runs";

const TABS: Array<{ id: DetailTab; label: string }> = [
  { id: "statistics", label: "Statistics" },
  { id: "source", label: "Source" },
  { id: "logs", label: "Logs" },
  { id: "runs", label: "Runs" },
];

type DetailSearch = {
  tab?: DetailTab;
};

export const Route = createFileRoute("/app/compute_/$function")({
  validateSearch: (search: Record<string, unknown>): DetailSearch => ({
    tab: isTab(search.tab) ? search.tab : undefined,
  }),
  component: FunctionDetailPage,
});

function isTab(value: unknown): value is DetailTab {
  return (
    value === "statistics" ||
    value === "source" ||
    value === "logs" ||
    value === "runs"
  );
}

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

type BundleDoc = {
  _id: string;
  sha256?: string;
  status?: string;
};

type RunDoc = {
  _id: string;
  _creationTime?: number;
  functionPath?: string;
  status?: string;
  durationMs?: number;
  startedAt?: number;
};

type EventDoc = {
  _id: string;
  _creationTime?: number;
  source?: string;
  level?: string;
  category?: string;
  message?: string;
  data?: Record<string, unknown> | null;
  correlationId?: string | null;
  createdAt?: number;
};

function FunctionDetailPage() {
  const { function: functionPath } = Route.useParams();
  const search = useSearch({ from: "/app/compute_/$function" });
  const navigate = useNavigate();
  const tab: DetailTab = search.tab ?? "statistics";

  const functions = useQuery(api.functions.list, {
    bundleId: null,
    kind: null,
    limit: 500,
  }) as FunctionDoc[] | undefined;

  const fn = useMemo<FunctionDoc | null>(() => {
    if (!functions) return null;
    return functions.find((f) => f.path === functionPath) ?? null;
  }, [functions, functionPath]);

  const bundles = useQuery(api.bundles.list, {
    status: null,
    limit: 50,
  }) as BundleDoc[] | undefined;
  const bundle = useMemo<BundleDoc | null>(() => {
    if (!fn?.bundleId || !bundles) return null;
    return bundles.find((b) => b._id === fn.bundleId) ?? null;
  }, [fn, bundles]);

  const spec = useMemo<SubDrawerSpec>(
    () => ({
      kind: "dynamic",
      title: "Functions",
      search: { placeholder: "Filter functions" },
      children: <DetailSubDrawer functions={functions} />,
    }),
    [functions],
  );
  useContributeSubDrawer(spec);

  const setTab = (next: DetailTab) =>
    navigate({
      to: "/app/compute/$function",
      params: { function: functionPath },
      search: { tab: next },
      replace: true,
    });

  return (
    <section
      className="flex h-full flex-col overflow-hidden"
      data-testid="page-function-detail"
    >
      <div className="flex shrink-0 flex-col gap-2 border-b border-app px-6 pb-3 pt-4">
        <Breadcrumb
          segments={[
            { label: "Compute", href: "/app/compute" },
            { label: functionPath, active: true },
          ]}
        />
        <header className="flex flex-wrap items-baseline gap-3">
          <h1
            className="font-mono text-default"
            style={{ fontSize: "var(--text-lg)" }}
          >
            {functionPath}
          </h1>
          {fn?.kind ? (
            <span className="rounded border border-app px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wide text-muted">
              {fn.kind}
            </span>
          ) : null}
          {fn?.adapter ? (
            <span className="rounded border border-app px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wide text-muted">
              {fn.adapter}
            </span>
          ) : null}
          {fn?.lastStatus ? (
            <StateChip state={fn.lastStatus} />
          ) : null}
          {bundle?.sha256 ? (
            <CopyChip
              label="bundle sha256"
              value={bundle.sha256}
              testid="function-detail-bundle"
            >
              {shortId(bundle.sha256, 12)}
            </CopyChip>
          ) : null}
        </header>
      </div>

      <nav
        aria-label="Function detail sections"
        className="flex shrink-0 gap-px border-b border-app bg-surface-2 px-6"
        data-testid="function-detail-tabs"
      >
        {TABS.map((t) => {
          const isActive = tab === t.id;
          return (
            <button
              key={t.id}
              type="button"
              onClick={() => setTab(t.id)}
              aria-current={isActive ? "page" : undefined}
              data-testid={`function-detail-tab-${t.id}`}
              className={cn(
                "flex items-center px-3 py-2 font-mono text-xs uppercase tracking-wide",
                isActive
                  ? "border-b-2 border-[color:var(--color-brand)] text-default"
                  : "text-muted hover:text-default",
              )}
            >
              {t.label}
            </button>
          );
        })}
      </nav>

      <div className="min-h-0 flex-1 overflow-hidden">
        {fn === null && functions === undefined ? (
          <Loading label="Loading function…" />
        ) : fn === null ? (
          <NotFound path={functionPath} />
        ) : (
          <TabBody tab={tab} fn={fn} bundle={bundle} />
        )}
      </div>

      {fn ? <FunctionRunner fn={fn} /> : null}
    </section>
  );
}

function TabBody({
  tab,
  fn,
  bundle,
}: {
  tab: DetailTab;
  fn: FunctionDoc;
  bundle: BundleDoc | null;
}) {
  if (tab === "statistics") return <StatisticsTab fn={fn} bundle={bundle} />;
  if (tab === "source") return <SourceTab fn={fn} />;
  if (tab === "logs") return <LogsTab fn={fn} />;
  return <RunsTab fn={fn} />;
}

function StatisticsTab({
  fn,
  bundle,
}: {
  fn: FunctionDoc;
  bundle: BundleDoc | null;
}) {
  return (
    <div
      className="flex h-full flex-col gap-3 overflow-auto px-6 py-4 text-sm text-default"
      data-testid="function-tab-statistics"
    >
      <Stat label="Kind" value={fn.kind ?? "—"} />
      <Stat label="Adapter" value={fn.adapter ?? "—"} />
      <Stat
        label="Bundle"
        value={
          bundle?.sha256 ? (
            <span className="font-mono">{shortId(bundle.sha256, 16)}</span>
          ) : (
            "—"
          )
        }
      />
      <Stat label="Last status" value={fn.lastStatus ?? "idle"} />
      <Stat
        label="Last run"
        value={
          typeof fn.lastRunAt === "number" ? (
            <RelativeTime epochMs={fn.lastRunAt} />
          ) : (
            "never"
          )
        }
      />
      <div className="rounded border border-app bg-surface-2 px-3 py-3 text-xs text-muted">
        Aggregate latency and invocation telemetry is not yet exposed by the
        system tenant. A follow-up plan will populate this panel with p50/p95/
        p99 latency and success/error rate from the runs index.
      </div>
    </div>
  );
}

function Stat({
  label,
  value,
}: {
  label: string;
  value: React.ReactNode;
}) {
  return (
    <div className="flex items-baseline gap-3">
      <span className="w-32 font-mono text-[10px] uppercase tracking-[0.18em] text-muted">
        {label}
      </span>
      <span className="font-mono text-xs text-default">{value}</span>
    </div>
  );
}

function SourceTab({ fn }: { fn: FunctionDoc }) {
  if (!fn.source || fn.source.length === 0) {
    return (
      <Empty
        title="Source not bundled"
        detail="This function's bundle does not include source. Re-bundle with source included to view it here."
      />
    );
  }
  const lines = fn.source.split("\n");
  const width = String(lines.length).length;
  return (
    <div
      className="h-full overflow-auto"
      data-testid="function-tab-source"
    >
      <pre className="m-0 min-w-full bg-surface p-0 font-mono text-[12px] leading-5 text-default">
        <table className="w-full border-collapse">
          <tbody>
            {lines.map((line, idx) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: stable per source line
              <tr key={idx} className="hover:bg-surface-2">
                <td
                  className="select-none border-r border-app bg-surface-2 px-3 py-0 text-right text-muted"
                  style={{ width: `${width + 2}ch` }}
                >
                  {idx + 1}
                </td>
                <td className="whitespace-pre px-3 py-0">{line}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </pre>
    </div>
  );
}

function LogsTab({ fn }: { fn: FunctionDoc }) {
  const events = useQuery(api.events.recent, {
    source: null,
    level: null,
    category: null,
    correlationId: null,
    limit: 200,
  }) as EventDoc[] | undefined;

  const filtered = useMemo(() => {
    if (!events || !fn.path) return events ?? [];
    return events.filter((ev) => {
      const data = ev.data;
      if (!data || typeof data !== "object") return false;
      const path = (data as Record<string, unknown>).functionPath;
      return path === fn.path;
    });
  }, [events, fn.path]);

  if (events === undefined) return <Loading label="Loading logs…" />;
  if (filtered.length === 0) {
    return (
      <Empty
        title="No logs for this function"
        detail="The Observability page hosts the full cross-function log feed. Run this function to populate its log stream."
      />
    );
  }
  return (
    <div
      className="h-full overflow-auto px-6 py-4 text-sm"
      data-testid="function-tab-logs"
    >
      <ul className="flex flex-col gap-1">
        {filtered.map((ev) => (
          <li
            key={ev._id}
            className="rounded border border-app bg-surface-2 px-3 py-2 font-mono text-xs"
          >
            <div className="flex items-baseline gap-3">
              <span className="text-[10px] uppercase tracking-wide text-muted">
                {ev.level ?? "info"}
              </span>
              <span className="text-default">{ev.message ?? ""}</span>
              {typeof ev.createdAt === "number" ? (
                <span className="ml-auto text-muted">
                  <RelativeTime epochMs={ev.createdAt} />
                </span>
              ) : null}
            </div>
          </li>
        ))}
      </ul>
    </div>
  );
}

function RunsTab({ fn }: { fn: FunctionDoc }) {
  const runs = useQuery(api.runs.recent, {
    bundleId: null,
    functionPath: fn.path ?? null,
    status: null,
    limit: 50,
  }) as RunDoc[] | undefined;
  if (runs === undefined) return <Loading label="Loading runs…" />;
  if (runs.length === 0) {
    return (
      <Empty
        title="No runs yet"
        detail="Once this function has been invoked, recent runs appear here. Click a run to open its detail page."
      />
    );
  }
  return (
    <div
      className="h-full overflow-auto px-6 py-4"
      data-testid="function-tab-runs"
    >
      <table className="w-full border-collapse text-sm">
        <thead className="text-[10px] uppercase tracking-[0.14em] text-muted">
          <tr>
            <th className="border-b border-app px-3 py-2 text-left font-normal">
              Run ID
            </th>
            <th className="border-b border-app px-3 py-2 text-left font-normal">
              Status
            </th>
            <th className="border-b border-app px-3 py-2 text-left font-normal">
              Duration
            </th>
            <th className="border-b border-app px-3 py-2 text-left font-normal">
              Started
            </th>
          </tr>
        </thead>
        <tbody>
          {runs.map((run) => (
            <tr key={run._id} className="border-t border-app hover:bg-surface-2">
              <td className="px-3 py-2">
                <Link
                  to="/app/compute/runs/$runId"
                  params={{ runId: run._id }}
                  className="font-mono text-xs text-default hover:underline"
                  data-testid={`function-tab-runs-link-${run._id}`}
                >
                  {shortId(run._id, 12)}
                </Link>
              </td>
              <td className="px-3 py-2">
                <StateChip state={run.status} />
              </td>
              <td className="px-3 py-2">
                {typeof run.durationMs === "number" ? (
                  <span className="tabular font-mono text-xs">
                    {formatDuration(run.durationMs)}
                  </span>
                ) : (
                  <span className="tabular text-muted">—</span>
                )}
              </td>
              <td className="px-3 py-2">
                {typeof run.startedAt === "number" ? (
                  <RelativeTime epochMs={run.startedAt} />
                ) : (
                  <span className="tabular text-muted">—</span>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function DetailSubDrawer({
  functions,
}: {
  functions: FunctionDoc[] | undefined;
}) {
  const filter = useSubDrawerSearch();
  const tree = useMemo(() => buildFunctionTree(functions ?? []), [functions]);
  if (functions === undefined) {
    return (
      <div className="px-3 py-3 text-xs text-muted">
        <span aria-hidden>·</span>
        <span className="sr-only">loading</span>
      </div>
    );
  }
  return (
    <FunctionTreeView tree={tree} filter={filter} testidPrefix="sub-drawer" />
  );
}

function Loading({ label }: { label: string }) {
  return (
    <div className="flex h-full items-center justify-center text-xs text-muted">
      {label}
    </div>
  );
}

function NotFound({ path }: { path: string }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-2 text-center">
      <span className="font-mono text-sm text-default">Function not found</span>
      <span className="max-w-md text-xs text-muted">
        No function matches the path{" "}
        <code className="font-mono text-default">{path}</code>. It may have
        been removed or renamed. Open Compute to see the current inventory.
      </span>
      <Link
        to="/app/compute"
        className="rounded border border-app px-3 py-1 font-mono text-[11px] uppercase tracking-wide text-muted hover:bg-surface hover:text-default"
      >
        ← back to compute
      </Link>
    </div>
  );
}

function Empty({ title, detail }: { title: string; detail: string }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-1 text-center">
      <span className="font-mono text-sm text-default">{title}</span>
      <span className="max-w-md text-xs text-muted">{detail}</span>
    </div>
  );
}
