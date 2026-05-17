import { createFileRoute } from "@tanstack/react-router";
import { useQuery } from "nimbus/react";
import { useCallback, useMemo, useState } from "react";
import { toast } from "sonner";

import { api } from "../../../convex/_generated/api";
import { ConfirmDialog } from "../../components/confirm-dialog";
import { CopyChip } from "../../components/copy-chip";
import { StateChip } from "../../components/state-chip";
import { RelativeTime } from "../../components/time";
import { cn } from "../../lib/cn";
import { shortId } from "../../lib/format";

export const Route = createFileRoute("/admin/machines")({
  component: MachinesPage,
});

type MachineDoc = {
  _id: string;
  _creationTime?: number;
  _updateTime?: number;
  name: string;
  kind?: string;
  state: string;
  provider?: string;
  resources?: {
    cpus?: number;
    memoryMiB?: number;
    diskGiB?: number;
  };
  meta?: Record<string, unknown> | null;
};

type ServiceDoc = {
  _id: string;
  name?: string;
  state?: string;
  tenantId?: string;
  machineId?: string;
  _updateTime?: number;
};

type EventDoc = {
  _id: string;
  level?: string;
  category?: string;
  source?: string;
  message?: string;
  createdAt?: number;
  _creationTime?: number;
  data?: Record<string, unknown> | null;
};

type LifecycleAction = "start" | "stop" | "restart" | "delete";

const OPTIMISTIC_STATES: Record<LifecycleAction, string> = {
  start: "starting",
  stop: "stopping",
  restart: "restarting",
  delete: "deleting",
};

function actionsForState(state: string | undefined): LifecycleAction[] {
  const value = (state ?? "").toLowerCase();
  if (value === "running" || value === "ready" || value === "ok") {
    return ["stop", "restart"];
  }
  if (value === "failed" || value === "error") {
    return ["start", "restart", "delete"];
  }
  if (
    value === "stopped" ||
    value === "created" ||
    value === "idle" ||
    value === "pending"
  ) {
    return ["start", "delete"];
  }
  if (value === "starting" || value === "restarting" || value === "stopping") {
    return [];
  }
  return ["start", "stop", "restart", "delete"];
}

function MachinesPage() {
  const machines = useQuery(api.machines.list, {
    state: null,
    provider: null,
    limit: 200,
  }) as MachineDoc[] | undefined;

  const [selected, setSelected] = useState<string | null>(null);
  const [pending, setPending] = useState<Record<string, LifecycleAction>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [confirmDelete, setConfirmDelete] = useState<MachineDoc | null>(null);

  const selectedMachine = useMemo(() => {
    if (!machines || !selected) return null;
    return machines.find((doc) => doc._id === selected) ?? null;
  }, [machines, selected]);

  const runAction = useCallback(
    async (machine: MachineDoc, action: LifecycleAction) => {
      const key = machine._id;
      setPending((prev) => ({ ...prev, [key]: action }));
      setErrors((prev) => {
        if (!(key in prev)) return prev;
        const next = { ...prev };
        delete next[key];
        return next;
      });
      try {
        const path =
          action === "delete"
            ? `/api/machines/${encodeURIComponent(machine.name)}`
            : `/api/machines/${encodeURIComponent(machine.name)}/${action}`;
        const response = await fetch(path, {
          method: action === "delete" ? "DELETE" : "POST",
          credentials: "same-origin",
          headers: {
            "content-type": "application/json",
            accept: "application/json",
          },
          body: action === "delete" ? undefined : JSON.stringify({}),
        });
        if (!response.ok) {
          const text = await response.text().catch(() => "");
          let message = `${action} failed (${response.status})`;
          if (text) {
            try {
              const parsed = JSON.parse(text);
              if (typeof parsed === "object" && parsed && "error" in parsed) {
                message = String((parsed as { error: unknown }).error);
              } else {
                message = text;
              }
            } catch {
              message = text;
            }
          }
          throw new Error(message);
        }
        toast(`${capitalize(action)} sent to ${machine.name}`);
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        setErrors((prev) => ({ ...prev, [key]: message }));
      } finally {
        setPending((prev) => {
          const next = { ...prev };
          delete next[key];
          return next;
        });
      }
    },
    [],
  );

  const handleAction = useCallback(
    (machine: MachineDoc, action: LifecycleAction) => {
      if (action === "delete") {
        setConfirmDelete(machine);
        return;
      }
      void runAction(machine, action);
    },
    [runAction],
  );

  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-machines"
    >
      <PageHeader count={machines?.length} loading={machines === undefined} />
      <div className="flex min-h-0 flex-1 gap-4">
        <div
          className="flex min-w-0 flex-1 flex-col overflow-hidden rounded-md border border-app bg-surface"
          data-testid="machines-table-container"
        >
          {machines === undefined ? (
            <LoadingRow label="Loading machines…" />
          ) : machines.length === 0 ? (
            <EmptyState
              title="No machines"
              detail="When you create a machine with the CLI it will appear here in real time."
            />
          ) : (
            <MachineTable
              machines={machines}
              selectedId={selected}
              onSelect={setSelected}
              pending={pending}
              errors={errors}
              onAction={handleAction}
            />
          )}
        </div>
        {selectedMachine ? (
          <MachineDetail
            machine={selectedMachine}
            onClose={() => setSelected(null)}
          />
        ) : null}
      </div>
      <ConfirmDialog
        open={confirmDelete !== null}
        title={
          confirmDelete
            ? `Delete machine "${confirmDelete.name}"?`
            : "Delete machine?"
        }
        description={
          <p>
            This stops and removes the machine from this deployment. Running
            workloads are terminated. This action cannot be undone.
          </p>
        }
        confirmLabel="Delete"
        danger
        busy={confirmDelete ? pending[confirmDelete._id] === "delete" : false}
        onCancel={() => setConfirmDelete(null)}
        onConfirm={() => {
          if (!confirmDelete) return;
          const target = confirmDelete;
          setConfirmDelete(null);
          void runAction(target, "delete");
        }}
        testid="machines-delete-dialog"
      />
    </section>
  );
}

function PageHeader({
  count,
  loading,
}: {
  count: number | undefined;
  loading: boolean;
}) {
  return (
    <header className="flex items-baseline justify-between">
      <div>
        <h1
          className="text-xl text-default"
          style={{ fontSize: "var(--text-xl)" }}
        >
          Machines
        </h1>
        <p className="text-sm text-muted">
          Host and guest lifecycle. Start, stop, and inspect machines bound to
          this deployment.
        </p>
      </div>
      <div
        className="font-mono text-xs text-muted"
        data-testid="machines-total"
      >
        {loading ? "loading…" : `${count ?? 0} total`}
      </div>
    </header>
  );
}

function MachineTable({
  machines,
  selectedId,
  onSelect,
  pending,
  errors,
  onAction,
}: {
  machines: MachineDoc[];
  selectedId: string | null;
  onSelect: (id: string | null) => void;
  pending: Record<string, LifecycleAction>;
  errors: Record<string, string>;
  onAction: (machine: MachineDoc, action: LifecycleAction) => void;
}) {
  return (
    <div className="overflow-auto">
      <table
        className="w-full border-collapse text-sm"
        data-testid="machines-table"
      >
        <thead className="sticky top-0 bg-surface-2 text-[10px] uppercase tracking-[0.14em] text-muted">
          <tr>
            <Th>Name</Th>
            <Th>State</Th>
            <Th>Provider</Th>
            <Th>Kind</Th>
            <Th className="text-right">CPU</Th>
            <Th className="text-right">Memory</Th>
            <Th className="text-right">Disk</Th>
            <Th>Updated</Th>
            <Th className="text-right">Actions</Th>
          </tr>
        </thead>
        <tbody>
          {machines.map((machine) => {
            const pendingAction = pending[machine._id];
            const optimisticState = pendingAction
              ? OPTIMISTIC_STATES[pendingAction]
              : machine.state;
            const error = errors[machine._id];
            const actions = actionsForState(optimisticState);
            const isSelected = selectedId === machine._id;
            const memoryMib = machine.resources?.memoryMiB;
            return (
              <tr
                key={machine._id}
                data-testid={`machines-row-${machine.name}`}
                data-selected={isSelected || undefined}
                className={cn(
                  "border-t border-app hover:bg-surface-2",
                  isSelected && "bg-surface-2",
                )}
              >
                <Td>
                  <button
                    type="button"
                    onClick={() => onSelect(isSelected ? null : machine._id)}
                    className="font-mono text-default hover:underline"
                  >
                    {machine.name}
                  </button>
                </Td>
                <Td>
                  <div
                    className="flex flex-col gap-1"
                    data-testid={`machines-state-${machine.name}`}
                  >
                    <StateChip state={optimisticState} />
                    {error ? (
                      <span
                        className="font-mono text-[11px] text-danger"
                        data-testid={`machines-error-${machine.name}`}
                      >
                        {error}
                      </span>
                    ) : null}
                  </div>
                </Td>
                <Td>
                  <span className="font-mono text-xs text-default">
                    {machine.provider ?? "—"}
                  </span>
                </Td>
                <Td>
                  <span className="font-mono text-xs text-default">
                    {machine.kind ?? "—"}
                  </span>
                </Td>
                <Td className="text-right tabular font-mono text-xs">
                  {machine.resources?.cpus ?? "—"}
                </Td>
                <Td className="text-right tabular font-mono text-xs">
                  {formatMemory(memoryMib)}
                </Td>
                <Td className="text-right tabular font-mono text-xs">
                  {machine.resources?.diskGiB !== undefined
                    ? `${machine.resources.diskGiB} GiB`
                    : "—"}
                </Td>
                <Td>
                  {typeof machine._updateTime === "number" ? (
                    <RelativeTime epochMs={machine._updateTime} />
                  ) : (
                    <span className="tabular text-muted">—</span>
                  )}
                </Td>
                <Td className="text-right">
                  <div className="inline-flex gap-1">
                    {actions.length === 0 ? (
                      <span className="font-mono text-xs text-muted">
                        {pendingAction ? "…" : "—"}
                      </span>
                    ) : (
                      actions.map((action) => (
                        <ActionButton
                          key={action}
                          action={action}
                          busy={pendingAction === action}
                          disabled={pendingAction !== undefined}
                          onClick={() => onAction(machine, action)}
                          machineName={machine.name}
                        />
                      ))
                    )}
                  </div>
                </Td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function MachineDetail({
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
  }) as ServiceDoc[] | undefined;
  const eventsRaw = useQuery(api.events.recent, {
    source: "machine",
    level: null,
    category: null,
    correlationId: null,
    limit: 100,
  }) as EventDoc[] | undefined;
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
      className="flex w-[420px] shrink-0 flex-col gap-3 overflow-y-auto rounded-md border border-app bg-surface p-4"
      data-testid="machines-detail"
    >
      <header className="flex items-start justify-between gap-2">
        <div>
          <h2 className="font-mono text-base text-default">{machine.name}</h2>
          <p className="text-xs text-muted">{machine.kind ?? "machine"}</p>
        </div>
        <button
          type="button"
          onClick={onClose}
          aria-label="Close machine detail"
          className="rounded px-2 py-1 font-mono text-xs text-muted hover:bg-surface-2 hover:text-default"
        >
          close
        </button>
      </header>

      <Section title="Identifiers">
        <KvRow label="state">
          <StateChip state={machine.state} />
        </KvRow>
        <KvRow label="provider">
          <span className="font-mono text-xs text-default">
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
          <span className="tabular font-mono text-xs text-default">
            {machine.resources?.cpus ?? "—"}
          </span>
        </KvRow>
        <KvRow label="memory">
          <span className="tabular font-mono text-xs text-default">
            {formatMemory(machine.resources?.memoryMiB)}
          </span>
        </KvRow>
        <KvRow label="disk">
          <span className="tabular font-mono text-xs text-default">
            {machine.resources?.diskGiB !== undefined
              ? `${machine.resources.diskGiB} GiB`
              : "—"}
          </span>
        </KvRow>
      </Section>

      <Section title={`Services (${services?.length ?? 0})`}>
        {services === undefined ? (
          <span className="text-xs text-muted">Loading…</span>
        ) : services.length === 0 ? (
          <span className="text-xs text-muted">
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
                <span className="truncate text-default">
                  {svc.name ?? svc._id}
                </span>
                <StateChip state={svc.state} />
              </li>
            ))}
          </ul>
        )}
      </Section>

      <Section title="Recent events">
        {events === undefined ? (
          <span className="text-xs text-muted">Loading…</span>
        ) : events.length === 0 ? (
          <span className="text-xs text-muted">No events recorded yet.</span>
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
                  className="flex items-baseline gap-2 font-mono text-[11px]"
                >
                  <StateChip state={evt.level ?? "info"} showDot={false} />
                  <span className="flex-1 truncate text-default">
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

function ActionButton({
  action,
  busy,
  disabled,
  onClick,
  machineName,
}: {
  action: LifecycleAction;
  busy: boolean;
  disabled: boolean;
  onClick: () => void;
  machineName: string;
}) {
  const tone =
    action === "delete"
      ? "text-danger hover:bg-danger/10"
      : action === "stop"
        ? "text-warning hover:bg-warning/10"
        : "text-default hover:bg-surface-2";
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      aria-busy={busy || undefined}
      data-testid={`machines-action-${action}-${machineName}`}
      className={cn(
        "rounded border border-app px-2 py-0.5 font-mono text-[11px] uppercase tracking-wide",
        "disabled:cursor-not-allowed disabled:opacity-50",
        tone,
      )}
    >
      {busy ? "…" : action}
    </button>
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
      <h3 className="text-[10px] uppercase tracking-[0.14em] text-muted">
        {title}
      </h3>
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
      <span className="font-mono text-[11px] uppercase tracking-wide text-muted">
        {label}
      </span>
      <span className="min-w-0 text-right">{children}</span>
    </div>
  );
}

function Th({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <th
      className={cn(
        "border-b border-app px-3 py-2 text-left font-normal",
        className,
      )}
    >
      {children}
    </th>
  );
}

function Td({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <td className={cn("px-3 py-2 align-middle", className)}>{children}</td>
  );
}

function LoadingRow({ label }: { label: string }) {
  return (
    <div className="flex h-32 items-center justify-center text-xs text-muted">
      {label}
    </div>
  );
}

function EmptyState({ title, detail }: { title: string; detail: string }) {
  return (
    <div
      className="flex h-32 flex-col items-center justify-center gap-1 text-center"
      data-testid="machines-empty"
    >
      <span className="font-mono text-sm text-default">{title}</span>
      <span className="max-w-md text-xs text-muted">{detail}</span>
    </div>
  );
}

function formatMemory(mib: number | undefined): string {
  if (mib === undefined || mib === null) return "—";
  if (mib >= 1024) {
    const gib = mib / 1024;
    return `${gib % 1 === 0 ? gib.toFixed(0) : gib.toFixed(1)} GiB`;
  }
  return `${mib} MiB`;
}

function capitalize(value: string): string {
  if (value.length === 0) return value;
  return value[0].toUpperCase() + value.slice(1);
}
