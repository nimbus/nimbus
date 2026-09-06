import { useQuery } from "@nimbus/nimbus/react";
import {
  createFileRoute,
  Link,
  useNavigate,
  useSearch,
} from "@tanstack/react-router";
import { useMemo } from "react";

import { api } from "../../../convex/_generated/api";
import { Breadcrumb } from "../../components/breadcrumb";
import { CategoryChip } from "../../components/category-chip";
import { CodeBlock } from "../../components/code-block";
import { CopyChip } from "../../components/copy-chip";
import { Td, Th } from "../../components/data-table";
import { EmptyState } from "../../components/empty-state";
import { FunctionRunner } from "../../components/function-runner/function-runner";
import { LoadingState, SkeletonRows } from "../../components/loading-state";
import { StateChip } from "../../components/state-chip";
import { RelativeTime } from "../../components/time";
import { useApiRead } from "../../hooks/use-api-read";
import { cn } from "../../lib/cn";
import { formatDuration, shortHash, shortId } from "../../lib/format";
import type { FunctionDoc } from "../../lib/types/function";
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
  // 1-based source line to highlight + scroll to in the Source tab (e.g. when
  // arriving from a failed run's error location).
  line?: number;
};

export const Route = createFileRoute("/developer/compute_/$function")({
  validateSearch: (search: Record<string, unknown>): DetailSearch => ({
    tab: isTab(search.tab) ? search.tab : undefined,
    line:
      typeof search.line === "number" && Number.isFinite(search.line)
        ? search.line
        : typeof search.line === "string" && /^\d+$/.test(search.line)
          ? Number.parseInt(search.line, 10)
          : undefined,
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
  const search = useSearch({ from: "/developer/compute_/$function" });
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
      to: "/developer/compute/$function",
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
            { label: "Compute", href: "/developer/compute" },
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
          {fn?.kind ? <CategoryChip value={fn.kind} /> : null}
          {fn?.adapter ? <CategoryChip value={fn.adapter} /> : null}
          {fn?.lastStatus ? <StateChip state={fn.lastStatus} /> : null}
          {bundle?.sha256 ? (
            <CopyChip
              label="bundle sha256"
              value={bundle.sha256}
              testid="function-detail-bundle"
            >
              {shortHash(bundle.sha256, 12)}
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
                  ? "border-b-2 border-[color:var(--nimbus-brand)] text-default"
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
          <LoadingState label="Loading function…" />
        ) : fn === null ? (
          <NotFound path={functionPath} />
        ) : (
          <TabBody tab={tab} fn={fn} bundle={bundle} line={search.line} />
        )}
      </div>

      {fn ? <FunctionRunner key={fn._id} fn={fn} /> : null}
    </section>
  );
}

function TabBody({
  tab,
  fn,
  bundle,
  line,
}: {
  tab: DetailTab;
  fn: FunctionDoc;
  bundle: BundleDoc | null;
  line?: number;
}) {
  if (tab === "statistics") return <StatisticsTab fn={fn} bundle={bundle} />;
  if (tab === "source") return <SourceTab fn={fn} highlightLine={line} />;
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
            <span className="font-mono">{shortHash(bundle.sha256, 16)}</span>
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

function Stat({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex items-baseline gap-3">
      <span className="w-32 font-mono text-xs uppercase tracking-[0.18em] text-muted">
        {label}
      </span>
      <span className="font-mono text-xs text-default">{value}</span>
    </div>
  );
}

// The Source tab reads the deployed module source from the content-addressed
// source-package store via the console source endpoint (FSV4) — never a copy
// stored on the function row. A function path `module:export` resolves to its
// module file, whose source backs every function defined in it.
type ModuleAnalysis = {
  exports: Array<{ name: string; line: number }>;
  imports: Array<{ specifier: string; name: string }>;
  references: Array<{ target: string; line: number }>;
};

type TypeHint = { name: string; line: number; col: number; hover: string };

type CalledByEdge = { target: string; caller: string };

// The module source once fetched: either present (with its analysis) or a
// typed "missing" variant for a 404 — a mapped value over `LoadingValue`, so
// the Source tab keeps the one loading vocabulary without a fourth state union.
type SourceReady = {
  source: string;
  digest: string;
  analysis: ModuleAnalysis | null;
  calledBy: CalledByEdge[] | null;
  typeInfo: TypeHint[] | null;
};
type SourceValue =
  | { kind: "present"; ready: SourceReady }
  | { kind: "missing" };

type RawSource = {
  source?: string;
  digest?: string;
  analysis?: ModuleAnalysis;
  called_by?: CalledByEdge[];
  type_info?: TypeHint[];
};

function SourceTab({
  fn,
  highlightLine,
}: {
  fn: FunctionDoc;
  highlightLine?: number;
}) {
  const modulePath = useMemo(() => {
    const path = fn.path ?? "";
    const separator = path.indexOf(":");
    return separator >= 0 ? path.slice(0, separator) : path;
  }, [fn.path]);

  const fetched = useApiRead<SourceValue, RawSource>(
    `/api/console/source?module=${encodeURIComponent(modulePath)}`,
    (result) => {
      if (result.ok) {
        return {
          kind: "ok",
          value: {
            kind: "present",
            ready: {
              source: result.data.source ?? "",
              digest: result.data.digest ?? "",
              analysis: result.data.analysis ?? null,
              calledBy: result.data.called_by ?? null,
              typeInfo: result.data.type_info ?? null,
            },
          },
        };
      }
      if (result.status === 404) {
        return { kind: "ok", value: { kind: "missing" } };
      }
      return { kind: "error", message: result.error };
    },
  );

  // A function with no resolvable module path is "missing" outright — surface it
  // immediately rather than waiting on a read for an empty module.
  const state = modulePath
    ? fetched
    : ({ kind: "ok", value: { kind: "missing" } } as const);

  if (state.kind === "loading") return <LoadingState label="Loading source…" />;
  if (state.kind === "error" || state.kind === "offline") {
    return (
      <EmptyState
        title="Could not load source"
        body={`The source endpoint returned an error (${
          state.kind === "error" ? state.message : "offline"
        }).`}
      />
    );
  }
  if (state.value.kind === "missing") {
    return (
      <EmptyState
        title="Source not available"
        body="This deployment did not capture source for this module. Deploy with the Nimbus CLI to make source viewable here."
      />
    );
  }
  const ready = state.value.ready;
  return (
    <div
      className="flex h-full flex-col overflow-hidden"
      data-testid="function-tab-source"
    >
      <div className="flex shrink-0 items-center gap-2 border-b border-app bg-surface-2 px-6 py-1.5 font-mono text-xs uppercase tracking-wide text-muted">
        <span>{modulePath}</span>
        {ready.digest ? (
          <span className="ml-auto normal-case" title={ready.digest}>
            source package {ready.digest.slice(0, 12)}…
          </span>
        ) : null}
      </div>
      {ready.analysis ? (
        <SymbolsBar
          modulePath={modulePath}
          analysis={ready.analysis}
          calledBy={ready.calledBy}
          typeInfo={ready.typeInfo}
        />
      ) : null}
      <div className="min-h-0 flex-1 overflow-hidden">
        <CodeBlock
          code={ready.source}
          lang="typescript"
          hints={ready.typeInfo ?? undefined}
          highlightLine={highlightLine}
        />
      </div>
    </div>
  );
}

// Navigable code-intelligence strip (oxc structural index, FSV7): functions
// defined in this module and the functions it calls, each a link.
function SymbolsBar({
  modulePath,
  analysis,
  calledBy,
  typeInfo,
}: {
  modulePath: string;
  analysis: ModuleAnalysis;
  calledBy: CalledByEdge[] | null;
  typeInfo: TypeHint[] | null;
}) {
  // Unique caller paths (which functions elsewhere call into this module).
  const callers = Array.from(
    new Set((calledBy ?? []).map((edge) => edge.caller)),
  ).sort();
  if (
    analysis.exports.length === 0 &&
    analysis.references.length === 0 &&
    callers.length === 0
  ) {
    return null;
  }
  return (
    <div
      className="flex shrink-0 flex-wrap items-center gap-x-4 gap-y-1 border-b border-app bg-surface px-6 py-2"
      data-testid="function-source-symbols"
    >
      {analysis.exports.length > 0 ? (
        <div className="flex flex-wrap items-center gap-1.5">
          <span className="font-mono text-xs uppercase tracking-wide text-muted">
            defines
          </span>
          {analysis.exports.map((symbol) => {
            // The TS-compiler hover for this export's declaration (FSV8),
            // shown as the chip's native tooltip.
            const hint = typeInfo?.find(
              (h) => h.name === symbol.name && h.line === symbol.line,
            );
            return (
              <SymbolLink
                key={symbol.name}
                path={`${modulePath}:${symbol.name}`}
                label={symbol.name}
                title={hint?.hover}
                testid={`function-source-define-${symbol.name}`}
              />
            );
          })}
        </div>
      ) : null}
      {analysis.references.length > 0 ? (
        <div className="flex flex-wrap items-center gap-1.5">
          <span className="font-mono text-xs uppercase tracking-wide text-muted">
            calls
          </span>
          {analysis.references.map((reference) => (
            <SymbolLink
              key={reference.target}
              path={reference.target}
              label={reference.target}
              testid={`function-source-call-${reference.target}`}
            />
          ))}
        </div>
      ) : null}
      {callers.length > 0 ? (
        <div className="flex flex-wrap items-center gap-1.5">
          <span className="font-mono text-xs uppercase tracking-wide text-muted">
            called by
          </span>
          {callers.map((caller) => (
            <SymbolLink
              key={caller}
              path={caller}
              label={caller}
              testid={`function-source-calledby-${caller}`}
            />
          ))}
        </div>
      ) : null}
    </div>
  );
}

function SymbolLink({
  path,
  label,
  title,
  testid,
}: {
  path: string;
  label: string;
  title?: string;
  testid: string;
}) {
  // A bordered chip, not an inline link: the border plus the hover fill carry
  // the affordance, so this keeps `text-link` and stays off `.link-inline` —
  // a resting underline inside a chip reads as a rendering defect.
  return (
    <Link
      to="/developer/compute/$function"
      params={{ function: path }}
      search={{ tab: "source" }}
      data-testid={testid}
      title={title}
      className="rounded border border-app px-1.5 py-0.5 font-mono text-xs text-link hover:bg-surface-2"
    >
      {label}
    </Link>
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

  if (events === undefined) return <LoadingState label="Loading logs…" />;
  if (filtered.length === 0) {
    return (
      <EmptyState
        title="No logs for this function"
        body="The Observability page hosts the full cross-function log feed. Run this function to populate its log stream."
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
              <span className="text-xs uppercase tracking-wide text-muted">
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

/**
 * Shared by the loaded table and its skeleton so the two cannot drift apart,
 * and the one place the column plan is written.
 *
 * Runs is the console's only table whose body is wider than its header -- a
 * mono run id, a status chip -- so it is the only one whose columns landed
 * somewhere else once the data arrived (measured: Status moved 50.89px at
 * 1280 and 61.72px at 1440). The shares below are the widths auto layout was
 * already choosing, to the nearest whole percent, so the loaded table looks
 * the same and the skeleton now agrees with it. Both states set `table-fixed`
 * for the plan to take effect.
 */
function RunsTableHead() {
  return (
    // Sticky because the pane below scrolls 50 runs and the four columns are a
    // mono id, a state glyph and two numbers, which read as nothing once their
    // labels leave the viewport. `bg-surface-2` is load-bearing, not styling: a
    // transparent sticky head lets the rows scroll visibly through it.
    <thead className="sticky top-0 z-20 bg-surface-2 text-xs uppercase tracking-[0.14em] text-muted">
      <tr>
        <Th width="29%">Run ID</Th>
        <Th width="21%">Status</Th>
        <Th width="26%">Duration</Th>
        <Th width="24%">Started</Th>
      </tr>
    </thead>
  );
}

/** Exported for spec coverage of the loading / empty / loaded branches. */
export function RunsTab({ fn }: { fn: FunctionDoc }) {
  const runs = useQuery(api.runs.recent, {
    bundleId: null,
    functionPath: fn.path ?? null,
    status: null,
    limit: 50,
  }) as RunDoc[] | undefined;
  if (runs === undefined) {
    // Keep the table mounted while the page is in flight: swapping the whole
    // table out drops the header and the column widths, so the panel jumps
    // once on load and again on data arrival.
    return (
      <div
        className="h-full overflow-auto px-6 py-4"
        data-testid="function-tab-runs"
      >
        <SkeletonRows
          className="min-w-[420px]"
          columns={4}
          fixed
          head={<RunsTableHead />}
          label="Loading runs…"
          testid="function-tab-runs-skeleton"
        />
      </div>
    );
  }
  if (runs.length === 0) {
    return (
      <EmptyState
        title="No runs yet"
        body="Once this function has been invoked, recent runs appear here. Click a run to open its detail page."
      />
    );
  }
  return (
    <div
      className="h-full overflow-auto px-6 py-4"
      data-testid="function-tab-runs"
    >
      <table className="w-full min-w-[420px] table-fixed border-collapse text-sm">
        <RunsTableHead />
        <tbody>
          {runs.map((run) => (
            <tr
              key={run._id}
              className="border-t border-app hover:bg-surface-2"
            >
              <Td>
                <Link
                  to="/developer/compute/runs/$runId"
                  params={{ runId: run._id }}
                  className="font-mono text-xs text-default hover:underline"
                  data-testid={`function-tab-runs-link-${run._id}`}
                >
                  {shortId(run._id, 12)}
                </Link>
              </Td>
              <Td>
                <StateChip state={run.status} />
              </Td>
              <Td>
                {typeof run.durationMs === "number" ? (
                  <span className="tabular font-mono text-xs">
                    {formatDuration(run.durationMs)}
                  </span>
                ) : (
                  <span className="tabular text-muted">—</span>
                )}
              </Td>
              <Td>
                {typeof run.startedAt === "number" ? (
                  <RelativeTime epochMs={run.startedAt} />
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

function NotFound({ path }: { path: string }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-2 text-center">
      <span className="font-mono text-sm text-default">Function not found</span>
      <span className="max-w-md text-xs text-muted">
        No function matches the path{" "}
        <code className="font-mono text-default">{path}</code>. It may have been
        removed or renamed. Open Compute to see the current inventory.
      </span>
      <Link
        to="/developer/compute"
        className="rounded border border-app px-3 py-1 font-mono text-xs uppercase tracking-wide text-muted hover:bg-surface hover:text-default"
      >
        ← back to compute
      </Link>
    </div>
  );
}
