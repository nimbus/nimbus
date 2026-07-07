import type { Doc } from "../../../convex/_generated/dataModel";

// UI view of a `machines` row: the generated shape with `resources`/`meta`
// narrowed to the fields the operator console reads.
export type MachineDoc = Omit<Doc<"machines">, "resources" | "meta"> & {
  resources?: {
    cpus?: number;
    memoryMiB?: number;
    diskGiB?: number;
  };
  meta?: Record<string, unknown> | null;
};

export type EventDoc = Doc<"events">;
