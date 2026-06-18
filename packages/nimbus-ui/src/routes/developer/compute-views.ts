import { Box, type LucideIcon, SquareFunction } from "lucide-react";

// Compute has exactly two kinds under Developer: request-scoped Functions and
// long-lived Sandboxes. They are mutually exclusive views (one at a time).
// Services and Sessions are separate top-level resources, not compute types.
export type ComputeView = "functions" | "sandboxes";

export type ComputeViewOption = {
  value: ComputeView;
  label: string;
  icon: LucideIcon;
};

export const COMPUTE_VIEWS: ReadonlyArray<ComputeViewOption> = [
  { value: "functions", label: "Functions", icon: SquareFunction },
  { value: "sandboxes", label: "Sandboxes", icon: Box },
];

export function parseComputeView(value: unknown): ComputeView {
  return value === "sandboxes" ? "sandboxes" : "functions";
}
