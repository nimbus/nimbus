import { useQuery } from "@nimbus/nimbus/react";
import { Link } from "@tanstack/react-router";
import { useCallback, useMemo } from "react";

import { api } from "../../../../convex/_generated/api";
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
import { TraceWaterfall } from "../../../components/trace-waterfall";
import { Button } from "../../../components/ui/button";
import { formatDuration, shortId } from "../../../lib/format";
import {
  ALL_OPTION,
  type ObservabilityTabProps,
  SystemLensButton,
  TenantFacet,
} from "./-facets";
import { RUN_STATUSES } from "./-runs";
import type { RunDoc } from "./-types";

const traceCol = dataColumns<RunDoc>();

// The trace list is the run list with the columns a trace reader picks by:
// when, which function, how it ended, how long, and how many spans.
const TRACE_COLUMNS = [
  traceCol.accessor("startedAt", {
    header: "Started",
    size: 110,
    cell: (ctx) => {
      const at = ctx.getValue() ?? ctx.row.original._creationTime;
      return typeof at === "number" ? (
        <RelativeTime epochMs={at} />
      ) : (
        <span className="tabular text-text-3">—</span>
      );
    },
  }),
  traceCol.accessor("functionPath", {
    header: "Function",
    size: 220,
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
  traceCol.accessor("status", {
    header: "Status",
    size: 96,
    cell: (ctx) => <StatePill state={ctx.getValue()} />,
  }),
  traceCol.accessor("durationMs", {
    header: () => <span className="block text-right">Duration</span>,
    size: 92,
    cell: (ctx) => (
      <span className="block text-right font-mono text-xs tabular text-text-3">
        {formatDuration(ctx.getValue())}
      </span>
    ),
  }),
  traceCol.accessor((row) => row.spans?.length ?? 0, {
    id: "spans",
    header: () => <span className="block text-right">Spans</span>,
    size: 72,
    cell: (ctx) => (
      <span className="block text-right font-mono text-xs tabular text-text-3">
        {ctx.getValue()}
      </span>
    ),
  }),
];

// TracesTab is the run list beside one run's waterfall. The list is the
// same `runs.recent` read as the Runs tab, so the two tabs agree on which
// runs exist; `?run=` names the trace on show, as it names the open sheet
// on the Runs tab, so a link from one lands on the same run in the other.
export function TracesTab({
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
    fingerprint: search.fingerprint ?? null,
    limit: 200,
  }) as RunDoc[] | undefined;

  const clearFilters = useCallback(
    () =>
      setSearchAction({
        status: undefined,
        functionPath: undefined,
        fingerprint: undefined,
      }),
    [setSearchAction],
  );
  const filtered =
    search.status !== undefined ||
    search.functionPath !== undefined ||
    search.fingerprint !== undefined;

  const rows = useMemo(() => runs ?? [], [runs]);
  const selected = search.run
    ? rows.find((row) => row._id === search.run)
    : undefined;
  const settledEmpty = runs !== undefined && runs.length === 0;

  return (
    <div
      className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden"
      data-testid="observability-traces"
    >
      <FacetBar
        label="Trace facets"
        testid="observability-trace-filters"
        trailing={
          <>
            <FacetButton
              onClick={clearFilters}
              testid="observability-trace-filter-clear"
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
          testid="observability-filter-trace-status"
        />
        <FacetInput
          id="trace-function"
          label="Function"
          value={search.functionPath ?? ""}
          placeholder="module:function"
          onChange={(v) => setSearch({ functionPath: v || undefined })}
          testid="observability-filter-trace-function"
        />
      </FacetBar>
      {settledEmpty ? (
        <div className="min-h-0 flex-1 overflow-auto rounded-md border border-border-2 bg-bg-panel">
          <EmptyState
            title={
              filtered ? "No runs match the current filters" : "No traces yet"
            }
            body={
              filtered
                ? "Status and function narrow the same list. Clear them to see every run the server has traced."
                : "A trace is the spans of one run: the function's own span and one span per host call under it. Call a function and its trace lands here."
            }
            cta={
              filtered
                ? { label: "Clear filters", onClick: clearFilters }
                : undefined
            }
            testid="observability-traces-empty"
          />
        </div>
      ) : (
        <div className="grid min-h-0 flex-1 grid-cols-1 gap-3 overflow-hidden lg:grid-cols-[minmax(0,5fr)_minmax(0,7fr)]">
          <DataTable
            columns={TRACE_COLUMNS}
            data={rows}
            getRowId={(row) => row._id}
            ariaLabel="Traced runs"
            loading={runs === undefined}
            onRowActivate={(row) => setSearchAction({ run: row._id })}
            rowTestid={(row) => `observability-trace-row-${row._id}`}
            rowClassName={(row) =>
              row._id === search.run ? "bg-bg-raised" : undefined
            }
            testid="observability-traces-table"
            className="min-h-0"
          />
          <TracePane
            runId={search.run}
            run={selected}
            loading={runs === undefined}
          />
        </div>
      )}
    </div>
  );
}

// TracePane is the right column: the chosen run's summary line and its
// waterfall, or the reason there is none to show.
function TracePane({
  runId,
  run,
  loading,
}: {
  runId: string | undefined;
  run: RunDoc | undefined;
  loading: boolean;
}) {
  if (runId === undefined) {
    return (
      <div className="min-h-0 overflow-auto rounded-md border border-border-2 bg-bg-panel">
        <EmptyState
          title="Pick a run to read its trace"
          body="The waterfall draws the run's spans on one time axis: the function's own span, then the database, scheduler, and nested function calls under it."
          testid="observability-trace-empty"
        />
      </div>
    );
  }
  if (run === undefined) {
    return (
      <div className="flex min-h-0 flex-col items-center gap-3 overflow-auto rounded-md border border-border-2 bg-bg-panel pb-6">
        <EmptyState
          title={
            loading ? "Loading runs…" : "This run is not in the current page"
          }
          body={
            loading
              ? undefined
              : "Open the run directly to read it by id, or clear the filters."
          }
          testid="observability-trace-missing"
        />
        {loading ? null : (
          <Button
            variant="outline"
            size="sm"
            render={
              <Link
                to="/developer/compute/runs/$runId"
                params={{ runId }}
                data-testid="observability-trace-missing-open"
              />
            }
          >
            Open run ↗
          </Button>
        )}
      </div>
    );
  }
  return (
    <div
      className="flex min-h-0 flex-col gap-3 overflow-auto"
      data-testid="observability-trace-pane"
    >
      <div className="flex flex-wrap items-center gap-2 rounded-md border border-border-2 bg-bg-panel px-3 py-2">
        <span
          className="min-w-0 flex-1 truncate font-mono text-sm text-text-1"
          title={run.functionPath ?? run._id}
        >
          {run.functionPath ?? shortId(run._id, 12)}
        </span>
        <CategoryPill value={run.kind} />
        <StatePill state={run.status} />
        <Button
          variant="outline"
          size="sm"
          render={
            <Link
              to="/developer/compute/runs/$runId"
              params={{ runId: run._id }}
              data-testid="observability-trace-open-run"
            />
          }
        >
          Open run ↗
        </Button>
      </div>
      <TraceWaterfall
        spans={run.spans}
        status={run.status}
        durationMs={run.durationMs}
        testid="observability-trace-waterfall"
      />
    </div>
  );
}
