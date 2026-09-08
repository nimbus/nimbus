import { createFileRoute, Link, notFound } from "@tanstack/react-router";
import { useMemo } from "react";
import { api } from "../../../convex/_generated/api";
import type { Doc, Id } from "../../../convex/_generated/dataModel";
import { Breadcrumb } from "../../components/breadcrumb";
import {
  RunCorrelatedEvents,
  RunErrorPanel,
  RunSummary,
} from "../../components/run-panels";
import { TraceWaterfall } from "../../components/trace-waterfall";
import { shortId } from "../../lib/format";
import { getNimbusClient } from "../../lib/nimbus-client";
import type { RunSpan } from "./observability/-types";

export const Route = createFileRoute("/developer/compute_/runs_/$runId")({
  loader: async ({ params }) => {
    const client = getNimbusClient();
    const [run, events] = await Promise.all([
      client.query(api.runs.byId, { id: params.runId as Id<"runs"> }),
      client.query(api.events.recent, {
        source: null,
        level: null,
        category: null,
        correlationId: params.runId,
        tenantId: null,
        limit: 200,
      }),
    ]);
    if (!run) throw notFound();
    return { run, events };
  },
  notFoundComponent: RunNotFound,
  component: RunDetailPage,
});

type RunDoc = Doc<"runs">;
type EventDoc = Doc<"events">;

function RunDetailPage() {
  const { runId } = Route.useParams();
  const { run, events } = Route.useLoaderData();

  const sortedEvents = useMemo(() => {
    return events
      .slice()
      .sort((a, b) => (a.createdAt ?? 0) - (b.createdAt ?? 0));
  }, [events]);

  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-run-detail"
    >
      <header className="flex flex-col gap-2">
        <Breadcrumb
          segments={[
            { label: "Compute", href: "/developer/compute" },
            {
              label: "Runs",
              href: "/developer/observability",
              search: { tab: "runs" },
            },
            {
              label: shortId(runId, 12),
              copyValue: runId,
              copyLabel: "run id",
              active: true,
            },
          ]}
          testid="run-detail-breadcrumb"
        />
        <h1 className="text-text-1" style={{ fontSize: "var(--text-xl)" }}>
          Run detail
        </h1>
      </header>

      <RunDetailBody run={run} runId={runId} events={sortedEvents} />
    </section>
  );
}

function RunDetailBody({
  run,
  runId,
  events,
}: {
  run: RunDoc;
  runId: string;
  events: EventDoc[];
}) {
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-auto pr-1">
      <RunSummary run={run} runId={runId} />
      <TraceWaterfall
        spans={run.spans as RunSpan[] | undefined}
        status={run.status}
        durationMs={run.durationMs}
        testid="run-detail-trace"
      />
      <RunCorrelatedEvents events={events} runId={runId} />
      {run.error ? (
        <RunErrorPanel error={run.error} functionPath={run.functionPath} />
      ) : null}
    </div>
  );
}

function RunNotFound() {
  const { runId } = Route.useParams();
  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-run-detail"
    >
      <div
        className="flex min-h-0 flex-1 flex-col items-center justify-center gap-2 rounded-md border border-border-2 bg-bg-panel px-6 py-10 text-center"
        data-testid="run-detail-missing"
      >
        <p className="font-mono text-sm text-text-1">Run not found</p>
        <p className="max-w-md text-xs text-text-3">
          No run with id <code className="font-mono text-text-1">{runId}</code>.
          It may have been pruned, or the correlation id does not point to a run
          record.
        </p>
        <Link
          to="/developer/observability"
          search={{ tab: "runs" }}
          className="mt-2 rounded-xs border border-border-2 px-2 py-1 text-xs font-medium text-text-3 hover:bg-bg-panel hover:text-text-1"
          data-testid="run-detail-back"
        >
          ← all runs
        </Link>
      </div>
    </section>
  );
}
