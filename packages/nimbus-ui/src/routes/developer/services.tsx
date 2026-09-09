import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { Ellipsis } from "lucide-react";
import { useCallback, useMemo, useState } from "react";

import { api } from "../../../convex/_generated/api";
import {
  DataTable,
  dataColumns,
  type RowAnchor,
} from "../../components/data-table";
import { EmptyState } from "../../components/empty-state";
import { PageHeader } from "../../components/page-header";
import { CategoryPill, StatePill } from "../../components/pill";
import { ServicesLoaderError } from "../../components/service-loader-errors";
import { StateDot } from "../../components/state-dot";
import {
  RowContextMenu,
  type RowMenuItem,
} from "../../components/storage/row-context-menu";
import { RelativeTime } from "../../components/time";
import { Button } from "../../components/ui/button";
import { shortId } from "../../lib/format";
import { getNimbusClient } from "../../lib/nimbus-client";
import type { ServiceDoc } from "../../lib/types/service";
import {
  type SubPanelSpec,
  useContributeSubPanel,
  useSubPanelSearch,
} from "../../shell/sub-panel";
import { useUiStore } from "../../store/ui-store";
import {
  ACTION_LABELS,
  actionsForState,
  OPTIMISTIC_STATES,
  type ServiceAction,
  useServiceActions,
} from "./services/-service-lifecycle";

export const Route = createFileRoute("/developer/services")({
  loaderDeps: () => ({
    activeTenant: useUiStore.getState().activeTenant,
  }),
  loader: async ({ deps }) => {
    const services = await getNimbusClient().query(api.services.list, {
      tenantId: deps.activeTenant,
      machineId: null,
      state: null,
      limit: 200,
    });
    return { services, activeTenant: deps.activeTenant };
  },
  component: ServicesPage,
  errorComponent: ServicesLoaderError,
});

function ServicesPage() {
  const { services } = Route.useLoaderData();
  const activeTenant = useUiStore((s) => s.activeTenant);

  const spec = useMemo<SubPanelSpec>(
    () => ({
      kind: "dynamic",
      title: "Services",
      search: { placeholder: "Filter services", rows: services.length },
      children: (
        <ServicesSubPanel services={services} activeTenant={activeTenant} />
      ),
    }),
    [services, activeTenant],
  );
  useContributeSubPanel(spec);

  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-services"
    >
      <PageHeader
        title="Services"
        subtitle={
          <>
            Services this tenant declares in <code>compose.yaml</code> —
            microVMs on Linux, containers in a macOS machine VM.
          </>
        }
        trailing={<ScopeChip activeTenant={activeTenant} />}
      />

      <div className="min-h-0 flex-1 overflow-hidden rounded-md border border-border-2 bg-bg-panel">
        <ServicesTable
          services={services}
          activeTenant={activeTenant}
          showTenantColumn={false}
        />
      </div>
    </section>
  );
}

function ScopeChip({ activeTenant }: { activeTenant: string | null }) {
  if (activeTenant === null) return null;
  return (
    <span
      className="inline-flex items-center gap-1 rounded-xs border border-border-2 px-2 py-0.5 font-mono text-xs text-text-3"
      data-testid="services-scope"
    >
      <span className="font-medium">tenant</span>
      <span className="font-mono text-text-1">{activeTenant}</span>
    </span>
  );
}

function ServicesSubPanel({
  services,
  activeTenant,
}: {
  services: ServiceDoc[];
  activeTenant: string | null;
}) {
  const filter = useSubPanelSearch().trim().toLowerCase();
  const filtered = filter
    ? services.filter(
        (s) =>
          (s.name ?? "").toLowerCase().includes(filter) ||
          (s.state ?? "").toLowerCase().includes(filter) ||
          (s.kind ?? "").toLowerCase().includes(filter),
      )
    : services;
  if (services.length === 0) {
    return (
      <div className="px-3 py-6 text-xs text-text-3">
        <p>No services declared.</p>
        <p className="mt-2">
          Author a <code>compose.yaml</code> and run{" "}
          <code className="whitespace-nowrap">nimbus compose up</code> to
          register services for{" "}
          {activeTenant ? `tenant ${activeTenant}` : "this tenant"}.
        </p>
      </div>
    );
  }
  if (filtered.length === 0) {
    return (
      <div className="px-3 py-6 text-xs text-text-3">
        No services match the filter.
      </div>
    );
  }
  return (
    <ul className="flex flex-col gap-px px-2 py-2">
      {filtered.map((svc) => (
        <li key={svc._id}>
          <Link
            to="/developer/services/$service"
            params={{ service: svc._id }}
            data-testid={`sub-panel-item-dev-service-${svc.name ?? svc._id}`}
            className="flex h-8 items-center gap-2 rounded-md px-2 text-sm text-text-3 hover:bg-bg-raised hover:text-text-1"
          >
            <StateDot state={svc.state} />
            <span className="flex-1 truncate font-mono text-xs">
              {svc.name ?? shortId(svc._id, 12)}
            </span>
            {svc.state ? (
              <span className="tabular text-xs font-medium text-text-3">
                {svc.state}
              </span>
            ) : null}
          </Link>
        </li>
      ))}
    </ul>
  );
}

// A row of the table: the service plus the state it shows right now, which
// is the optimistic state while a lifecycle request is in flight.
type ServiceRow = ServiceDoc & {
  shownState: string | undefined;
  actionError: string | undefined;
};

type MenuState = RowAnchor & { row: ServiceRow };

const col = dataColumns<ServiceRow>();

function nameOf(row: ServiceDoc): string {
  return row.name ?? shortId(row._id, 12);
}

// ServicesTable backs both the developer list (one tenant) and the operator
// list (every tenant, with the tenant column). Row activation opens the
// detail page for the surface; the row menu holds the lifecycle actions the
// state allows and a jump to the service's log lines.
export function ServicesTable({
  services,
  activeTenant,
  showTenantColumn,
}: {
  services: ServiceDoc[];
  activeTenant: string | null;
  showTenantColumn: boolean;
}) {
  const navigate = useNavigate();
  const { pending, errors, runAction } = useServiceActions();
  const [menu, setMenu] = useState<MenuState | null>(null);

  const detailTo = showTenantColumn
    ? "/operator/services/$service"
    : "/developer/services/$service";

  const rows = useMemo<ServiceRow[]>(
    () =>
      services.map((svc) => {
        const inFlight = pending[svc._id];
        return {
          ...svc,
          shownState: inFlight ? OPTIMISTIC_STATES[inFlight] : svc.state,
          actionError: errors[svc._id],
        };
      }),
    [services, pending, errors],
  );

  const openRow = useCallback(
    (row: ServiceRow) =>
      navigate({ to: detailTo, params: { service: row._id } }),
    [navigate, detailTo],
  );

  const menuItems = useCallback(
    (row: ServiceRow): RowMenuItem[] => {
      const lifecycle = actionsForState(row.shownState).map(
        (action): RowMenuItem => ({
          id: action,
          label: ACTION_LABELS[action],
          onSelect: () => void runAction(row, action),
        }),
      );
      return [
        {
          id: "open",
          label: "Open service",
          onSelect: () => void openRow(row),
        },
        ...lifecycle,
        {
          id: "logs",
          label: "Show logs",
          onSelect: () =>
            void navigate({
              to: "/developer/observability",
              search: {
                tab: "logs",
                source: "service",
                tenant: row.tenantId ?? undefined,
              },
            }),
        },
      ];
    },
    [navigate, openRow, runAction],
  );

  const columns = useMemo(
    () => [
      col.accessor("name", {
        header: "Name",
        size: 128,
        cell: (ctx) => {
          const row = ctx.row.original;
          return (
            <Link
              to={detailTo}
              params={{ service: row._id }}
              className="block truncate font-mono text-text-1 hover:underline"
              data-testid={`services-link-${nameOf(row)}`}
            >
              {nameOf(row)}
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
                data-testid={`services-state-${nameOf(row)}`}
              />
              {row.actionError ? (
                <span
                  className="truncate text-xs text-[color:var(--error)]"
                  title={row.actionError}
                  data-testid={`services-row-error-${nameOf(row)}`}
                >
                  {row.actionError}
                </span>
              ) : null}
            </span>
          );
        },
      }),
      col.accessor("kind", {
        header: "Kind",
        size: 100,
        cell: (ctx) => <CategoryPill value={ctx.getValue()} />,
      }),
      ...(showTenantColumn
        ? [
            col.accessor("tenantId", {
              header: "Tenant",
              size: 96,
              cell: (ctx) => (
                <span className="block truncate font-mono text-xs text-text-1">
                  {ctx.getValue() ?? "—"}
                </span>
              ),
            }),
          ]
        : []),
      col.accessor("machineId", {
        header: "Machine",
        size: 92,
        cell: (ctx) => {
          const id = ctx.getValue();
          return (
            <span
              className="block truncate font-mono text-xs text-text-3"
              title={id ?? undefined}
            >
              {id ? shortId(id, 14) : "—"}
            </span>
          );
        },
      }),
      col.accessor(
        (row) => (Array.isArray(row.endpoints) ? row.endpoints.length : 0),
        {
          id: "endpoints",
          header: () => <span className="block text-right">Endpoints</span>,
          size: 88,
          cell: (ctx) => (
            <span className="block text-right font-mono text-xs tabular text-text-3">
              {ctx.getValue()}
            </span>
          ),
        },
      ),
      col.accessor("_updateTime", {
        header: "Updated",
        size: 88,
        cell: (ctx) => {
          const at = ctx.getValue();
          return typeof at === "number" ? (
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
                aria-label={`Actions for ${nameOf(row)}`}
                data-testid={`services-row-actions-${nameOf(row)}`}
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
    [detailTo, showTenantColumn],
  );

  if (services.length === 0) {
    return (
      <EmptyState
        title="No services"
        body={
          activeTenant ? (
            <>
              This tenant has no declared services. Add them to{" "}
              <code>compose.yaml</code> and run{" "}
              <code className="whitespace-nowrap">nimbus compose up</code>.
            </>
          ) : (
            "No services declared across any tenant."
          )
        }
        testid="services-empty"
      />
    );
  }

  return (
    <>
      <DataTable
        columns={columns}
        data={rows}
        getRowId={(row) => row._id}
        ariaLabel="Services"
        onRowActivate={(row) => void openRow(row)}
        onRowContextMenu={(row, anchor) => setMenu({ ...anchor, row })}
        rowTestid={(row) => `services-row-${nameOf(row)}`}
        testid="services-table"
        className="h-full"
      />
      {menu ? (
        <RowContextMenu
          x={menu.x}
          y={menu.y}
          label={`Actions for ${nameOf(menu.row)}`}
          items={menuItems(menu.row)}
          restoreFocus={menu.element}
          onClose={() => setMenu(null)}
          testid="services-row-menu"
        />
      ) : null}
    </>
  );
}

export type { ServiceAction };
