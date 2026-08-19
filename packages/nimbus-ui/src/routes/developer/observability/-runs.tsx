import { useQuery } from "@nimbus/nimbus/react";
import { Link, useNavigate } from "@tanstack/react-router";
import { type ReactNode, useCallback } from "react";

import { api } from "../../../../convex/_generated/api";
import { CategoryChip } from "../../../components/category-chip";
import { CopyChip } from "../../../components/copy-chip";
import { Td, Th } from "../../../components/data-table";
import { EmptyState } from "../../../components/empty-state";
import { LoadingState } from "../../../components/loading-state";
import { StateChip } from "../../../components/state-chip";
import { RelativeTime } from "../../../components/time";
import { formatDuration, shortId } from "../../../lib/format";
import { FilterInput, FilterSelect } from "./-filters";
import type { ObservabilitySearch, RunDoc } from "./-types";

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

export function RunsTab({ search }: { search: ObservabilitySearch }) {
  const navigate = useNavigate({ from: "/developer/observability" });
  const runs = useQuery(api.runs.recent, {
    bundleId: null,
    functionPath: search.functionPath ?? null,
    status: search.status ?? null,
    limit: 200,
  }) as RunDoc[] | undefined;

  const setSearch = useCallback(
    (patch: Partial<ObservabilitySearch>) => {
      void navigate({
        to: "/developer/observability",
        search: (prev) => ({ ...prev, ...patch }),
        replace: true,
      });
    },
    [navigate],
  );

  const clearFilters = useCallback(
    () => setSearch({ status: undefined, functionPath: undefined }),
    [setSearch],
  );

  // An empty result means two different things and the table cannot tell them
  // apart on its own: a deployment that has never run a function, or a filter
  // the user set that nothing matches. Blaming filters that are not set sends
  // the reader hunting for a control they never touched.
  const filtered =
    search.status !== undefined || search.functionPath !== undefined;

  return (
    <div
      className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden"
      data-testid="observability-runs"
    >
      <AdapterHonesty />
      <div
        className="flex flex-wrap items-center gap-2"
        data-testid="observability-run-filters"
      >
        <FilterSelect
          id="run-status"
          label="Status"
          value={search.status ?? ""}
          options={[
            { value: "", label: "all" },
            ...RUN_STATUSES.map((s) => ({ value: s, label: s })),
          ]}
          onChange={(v) => setSearch({ status: v || undefined })}
          testid="observability-filter-run-status"
        />
        <FilterInput
          id="run-function"
          label="Function"
          value={search.functionPath ?? ""}
          placeholder="path/to/fn"
          onChange={(v) => setSearch({ functionPath: v || undefined })}
          testid="observability-filter-run-function"
        />
        <div className="ml-auto flex justify-end">
          <button
            type="button"
            onClick={clearFilters}
            className="rounded border border-app px-2 py-1 font-mono text-xs uppercase tracking-wide text-muted hover:bg-surface hover:text-default"
            data-testid="observability-run-filter-clear"
          >
            clear
          </button>
        </div>
      </div>
      <RunsTable runs={runs} filtered={filtered} onClear={clearFilters} />
    </div>
  );
}

function AdapterHonesty() {
  return (
    <div
      className="rounded-md border border-app bg-surface-2 px-3 py-2 font-mono text-xs text-muted"
      data-testid="observability-adapter-honesty"
    >
      <span className="text-default">
        Convex / Nimbus runtime invocation history.
      </span>{" "}
      Native HTTP, scheduler, MongoDB, Firebase, and Cloud Functions traffic is
      surfaced under Logs — see the{" "}
      <Link
        to="/developer/observability"
        search={(prev) => ({ ...prev, tab: "logs" })}
        className="underline hover:text-default focus-visible:text-default"
        data-testid="observability-adapter-honesty-events-link"
      >
        Events view
      </Link>{" "}
      for cross-adapter coverage.
    </div>
  );
}

/**
 * Three states, three treatments, one frame. The panel keeps its border, its
 * fill and its box in every state, so the swap from placeholder to data moves
 * nothing around it; only the contents change.
 *
 * Loading uses `LoadingState` and empty uses `EmptyState` — the console's two
 * panel-scope primitives — rather than the one-line muted box this used to
 * hand-roll. The same empty condition one nav entry away (Operator →
 * Observability → Runs) already renders `EmptyState`, and DESIGN.md's
 * whole-tab empty state is a mono title plus a two-line body plus a next
 * action. A fresh install lands here first and got none of it.
 */
function RunsTable({
  runs,
  filtered,
  onClear,
}: {
  runs: RunDoc[] | undefined;
  filtered: boolean;
  onClear: () => void;
}) {
  if (runs === undefined) {
    return (
      <RunsFrame>
        <LoadingState
          label="Loading runs…"
          testid="observability-runs-loading"
        />
      </RunsFrame>
    );
  }
  if (runs.length === 0) {
    return (
      <RunsFrame>
        {filtered ? (
          <EmptyState
            title="No runs match the current filters"
            body="Status and function path narrow the same list, so a run has to satisfy both. Clear them to see every run this deployment has recorded."
            cta={{ label: "Clear filters", onClick: onClear }}
            testid="observability-runs-empty"
          />
        ) : (
          <EmptyState
            title="No runs yet"
            body="A run is recorded each time a query, mutation, or action executes. Invoke a function and it appears here."
            cta={{ label: "Open Compute", to: "/developer/compute" }}
            testid="observability-runs-empty"
          />
        )}
      </RunsFrame>
    );
  }
  return (
    <RunsFrame>
      <table
        className="w-full border-collapse text-sm"
        data-testid="observability-runs-table"
      >
        <thead className="sticky top-0 bg-surface-2 text-xs uppercase tracking-[0.14em] text-muted">
          <tr>
            <Th>Function</Th>
            <Th>Status</Th>
            <Th>Kind</Th>
            <Th align="right">Duration</Th>
            <Th>Started</Th>
            <Th>Run id</Th>
          </tr>
        </thead>
        <tbody>
          {runs.map((run) => (
            <tr
              key={run._id}
              className="border-t border-app hover:bg-surface-2"
              data-testid={`observability-run-row-${run._id}`}
            >
              <Td>
                <Link
                  to="/developer/compute/runs/$runId"
                  params={{ runId: run._id }}
                  className="font-mono text-default hover:underline"
                  data-testid={`observability-run-link-${run._id}`}
                >
                  {run.functionPath ?? shortId(run._id, 12)}
                </Link>
              </Td>
              <Td>
                <StateChip state={run.status} />
              </Td>
              <Td>
                <CategoryChip value={run.kind} />
              </Td>
              <Td align="right" mono>
                {formatDuration(run.durationMs)}
              </Td>
              <Td>
                {typeof run.startedAt === "number" ? (
                  <RelativeTime epochMs={run.startedAt} />
                ) : (
                  <span className="tabular text-muted">—</span>
                )}
              </Td>
              <Td>
                <CopyChip
                  label="run id"
                  value={run._id}
                  testid={`observability-run-copy-${run._id}`}
                >
                  {shortId(run._id, 10)}
                </CopyChip>
              </Td>
            </tr>
          ))}
        </tbody>
      </table>
    </RunsFrame>
  );
}

function RunsFrame({ children }: { children: ReactNode }) {
  return (
    <div className="min-h-0 flex-1 overflow-auto rounded-md border border-app bg-surface">
      {children}
    </div>
  );
}
