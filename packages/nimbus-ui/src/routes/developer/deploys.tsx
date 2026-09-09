import { createFileRoute } from "@tanstack/react-router";
import { Ellipsis, Rocket } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { toast } from "sonner";

import { ConfirmDialog } from "../../components/confirm-dialog";
import { CopyChip } from "../../components/copy-chip";
import {
  DataTable,
  dataColumns,
  type RowAnchor,
} from "../../components/data-table";
import { EmptyState } from "../../components/empty-state";
import { LoadingState } from "../../components/loading-state";
import { PageHeader } from "../../components/page-header";
import { CategoryPill, StatePill } from "../../components/pill";
import {
  RowContextMenu,
  type RowMenuItem,
} from "../../components/storage/row-context-menu";
import { RelativeTime } from "../../components/time";
import { Button } from "../../components/ui/button";
import { useApiRead } from "../../hooks/use-api-read";
import { deploys } from "../../lib/api-mutations";
import { shortHash } from "../../lib/format";
import {
  canRollBack,
  type DeployActivation,
  type DeployHistory,
  diffFunctionPaths,
  type FunctionPathDiff,
} from "../../lib/types/deploy";
import type { LoadingValue } from "../../shell/loading-value";
import {
  type SubPanelSpec,
  useContributeSubPanel,
} from "../../shell/sub-panel";
import { DeploySubPanel } from "./deploys/-deploy-sub-panel";

export const Route = createFileRoute("/developer/deploys")({
  component: DeploysPage,
});

// Deploy history is server-wide: every bundle activation this server has
// recorded, whichever tenant silo ran it. The page reads the local-admin
// route once and again after a rollback; there is no subscription because
// an activation is a rare, whole-server event, not a document stream.
function DeploysPage() {
  const [revision, setRevision] = useState(0);
  const bump = useCallback(() => setRevision((n) => n + 1), []);
  const read = useApiRead<DeployHistory>(
    "/api/admin/deploys",
    undefined,
    revision,
  );

  const spec = useMemo<SubPanelSpec>(
    () => ({
      kind: "dynamic",
      title: "Deploys",
      children: <DeploySubPanel read={read} />,
    }),
    [read],
  );
  useContributeSubPanel(spec);

  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-deploys"
    >
      <PageHeader title="Deploys" subtitle={SUBTITLE} />
      <div className="min-h-0 flex-1 overflow-hidden rounded-md border border-border-2 bg-bg-panel">
        <DeploysBody read={read} onChanged={bump} />
      </div>
    </section>
  );
}

const SUBTITLE =
  "Every bundle activation on this server, newest first. Roll back to any retained bundle.";

function DeploysBody({
  read,
  onChanged,
}: {
  read: LoadingValue<DeployHistory>;
  onChanged: () => void;
}) {
  if (read.kind === "loading") {
    return (
      <LoadingState label="Reading deploy history…" testid="deploys-loading" />
    );
  }
  if (read.kind === "offline") {
    return (
      <EmptyState
        icon={Rocket}
        title="Server unreachable"
        body="The deploy history could not be read. Check the server, then reload."
        testid="deploys-offline"
      />
    );
  }
  if (read.kind === "error") {
    return (
      <EmptyState
        icon={Rocket}
        title="Deploy history did not load"
        body={read.message}
        testid="deploys-error"
      />
    );
  }
  if (read.value.activations.length === 0) {
    return (
      <EmptyState
        icon={Rocket}
        title="No deploys yet"
        body="A deploy arrives through the CLI. Run it against this server to publish a bundle; every activation lands here with its hash, and a retained bundle can be rolled back to."
        snippet="nimbus deploy"
        testid="deploys-empty"
      />
    );
  }
  return <DeploysTable history={read.value} onChanged={onChanged} />;
}

type DeployRow = DeployActivation & {
  isActive: boolean;
  // Function paths this activation added and removed against the one
  // before it in time, so the history reads as a sequence of changes.
  delta: FunctionPathDiff | null;
};

type MenuState = RowAnchor & { row: DeployRow };

const col = dataColumns<DeployRow>();

function rowId(row: DeployActivation): string {
  return `${row.generation}`;
}

// One row per activation with its hash, kind and function count; the row
// menu and Enter select a row, and the strip under the table compares the
// selected bundle's function paths with the active one. Roll back sits
// behind a confirmation, and only on a retained bundle that is not the
// active one: the server refuses both, and the console does not offer
// what the server will refuse.
export function DeploysTable({
  history,
  onChanged,
}: {
  history: DeployHistory;
  onChanged: () => void;
}) {
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [rollingBack, setRollingBack] = useState<DeployRow | null>(null);
  const [busy, setBusy] = useState(false);
  const [rollbackError, setRollbackError] = useState<string | undefined>();

  const rows = useMemo<DeployRow[]>(
    () =>
      history.activations.map((activation, index) => {
        const previous = history.activations[index + 1];
        return {
          ...activation,
          isActive: activation.sha256 === history.active,
          delta: previous
            ? diffFunctionPaths(previous.functions, activation.functions)
            : null,
        };
      }),
    [history],
  );
  const active = useMemo(
    () => rows.find((row) => row.isActive) ?? null,
    [rows],
  );
  const selected = useMemo(
    () => rows.find((row) => rowId(row) === selectedId) ?? null,
    [rows, selectedId],
  );

  const rollBack = useCallback(
    async (row: DeployRow) => {
      setBusy(true);
      setRollbackError(undefined);
      const result = await deploys.rollback(row.sha256);
      setBusy(false);
      if (!result.ok) {
        setRollbackError(result.error);
        return;
      }
      setRollingBack(null);
      toast.success(
        `Rolled back to ${shortHash(row.sha256)} as generation ${result.data.generation}`,
      );
      onChanged();
    },
    [onChanged],
  );

  const menuItems = useCallback(
    (row: DeployRow): RowMenuItem[] => [
      {
        id: "compare",
        label: row.isActive ? "Show functions" : "Compare with active",
        onSelect: () => setSelectedId(rowId(row)),
      },
      {
        id: "rollback",
        label: "Roll back to this bundle",
        hint: row.isActive
          ? "already active"
          : row.retained
            ? "re-activates the retained files"
            : "files not retained",
        danger: true,
        disabled: !canRollBack(row, history.active),
        onSelect: () => {
          setRollbackError(undefined);
          setRollingBack(row);
        },
      },
    ],
    [history.active],
  );

  const columns = useMemo(
    () => [
      col.accessor("activatedAt", {
        header: "When",
        size: 96,
        cell: (ctx) => <RelativeTime epochMs={ctx.getValue()} />,
      }),
      col.accessor("sha256", {
        header: "Bundle",
        size: 180,
        cell: (ctx) => {
          const row = ctx.row.original;
          return (
            <span className="flex min-w-0 items-center gap-2">
              <CopyChip
                label="bundle sha256"
                value={row.sha256}
                testid={`deploys-sha-${row.generation}`}
              >
                <span className="font-mono text-xs">
                  {shortHash(row.sha256)}
                </span>
              </CopyChip>
              {row.isActive ? (
                <StatePill
                  state="active"
                  data-testid={`deploys-active-${row.generation}`}
                />
              ) : null}
            </span>
          );
        },
      }),
      col.accessor("kind", {
        header: "Kind",
        size: 88,
        cell: (ctx) => <CategoryPill value={ctx.getValue()} />,
      }),
      col.accessor("generation", {
        header: () => <span className="block text-right">Gen</span>,
        size: 56,
        cell: (ctx) => (
          <span className="block text-right font-mono text-xs tabular text-text-3">
            {ctx.getValue()}
          </span>
        ),
      }),
      col.accessor("actor", {
        header: "Actor",
        size: 120,
        cell: (ctx) => (
          <span className="block truncate font-mono text-xs text-text-3">
            {ctx.getValue()}
          </span>
        ),
      }),
      col.accessor((row) => row.functions.length, {
        id: "functions",
        header: () => <span className="block text-right">Functions</span>,
        size: 120,
        cell: (ctx) => {
          const row = ctx.row.original;
          return (
            <span
              className="flex justify-end gap-1.5 font-mono text-xs tabular"
              data-testid={`deploys-functions-${row.generation}`}
            >
              <span className="text-text-1">{ctx.getValue()}</span>
              {row.delta && row.delta.added.length > 0 ? (
                <span className="text-success">+{row.delta.added.length}</span>
              ) : null}
              {row.delta && row.delta.removed.length > 0 ? (
                <span className="text-error">−{row.delta.removed.length}</span>
              ) : null}
            </span>
          );
        },
      }),
      col.accessor("retained", {
        header: "Retained",
        size: 80,
        cell: (ctx) => (
          <span className="block text-xs text-text-3">
            {ctx.getValue() ? "yes" : "no"}
          </span>
        ),
      }),
      col.display({
        id: "actions",
        header: () => <span className="sr-only">Actions</span>,
        size: 40,
        enableSorting: false,
        cell: (ctx) => {
          const row = ctx.row.original;
          return (
            <span className="flex justify-end">
              <Button
                type="button"
                variant="ghost"
                size="icon-xs"
                aria-label={`Actions for generation ${row.generation}`}
                data-testid={`deploys-row-actions-${row.generation}`}
                onClick={(event) => {
                  const rect = event.currentTarget.getBoundingClientRect();
                  setMenu({
                    row,
                    x: rect.left,
                    y: rect.bottom + 2,
                    element: event.currentTarget,
                  });
                }}
              >
                <Ellipsis />
              </Button>
            </span>
          );
        },
      }),
    ],
    [],
  );

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="min-h-0 flex-1">
        <DataTable
          columns={columns}
          data={rows}
          getRowId={rowId}
          ariaLabel="Deploy history"
          onRowActivate={(row) => setSelectedId(rowId(row))}
          onRowContextMenu={(row, anchor) => setMenu({ ...anchor, row })}
          rowTestid={(row) => `deploys-row-${row.generation}`}
          testid="deploys-table"
          className="h-full"
        />
      </div>
      {selected ? (
        <FunctionDiffStrip
          selected={selected}
          active={active}
          onClose={() => setSelectedId(null)}
        />
      ) : null}
      {menu ? (
        <RowContextMenu
          x={menu.x}
          y={menu.y}
          label={`Actions for generation ${menu.row.generation}`}
          items={menuItems(menu.row)}
          restoreFocus={menu.element}
          onClose={() => setMenu(null)}
          testid="deploys-row-menu"
        />
      ) : null}
      <ConfirmDialog
        open={rollingBack !== null}
        title={
          rollingBack
            ? `Roll back to ${shortHash(rollingBack.sha256)}?`
            : "Roll back?"
        }
        description={
          rollingBack ? (
            <RollbackDescription row={rollingBack} active={active} />
          ) : undefined
        }
        confirmLabel="Roll back"
        danger
        busy={busy}
        error={rollbackError}
        onConfirm={() => {
          if (rollingBack) void rollBack(rollingBack);
        }}
        onCancel={() => {
          setRollingBack(null);
          setRollbackError(undefined);
        }}
        testid="deploys-rollback-dialog"
      />
    </div>
  );
}

// The confirmation says what the rollback does to the function inventory,
// because a path that exists only in the active bundle stops resolving
// the moment the older bundle takes over.
function RollbackDescription({
  row,
  active,
}: {
  row: DeployRow;
  active: DeployRow | null;
}) {
  const diff = active
    ? diffFunctionPaths(active.functions, row.functions)
    : null;
  return (
    <span className="flex flex-col gap-1">
      <span>
        The retained files of bundle{" "}
        <span className="font-mono">{shortHash(row.sha256)}</span> pass the same
        integrity check as a deploy, then become the next generation. The bundle
        serving requests now stays in the history.
      </span>
      {diff ? (
        <span data-testid="deploys-rollback-impact">
          {diff.removed.length === 0 && diff.added.length === 0
            ? "The function paths do not change."
            : `${diff.removed.length} function path${diff.removed.length === 1 ? "" : "s"} stop resolving and ${diff.added.length} come back.`}
        </span>
      ) : null}
    </span>
  );
}

// The strip under the table: the selected bundle's function paths against
// the active bundle's, as three lists. On the active row it is the plain
// inventory.
function FunctionDiffStrip({
  selected,
  active,
  onClose,
}: {
  selected: DeployRow;
  active: DeployRow | null;
  onClose: () => void;
}) {
  const diff = useMemo(
    () =>
      active && !selected.isActive
        ? diffFunctionPaths(active.functions, selected.functions)
        : null,
    [active, selected],
  );
  const inventory = useMemo(
    () => [...selected.functions].sort((a, b) => a.path.localeCompare(b.path)),
    [selected],
  );
  return (
    <div
      className="flex max-h-[45%] min-h-0 flex-col gap-2 border-t border-border-2 bg-bg-raised/40 px-4 py-3"
      data-testid="deploys-diff"
    >
      <div className="flex items-center justify-between gap-2">
        <span className="text-xs text-text-3">
          {diff ? (
            <>
              Generation {selected.generation}{" "}
              <span className="font-mono">{shortHash(selected.sha256)}</span>{" "}
              against the active bundle
              {active ? (
                <>
                  {" "}
                  <span className="font-mono">{shortHash(active.sha256)}</span>
                </>
              ) : null}
              .
            </>
          ) : (
            <>
              Functions in generation {selected.generation}{" "}
              <span className="font-mono">{shortHash(selected.sha256)}</span>
              {selected.isActive ? ", the active bundle" : ""}.
            </>
          )}
        </span>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={onClose}
          data-testid="deploys-diff-close"
        >
          Close
        </Button>
      </div>
      <div className="grid min-h-0 grid-cols-1 gap-3 overflow-auto md:grid-cols-3">
        {diff ? (
          <>
            <PathList
              title="Come back"
              tone="text-success"
              paths={diff.added}
              testid="deploys-diff-added"
            />
            <PathList
              title="Stop resolving"
              tone="text-error"
              paths={diff.removed}
              testid="deploys-diff-removed"
            />
            <PathList
              title="Unchanged"
              tone="text-text-3"
              paths={diff.kept}
              testid="deploys-diff-kept"
            />
          </>
        ) : (
          <ul
            className="col-span-full flex flex-col gap-0.5"
            data-testid="deploys-inventory"
          >
            {inventory.map((fn) => (
              <li key={fn.path} className="flex items-baseline gap-2 text-xs">
                <span className="font-mono text-text-1">{fn.path}</span>
                <span className="text-text-3">{fn.kind}</span>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}

function PathList({
  title,
  tone,
  paths,
  testid,
}: {
  title: string;
  tone: string;
  paths: string[];
  testid: string;
}) {
  return (
    <div className="min-w-0" data-testid={testid}>
      <h3 className={`mb-1 text-xs font-medium ${tone}`}>
        {title} ({paths.length})
      </h3>
      {paths.length === 0 ? (
        <p className="text-xs text-text-3">—</p>
      ) : (
        <ul className="flex flex-col gap-0.5">
          {paths.map((path) => (
            <li key={path} className="truncate font-mono text-xs" title={path}>
              {path}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
