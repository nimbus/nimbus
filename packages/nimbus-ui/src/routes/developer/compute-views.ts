import { Box, type LucideIcon, Network, SquareFunction } from "lucide-react";

// Compute kinds under Developer: request-scoped Functions, long-lived Sandboxes,
// and a deployment-wide call Graph. Mutually exclusive views (one at a time).
// Services and Sessions are separate top-level resources, not compute types.
export type ComputeView = "functions" | "sandboxes" | "graph";

export type ComputeViewOption = {
  value: ComputeView;
  label: string;
  icon: LucideIcon;
};

export const COMPUTE_VIEWS: ReadonlyArray<ComputeViewOption> = [
  { value: "functions", label: "Functions", icon: SquareFunction },
  { value: "sandboxes", label: "Sandboxes", icon: Box },
  { value: "graph", label: "Call graph", icon: Network },
];

export function parseComputeView(value: unknown): ComputeView {
  if (value === "sandboxes") return "sandboxes";
  if (value === "graph") return "graph";
  return "functions";
}
