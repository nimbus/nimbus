import { useQuery } from "@nimbus/nimbus/react";
import { Link } from "@tanstack/react-router";

import { api } from "../../../../convex/_generated/api";
import { CategoryPill, StatePill } from "../../../components/pill";
import {
  RunCorrelatedEvents,
  RunErrorPanel,
  RunSummary,
} from "../../../components/run-panels";
import { TraceWaterfall } from "../../../components/trace-waterfall";
import { Button } from "../../../components/ui/button";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "../../../components/ui/sheet";
import { shortId } from "../../../lib/format";
import type { ObservabilityTabProps } from "./-facets";
import type { EventDoc, RunDoc } from "./-types";

const TESTID = "observability-run-sheet";

// RunSheet is the right-side detail for one run on the Runs tab. It reads
// the same panels as the run page, so the sheet is a glance and the page is
// the full view; "Open run" is the handoff between them.
export function RunSheet({
  runId,
  runs,
  tenantId,
  setSearch,
  setSearchAction,
}: {
  runId: string | undefined;
  // The rows the table already holds; the sheet does not read the run again.
  runs: readonly RunDoc[] | undefined;
  tenantId: string | null;
  setSearch: ObservabilityTabProps["setSearch"];
  setSearchAction: ObservabilityTabProps["setSearchAction"];
}) {
  const open = runId !== undefined;
  const run = runId ? runs?.find((r) => r._id === runId) : undefined;
  return (
    <Sheet
      open={open}
      onOpenChange={(next) => {
        if (!next) setSearch({ run: undefined });
      }}
    >
      <SheetContent
        className="w-full sm:max-w-md"
        data-testid={TESTID}
        aria-label={run ? `Run ${run.functionPath ?? runId}` : "Run"}
      >
        {runId ? (
          run ? (
            <RunSheetBody
              run={run}
              runId={runId}
              tenantId={tenantId}
              setSearchAction={setSearchAction}
            />
          ) : (
            <RunSheetMissing runId={runId} loading={runs === undefined} />
          )
        ) : null}
      </SheetContent>
    </Sheet>
  );
}

function RunSheetBody({
  run,
  runId,
  tenantId,
  setSearchAction,
}: {
  run: RunDoc;
  runId: string;
  tenantId: string | null;
  setSearchAction: ObservabilityTabProps["setSearchAction"];
}) {
  const events = useQuery(api.events.recent, {
    correlationId: runId,
    tenantId,
    source: null,
    level: null,
    category: null,
    limit: 200,
  }) as EventDoc[] | undefined;
  return (
    <>
      <SheetHeader className="pr-12">
        <SheetTitle className="flex min-w-0 items-center gap-2">
          <span
            className="min-w-0 truncate font-mono text-sm text-text-1"
            title={run.functionPath ?? runId}
          >
            {run.functionPath ?? shortId(runId, 12)}
          </span>
          <span data-testid={`${TESTID}-head-status`}>
            <StatePill state={run.status} />
          </span>
        </SheetTitle>
        <SheetDescription className="flex items-center gap-2">
          <CategoryPill value={run.kind} />
          <span className="font-mono text-xs text-text-3">
            {shortId(runId, 14)}
          </span>
        </SheetDescription>
      </SheetHeader>
      <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-auto px-4">
        <RunSummary
          run={run}
          runId={runId}
          testid={TESTID}
          className="grid-cols-2 md:grid-cols-2"
        />
        <TraceWaterfall
          spans={run.spans}
          status={run.status}
          durationMs={run.durationMs}
          testid={`${TESTID}-trace`}
        />
        {run.status === "error" || run.error !== undefined ? (
          <RunErrorPanel
            error={run.error}
            functionPath={run.functionPath}
            testid={TESTID}
          />
        ) : null}
        <RunCorrelatedEvents
          events={events ?? []}
          runId={runId}
          testid={TESTID}
          logsLink={null}
        />
      </div>
      <SheetFooter className="flex-row justify-end">
        <Button
          variant="outline"
          size="sm"
          onClick={() =>
            setSearchAction({
              tab: "logs",
              correlationId: runId,
              run: undefined,
            })
          }
          data-testid={`${TESTID}-show-logs`}
        >
          Show in logs
        </Button>
        <Button
          size="sm"
          render={
            <Link
              to="/developer/compute/runs/$runId"
              params={{ runId }}
              data-testid={`${TESTID}-open-run`}
            />
          }
        >
          Open run ↗
        </Button>
      </SheetFooter>
    </>
  );
}

// The address names a run the table does not hold: the read is still in
// flight, the run fell outside the page, or the id is wrong. The sheet
// says which it can and offers the run page, which reads by id.
function RunSheetMissing({
  runId,
  loading,
}: {
  runId: string;
  loading: boolean;
}) {
  return (
    <>
      <SheetHeader className="pr-12">
        <SheetTitle className="font-mono text-sm text-text-1">
          {shortId(runId, 14)}
        </SheetTitle>
        <SheetDescription data-testid={`${TESTID}-missing`}>
          {loading
            ? "Loading runs…"
            : "This run is not in the current page of results. Open it directly to read it by id."}
        </SheetDescription>
      </SheetHeader>
      <SheetFooter className="flex-row justify-end">
        <Button
          size="sm"
          render={
            <Link
              to="/developer/compute/runs/$runId"
              params={{ runId }}
              data-testid={`${TESTID}-open-run`}
            />
          }
        >
          Open run ↗
        </Button>
      </SheetFooter>
    </>
  );
}
