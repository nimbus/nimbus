import type { PageTab } from "../../components/page-tabs";

// Compute has three views: request-scoped Functions, long-lived Sandboxes,
// and the deployment-wide call Graph. They are page tabs on `?tab=`, so each
// has an address; the sub-panel holds the function tree, which is the list
// an operator filters, not the view switch.
export type ComputeTab = "functions" | "sandboxes" | "graph";

export const COMPUTE_TABS: ReadonlyArray<PageTab<ComputeTab>> = [
  { id: "functions", label: "Functions" },
  { id: "sandboxes", label: "Sandboxes" },
  { id: "graph", label: "Graph" },
];

export function parseComputeTab(value: unknown): ComputeTab | undefined {
  if (value === "sandboxes" || value === "graph" || value === "functions") {
    return value;
  }
  return undefined;
}
