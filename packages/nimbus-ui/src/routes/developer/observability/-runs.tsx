import { useQuery } from "@nimbus/nimbus/react";
import { useCallback, useMemo } from "react";

import { api } from "../../../../convex/_generated/api";
import { CopyChip } from "../../../components/copy-chip";
import { DataTable, dataColumns } from "../../../components/data-table";
import { EmptyState } from "../../../components/empty-state";
import {
  FacetBar,
  FacetButton,
  FacetInput,
} from "../../../components/facet-bar";
import { CategoryPill, StatePill } from "../../../components/pill";
import { Select } from "../../../components/select";
import { RelativeTime } from "../../../components/time";
import { formatDuration, shortId } from "../../../lib/format";
import {
  ALL_OPTION,
  type ObservabilityTabProps,
  SystemLensButton,
  TenantFacet,
  TenantScopeNote,
} from "./-facets";
import { RunSheet } from "./-run-sheet";
import type { RunDoc } from "./-types";

/**
 * Every value the server can write into `runs.status`.
 *
 * A run row is only written *after* the invocation returns — the status is
 * `result.is_ok() ? "ok" : "error"` at the four Convex function handlers
 * (crates/nimbus-server/src/adapters/convex/handlers/function_routes/
 * {queries,mutations,actions}.rs), through the single writer
 * `record_run_async` (crates/nimbus-system/src/records/run.rs). There is no
 * in-flight row, so `running` and `queued` were filter options that could
 * only ever return "No runs".
 */
export const RUN_STATUSES = ["ok", "error"] as const;

const runCol = dataColumns<RunDoc>();

const RUN_COLUMNS = [
  runCol.accessor("startedAt", {
    header: "Started",
    size: 120,
    cell: (ctx) => {
      const at = ctx.getValue() ?? ctx.row.original._creationTime;
      return typeof at === "number" ? (
        <RelativeTime epochMs={at} />
      ) : (
        <span className="tabular text-text-3">—</span>
      );
    },
  }),
  runCol.accessor("functionPath", {
    header: "Function",
    size: 280,
    cell: (ctx) => {
      const path = ctx.getValue();
      return (
        <span
          className="block truncate font-mono text-xs text-text-1"
          title={path ?? ctx.row.original._id}
        >
          {path ?? shortId(ctx.row.original._id, 12)}
        </span>
      );
    },
  }),
  runCol.accessor("status", {
    header: "Status",
    size: 104,
    cell: (ctx) => <StatePill state={ctx.getValue()} />,
  }),
  runCol.accessor("kind", {
    header: "Kind",
    size: 112,
    cell: (ctx) => <CategoryPill value={ctx.getValue()} />,
  }),
  runCol.accessor("durationMs", {
    header: () => <span className="block text-right">Duration</span>,
    size: 96,
    cell: (ctx) => (
      <span className="block text-right font-mono text-xs tabular text-text-3">
        {formatDuration(ctx.getValue())}
      </span>
    ),
  }),
  runCol.accessor("_id", {
    header: "Run id",
    size: 160,
    enableSorting: false,
    cell: (ctx) => (
      <CopyChip
        label="run id"
        value={ctx.getValue()}
        testid={`observability-run-copy-${ctx.getValue()}`}
      >
        {shortId(ctx.getValue(), 10)}
      </CopyChip>
    ),
  }),
];

export function RunsTab({
  search,
  tenantId,
  allowAllTenants,
  setSearch,
  setSearchAction,
}: ObservabilityTabProps) {
  const runs = useQuery(api.runs.recent, {
    tenantId,
    bundleId: null,
    functionPath: search.functionPath ?? null,
    status: search.status ?? null,
    limit: 200,
  }) as RunDoc[] | undefined;

  const clearFilters = useCallback(
    () => setSearchAction({ status: undefined, functionPath: undefined }),
    [setSearchAction],
  );

  // An empty result means two different things and the table cannot tell them
  // apart on its own: a deployment that has never run a function, or a filter
  // the user set that nothing matches. Blaming filters that are not set sends
  // the reader hunting for a control they never touched.
  const filtered =
    search.status !== undefined || search.functionPath !== undefined;

  const rows = useMemo(() => runs ?? [], [runs]);
  const settledEmpty = runs !== undefined && runs.length === 0;

  return (
    <div
      className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden"
      data-testid="observability-runs"
    >
      <FacetBar
        label="Run facets"
        testid="observability-run-filters"
        trailing={
          <>
            <FacetButton
              onClick={clearFilters}
              testid="observability-run-filter-clear"
            >
              clear
            </FacetButton>
            <SystemLensButton />
          </>
        }
      >
        <TenantFacet
          tenantId={tenantId}
          allowAllTenants={allowAllTenants}
          setSearch={setSearch}
        />
        <Select
          label="Status"
          value={search.status ?? ALL_OPTION}
          options={[
            { value: ALL_OPTION, label: "all" },
            ...RUN_STATUSES.map((s) => ({ value: s, label: s })),
          ]}
          onChange={(v) =>
            setSearch({ status: v === ALL_OPTION ? undefined : v })
          }
          testid="observability-filter-run-status"
        />
        <FacetInput
          id="run-function"
          label="Function"
          value={search.functionPath ?? ""}
          placeholder="module:function"
          onChange={(v) => setSearch({ functionPath: v || undefined })}
          testid="observability-filter-run-function"
        />
      </FacetBar>
      <TenantScopeNote />
      <AdapterHonesty onShowLogs={() => setSearchAction({ tab: "logs" })} />
      {settledEmpty ? (
        <div className="min-h-0 flex-1 overflow-auto rounded-md border border-border-2 bg-bg-panel">
          <RunsEmptyState filtered={filtered} onClear={clearFilters} />
        </div>
      ) : (
        <DataTable
          columns={RUN_COLUMNS}
          data={rows}
          getRowId={(row) => row._id}
          ariaLabel="Recent runs"
          loading={runs === undefined}
          onRowActivate={(row) => setSearchAction({ run: row._id })}
          rowTestid={(row) => `observability-run-row-${row._id}`}
          testid="observability-runs-table"
          className="min-h-0 flex-1"
        />
      )}
      <RunSheet
        runId={search.run}
        runs={runs}
        tenantId={tenantId}
        setSearch={setSearch}
        setSearchAction={setSearchAction}
      />
    </div>
  );
}

// The table shows runtime invocations only. The other front doors write
// log lines and no run row, so their traffic is on the Logs tab; the note
// says so before an operator concludes the traffic is missing.
function AdapterHonesty({ onShowLogs }: { onShowLogs: () => void }) {
  return (
    <div
      className="shrink-0 rounded-md border border-border-2 bg-bg-raised px-3 py-2 font-mono text-xs text-text-3"
      data-testid="observability-adapter-honesty"
    >
      <span className="text-text-1">
        Convex / Nimbus runtime invocation history.
      </span>{" "}
      Native HTTP, scheduler, MongoDB, Firebase, and Cloud Functions traffic is
      surfaced under{" "}
      <button
        type="button"
        onClick={onShowLogs}
        className="underline hover:text-text-1 focus-visible:text-text-1"
        data-testid="observability-adapter-honesty-events-link"
      >
        Logs
      </button>{" "}
      for cross-adapter coverage.
    </div>
  );
}

function RunsEmptyState({
  filtered,
  onClear,
}: {
  filtered: boolean;
  onClear: () => void;
}) {
  if (filtered) {
    return (
      <EmptyState
        title="No runs match the current filters"
        body="Status and function narrow the same list, so a run has to satisfy both. Clear them to see every run the server has recorded."
        cta={{ label: "Clear filters", onClick: onClear }}
        testid="observability-runs-empty"
      />
    );
  }
  return (
    <EmptyState
      title="No runs yet"
      body="A run is one query, mutation, or action invocation. Call a function and its row lands here with its status, duration, and correlated log lines."
      cta={{ label: "Open Compute", to: "/developer/compute" }}
      testid="observability-runs-empty"
    />
  );
}
