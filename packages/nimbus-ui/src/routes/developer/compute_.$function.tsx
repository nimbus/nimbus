import { useQuery } from "@nimbus/nimbus/react";
import {
  createFileRoute,
  Link,
  useNavigate,
  useSearch,
} from "@tanstack/react-router";
import { FileCode } from "lucide-react";
import { useMemo } from "react";

import { Button } from "@/components/ui/button";
import { api } from "../../../convex/_generated/api";
import { Breadcrumb } from "../../components/breadcrumb";
import { CodeBlock } from "../../components/code-block";
import { CopyChip } from "../../components/copy-chip";
import { DataTable, dataColumns } from "../../components/data-table";
import { EmptyState } from "../../components/empty-state";
import { parseArgsValidator } from "../../components/function-runner/args-validator";
import { FunctionRunner } from "../../components/function-runner/function-runner";
import { LoadingState, SkeletonRows } from "../../components/loading-state";
import { PageTabs } from "../../components/page-tabs";
import { CategoryPill, StatePill } from "../../components/pill";
import { RelativeTime } from "../../components/time";
import { useApiRead } from "../../hooks/use-api-read";
import { formatDuration, shortHash, shortId } from "../../lib/format";
import type { FunctionDoc } from "../../lib/types/function";
import { FunctionSubPanel } from "../../shell/function-sub-panel";
import {
  type SubPanelSpec,
  useContributeSubPanel,
} from "../../shell/sub-panel";
import { GraphView } from "./-graph-view";

type DetailTab = "overview" | "source" | "runs" | "graph";

const TABS: ReadonlyArray<{ id: DetailTab; label: string }> = [
  { id: "overview", label: "Overview" },
  { id: "source", label: "Source" },
  { id: "runs", label: "Runs" },
  { id: "graph", label: "Graph" },
];

type DetailSearch = {
  tab?: DetailTab;
  // 1-based source line to highlight and scroll to in the Source tab, for
  // example when arriving from a failed run's error location.
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
    value === "overview" ||
    value === "source" ||
    value === "runs" ||
    value === "graph"
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

function FunctionDetailPage() {
  const { function: functionPath } = Route.useParams();
  const search = useSearch({ from: "/developer/compute_/$function" });
  const tab: DetailTab = search.tab ?? "overview";

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
  // The server keys a function to its bundle by the bundle's sha256, not by
  // the bundle document id (crates/nimbus-system/src/records/deployment.rs).
  const bundle = useMemo<BundleDoc | null>(() => {
    if (!fn?.bundleId || !bundles) return null;
    return (
      bundles.find((b) => b.sha256 === fn.bundleId || b._id === fn.bundleId) ??
      null
    );
  }, [fn, bundles]);

  const spec = useMemo<SubPanelSpec>(
    () => ({
      kind: "dynamic",
      title: "Functions",
      search: {
        placeholder: "Filter functions",
        rows: functions?.length ?? 0,
      },
      children: <FunctionSubPanel functions={functions} />,
    }),
    [functions],
  );
  useContributeSubPanel(spec);

  return (
    <section
      className="flex h-full flex-col overflow-hidden"
      data-testid="page-function-detail"
    >
      <div className="flex shrink-0 flex-col gap-3 border-b border-border-2 px-6 pb-3 pt-4">
        <Breadcrumb
          segments={[
            { label: "Compute", href: "/developer/compute" },
            {
              label: functionPath,
              copyValue: functionPath,
              copyLabel: "function path",
              active: true,
            },
          ]}
        />
        <header className="flex flex-wrap items-center gap-3">
          <h1
            className="font-mono text-text-1"
            style={{ fontSize: "var(--text-lg)" }}
          >
            {functionPath}
          </h1>
          {fn?.kind ? <CategoryPill value={fn.kind} /> : null}
          {fn?.adapter ? <CategoryPill value={fn.adapter} /> : null}
          {fn?.lastStatus ? <StatePill state={fn.lastStatus} /> : null}
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
        <PageTabs
          label="Function detail sections"
          tabs={TABS}
          active={tab}
          testid="function-detail-tabs"
          itemTestid="function-detail-tab"
        />
      </div>

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
  if (tab === "source") return <SourceTab fn={fn} highlightLine={line} />;
  if (tab === "runs") return <RunsTab fn={fn} />;
  if (tab === "graph") {
    return (
      <div className="flex h-full flex-col px-6 py-4">
        <GraphView focus={fn.path} />
      </div>
    );
  }
  return <OverviewTab fn={fn} bundle={bundle} />;
}

// ---------------------------------------------------------------------------
// Overview: what the function is, and the arguments it takes.

function OverviewTab({
  fn,
  bundle,
}: {
  fn: FunctionDoc;
  bundle: BundleDoc | null;
}) {
  const args = useMemo(() => parseArgsValidator(fn.argsSchema), [fn]);
  return (
    <div
      className="flex h-full flex-col gap-5 overflow-auto px-6 py-4"
      data-testid="function-tab-overview"
    >
      <dl className="grid grid-cols-[minmax(0,8rem)_1fr] gap-x-4 gap-y-2 text-xs">
        <Fact label="Kind">
          <CategoryPill value={fn.kind} />
        </Fact>
        <Fact label="Adapter">
          {fn.adapter ? <CategoryPill value={fn.adapter} /> : "—"}
        </Fact>
        <Fact label="Bundle">
          {bundle?.sha256 ? (
            <span className="font-mono">{shortHash(bundle.sha256, 16)}</span>
          ) : (
            "—"
          )}
        </Fact>
        <Fact label="Last status">
          {fn.lastStatus ? <StatePill state={fn.lastStatus} /> : "never run"}
        </Fact>
        <Fact label="Last run">
          {typeof fn.lastRunAt === "number" ? (
            <RelativeTime epochMs={fn.lastRunAt} />
          ) : (
            "never"
          )}
        </Fact>
      </dl>
      <section
        className="flex flex-col gap-2"
        aria-labelledby="function-args-title"
        data-testid="function-overview-args"
      >
        <h2
          id="function-args-title"
          className="text-xs font-medium text-text-3"
        >
          Arguments
        </h2>
        {args === null ? (
          <p className="text-xs text-text-3">
            No argument validator is recorded for this function. The runner
            takes arguments as JSON.
          </p>
        ) : args.length === 0 ? (
          <p className="text-xs text-text-3">
            This function takes no arguments.
          </p>
        ) : (
          <ul className="flex flex-col divide-y divide-border-2 rounded-md border border-border-2 bg-bg-panel">
            {args.map((arg) => (
              <li
                key={arg.name}
                className="flex items-baseline gap-3 px-3 py-1.5 font-mono text-xs"
                data-testid={`function-overview-arg-${arg.name}`}
              >
                <span className="text-text-1">{arg.name}</span>
                <span className="text-text-3">{arg.type}</span>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}

function Fact({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <>
      <dt className="pt-0.5 font-medium text-text-3">{label}</dt>
      <dd className="min-w-0 text-text-1">{children}</dd>
    </>
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

// SOURCE_CAPTURE_COMMAND is the exact command that captures source for a
// deployment, with the app directory to fill in.
export const SOURCE_CAPTURE_COMMAND = "nimbus dev --app-dir .";

/** Exported for spec coverage of the missing-source snippet. */
export function SourceTab({
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
    // The source store has nothing for this module. The command is the
    // one that captures it: `nimbus dev` bundles the app directory and
    // records its source with the deployment.
    return (
      <EmptyState
        icon={FileCode}
        title="Source not available"
        body="This deployment did not capture source for this module. Run the app with the Nimbus CLI and the source shows here."
        snippet={SOURCE_CAPTURE_COMMAND}
        testid="function-source-missing"
      />
    );
  }
  const ready = state.value.ready;
  return (
    <div
      className="flex h-full flex-col overflow-hidden"
      data-testid="function-tab-source"
    >
      <div className="flex shrink-0 items-center gap-2 border-b border-border-2 bg-bg-raised px-6 py-1.5 text-xs font-medium text-text-3">
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
      className="flex shrink-0 flex-wrap items-center gap-x-4 gap-y-1 border-b border-border-2 bg-bg-panel px-6 py-2"
      data-testid="function-source-symbols"
    >
      {analysis.exports.length > 0 ? (
        <div className="flex flex-wrap items-center gap-1.5">
          <span className="text-xs font-medium text-text-3">defines</span>
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
          <span className="text-xs font-medium text-text-3">calls</span>
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
          <span className="text-xs font-medium text-text-3">called by</span>
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
  // the affordance, so this keeps `text-accent-link` and stays off `.link-inline` —
  // a resting underline inside a chip reads as a rendering defect.
  return (
    <Link
      to="/developer/compute/$function"
      params={{ function: path }}
      search={{ tab: "source" }}
      data-testid={testid}
      title={title}
      className="rounded-xs border border-border-2 px-1.5 py-0.5 font-mono text-xs text-accent-link hover:bg-bg-raised"
    >
      {label}
    </Link>
  );
}

// ---------------------------------------------------------------------------
// Runs: the recent runs of this one function.

const runCol = dataColumns<RunDoc>();
const RUN_COLUMNS = [
  runCol.accessor("status", {
    header: "Status",
    size: 104,
    cell: (ctx) => <StatePill state={ctx.getValue()} />,
  }),
  runCol.accessor("_id", {
    header: "Run",
    cell: (ctx) => (
      <Link
        to="/developer/compute/runs/$runId"
        params={{ runId: ctx.getValue() }}
        className="truncate font-mono text-xs text-text-1 hover:underline"
        data-testid={`function-tab-runs-link-${ctx.getValue()}`}
      >
        {shortId(ctx.getValue(), 12)}
      </Link>
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
        <span className="block text-right text-xs text-text-3">
          {typeof startedAt === "number" ? (
            <RelativeTime epochMs={startedAt} />
          ) : (
            "—"
          )}
        </span>
      );
    },
  }),
];

/** Exported for spec coverage of the loading, empty, and loaded branches. */
export function RunsTab({ fn }: { fn: FunctionDoc }) {
  const navigate = useNavigate();
  const runs = useQuery(api.runs.recent, {
    bundleId: null,
    functionPath: fn.path ?? null,
    status: null,
    tenantId: null,
    limit: 50,
  }) as RunDoc[] | undefined;
  if (runs === undefined) {
    return (
      <div className="h-full overflow-auto px-6 py-4">
        <SkeletonRows
          columns={4}
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
        body="Once this function has run, its recent runs show here. Open a run for its trace and error."
        testid="function-tab-runs-empty"
      />
    );
  }
  return (
    <div className="h-full overflow-hidden px-6 py-4">
      <DataTable
        columns={RUN_COLUMNS}
        data={runs}
        getRowId={(row) => row._id}
        ariaLabel={`Runs of ${fn.path ?? "this function"}`}
        onRowActivate={(row) =>
          void navigate({
            to: "/developer/compute/runs/$runId",
            params: { runId: row._id },
          })
        }
        testid="function-tab-runs"
        className="h-full"
      />
    </div>
  );
}

function NotFound({ path }: { path: string }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-2 text-center">
      <span className="font-mono text-sm text-text-1">Function not found</span>
      <span className="max-w-md text-xs text-text-3">
        No function matches the path{" "}
        <code className="font-mono text-text-1">{path}</code>. It may have been
        removed or renamed. Open Compute to see the current inventory.
      </span>
      <Button
        variant="outline"
        size="sm"
        render={<Link to="/developer/compute" />}
      >
        Back to Compute
      </Button>
    </div>
  );
}
