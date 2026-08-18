import { useQuery } from "@nimbus/nimbus/react";
import { createFileRoute, redirect } from "@tanstack/react-router";

import { api } from "../../../convex/_generated/api";
import { AppearanceSection } from "../../components/appearance-section";
import { EmptyState } from "../../components/empty-state";
import { PageHeader } from "../../components/page-header";
import { useContributeSubDrawer } from "../../shell/sub-drawer";
import { ConfigurationSection } from "./settings/configuration";
import { DangerZoneSection } from "./settings/danger-zone";
import { DeploysSection } from "./settings/deploys";
import {
  useEncryptionStatus,
  useLicenseSnapshot,
  useRuntimeDiagnostics,
} from "./settings/hooks";
import { IntegrationsSection } from "./settings/integrations";
import { ServerInfoSection, TenantHeaderStrip } from "./settings/server-info";
import { ADMIN_SETTINGS_SUB_DRAWER } from "./settings/sub-drawer";
import type {
  AdapterCapabilityDoc,
  BundleDoc,
  FunctionDoc,
  SystemStatusDoc,
} from "./settings/types";

// The sub-drawer's seven sub-pages are the section space: the route validates
// exactly the ids the menu can produce. `settings.spec.tsx` asserts the two
// stay in step at compile time.
const SECTIONS = [
  "general",
  "endpoints",
  "deploys",
  "token",
  "environment",
  "integrations",
  "shutdown",
] as const;

export type SettingsSection = (typeof SECTIONS)[number];
type SettingsSearch = { section: SettingsSection };

export function parseSettingsSection(
  value: unknown,
): SettingsSection | undefined {
  return typeof value === "string" &&
    (SECTIONS as readonly string[]).includes(value)
    ? (value as SettingsSection)
    : undefined;
}

export const Route = createFileRoute("/operator/settings")({
  component: SettingsPage,
  validateSearch: (search: Record<string, unknown>): SettingsSearch => ({
    section: parseSettingsSection(search.section) ?? "general",
  }),
  // `isItemActive` in the sub-drawer compares search values exactly, so a bare
  // `/operator/settings` would leave every item inactive. Normalizing the URL
  // to the default section is what makes the menu locate the operator, and it
  // keeps each section deep-linkable (DESIGN.md: "URL is state").
  beforeLoad: ({ search }) => {
    if (
      parseSettingsSection((search as Record<string, unknown>).section) ===
      undefined
    ) {
      throw redirect({
        to: "/operator/settings",
        search: { section: "general" },
        replace: true,
      });
    }
  },
});

const SECTION_SUBTITLES: Record<SettingsSection, string> = {
  general:
    "Appearance, server build and runtime info, and effective configuration.",
  endpoints: "Adapter base URLs and published listener endpoints.",
  deploys:
    "Bundle history, active release, and the functions each bundle ships.",
  token: "Admin token lifetime and rotation.",
  environment: "Process-level environment variables visible to the server.",
  integrations:
    "Adapter capability matrices — what each protocol surface implements today.",
  shutdown:
    "Session lifecycle: admin-token rotation and graceful server shutdown.",
};

// Sections the console does not implement yet. They stay in the menu because
// DESIGN.md specifies the sub-page list, but the pane says so directly instead
// of rendering an empty frame (DESIGN.md: Adapter Honesty).
const UNBUILT: Partial<
  Record<SettingsSection, { title: string; body: string }>
> = {
  endpoints: {
    title: "Endpoints",
    body: "Endpoint inventory is not available in this build. Registered HTTP routes and listeners are visible under Operator → Network.",
  },
  token: {
    title: "Token",
    body: "Token policy is not available in this build. Admin-token rotation currently lives under Settings → Shutdown.",
  },
  environment: {
    title: "Environment",
    body: "Environment inspection is not available in this build. Effective configuration values are listed under Settings → General.",
  },
};

function SettingsPage() {
  useContributeSubDrawer(ADMIN_SETTINGS_SUB_DRAWER);
  const section = Route.useSearch().section;
  const status = useQuery(api.system.status, {}) as SystemStatusDoc | undefined;
  const capabilities = useQuery(api.adapter_capabilities.list, {
    adapter: null,
    status: null,
    limit: 500,
  }) as AdapterCapabilityDoc[] | undefined;
  const bundles = useQuery(api.bundles.list, {
    status: null,
    limit: 50,
  }) as BundleDoc[] | undefined;
  const functions = useQuery(api.functions.list, {
    bundleId: null,
    kind: null,
    limit: 500,
  }) as FunctionDoc[] | undefined;

  const license = useLicenseSnapshot();
  const encryption = useEncryptionStatus();
  const diagnostics = useRuntimeDiagnostics();

  const unbuilt = UNBUILT[section];

  return (
    <section
      className="flex h-full flex-col gap-5 overflow-y-auto px-6 py-5"
      data-testid="page-settings"
      data-section={section}
    >
      <PageHeader title="Settings" subtitle={SECTION_SUBTITLES[section]} />

      {unbuilt ? (
        <div className="flex min-h-0 flex-1 rounded-md border border-app bg-surface">
          <EmptyState
            title={unbuilt.title}
            body={unbuilt.body}
            testid={`settings-${section}-unavailable`}
          />
        </div>
      ) : section === "deploys" ? (
        <DeploysSection bundles={bundles} functions={functions} />
      ) : section === "integrations" ? (
        <IntegrationsSection capabilities={capabilities} />
      ) : section === "shutdown" ? (
        <DangerZoneSection />
      ) : (
        <>
          <AppearanceSection />
          <TenantHeaderStrip status={status} license={license} />
          <ServerInfoSection status={status} encryption={encryption} />
          <ConfigurationSection
            diagnostics={diagnostics}
            license={license}
            status={status}
          />
        </>
      )}
    </section>
  );
}
