import type { EventDoc, RunDoc } from "./-types";

// A log group is one run and the lines it wrote, or the server group: the
// lines that belong to no run. The stream is a list of groups, newest first,
// because a function invocation writes a run row and, today, no event rows:
// a flat event list stays empty after an operator's own functions ran, while
// the run rows say exactly what happened.
export type LogGroup =
  | {
      kind: "run";
      id: string;
      // Absent when the lines name a run the bounded runs read did not
      // return. The head then shows the id instead of the function.
      run?: RunDoc;
      events: EventDoc[];
      at: number;
    }
  | { kind: "server"; id: "server"; events: EventDoc[]; at: number };

export type GroupLogsOptions = {
  // A correlation id narrows the stream to that run's group alone.
  correlationId?: string;
  // A line facet (level, source, category) is set. A run with no matching
  // line is then noise, so run groups with no lines are dropped.
  lineFiltered: boolean;
};

const SERVER_ID = "server";

function eventTime(event: EventDoc): number {
  return event.createdAt ?? event._creationTime ?? 0;
}

function runTime(run: RunDoc): number {
  return run.startedAt ?? run._creationTime ?? 0;
}

export function groupLogs(
  runs: readonly RunDoc[],
  events: readonly EventDoc[],
  { correlationId, lineFiltered }: GroupLogsOptions,
): LogGroup[] {
  const scopedRuns = correlationId
    ? runs.filter((run) => run._id === correlationId)
    : runs;
  const byRun = new Map<string, LogGroup & { kind: "run" }>();
  for (const run of scopedRuns) {
    byRun.set(run._id, {
      kind: "run",
      id: run._id,
      run,
      events: [],
      at: runTime(run),
    });
  }

  const server: LogGroup & { kind: "server" } = {
    kind: "server",
    id: SERVER_ID,
    events: [],
    at: 0,
  };

  const sortedEvents = events
    .slice()
    .sort((a, b) => eventTime(b) - eventTime(a));
  for (const event of sortedEvents) {
    const key = event.correlationId ?? null;
    if (key === null) {
      if (correlationId) continue;
      server.events.push(event);
      continue;
    }
    if (correlationId && key !== correlationId) continue;
    let group = byRun.get(key);
    if (!group) {
      group = { kind: "run", id: key, events: [], at: eventTime(event) };
      byRun.set(key, group);
    }
    group.events.push(event);
  }

  const groups: LogGroup[] = [];
  for (const group of byRun.values()) {
    if (lineFiltered && group.events.length === 0) continue;
    if (!group.run && group.events.length > 0) {
      group.at = eventTime(group.events[0]);
    }
    groups.push(group);
  }
  if (server.events.length > 0) {
    server.at = eventTime(server.events[0]);
    groups.push(server);
  }
  groups.sort((a, b) => b.at - a.at);
  return groups;
}
