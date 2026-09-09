import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { Box, Ellipsis, Plus } from "lucide-react";
import { useCallback, useMemo, useState } from "react";

import { ConfirmDialog } from "../../components/confirm-dialog";
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
import type { SandboxResource } from "../../lib/types/sandbox";
import { sandboxCanStop, sandboxDisplayName } from "../../lib/types/sandbox";
import type { LoadingValue } from "../../shell/loading-value";
import {
  type SubPanelSpec,
  useContributeSubPanel,
} from "../../shell/sub-panel";
import { useUiStore } from "../../store/ui-store";
import { CreateSandboxDialog } from "./sandboxes/-create-sandbox-dialog";
import { useSandboxActions } from "./sandboxes/-sandbox-actions";
import {
  type SandboxListRead,
  useSandboxList,
} from "./sandboxes/-sandbox-read";
import { SandboxSubPanel } from "./sandboxes/-sandbox-sub-panel";

export const Route = createFileRoute("/developer/sandboxes")({
  component: SandboxesPage,
});

// Sandboxes are live runtime state read over the service-control HTTP
// routes, not documents in the engine: the page reads and polls them itself
// instead of going through a router loader, so a stop or a create shows
// its transition without a navigation.
function SandboxesPage() {
  const activeTenant = useUiStore((s) => s.activeTenant);
  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-sandboxes"
    >
      {activeTenant === null ? (
        <NoTenant />
      ) : (
        <TenantSandboxes tenant={activeTenant} />
      )}
    </section>
  );
}

function NoTenant() {
  const spec = useMemo<SubPanelSpec>(
    () => ({
      kind: "dynamic",
      title: "Sandboxes",
      search: { placeholder: "Filter sandboxes", rows: 0 },
      children: (
        <SandboxSubPanel read={{ kind: "loading" }} activeTenant={null} />
      ),
    }),
    [],
  );
  useContributeSubPanel(spec);
  return (
    <>
      <PageHeader title="Sandboxes" subtitle={SUBTITLE} />
      <div className="min-h-0 flex-1 overflow-hidden rounded-md border border-border-2 bg-bg-panel">
        <EmptyState
          icon={Box}
          title="Choose a tenant"
          body="Sandboxes belong to a tenant. Pick one in the sidebar to list its live sandboxes."
          testid="sandboxes-no-tenant"
        />
      </div>
    </>
  );
}

function TenantSandboxes({ tenant }: { tenant: string }) {
  const navigate = useNavigate();
  const [revision, setRevision] = useState(0);
  const bump = useCallback(() => setRevision((n) => n + 1), []);
  const read = useSandboxList(tenant, revision);
  const [creating, setCreating] = useState(false);

  const items = useMemo(
    () =>
      read.kind === "ok" && read.value.kind === "list" ? read.value.items : [],
    [read],
  );
  const spec = useMemo<SubPanelSpec>(
    () => ({
      kind: "dynamic",
      title: "Sandboxes",
      search: { placeholder: "Filter sandboxes", rows: items.length },
      children: <SandboxSubPanel read={read} activeTenant={tenant} />,
    }),
    [read, items.length, tenant],
  );
  useContributeSubPanel(spec);

  const canCreate = read.kind === "ok" && read.value.kind === "list";

  return (
    <>
      <PageHeader
        title="Sandboxes"
        subtitle={SUBTITLE}
        trailing={
          <span className="flex items-center gap-2">
            <ScopeChip tenant={tenant} />
            <Button
              type="button"
              size="sm"
              disabled={!canCreate}
              onClick={() => setCreating(true)}
              data-testid="sandboxes-create"
            >
              <Plus className="size-3.5" aria-hidden /> New sandbox
            </Button>
          </span>
        }
      />
      <div className="min-h-0 flex-1 overflow-hidden rounded-md border border-border-2 bg-bg-panel">
        <SandboxesBody
          tenant={tenant}
          read={read}
          onChanged={bump}
          onCreate={() => setCreating(true)}
        />
      </div>
      <CreateSandboxDialog
        open={creating}
        tenant={tenant}
        onCancel={() => setCreating(false)}
        onCreated={(sandbox) => {
          setCreating(false);
          bump();
          void navigate({
            to: "/developer/sandboxes/$sandbox",
            params: { sandbox: sandbox.metadata.id },
          });
        }}
      />
    </>
  );
}

function SandboxesBody({
  tenant,
  read,
  onChanged,
  onCreate,
}: {
  tenant: string;
  read: LoadingValue<SandboxListRead>;
  onChanged: () => void;
  onCreate: () => void;
}) {
  if (read.kind === "loading") {
    return (
      <LoadingState label="Reading sandboxes…" testid="sandboxes-loading" />
    );
  }
  if (read.kind === "offline") {
    return (
      <EmptyState
        icon={Box}
        title="Server unreachable"
        body="The sandbox list could not be read. Check the server, then reload."
        testid="sandboxes-offline"
      />
    );
  }
  if (read.kind === "error") {
    return (
      <EmptyState
        icon={Box}
        title="Sandboxes did not load"
        body={read.message}
        testid="sandboxes-error"
      />
    );
  }
  if (read.value.kind === "unavailable") {
    return (
      <EmptyState
        icon={Box}
        title="Sandbox routes not available on this server"
        body={
          <>
            This server runs no service manager, so it has no sandbox routes.
            Start a server with managed workloads to list and run sandboxes. The
            server said: {read.value.message}
          </>
        }
        testid="sandboxes-unavailable"
      />
    );
  }
  if (read.value.items.length === 0) {
    return (
      <EmptyState
        icon={Box}
        title="No live sandboxes"
        body={`Tenant ${tenant} has no sandbox running. Create one from an OCI image, or declare services in compose.yaml.`}
        cta={{ label: "New sandbox", onClick: onCreate }}
        testid="sandboxes-empty"
      />
    );
  }
  return <SandboxesTable sandboxes={read.value.items} onChanged={onChanged} />;
}

const SUBTITLE =
  "Live sandboxes in this tenant: microVMs and containers, with a console on each.";

function ScopeChip({ tenant }: { tenant: string }) {
  return (
    <span
      className="inline-flex items-center gap-1 rounded-xs border border-border-2 px-2 py-0.5 font-mono text-xs text-text-3"
      data-testid="sandboxes-scope"
    >
      <span className="font-medium">tenant</span>
      <span className="font-mono text-text-1">{tenant}</span>
    </span>
  );
}

type SandboxRow = SandboxResource & {
  shownState: string;
  actionError: string | undefined;
};

type MenuState = RowAnchor & { row: SandboxRow };

const col = dataColumns<SandboxRow>();

function idOf(row: SandboxResource): string {
  return row.metadata.id;
}

// One row per sandbox with its lifecycle pill. The row menu opens the
// sandbox or its console, and stops it behind a confirmation: a stopped
// sandbox does not come back, so the terminal action is never one click.
export function SandboxesTable({
  sandboxes,
  onChanged,
}: {
  sandboxes: SandboxResource[];
  onChanged: () => void;
}) {
  const navigate = useNavigate();
  const { pending, errors, stop } = useSandboxActions(onChanged);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [stopping, setStopping] = useState<SandboxRow | null>(null);

  const rows = useMemo<SandboxRow[]>(
    () =>
      sandboxes.map((sandbox) => ({
        ...sandbox,
        shownState: pending[idOf(sandbox)]
          ? "stopping"
          : sandbox.status.lifecycleState,
        actionError: errors[idOf(sandbox)],
      })),
    [sandboxes, pending, errors],
  );

  const openRow = useCallback(
    (row: SandboxRow, tab?: "console") =>
      navigate({
        to: "/developer/sandboxes/$sandbox",
        params: { sandbox: idOf(row) },
        search: tab ? { tab } : {},
      }),
    [navigate],
  );

  const menuItems = useCallback(
    (row: SandboxRow): RowMenuItem[] => [
      { id: "open", label: "Open sandbox", onSelect: () => void openRow(row) },
      {
        id: "console",
        label: "Open console",
        onSelect: () => void openRow(row, "console"),
      },
      ...(sandboxCanStop(row.shownState)
        ? [
            {
              id: "stop",
              label: "Stop sandbox",
              hint: "cannot be started again",
              danger: true,
              onSelect: () => setStopping(row),
            } satisfies RowMenuItem,
          ]
        : []),
    ],
    [openRow],
  );

  const columns = useMemo(
    () => [
      col.accessor((row) => sandboxDisplayName(row), {
        id: "name",
        header: "Name",
        size: 140,
        cell: (ctx) => {
          const row = ctx.row.original;
          return (
            <Link
              to="/developer/sandboxes/$sandbox"
              params={{ sandbox: idOf(row) }}
              className="block truncate font-mono text-text-1 hover:underline"
              title={idOf(row)}
              data-testid={`sandboxes-link-${idOf(row)}`}
            >
              {ctx.getValue()}
            </Link>
          );
        },
      }),
      col.accessor("shownState", {
        header: "State",
        size: 104,
        cell: (ctx) => {
          const row = ctx.row.original;
          return (
            <span className="flex min-w-0 flex-col gap-0.5">
              <StatePill
                state={ctx.getValue()}
                data-testid={`sandboxes-state-${idOf(row)}`}
              />
              {row.actionError ? (
                <span
                  className="truncate text-xs text-[color:var(--error)]"
                  title={row.actionError}
                  data-testid={`sandboxes-row-error-${idOf(row)}`}
                >
                  {row.actionError}
                </span>
              ) : null}
            </span>
          );
        },
      }),
      col.accessor((row) => row.spec.profile, {
        id: "profile",
        header: "Profile",
        size: 88,
        cell: (ctx) => <CategoryPill value={ctx.getValue()} />,
      }),
      col.accessor((row) => row.status.backend, {
        id: "backend",
        header: "Backend",
        size: 96,
        cell: (ctx) => <CategoryPill value={ctx.getValue()} />,
      }),
      col.accessor((row) => row.status.health, {
        id: "health",
        header: "Health",
        size: 88,
        cell: (ctx) => (
          <span className="block truncate font-mono text-xs text-text-3">
            {ctx.getValue()}
          </span>
        ),
      }),
      col.accessor((row) => row.status.endpoints.length, {
        id: "endpoints",
        header: () => <span className="block text-right">Endpoints</span>,
        size: 88,
        cell: (ctx) => (
          <span className="block text-right font-mono text-xs tabular text-text-3">
            {ctx.getValue()}
          </span>
        ),
      }),
      col.accessor((row) => Date.parse(row.metadata.updatedAt), {
        id: "updated",
        header: "Updated",
        size: 88,
        cell: (ctx) => {
          const at = ctx.getValue();
          return Number.isFinite(at) ? (
            <RelativeTime epochMs={at} />
          ) : (
            <span className="tabular text-text-3">—</span>
          );
        },
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
                aria-label={`Actions for ${sandboxDisplayName(row)}`}
                data-testid={`sandboxes-row-actions-${idOf(row)}`}
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
    <>
      <DataTable
        columns={columns}
        data={rows}
        getRowId={idOf}
        ariaLabel="Sandboxes"
        onRowActivate={(row) => void openRow(row)}
        onRowContextMenu={(row, anchor) => setMenu({ ...anchor, row })}
        rowTestid={(row) => `sandboxes-row-${idOf(row)}`}
        testid="sandboxes-table"
        className="h-full"
      />
      {menu ? (
        <RowContextMenu
          x={menu.x}
          y={menu.y}
          label={`Actions for ${sandboxDisplayName(menu.row)}`}
          items={menuItems(menu.row)}
          restoreFocus={menu.element}
          onClose={() => setMenu(null)}
          testid="sandboxes-row-menu"
        />
      ) : null}
      <StopSandboxDialog
        sandbox={stopping}
        onConfirm={() => {
          if (stopping) void stop(stopping);
          setStopping(null);
        }}
        onCancel={() => setStopping(null)}
      />
    </>
  );
}

// The one confirmation both the list and the detail page put in front of
// stop. There is no start route: a stopped sandbox is gone for good and a
// new one takes its place.
export function StopSandboxDialog({
  sandbox,
  onConfirm,
  onCancel,
}: {
  sandbox: SandboxResource | null;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <ConfirmDialog
      open={sandbox !== null}
      title={sandbox ? `Stop ${sandboxDisplayName(sandbox)}?` : "Stop sandbox?"}
      description="The process gets a stop signal and the sandbox ends. A stopped sandbox cannot be started again; create a new one to run the image again."
      confirmLabel="Stop sandbox"
      danger
      onConfirm={onConfirm}
      onCancel={onCancel}
      testid="sandboxes-stop-dialog"
    />
  );
}
