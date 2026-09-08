import { useQuery } from "@nimbus/nimbus/react";
import { createFileRoute } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { toast } from "sonner";
import { cn } from "@/lib/utils";
import { api } from "../../../convex/_generated/api";
import { ConfirmDialog } from "../../components/confirm-dialog";
import { EmptyState } from "../../components/empty-state";
import { SkeletonRows } from "../../components/loading-state";
import { PageHeader } from "../../components/page-header";
import { StatePill } from "../../components/pill";
import { PIN_R, Td, Th } from "../../components/table-cells";
import { RelativeTime } from "../../components/time";
import { formatMemory } from "../../lib/format";
import {
  type SubPanelSpec,
  useContributeSubPanel,
} from "../../shell/sub-panel";
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

// The clipboard API exists only in a secure context, and this console is
// normally reached over plain http on a remote host, where `navigator.clipboard`
// is not defined at all. Reading `.writeText` off it outside a try block threw
// before there was a promise to reject, so the rejection handler could never
// run: the click did nothing and said nothing. Awaiting inside the try catches
// the missing API the same way it catches a refused write, and matches the
// upgrade popover's copy action.
async function copyMachineInitCommand() {
  try {
    await navigator.clipboard.writeText(MACHINE_INIT_COMMAND);
  } catch {
    toast.error("Copy failed. The clipboard is not available.");
    return;
  }
  toast(`Copied ${MACHINE_INIT_COMMAND}`);
}

function MachinesPage() {
  const machines = useQuery(api.machines.list, {
    state: null,
    provider: null,
    limit: 200,
  }) as MachineDoc[] | undefined;

  const subPanelSpec = useMemo<SubPanelSpec>(() => {
    const list = machines ?? [];
    return {
      kind: "dynamic",
      title: "Machines",
      search: { placeholder: "Filter machines", rows: list.length },
      children:
        machines === undefined ? (
          <div className="px-3 py-3 text-xs text-text-3">
            <span aria-hidden>·</span>
            <span className="sr-only">loading</span>
          </div>
        ) : list.length === 0 ? (
          <div className="px-3 py-6 text-xs text-text-3">
            <p>No machines yet.</p>
          </div>
        ) : (
          <ul className="flex flex-col gap-px px-2 py-2">
            {list.map((machine) => (
              <li key={machine._id}>
                <a
                  href={`/operator/machines?selected=${machine._id}`}
                  data-testid={`sub-panel-item-op-${machine._id}`}
                  className="flex h-8 items-center gap-2 rounded-md px-2 text-sm text-text-3 hover:bg-bg-raised hover:text-text-1"
                >
                  <span className="flex-1 truncate">{machine.name}</span>
                  <span className="tabular text-xs font-medium text-text-3">
                    {machine.state}
                  </span>
                </a>
              </li>
            ))}
          </ul>
        ),
    };
  }, [machines]);
  useContributeSubPanel(subPanelSpec);

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
        subtitle="Outer Linux VMs hosting sandboxes on macOS/Windows dev hosts (krunkit / WSL2). Not cluster Nodes."
        trailing={
          <span
            className="font-mono text-xs text-text-3"
            data-testid="machines-total"
          >
            {machines === undefined ? "loading…" : `${machines.length} total`}
          </span>
        }
      />
      <div className="flex min-h-0 flex-1 gap-4">
        <div
          className="flex min-w-0 flex-1 flex-col overflow-hidden rounded-md border border-border-2 bg-bg-panel"
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
                  <code className="whitespace-nowrap rounded-xs border border-border-2 bg-bg-raised px-1 font-mono text-text-1">
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
    <div className="h-full overflow-auto">
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
                  "border-t border-border-2 hover:bg-bg-raised",
                  // The pinned Actions cell paints `--row-bg`, so the row has
                  // to publish its own background — hover included, or the
                  // data columns show through the cell exactly while the
                  // operator is aiming at it.
                  isSelected
                    ? "bg-bg-raised [--row-bg:var(--bg-raised)]"
                    : "[--row-bg:var(--bg-panel)] hover:[--row-bg:var(--bg-raised)]",
                )}
              >
                <Td>
                  <button
                    type="button"
                    onClick={() => onSelect(isSelected ? null : machine._id)}
                    className="font-mono text-text-1 hover:underline"
                  >
                    {machine.name}
                  </button>
                </Td>
                <Td>
                  <div
                    className="flex flex-col gap-1"
                    data-testid={`machines-state-${machine.name}`}
                  >
                    <StatePill state={optimisticState} />
                    {error ? (
                      <span
                        className="font-mono text-xs text-error"
                        data-testid={`machines-error-${machine.name}`}
                      >
                        {error}
                      </span>
                    ) : null}
                  </div>
                </Td>
                <Td>
                  <span className="font-mono text-xs text-text-1">
                    {machine.provider ?? "—"}
                  </span>
                </Td>
                <Td>
                  <span className="font-mono text-xs text-text-1">
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
                    <span className="tabular text-text-3">—</span>
                  )}
                </Td>
                <Td
                  className={cn(
                    "w-px border-l border-border-2 text-right",
                    PIN_R,
                  )}
                >
                  <div className="inline-flex gap-1">
                    {actions.length === 0 ? (
                      <span className="font-mono text-xs text-text-3">
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
    <thead className="sticky top-0 z-20 bg-bg-raised [--row-bg:var(--bg-raised)] text-xs font-medium text-text-3">
      <tr>
        <Th>Name</Th>
        <Th>State</Th>
        <Th>Provider</Th>
        <Th>Kind</Th>
        <Th className="text-right">CPU</Th>
        <Th className="text-right">Memory</Th>
        <Th className="text-right">Disk</Th>
        <Th>Updated</Th>
        <Th className={cn("w-px border-l border-border-2 text-right", PIN_R)}>
          Actions
        </Th>
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
  // Graying out swaps the tone rather than dimming it. CSS `opacity`
  // composites in gamma-encoded sRGB, so the `opacity-50` this used to carry
  // measured between 1.5:1 and 3.1:1 for the delete and stop tones against the
  // row behind them — unreadable, worst in the mono dark palette. `text-text-3`
  // is a palette token and stays above 4.5:1 in every palette and theme.
  const tone = disabled
    ? "text-text-3"
    : action === "delete"
      ? "text-error hover:bg-error/10"
      : action === "stop"
        ? "text-warning hover:bg-warning/10"
        : "text-text-1 hover:bg-bg-raised";
  return (
    <button
      type="button"
      onClick={() => {
        if (disabled) return;
        onClick();
      }}
      // `aria-disabled`, not `disabled`, so the control keeps its tab stop:
      // a disabled element cannot take focus, which turns a focus restore
      // into a silent no-op and leaves the operator on <body>.
      //
      // On this page the branch is unreached today, and deliberately kept.
      // `OPTIMISTIC_STATES` maps every action onto an in-flight state and
      // `actionsForState` offers nothing for those, so a row with a pending
      // action renders no buttons rather than grayed-out ones — `disabled`
      // is therefore always false wherever this component is mounted. It
      // stays because it is this component's contract rather than the
      // caller's, and the caller is one edit away from graying a row in
      // place. `machines.spec.tsx` pins the render rule that makes it dead,
      // so a change to either side shows up as a failing test.
      aria-disabled={disabled}
      aria-busy={busy || undefined}
      data-testid={`machines-action-${action}-${machineName}`}
      className={cn(
        "rounded-xs border border-border-2 px-2 py-0.5 text-xs font-medium",
        "aria-disabled:cursor-not-allowed",
        tone,
      )}
    >
      {busy ? "…" : action}
    </button>
  );
}
