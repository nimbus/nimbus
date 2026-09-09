import { useQuery } from "@nimbus/nimbus/react";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { Box } from "lucide-react";
import { useMemo } from "react";

import { api } from "../../../convex/_generated/api";
import { DataTable, dataColumns } from "../../components/data-table";
import { EmptyState } from "../../components/empty-state";
import { LoadingState } from "../../components/loading-state";
import { PageHeader } from "../../components/page-header";
import { PageTabs } from "../../components/page-tabs";
import { CategoryPill, StatePill } from "../../components/pill";
import { RelativeTime } from "../../components/time";
import type { FunctionDoc } from "../../lib/types/function";
import { FunctionSubPanel } from "../../shell/function-sub-panel";
import {
  type SubPanelSpec,
  useContributeSubPanel,
} from "../../shell/sub-panel";
import {
  COMPUTE_TABS,
  type ComputeTab,
  parseComputeTab,
} from "./-compute-tabs";
import { GraphView } from "./-graph-view";

type ComputeSearch = { tab?: ComputeTab };

export const Route = createFileRoute("/developer/compute")({
  validateSearch: (search: Record<string, unknown>): ComputeSearch => ({
    tab: parseComputeTab(search.tab),
  }),
  component: ComputePage,
});

type BundleDoc = {
  _id: string;
  sha256?: string;
  status?: string;
  sourceRef?: string;
  _creationTime?: number;
};

const SUBTITLES: Record<ComputeTab, string> = {
  functions:
    "Functions registered to this tenant. Open one for its source, runs, and the runner.",
  sandboxes:
    "Isolated execution environments for this tenant, read live from the sandbox runtime.",
  graph:
    "Which function calls which, laid out by module. Select a node to open its source.",
};

function ComputePage() {
  const tab: ComputeTab = Route.useSearch().tab ?? "functions";

  const functions = useQuery(api.functions.list, {
    bundleId: null,
    kind: null,
    limit: 200,
  }) as FunctionDoc[] | undefined;
  const bundles = useQuery(api.bundles.list, {
    status: null,
    limit: 50,
  }) as BundleDoc[] | undefined;

  // The sub-panel is the function tree, the same one the function page
  // shows, so the list an operator is scanning does not change shape when
  // they open an item from it.
  const spec = useMemo<SubPanelSpec>(
    () => ({
      kind: "dynamic",
      title: "Functions",
      search: { placeholder: "Filter functions", rows: functions?.length ?? 0 },
      children: <FunctionSubPanel functions={functions} />,
    }),
    [functions],
  );
  useContributeSubPanel(spec);

  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-compute"
    >
      <div className="flex shrink-0 flex-col gap-3">
        <PageHeader
          title="Compute"
          subtitle={SUBTITLES[tab]}
          trailing={<BundleHint bundles={bundles} />}
        />
        <PageTabs
          label="Compute tabs"
          tabs={COMPUTE_TABS}
          active={tab}
          testid="compute-tabs"
          itemTestid="compute-tab"
        />
      </div>

      {tab === "graph" ? (
        <GraphView />
      ) : tab === "sandboxes" ? (
        <SandboxesView />
      ) : (
        <FunctionsTable functions={functions} />
      )}
    </section>
  );
}

// ---------------------------------------------------------------------------
// Functions: one row per registered function.

const fnCol = dataColumns<FunctionDoc>();
const FUNCTION_COLUMNS = [
  fnCol.accessor("path", {
    header: "Function",
    cell: (ctx) => (
      <span className="truncate font-mono text-xs text-text-1">
        {ctx.getValue() ?? "—"}
      </span>
    ),
  }),
  fnCol.accessor("kind", {
    header: "Kind",
    size: 136,
    cell: (ctx) => <CategoryPill value={ctx.getValue()} />,
  }),
  fnCol.accessor("adapter", {
    header: "Adapter",
    size: 104,
    cell: (ctx) => {
      const adapter = ctx.getValue();
      return adapter ? (
        <CategoryPill value={adapter} />
      ) : (
        <span className="text-xs text-text-3">—</span>
      );
    },
  }),
  fnCol.accessor("lastStatus", {
    header: "Last status",
    size: 112,
    cell: (ctx) => {
      const status = ctx.getValue();
      return status ? (
        <StatePill state={status} />
      ) : (
        <span className="text-xs text-text-3">never run</span>
      );
    },
  }),
  fnCol.accessor("lastRunAt", {
    header: "Last run",
    size: 120,
    cell: (ctx) => {
      const at = ctx.getValue();
      return (
        <span className="block text-right text-xs text-text-3">
          {typeof at === "number" ? <RelativeTime epochMs={at} /> : "—"}
        </span>
      );
    },
  }),
];

function FunctionsTable({
  functions,
}: {
  functions: FunctionDoc[] | undefined;
}) {
  const navigate = useNavigate();
  const rows = useMemo(
    () =>
      (functions ?? [])
        .filter((fn) => typeof fn.path === "string")
        .sort((a, b) => (a.path ?? "").localeCompare(b.path ?? "")),
    [functions],
  );
  if (functions === undefined) {
    return <LoadingState label="Loading functions…" />;
  }
  if (rows.length === 0) {
    return (
      <EmptyState
        icon={Box}
        title="No functions deployed"
        body="Deploy an app to register its functions with this tenant."
        snippet="nimbus dev --app-dir ."
        testid="compute-functions-empty"
      />
    );
  }
  return (
    <div className="min-h-0 flex-1 overflow-hidden">
      <DataTable
        columns={FUNCTION_COLUMNS}
        data={rows}
        getRowId={(row) => row._id}
        ariaLabel="Functions"
        onRowActivate={(row) => {
          if (!row.path) return;
          void navigate({
            to: "/developer/compute/$function",
            params: { function: row.path },
          });
        }}
        testid="compute-functions"
        className="h-full"
      />
    </div>
  );
}

function BundleHint({ bundles }: { bundles: BundleDoc[] | undefined }) {
  if (bundles === undefined) {
    return (
      <span
        className="font-mono text-xs text-text-3"
        data-testid="compute-bundles-loading"
      >
        bundles: loading…
      </span>
    );
  }
  const active = bundles.filter((b) => b.status === "active").length;
  return (
    <span
      className="font-mono text-xs text-text-3"
      data-testid="compute-bundles"
    >
      {bundles.length} bundle{bundles.length === 1 ? "" : "s"}
      {active > 0 ? ` · ${active} active` : ""}
    </span>
  );
}

// ---------------------------------------------------------------------------
// Sandboxes are live runtime state, not deployment records. Live wiring is a
// tracked follow-on; until then this is an honest empty state, never
// placeholder rows.

function SandboxesView() {
  return (
    <div
      className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-md border border-border-2 bg-bg-panel"
      data-testid="compute-sandboxes"
    >
      <EmptyState
        icon={Box}
        title="No live sandboxes"
        body="Sandboxes are live runtime state, not deployment records. Running sandboxes for this tenant appear here once the runtime is connected."
        testid="compute-sandboxes-empty"
      />
    </div>
  );
}
