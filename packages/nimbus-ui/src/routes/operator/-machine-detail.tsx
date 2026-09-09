import { useQuery } from "@nimbus/nimbus/react";
import { useMemo } from "react";

import { api } from "../../../convex/_generated/api";
import { CopyChip } from "../../components/copy-chip";
import { StatePill } from "../../components/pill";
import { RelativeTime } from "../../components/time";
import { formatMemory, shortId } from "../../lib/format";
import type { EventDoc, MachineDoc } from "./-machine-types";

// Right-hand inspector for a selected machine: status, resources, bound
// services, and the recent machine events filtered to this machine.
export function MachineDetail({
  machine,
  onClose,
}: {
  machine: MachineDoc;
  onClose: () => void;
}) {
  const services = useQuery(api.services.list, {
    tenantId: null,
    machineId: machine._id,
    state: null,
    limit: 50,
  });
  const eventsRaw = useQuery(api.events.recent, {
    source: "machine",
    level: null,
    category: null,
    correlationId: null,
    tenantId: null,
    limit: 100,
  });
  const events = useMemo<EventDoc[] | undefined>(() => {
    if (eventsRaw === undefined) return undefined;
    return eventsRaw.filter(
      (evt) =>
        evt.data &&
        typeof evt.data === "object" &&
        (evt.data as { machineId?: string }).machineId === machine.name,
    );
  }, [eventsRaw, machine.name]);

  return (
    <aside
      // 420px is the preferred width, not a floor. With `shrink-0` it was both,
      // so a narrow window kept all 420px and the page's `overflow-hidden` cut
      // off the inspector's right edge — the close button included — leaving no
      // way to dismiss it. `min-w-0` lets the panel go below its min-content
      // width so it stays whole and closeable at any width; the machines table
      // beside it still yields its space first, because its `flex-1` basis of 0
      // absorbs no shrink. The storage inspectors carry the same constraint.
      className="flex w-[420px] min-w-0 flex-col gap-3 overflow-y-auto rounded-md border border-border-2 bg-bg-panel p-4"
      data-testid="machines-detail"
    >
      <header className="flex items-start justify-between gap-2">
        <div>
          <h2 className="font-mono text-base text-text-1">{machine.name}</h2>
          <p className="text-xs text-text-3">{machine.kind ?? "machine"}</p>
        </div>
        <button
          type="button"
          onClick={onClose}
          aria-label="Close machine detail"
          className="rounded-xs px-2 py-1 font-mono text-xs text-text-3 hover:bg-bg-raised hover:text-text-1"
        >
          close
        </button>
      </header>

      <Section title="Status">
        <KvRow label="state">
          <StatePill state={machine.state} />
        </KvRow>
        <KvRow label="provider">
          <span className="font-mono text-xs text-text-1">
            {machine.provider ?? "—"}
          </span>
        </KvRow>
        <KvRow label="_id">
          <CopyChip
            label="machine id"
            value={machine._id}
            testid={`machines-detail-id-${machine.name}`}
          >
            {shortId(machine._id, 12)}
          </CopyChip>
        </KvRow>
        {typeof machine._creationTime === "number" ? (
          <KvRow label="created">
            <RelativeTime epochMs={machine._creationTime} />
          </KvRow>
        ) : null}
        {typeof machine._updateTime === "number" ? (
          <KvRow label="updated">
            <RelativeTime epochMs={machine._updateTime} />
          </KvRow>
        ) : null}
      </Section>

      <Section title="Resources">
        <KvRow label="cpus">
          <span className="tabular font-mono text-xs text-text-1">
            {machine.resources?.cpus ?? "—"}
          </span>
        </KvRow>
        <KvRow label="memory">
          <span className="tabular font-mono text-xs text-text-1">
            {formatMemory(machine.resources?.memoryMiB)}
          </span>
        </KvRow>
        <KvRow label="disk">
          <span className="tabular font-mono text-xs text-text-1">
            {machine.resources?.diskGiB !== undefined
              ? `${machine.resources.diskGiB} GiB`
              : "—"}
          </span>
        </KvRow>
      </Section>

      <Section title={`Services (${services?.length ?? 0})`}>
        {services === undefined ? (
          <span className="text-xs text-text-3">Loading…</span>
        ) : services.length === 0 ? (
          <span className="text-xs text-text-3">
            No services bound to this machine.
          </span>
        ) : (
          <ul className="flex flex-col gap-1">
            {services.map((svc) => (
              <li
                key={svc._id}
                className="flex items-center justify-between gap-2 font-mono text-xs"
                data-testid={`machines-detail-service-${svc.name ?? svc._id}`}
              >
                <span className="truncate text-text-1">
                  {svc.name ?? svc._id}
                </span>
                <StatePill state={svc.state} />
              </li>
            ))}
          </ul>
        )}
      </Section>

      <Section title="Recent events">
        {events === undefined ? (
          <span className="text-xs text-text-3">Loading…</span>
        ) : events.length === 0 ? (
          <span className="text-xs text-text-3">No events recorded yet.</span>
        ) : (
          <ul
            className="flex flex-col gap-1"
            data-testid="machines-detail-events"
          >
            {events.slice(0, 12).map((evt) => {
              const ts =
                typeof evt.createdAt === "number"
                  ? evt.createdAt
                  : typeof evt._creationTime === "number"
                    ? evt._creationTime
                    : null;
              return (
                <li
                  key={evt._id}
                  className="flex items-baseline gap-2 font-mono text-xs"
                >
                  <StatePill state={evt.level ?? "info"} />
                  <span className="flex-1 truncate text-text-1">
                    {evt.message ?? evt.category ?? "(event)"}
                  </span>
                  {ts !== null ? <RelativeTime epochMs={ts} /> : null}
                </li>
              );
            })}
          </ul>
        )}
      </Section>
    </aside>
  );
}

function Section({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <section className="flex flex-col gap-1.5">
      <h3 className="text-xs font-medium text-text-3">{title}</h3>
      <div className="flex flex-col gap-1">{children}</div>
    </section>
  );
}

function KvRow({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-2">
      <span className="text-xs font-medium text-text-3">{label}</span>
      <span className="min-w-0 text-right">{children}</span>
    </div>
  );
}
