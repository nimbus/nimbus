import {
  createFileRoute,
  Link,
  notFound,
  useNavigate,
  useSearch,
} from "@tanstack/react-router";
import { useMemo } from "react";
import { cn } from "@/lib/utils";
import { api } from "../../../convex/_generated/api";
import type { Doc, Id } from "../../../convex/_generated/dataModel";
import { Breadcrumb } from "../../components/breadcrumb";
import { CopyChip } from "../../components/copy-chip";
import { StatePill } from "../../components/pill";
import { AdminServiceDetailLoaderError } from "../../components/service-loader-errors";
import { shortHash, shortId } from "../../lib/format";
import { getNimbusClient } from "../../lib/nimbus-client";
import type { ServiceDoc } from "../../lib/types/service";
import {
  type SubDrawerSpec,
  useContributeSubDrawer,
  useSubDrawerSearch,
} from "../../shell/sub-drawer";
import { groupByTenant } from "./services";

export type DetailTab = "placement";

export const TABS: ReadonlyArray<{ id: DetailTab; label: string }> = [
  { id: "placement", label: "Placement" },
];

type DetailSearch = {
  tab?: DetailTab;
};

export const Route = createFileRoute("/operator/services_/$service")({
  validateSearch: (search: Record<string, unknown>): DetailSearch => ({
    tab: isTab(search.tab) ? search.tab : undefined,
  }),
  loader: async ({ params }) => {
    const client = getNimbusClient();
    const [service, services, bundles, machines] = await Promise.all([
      client.query(api.services.byId, {
        id: params.service as Id<"services">,
      }),
      client.query(api.services.list, {
        tenantId: null,
        machineId: null,
        state: null,
        limit: 200,
      }),
      client.query(api.bundles.list, { status: null, limit: 50 }),
      client.query(api.machines.list, {
        state: null,
        provider: null,
        limit: 200,
      }),
    ]);
    if (!service) throw notFound();
    return { service, services, bundles, machines };
  },
  notFoundComponent: AdminServiceNotFound,
  errorComponent: AdminServiceDetailLoaderError,
  component: AdminServiceDetailPage,
});

export function isTab(value: unknown): value is DetailTab {
  return value === "placement";
}

function AdminServiceDetailPage() {
  const { service: serviceId } = Route.useParams();
  const { service, services, bundles, machines } = Route.useLoaderData();
  const search = useSearch({ from: "/operator/services_/$service" });
  const navigate = useNavigate();
  const tab: DetailTab = search.tab ?? "placement";

  const bundle = useMemo<Doc<"bundles"> | null>(() => {
    if (!service.bundleId) return null;
    return bundles.find((b) => b._id === service.bundleId) ?? null;
  }, [service, bundles]);

  const spec = useMemo<SubDrawerSpec>(
    () => ({
      kind: "dynamic",
      title: "Services",
      search: { placeholder: "Filter services" },
      children: (
        <AdminDetailSubDrawer services={services} activeServiceId={serviceId} />
      ),
    }),
    [services, serviceId],
  );
  useContributeSubDrawer(spec);

  const setTab = (next: DetailTab) =>
    navigate({
      to: "/operator/services/$service",
      params: { service: serviceId },
      search: { tab: next },
      replace: true,
    });

  const displayName = service.name ?? shortId(serviceId, 12);

  return (
    <section
      className="flex h-full flex-col overflow-hidden"
      data-testid="page-admin-service-detail"
    >
      <div className="flex shrink-0 flex-col gap-2 border-b border-border-2 px-6 pb-3 pt-4">
        <Breadcrumb
          segments={[
            { label: "Services", href: "/operator/services" },
            { label: displayName, active: true },
          ]}
        />
        <header className="flex flex-wrap items-baseline gap-3">
          <h1
            className="font-mono text-text-1"
            style={{ fontSize: "var(--text-lg)" }}
          >
            {displayName}
          </h1>
          {service.kind ? (
            <span className="rounded-xs border border-border-2 px-1.5 py-0.5 text-xs font-medium text-text-3">
              {service.kind}
            </span>
          ) : null}
          {service.state ? <StatePill state={service.state} /> : null}
          {service.tenantId ? (
            <span className="rounded-xs border border-border-2 px-1.5 py-0.5 text-xs font-medium text-text-3">
              {service.tenantId}
            </span>
          ) : null}
          {bundle?.sha256 ? (
            <CopyChip
              label="bundle sha256"
              value={bundle.sha256}
              testid="admin-service-detail-bundle"
            >
              {shortHash(bundle.sha256, 12)}
            </CopyChip>
          ) : null}
        </header>
      </div>

      <nav
        aria-label="Admin service detail sections"
        className="flex shrink-0 gap-px border-b border-border-2 bg-bg-raised px-6"
        data-testid="admin-service-detail-tabs"
      >
        {TABS.map((t) => {
          const isActive = tab === t.id;
          return (
            <button
              key={t.id}
              type="button"
              onClick={() => setTab(t.id)}
              aria-current={isActive ? "page" : undefined}
              data-testid={`admin-service-detail-tab-${t.id}`}
              className={cn(
                "flex items-center px-3 py-2 font-mono text-xs",
                isActive
                  ? "border-b-2 border-[color:var(--accent)] text-text-1"
                  : "text-text-3 hover:text-text-1",
              )}
            >
              {t.label}
            </button>
          );
        })}
      </nav>

      <div className="min-h-0 flex-1 overflow-hidden">
        <PlacementTab service={service} machines={machines} />
      </div>
    </section>
  );
}

function PlacementTab({
  service,
  machines,
}: {
  service: ServiceDoc;
  machines: Doc<"machines">[];
}) {
  const machine = useMemo<Doc<"machines"> | null>(() => {
    if (!service.machineId) return null;
    return machines.find((m) => m._id === service.machineId) ?? null;
  }, [service.machineId, machines]);

  return (
    <div
      className="flex h-full flex-col gap-3 overflow-auto px-6 py-4 text-sm text-text-1"
      data-testid="admin-service-tab-placement"
    >
      <Stat label="Tenant" value={service.tenantId ?? "—"} />
      <Stat
        label="Machine"
        value={
          service.machineId ? (
            <Link
              to="/operator/machines"
              className="font-mono text-text-1 hover:underline"
            >
              {machine?.name ?? shortId(service.machineId, 12)}
            </Link>
          ) : (
            "—"
          )
        }
      />
      <Stat
        label="Machine state"
        value={machine?.state ? <StatePill state={machine.state} /> : "—"}
      />
    </div>
  );
}

function Stat({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex items-baseline gap-3">
      <span className="w-32 text-xs font-medium text-text-3">{label}</span>
      <span className="font-mono text-xs text-text-1">{value}</span>
    </div>
  );
}

function AdminDetailSubDrawer({
  services,
  activeServiceId,
}: {
  services: ServiceDoc[];
  activeServiceId: string;
}) {
  const filter = useSubDrawerSearch().trim().toLowerCase();
  const filtered = filter
    ? services.filter(
        (s) =>
          (s.name ?? "").toLowerCase().includes(filter) ||
          (s.state ?? "").toLowerCase().includes(filter) ||
          (s.tenantId ?? "").toLowerCase().includes(filter) ||
          (s.kind ?? "").toLowerCase().includes(filter),
      )
    : services;
  if (services.length === 0) {
    return (
      <div className="px-3 py-6 text-xs text-text-3">
        No services registered.
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
  const grouped = groupByTenant(filtered);
  return (
    <ul className="flex flex-col gap-2 px-2 py-2">
      {grouped.map(([tenant, items]) => (
        <li key={tenant} className="flex flex-col gap-px">
          <div className="px-2 pb-1 pt-2 text-xs font-medium text-text-3">
            {tenant}
          </div>
          {items.map((svc) => {
            const isActive = svc._id === activeServiceId;
            return (
              <Link
                key={svc._id}
                to="/operator/services/$service"
                params={{ service: svc._id }}
                data-testid={`sub-drawer-item-op-service-${svc.name ?? svc._id}`}
                className={cn(
                  "flex h-8 items-center gap-2 rounded-md px-2 text-sm",
                  isActive
                    ? "bg-bg-raised text-text-1"
                    : "text-text-3 hover:bg-bg-raised hover:text-text-1",
                )}
              >
                <span className="flex-1 truncate font-mono text-xs">
                  {svc.name ?? shortId(svc._id, 12)}
                </span>
                {svc.state ? (
                  <span className="tabular text-xs font-medium text-text-3">
                    {svc.state}
                  </span>
                ) : null}
              </Link>
            );
          })}
        </li>
      ))}
    </ul>
  );
}

function AdminServiceNotFound() {
  const { service: serviceId } = Route.useParams();
  return (
    <div
      className="flex h-full flex-col items-center justify-center gap-2 text-center"
      data-testid="admin-service-not-found"
    >
      <span className="font-mono text-sm text-text-1">Service not found</span>
      <span className="max-w-md text-xs text-text-3">
        No service matches the id{" "}
        <code className="font-mono text-text-1">{shortId(serviceId, 12)}</code>.
      </span>
      <Link
        to="/operator/services"
        className="rounded-xs border border-border-2 px-3 py-1 text-xs font-medium text-text-3 hover:bg-bg-panel hover:text-text-1"
      >
        ← back to services
      </Link>
    </div>
  );
}
