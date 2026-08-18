import { useQuery } from "@nimbus/nimbus/react";
import { Link, useNavigate } from "@tanstack/react-router";
import { useCallback } from "react";

import { api } from "../../../../convex/_generated/api";
import { CategoryChip } from "../../../components/category-chip";
import { CopyChip } from "../../../components/copy-chip";
import { Td, Th } from "../../../components/data-table";
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
            onClick={() =>
              setSearch({ status: undefined, functionPath: undefined })
            }
            className="rounded border border-app px-2 py-1 font-mono text-xs uppercase tracking-wide text-muted hover:bg-surface hover:text-default"
            data-testid="observability-run-filter-clear"
          >
            clear
          </button>
        </div>
      </div>
      <RunsTable runs={runs} />
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

function RunsTable({ runs }: { runs: RunDoc[] | undefined }) {
  if (runs === undefined) {
    return (
      <div
        className="flex min-h-0 flex-1 items-center justify-center rounded-md border border-app bg-surface font-mono text-xs text-muted"
        data-testid="observability-runs-loading"
      >
        Loading runs…
      </div>
    );
  }
  if (runs.length === 0) {
    return (
      <div
        className="flex min-h-0 flex-1 items-center justify-center rounded-md border border-app bg-surface font-mono text-xs text-muted"
        data-testid="observability-runs-empty"
      >
        No runs recorded yet.
      </div>
    );
  }
  return (
    <div className="min-h-0 flex-1 overflow-auto rounded-md border border-app bg-surface">
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
    </div>
  );
}
