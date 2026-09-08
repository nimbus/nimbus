import { useQuery } from "@nimbus/nimbus/react";
import { Link } from "@tanstack/react-router";
import { useMemo } from "react";

import { api } from "../../../../convex/_generated/api";
import { EmptyState } from "../../../components/empty-state";
import { StatePill } from "../../../components/pill";
import { RelativeTime } from "../../../components/time";
import type { EventDoc } from "../observability/-types";

// The lines a service wrote. The service manager records each lifecycle
// step as a system event with `source: "service"`, the service name in
// `data.serviceName`, and a correlation id of `service:<tenant>:<name>:<action>`,
// so the read is one reactive query on the source, narrowed here to this
// service by either mark.
export function serviceOwnsEvent(
  event: EventDoc,
  tenantId: string,
  serviceName: string,
): boolean {
  const data = event.data ?? undefined;
  if (data && data.serviceName === serviceName) {
    return data.tenantId === undefined || data.tenantId === tenantId;
  }
  const correlation = event.correlationId ?? "";
  return correlation.startsWith(`service:${tenantId}:${serviceName}:`);
}

export function ServiceLogs({
  tenantId,
  serviceName,
  testid,
}: {
  tenantId: string | undefined;
  serviceName: string | undefined;
  testid: string;
}) {
  const events = useQuery(api.events.recent, {
    source: "service",
    tenantId: tenantId ?? null,
    level: null,
    category: null,
    correlationId: null,
    limit: 200,
  }) as EventDoc[] | undefined;

  const lines = useMemo(() => {
    if (!events || !tenantId || !serviceName) return [];
    return events
      .filter((event) => serviceOwnsEvent(event, tenantId, serviceName))
      .sort(
        (a, b) =>
          (b.createdAt ?? b._creationTime ?? 0) -
          (a.createdAt ?? a._creationTime ?? 0),
      );
  }, [events, tenantId, serviceName]);

  const logsSearch = {
    tab: "logs" as const,
    source: "service",
    tenant: tenantId,
  };

  return (
    <div
      className="flex h-full min-h-0 flex-col overflow-hidden"
      data-testid={testid}
    >
      <div className="flex shrink-0 items-baseline justify-between border-b border-border-2 px-6 py-3">
        <span className="text-xs font-medium text-text-3">
          {events === undefined
            ? "Reading service events…"
            : `${lines.length} line${lines.length === 1 ? "" : "s"} from the service manager, newest first. Updates live.`}
        </span>
        <Link
          to="/developer/observability"
          search={logsSearch}
          className="text-xs font-medium text-text-3 hover:text-text-1 focus-visible:text-text-1"
          data-testid={`${testid}-open`}
        >
          open in Observability →
        </Link>
      </div>
      {events !== undefined && lines.length === 0 ? (
        <EmptyState
          title="No log lines yet"
          body="The service manager writes a line for every start, stop, and restart it completes. Send a lifecycle action and the line lands here."
          testid={`${testid}-empty`}
        />
      ) : (
        <ul
          className="min-h-0 flex-1 divide-y divide-border-2 overflow-auto"
          aria-busy={events === undefined || undefined}
        >
          {lines.map((event) => (
            <li
              key={event._id}
              className="grid grid-cols-[auto_auto_auto_1fr] items-baseline gap-3 px-6 py-1.5 text-xs"
              data-testid={`${testid}-line-${event._id}`}
            >
              <RelativeTime
                epochMs={event.createdAt ?? event._creationTime ?? 0}
              />
              <StatePill state={event.level ?? "info"} />
              <span className="font-medium text-text-3">
                {event.category ?? "—"}
              </span>
              <span
                className="truncate font-mono text-text-1"
                title={event.message}
              >
                {event.message ?? "(no message)"}
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
