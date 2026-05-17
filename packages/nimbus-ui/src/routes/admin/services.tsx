import { createFileRoute } from "@tanstack/react-router";
import { PlaceholderPage } from "../../shell/placeholder-page";

export const Route = createFileRoute("/admin/services")({
  component: ServicesPage,
});

function ServicesPage() {
  return (
    <PlaceholderPage
      title="Services"
      summary="Tenant runtime services and bundles: which microVMs are running, which versions are live, recent restarts, and per-tenant resource usage."
      hint="Service list + detail drawer lands in DU-shell O3 alongside the machine lifecycle API surface."
    />
  );
}
