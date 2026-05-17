import { createFileRoute } from "@tanstack/react-router";
import { PlaceholderPage } from "../../shell/placeholder-page";

export const Route = createFileRoute("/app/settings")({
  component: TenantSettingsPage,
});

function TenantSettingsPage() {
  return (
    <PlaceholderPage
      title="Settings"
      summary="Tenant-scoped settings: members, API keys, environment variables, deploy keys, and appearance preferences."
      hint="Tenant settings sub-drawer lands in DU-shell O4. Operator-wide server settings live under /admin/settings."
    />
  );
}
