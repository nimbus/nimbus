import { createFileRoute } from "@tanstack/react-router";
import { Settings } from "lucide-react";

import { EmptyState } from "../../components/empty-state";
import { PageHeader } from "../../components/page-header";

// DESIGN.md plans five tenant sub-pages here: Environment, Secrets, Schema,
// Integrations, and Adapter binding. None has a tenant-scoped API in this
// build, so the page contributes no sub-panel (a static menu lists only
// pages that exist) and says so once instead of five times. The section
// space and its menu arrive with the first tenant-scoped setting.
export const Route = createFileRoute("/developer/settings")({
  component: TenantSettingsPage,
});

export const TENANT_SETTINGS_PLANNED = [
  "Environment",
  "Secrets",
  "Schema",
  "Integrations",
  "Adapter binding",
] as const;

function TenantSettingsPage() {
  return (
    <section
      className="flex h-full flex-col gap-5 overflow-y-auto px-6 py-5"
      data-testid="page-settings"
    >
      <PageHeader
        title="Settings"
        subtitle="Tenant-scoped settings for the active tenant."
      />
      <div className="flex min-h-0 flex-1 rounded-md border border-border-2 bg-bg-panel">
        <EmptyState
          icon={Settings}
          title="No tenant settings in this build"
          body={
            <>
              {TENANT_SETTINGS_PLANNED.slice(0, -1).join(", ")}, and{" "}
              {TENANT_SETTINGS_PLANNED.at(-1)} are planned here. Until a tenant
              setting has an API, every tenant runs on the server settings:
              effective configuration under General, and the adapters each
              tenant routes through under Integrations.
            </>
          }
          cta={{ label: "Operator settings", to: "/operator/settings" }}
          testid="settings-empty"
        />
      </div>
    </section>
  );
}
