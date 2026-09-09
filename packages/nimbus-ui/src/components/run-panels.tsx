import { Link } from "@tanstack/react-router";
import type { ReactNode } from "react";

import { cn } from "@/lib/utils";
import { formatAbsoluteTime, formatDuration, shortId } from "../lib/format";
import { locationLine, parseRunError } from "../lib/run-error";
import { CopyChip } from "./copy-chip";
import { CategoryPill, StatePill } from "./pill";
import { RelativeTime } from "./time";

// The run panels: the summary grid, the correlated log lines and the error
// box. The run page composes all three with the trace waterfall; the
// observability run sheet composes them in a narrow column. Both read the
// same run shape, so the types are structural and fit a `Doc<"runs">` as
// well as the observability route's own `RunDoc`.
export type RunLike = {
  _id?: string;
  _creationTime?: number;
  bundleId?: string;
  functionPath?: string;
  kind?: string;
  durationMs?: number;
  status?: string;
  error?: unknown;
  startedAt?: number;
};

export type EventLike = {
  _id: string;
  _creationTime?: number;
  source?: string;
  level?: string;
  category?: string;
  message?: string;
  createdAt?: number;
};

export function RunSummary({
  run,
  runId,
  testid = "run-detail",
  className,
}: {
  run: RunLike;
  runId: string;
  testid?: string;
  className?: string;
}) {
  const startedAt = run.startedAt ?? run._creationTime;
  return (
    <div
      className={cn(
        "grid grid-cols-2 gap-x-6 gap-y-3 rounded-md border border-border-2 bg-bg-panel p-4 md:grid-cols-4",
        className,
      )}
      data-testid={`${testid}-summary`}
    >
      <Field label="Function" testid={`${testid}-function`}>
        <span className="font-mono text-sm text-text-1">
          {run.functionPath ?? "—"}
        </span>
      </Field>
      <Field label="Status" testid={`${testid}-status`}>
        <StatePill state={run.status} />
      </Field>
      <Field label="Kind" testid={`${testid}-kind`}>
        <CategoryPill value={run.kind} className="self-start" />
      </Field>
      <Field label="Duration" testid={`${testid}-duration`}>
        <span className="font-mono tabular text-sm text-text-1">
          {formatDuration(run.durationMs)}
        </span>
      </Field>
      <Field label="Run id" testid={`${testid}-id`}>
        <CopyChip label="run id" value={runId} testid={`${testid}-id-copy`}>
          {shortId(runId, 14)}
        </CopyChip>
      </Field>
      <Field label="Bundle" testid={`${testid}-bundle`}>
        {run.bundleId ? (
          <CopyChip
            label="bundle id"
            value={run.bundleId}
            testid={`${testid}-bundle-copy`}
          >
            {shortId(run.bundleId, 12)}
          </CopyChip>
        ) : (
          <span className="tabular text-text-3">—</span>
        )}
      </Field>
      <Field label="Started" testid={`${testid}-started`}>
        {typeof startedAt === "number" ? (
          <span
            className="font-mono tabular text-xs text-text-1"
            title={formatAbsoluteTime(startedAt)}
          >
            <RelativeTime epochMs={startedAt} />
          </span>
        ) : (
          <span className="tabular text-text-3">—</span>
        )}
      </Field>
      <Field label="Correlation" testid={`${testid}-correlation`}>
        <CopyChip
          label="correlation id"
          value={runId}
          testid={`${testid}-correlation-copy`}
        >
          {shortId(runId, 14)}
        </CopyChip>
      </Field>
    </div>
  );
}

function Field({
  label,
  testid,
  children,
}: {
  label: string;
  testid: string;
  children: ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1" data-testid={testid}>
      <span className="text-xs font-medium text-text-3">{label}</span>
      {children}
    </div>
  );
}

export function RunCorrelatedEvents({
  events,
  runId,
  testid = "run-detail",
  logsLink,
}: {
  events: readonly EventLike[];
  runId: string;
  testid?: string;
  // The header's trailing link. Defaults to the Logs tab narrowed to this
  // run, where the search field reads the run's lines; the run sheet passes
  // null because it has its own "Show in logs" action.
  logsLink?: ReactNode | null;
}) {
  const trailing =
    logsLink === undefined ? (
      <Link
        to="/developer/observability"
        search={{ tab: "logs", correlationId: runId }}
        className="text-xs font-medium text-text-3 hover:text-text-1 focus-visible:text-text-1"
        data-testid={`${testid}-open-logs`}
      >
        search run logs →
      </Link>
    ) : (
      logsLink
    );
  return (
    <div
      className="rounded-md border border-border-2 bg-bg-panel"
      data-testid={`${testid}-events`}
    >
      <div className="flex items-baseline justify-between gap-3 border-b border-border-2 px-4 py-3">
        <h2 className="text-xs font-medium text-text-3">
          Correlated events
          <span
            className="ml-2 font-normal tabular"
            data-testid={`${testid}-events-count`}
          >
            {events.length === 1 ? "1 line" : `${events.length} lines`}
          </span>
        </h2>
        {trailing}
      </div>
      {events.length === 0 ? (
        <div
          className="px-4 py-6 font-mono text-xs text-text-3"
          data-testid={`${testid}-events-empty`}
        >
          No events recorded for this run.
        </div>
      ) : (
        <ul className="divide-y divide-border-2">
          {events.map((event) => (
            <li
              key={event._id}
              className="grid grid-cols-[auto_auto_auto_1fr] items-baseline gap-2 px-4 py-1.5 text-xs"
              data-testid={`${testid}-event-${event._id}`}
            >
              <RelativeTime
                epochMs={event.createdAt ?? event._creationTime ?? 0}
              />
              <StatePill state={event.level ?? "info"} />
              <span className="text-xs font-medium text-text-3">
                {event.source ?? "—"}
                {event.category ? ` · ${event.category}` : ""}
              </span>
              <span className="font-mono text-text-1 truncate">
                {event.message ?? "(no message)"}
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export function RunErrorPanel({
  error,
  functionPath,
  testid = "run-detail",
}: {
  error: unknown;
  functionPath?: string;
  testid?: string;
}) {
  const { message, location, stack } = parseRunError(error);
  const line = location ? locationLine(location) : undefined;
  return (
    <div
      className="rounded-md border border-error bg-bg-panel p-4"
      data-testid={`${testid}-error`}
    >
      <h2 className="mb-2 text-xs font-medium text-error">Error</h2>
      {location ? (
        functionPath && line ? (
          <Link
            to="/developer/compute/$function"
            params={{ function: functionPath }}
            search={{ tab: "source", line }}
            className="mb-2 inline-block rounded-xs border border-error px-2 py-0.5 font-mono text-xs text-error hover:bg-bg-raised"
            data-testid={`${testid}-error-location`}
          >
            at {location} ↗
          </Link>
        ) : (
          <span
            className="mb-2 inline-block font-mono text-xs text-error"
            data-testid={`${testid}-error-location`}
          >
            at {location}
          </span>
        )
      ) : null}
      <pre className="overflow-auto font-mono text-xs text-text-1 whitespace-pre-wrap">
        {message}
      </pre>
      {stack ? (
        <details className="mt-2 text-xs" data-testid={`${testid}-error-stack`}>
          <summary className="cursor-pointer text-text-3 hover:text-text-1">
            Stack
          </summary>
          <pre className="mt-1 max-h-64 overflow-auto font-mono text-xs text-text-3 whitespace-pre">
            {stack}
          </pre>
        </details>
      ) : null}
    </div>
  );
}
