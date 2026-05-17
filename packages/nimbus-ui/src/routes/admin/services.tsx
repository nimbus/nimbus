import { createFileRoute } from "@tanstack/react-router";
import { useQuery } from "nimbus/react";
import { useMemo } from "react";
import { api } from "../../../convex/_generated/api";
import { PlaceholderPage } from "../../shell/placeholder-page";
import {
  type SubDrawerSpec,
  useContributeSubDrawer,
} from "../../shell/sub-drawer";

export const Route = createFileRoute("/admin/services")({
  component: ServicesPage,
});

type ServiceDoc = {
  _id: string;
  tenantId?: string;
  bundleId?: string;
  state?: string;
  machineId?: string;
};

function ServicesPage() {
  const services = useQuery(api.services.list, {
    tenantId: null,
    machineId: null,
    state: null,
    limit: 200,
  }) as ServiceDoc[] | Error | undefined;
  const spec = useMemo<SubDrawerSpec>(() => {
    const list = Array.isArray(services) ? services : [];
    return {
      kind: "dynamic",
      title: "Services",
      search: { placeholder: "Filter services" },
      children:
        services === undefined ? (
          <div className="px-3 py-3 text-xs text-muted">
            <span aria-hidden>·</span>
            <span className="sr-only">loading</span>
          </div>
        ) : list.length === 0 ? (
          <div className="px-3 py-6 text-xs text-muted">
            <p>No services yet.</p>
            <p className="mt-2">
              Services appear here once a tenant deploys a runtime bundle.
            </p>
          </div>
        ) : (
          <ul className="flex flex-col gap-px px-2 py-2">
            {list.map((service) => (
              <li key={service._id}>
                <a
                  href={`/admin/services?selected=${service._id}`}
                  data-testid={`sub-drawer-item-op-${service._id}`}
                  className="flex h-8 items-center gap-2 rounded-md px-2 text-sm text-muted hover:bg-surface-2 hover:text-default"
                >
                  <span className="flex-1 truncate font-mono text-xs">
                    {service._id}
                  </span>
                  {service.state ? (
                    <span className="tabular font-mono text-[10px] uppercase tracking-[0.18em] text-muted">
                      {service.state}
                    </span>
                  ) : null}
                </a>
              </li>
            ))}
          </ul>
        ),
    };
  }, [services]);
  useContributeSubDrawer(spec);
  return (
    <PlaceholderPage
      title="Services"
      summary="Tenant runtime services and bundles: which microVMs are running, which versions are live, recent restarts, and per-tenant resource usage."
      hint="Service list + detail drawer lands in DU-shell O3 alongside the machine lifecycle API surface."
    />
  );
}
