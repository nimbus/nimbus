import type { StaticSubPanelSpec } from "../../../shell/sub-panel";

// A static menu lists only pages that exist (DESIGN.md: sub-panel rules).
// Endpoints, Token, and Environment are planned sub-pages with no pane in
// this build, so they are not items, disabled or otherwise. Deploys has its
// own page under the Developer view (`/developer/deploys`).
export const ADMIN_SETTINGS_SUB_PANEL = {
  kind: "static",
  title: "Settings",
  items: [
    {
      id: "general",
      label: "General",
      to: "/operator/settings",
      search: { section: "general" },
    },
    {
      id: "system",
      label: "System",
      to: "/operator/settings",
      search: { section: "system" },
    },
    {
      id: "integrations",
      label: "Integrations",
      to: "/operator/settings",
      search: { section: "integrations" },
    },
    {
      id: "shutdown",
      label: "Shutdown",
      to: "/operator/settings",
      search: { section: "shutdown" },
    },
  ],
} as const satisfies StaticSubPanelSpec<
  "general" | "system" | "integrations" | "shutdown"
>;
