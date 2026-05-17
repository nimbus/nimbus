import { createFileRoute } from "@tanstack/react-router";
import {
  type SubDrawerSpec,
  useContributeSubDrawer,
} from "../../shell/sub-drawer";
import { PlaceholderPage } from "../../shell/placeholder-page";

export const Route = createFileRoute("/app/settings")({
  component: TenantSettingsPage,
});

const TENANT_SETTINGS_SUB_DRAWER: SubDrawerSpec = {
  kind: "static",
  title: "Settings",
  items: [
    {
      id: "environment",
      label: "Environment",
      to: "/app/settings",
      search: { section: "environment" },
    },
    {
      id: "secrets",
      label: "Secrets",
      to: "/app/settings",
      search: { section: "secrets" },
    },
    {
      id: "schema",
      label: "Schema",
      to: "/app/settings",
      search: { section: "schema" },
    },
    {
      id: "integrations",
      label: "Integrations",
      to: "/app/settings",
      search: { section: "integrations" },
    },
  ],
};

function TenantSettingsPage() {
  useContributeSubDrawer(TENANT_SETTINGS_SUB_DRAWER);
  return (
    <PlaceholderPage
      title="Settings"
      summary="Tenant-scoped settings: members, API keys, environment variables, deploy keys, and appearance preferences."
      hint="Tenant settings sub-drawer lands in DU-shell O4. Operator-wide server settings live under /admin/settings."
    />
  );
}
