import { useQuery } from "@nimbus/nimbus/react";
import { createFileRoute } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { toast } from "sonner";

import { api } from "../../../convex/_generated/api";
import { ConfirmDialog } from "../../components/confirm-dialog";
import { Td, Th } from "../../components/data-table";
import { EmptyState } from "../../components/empty-state";
import { SkeletonRows } from "../../components/loading-state";
import { PageHeader } from "../../components/page-header";
import { StateChip } from "../../components/state-chip";
import { RelativeTime } from "../../components/time";
import { cn } from "../../lib/cn";
import { formatMemory } from "../../lib/format";
import {
  type SubDrawerSpec,
  useContributeSubDrawer,
} from "../../shell/sub-drawer";
import { MachineDetail } from "./-machine-detail";
import type { MachineDoc } from "./-machine-types";
import {
  actionsForState,
  type LifecycleAction,
  OPTIMISTIC_STATES,
  useMachineActions,
} from "./-use-machine-actions";

export const Route = createFileRoute("/operator/machines")({
  component: MachinesPage,
});

const MACHINE_INIT_COMMAND = "nimbus machine init";

function copyMachineInitCommand() {
  navigator.clipboard.writeText(MACHINE_INIT_COMMAND).then(
    () => toast(`Copied ${MACHINE_INIT_COMMAND}`),
    () => toast.error("Failed to copy command"),
  );
}

function MachinesPage() {
  const machines = useQuery(api.machines.list, {
    state: null,
    provider: null,
    limit: 200,
  }) as MachineDoc[] | undefined;

  const subDrawerSpec = useMemo<SubDrawerSpec>(() => {
    const list = machines ?? [];
    return {
      kind: "dynamic",
      title: "Machines",
      search: { placeholder: "Filter machines" },
      children:
        machines === undefined ? (
          <div className="px-3 py-3 text-xs text-muted">
            <span aria-hidden>·</span>
            <span className="sr-only">loading</span>
          </div>
        ) : list.length === 0 ? (
          <div className="px-3 py-6 text-xs text-muted">
            <p>No machines yet.</p>
          </div>
        ) : (
          <ul className="flex flex-col gap-px px-2 py-2">
            {list.map((machine) => (
              <li key={machine._id}>
                <a
                  href={`/operator/machines?selected=${machine._id}`}
                  data-testid={`sub-drawer-item-op-${machine._id}`}
                  className="flex h-8 items-center gap-2 rounded-md px-2 text-sm text-muted hover:bg-surface-2 hover:text-default"
                >
                  <span className="flex-1 truncate">{machine.name}</span>
                  <span className="tabular font-mono text-xs uppercase tracking-[0.18em] text-muted">
                    {machine.state}
                  </span>
                </a>
              </li>
            ))}
          </ul>
        ),
    };
  }, [machines]);
  useContributeSubDrawer(subDrawerSpec);

  const [selected, setSelected] = useState<string | null>(null);
  const {
    pending,
    errors,
    confirmDelete,
    setConfirmDelete,
    runAction,
    handleAction,
  } = useMachineActions();

  const selectedMachine = useMemo(() => {
    if (!machines || !selected) return null;
    return machines.find((doc) => doc._id === selected) ?? null;
  }, [machines, selected]);

  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-machines"
    >
      <PageHeader
        title="Machines"
        subtitle="Outer Linux VMs that host sandboxes on macOS and Windows dev hosts (krunkit / WSL2). Start, stop, and inspect them. Not the same as cluster Nodes."
        trailing={
          <span
            className="font-mono text-xs text-muted"
            data-testid="machines-total"
          >
            {machines === undefined ? "loading…" : `${machines.length} total`}
          </span>
        }
      />
      <div className="flex min-h-0 flex-1 gap-4">
        <div
          className="flex min-w-0 flex-1 flex-col overflow-hidden rounded-md border border-app bg-surface"
          data-testid="machines-table-container"
        >
          {machines === undefined ? (
            // Skeleton rows, not a centered spinner: the header, the panel and
            // the 40px row rhythm all survive the load, so arriving machines
            // move nothing vertically. `table-auto` still re-proportions the
            // nine columns on arrival. No `rowContentHeight`: `Td`'s 40px row
            // floor already sizes the real and the placeholder rows alike
            // (measured 40.00px in both states).
            <SkeletonRows
              columns={9}
              head={<MachineTableHead />}
              label="Loading machines…"
              testid="machines-loading"
            />
          ) : machines.length === 0 ? (
            <EmptyState
              title="No machines"
              body={
                <>
                  Machines are the outer dev VM on macOS and Windows. Run{" "}
                  <code className="whitespace-nowrap rounded border border-app bg-surface-2 px-1 font-mono text-default">
                    {MACHINE_INIT_COMMAND}
                  </code>{" "}
                  to create one — it appears here in real time. Pure-Linux nodes
                  run sandboxes directly and have none.
                </>
              }
              cta={{ label: "Copy command", onClick: copyMachineInitCommand }}
              testid="machines-empty"
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
        <MachineTableHead />
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
                        className="font-mono text-xs text-danger"
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

function MachineTableHead() {
  return (
    <thead className="sticky top-0 bg-surface-2 text-xs uppercase tracking-[0.14em] text-muted">
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
        "rounded border border-app px-2 py-0.5 font-mono text-xs uppercase tracking-wide",
        "disabled:cursor-not-allowed disabled:opacity-50",
        tone,
      )}
    >
      {busy ? "…" : action}
    </button>
  );
}
