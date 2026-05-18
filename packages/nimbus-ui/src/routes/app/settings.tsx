import { createFileRoute } from "@tanstack/react-router";

import { EmptyState } from "../../components/empty-state";
import {
  type SubDrawerSpec,
  useContributeSubDrawer,
} from "../../shell/sub-drawer";

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
    <section
      className="flex h-full flex-col"
      data-testid="page-settings"
    >
      <EmptyState
        title="Tenant settings"
        body="Members, API keys, environment variables, deploy keys, and appearance preferences will live here. Server-wide configuration lives under the operator console."
        cta={{ label: "Operator settings", to: "/admin/settings" }}
        testid="settings-empty"
      />
    </section>
  );
}
