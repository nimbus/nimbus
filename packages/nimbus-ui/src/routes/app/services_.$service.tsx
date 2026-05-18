import {
  createFileRoute,
  Link,
  useNavigate,
  useSearch,
} from "@tanstack/react-router";
import { useQuery } from "nimbus/react";
import { useMemo } from "react";

import { api } from "../../../convex/_generated/api";
import { Breadcrumb } from "../../components/breadcrumb";
import { CopyChip } from "../../components/copy-chip";
import { StateChip } from "../../components/state-chip";
import { RelativeTime } from "../../components/time";
import { cn } from "../../lib/cn";
import { shortId } from "../../lib/format";
import {
  type SubDrawerSpec,
  useContributeSubDrawer,
  useSubDrawerSearch,
} from "../../shell/sub-drawer";
import { useUiStore } from "../../store/ui-store";
import type { ServiceDoc } from "./services";

type DetailTab = "overview" | "endpoints" | "health" | "bundle";

const TABS: Array<{ id: DetailTab; label: string }> = [
  { id: "overview", label: "Overview" },
  { id: "endpoints", label: "Endpoints" },
  { id: "health", label: "Health" },
  { id: "bundle", label: "Bundle" },
];

type DetailSearch = {
  tab?: DetailTab;
};

type BundleDoc = {
  _id: string;
  sha256?: string;
  status?: string;
  sourceRef?: string;
  _creationTime?: number;
};

export const Route = createFileRoute("/app/services_/$service")({
  validateSearch: (search: Record<string, unknown>): DetailSearch => ({
    tab: isTab(search.tab) ? search.tab : undefined,
  }),
  component: ServiceDetailPage,
});

function isTab(value: unknown): value is DetailTab {
  return (
    value === "overview" ||
    value === "endpoints" ||
    value === "health" ||
    value === "bundle"
  );
}

function ServiceDetailPage() {
  const { service: serviceId } = Route.useParams();
  const search = useSearch({ from: "/app/services_/$service" });
  const navigate = useNavigate();
  const tab: DetailTab = search.tab ?? "overview";
  const activeTenant = useUiStore((s) => s.activeTenant);

  const service = useQuery(api.services.byId, {
    id: serviceId as never,
  }) as ServiceDoc | null | undefined;

  const services = useQuery(api.services.list, {
    tenantId: activeTenant,
    machineId: null,
    state: null,
    limit: 200,
  }) as ServiceDoc[] | undefined;

  const bundles = useQuery(api.bundles.list, {
    status: null,
    limit: 50,
  }) as BundleDoc[] | undefined;
  const bundle = useMemo<BundleDoc | null>(() => {
    if (!service?.bundleId || !bundles) return null;
    return bundles.find((b) => b._id === service.bundleId) ?? null;
  }, [service, bundles]);

  const spec = useMemo<SubDrawerSpec>(
    () => ({
      kind: "dynamic",
      title: "Services",
      search: { placeholder: "Filter services" },
      children: (
        <DetailSubDrawer services={services} activeServiceId={serviceId} />
      ),
    }),
    [services, serviceId],
  );
  useContributeSubDrawer(spec);

  const setTab = (next: DetailTab) =>
    navigate({
      to: "/app/services/$service",
      params: { service: serviceId },
      search: { tab: next },
      replace: true,
    });

  const displayName = service?.name ?? shortId(serviceId, 12);

  return (
    <section
      className="flex h-full flex-col overflow-hidden"
      data-testid="page-service-detail"
    >
      <div className="flex shrink-0 flex-col gap-2 border-b border-app px-6 pb-3 pt-4">
        <Breadcrumb
          segments={[
            { label: "Services", href: "/app/services" },
            { label: displayName, active: true },
          ]}
        />
        <header className="flex flex-wrap items-baseline gap-3">
          <h1
            className="font-mono text-default"
            style={{ fontSize: "var(--text-lg)" }}
          >
            {displayName}
          </h1>
          {service?.kind ? (
            <span className="rounded border border-app px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wide text-muted">
              {service.kind}
            </span>
          ) : null}
          {service?.state ? <StateChip state={service.state} /> : null}
          {bundle?.sha256 ? (
            <CopyChip
              label="bundle sha256"
              value={bundle.sha256}
              testid="service-detail-bundle"
            >
              {shortId(bundle.sha256, 12)}
            </CopyChip>
          ) : null}
        </header>
      </div>

      <nav
        aria-label="Service detail sections"
        className="flex shrink-0 gap-px border-b border-app bg-surface-2 px-6"
        data-testid="service-detail-tabs"
      >
        {TABS.map((t) => {
          const isActive = tab === t.id;
          return (
            <button
              key={t.id}
              type="button"
              onClick={() => setTab(t.id)}
              aria-current={isActive ? "page" : undefined}
              data-testid={`service-detail-tab-${t.id}`}
              className={cn(
                "flex items-center px-3 py-2 font-mono text-xs uppercase tracking-wide",
                isActive
                  ? "border-b-2 border-[color:var(--color-brand)] text-default"
                  : "text-muted hover:text-default",
              )}
            >
              {t.label}
            </button>
          );
        })}
      </nav>

      <div className="min-h-0 flex-1 overflow-hidden">
        {service === undefined ? (
          <Loading label="Loading service…" />
        ) : service === null ? (
          <NotFound id={serviceId} />
        ) : (
          <TabBody tab={tab} service={service} bundle={bundle} />
        )}
      </div>
    </section>
  );
}

function TabBody({
  tab,
  service,
  bundle,
}: {
  tab: DetailTab;
  service: ServiceDoc;
  bundle: BundleDoc | null;
}) {
  if (tab === "overview") return <OverviewTab service={service} />;
  if (tab === "endpoints") return <EndpointsTab service={service} />;
  if (tab === "health") return <HealthTab service={service} />;
  return <BundleTab service={service} bundle={bundle} />;
}

function OverviewTab({ service }: { service: ServiceDoc }) {
  return (
    <div
      className="flex h-full flex-col gap-3 overflow-auto px-6 py-4 text-sm text-default"
      data-testid="service-tab-overview"
    >
      <Stat label="Name" value={service.name ?? "—"} />
      <Stat label="Kind" value={service.kind ?? "—"} />
      <Stat label="State" value={<StateChip state={service.state} />} />
      <Stat label="Tenant" value={service.tenantId ?? "—"} />
      <Stat label="Machine" value={service.machineId ?? "—"} />
      <Stat
        label="Updated"
        value={
          typeof service._updateTime === "number" ? (
            <RelativeTime epochMs={service._updateTime} />
          ) : (
            "—"
          )
        }
      />
      <div className="rounded border border-app bg-surface-2 px-3 py-3 text-xs text-muted">
        Logs, environment variables, ports, and code-ref details are not yet
        surfaced by the system tenant for the services index. A follow-up plan
        will wire these dimensions through the SandboxServiceManager and
        compose.yaml metadata.
      </div>
    </div>
  );
}

function EndpointsTab({ service }: { service: ServiceDoc }) {
  const endpoints = Array.isArray(service.endpoints) ? service.endpoints : [];
  if (endpoints.length === 0) {
    return (
      <Empty
        title="No endpoints declared"
        detail="Services expose endpoints once their compose.yaml binds host ports or a sidecar registers a route."
      />
    );
  }
  return (
    <div
      className="h-full overflow-auto px-6 py-4"
      data-testid="service-tab-endpoints"
    >
      <pre className="m-0 rounded border border-app bg-surface-2 p-3 font-mono text-xs text-default">
        {JSON.stringify(endpoints, null, 2)}
      </pre>
    </div>
  );
}

function HealthTab({ service }: { service: ServiceDoc }) {
  const health = service.health;
  if (!health) {
    return (
      <Empty
        title="No health snapshot"
        detail="Health probes populate this panel once the service manager records its first readiness check."
      />
    );
  }
  return (
    <div
      className="h-full overflow-auto px-6 py-4"
      data-testid="service-tab-health"
    >
      <pre className="m-0 rounded border border-app bg-surface-2 p-3 font-mono text-xs text-default">
        {JSON.stringify(health, null, 2)}
      </pre>
    </div>
  );
}

function BundleTab({
  service,
  bundle,
}: {
  service: ServiceDoc;
  bundle: BundleDoc | null;
}) {
  if (!service.bundleId) {
    return (
      <Empty
        title="No bundle attached"
        detail="This service has not been associated with a runtime bundle. Run `nimbus compose up` to register one."
      />
    );
  }
  if (bundle === null) {
    return (
      <Empty
        title="Bundle not found"
        detail={`Service references bundleId ${shortId(service.bundleId, 12)} but no matching bundle is registered.`}
      />
    );
  }
  return (
    <div
      className="flex h-full flex-col gap-3 overflow-auto px-6 py-4 text-sm text-default"
      data-testid="service-tab-bundle"
    >
      <Stat
        label="SHA-256"
        value={
          bundle.sha256 ? (
            <CopyChip
              label="bundle sha256"
              value={bundle.sha256}
              testid="service-bundle-sha"
            >
              {shortId(bundle.sha256, 16)}
            </CopyChip>
          ) : (
            "—"
          )
        }
      />
      <Stat label="Status" value={bundle.status ?? "—"} />
      <Stat label="Source ref" value={bundle.sourceRef ?? "—"} />
      <Stat
        label="Registered"
        value={
          typeof bundle._creationTime === "number" ? (
            <RelativeTime epochMs={bundle._creationTime} />
          ) : (
            "—"
          )
        }
      />
    </div>
  );
}

function Stat({
  label,
  value,
}: {
  label: string;
  value: React.ReactNode;
}) {
  return (
    <div className="flex items-baseline gap-3">
      <span className="w-32 font-mono text-[10px] uppercase tracking-[0.18em] text-muted">
        {label}
      </span>
      <span className="font-mono text-xs text-default">{value}</span>
    </div>
  );
}

function DetailSubDrawer({
  services,
  activeServiceId,
}: {
  services: ServiceDoc[] | undefined;
  activeServiceId: string;
}) {
  const filter = useSubDrawerSearch().trim().toLowerCase();
  if (services === undefined) {
    return (
      <div className="px-3 py-3 text-xs text-muted">
        <span aria-hidden>·</span>
        <span className="sr-only">loading</span>
      </div>
    );
  }
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
      <div className="px-3 py-6 text-xs text-muted">
        No services declared.
      </div>
    );
  }
  if (filtered.length === 0) {
    return (
      <div className="px-3 py-6 text-xs text-muted">
        No services match the filter.
      </div>
    );
  }
  return (
    <ul className="flex flex-col gap-px px-2 py-2">
      {filtered.map((svc) => {
        const isActive = svc._id === activeServiceId;
        return (
          <li key={svc._id}>
            <Link
              to="/app/services/$service"
              params={{ service: svc._id }}
              data-testid={`sub-drawer-item-dev-service-${svc.name ?? svc._id}`}
              className={cn(
                "flex h-8 items-center gap-2 rounded-md px-2 text-sm",
                isActive
                  ? "bg-surface-2 text-default"
                  : "text-muted hover:bg-surface-2 hover:text-default",
              )}
            >
              <span className="flex-1 truncate font-mono text-xs">
                {svc.name ?? shortId(svc._id, 12)}
              </span>
              {svc.state ? (
                <span className="tabular font-mono text-[10px] uppercase tracking-[0.18em] text-muted">
                  {svc.state}
                </span>
              ) : null}
            </Link>
          </li>
        );
      })}
    </ul>
  );
}

function Loading({ label }: { label: string }) {
  return (
    <div className="flex h-full items-center justify-center text-xs text-muted">
      {label}
    </div>
  );
}

function NotFound({ id }: { id: string }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-2 text-center">
      <span className="font-mono text-sm text-default">Service not found</span>
      <span className="max-w-md text-xs text-muted">
        No service matches the id{" "}
        <code className="font-mono text-default">{shortId(id, 12)}</code>. It
        may have been stopped or never registered.
      </span>
      <Link
        to="/app/services"
        className="rounded border border-app px-3 py-1 font-mono text-[11px] uppercase tracking-wide text-muted hover:bg-surface hover:text-default"
      >
        ← back to services
      </Link>
    </div>
  );
}

function Empty({ title, detail }: { title: string; detail: string }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-1 text-center">
      <span className="font-mono text-sm text-default">{title}</span>
      <span className="max-w-md text-xs text-muted">{detail}</span>
    </div>
  );
}
