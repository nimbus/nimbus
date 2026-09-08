import { formatCount, formatMemory } from "./format";

// A count on its own is a number with no story. These summaries name what
// a count is made of, so a tile or a header reads "3 machines · 2 running
// · 1 stopped" and not a bare 3.

// stateSummary names the count of every state in a list, largest first,
// and stops at three so a long tail of one-offs does not crowd the line.
export function stateSummary(
  items: ReadonlyArray<{ state?: string | null }>,
): string | null {
  if (items.length === 0) return null;
  const counts = new Map<string, number>();
  for (const item of items) {
    const word = item.state ? item.state.toLowerCase() : "unknown";
    counts.set(word, (counts.get(word) ?? 0) + 1);
  }
  return [...counts.entries()]
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
    .slice(0, 3)
    .map(([word, count]) => `${formatCount(count)} ${word}`)
    .join(" · ");
}

// adapterSummary names the adapters behind a listener count.
export function adapterSummary(
  listeners: ReadonlyArray<{ adapter?: string | null }>,
): string | null {
  const adapters = new Set<string>();
  for (const listener of listeners) {
    if (listener.adapter) adapters.add(listener.adapter);
  }
  if (adapters.size === 0) return null;
  return [...adapters].sort().join(", ");
}

export type Capacity = {
  cpus: number;
  memoryMiB: number;
  diskGiB: number;
};

// capacityOf adds up what a list of machines allocates. A machine that
// reports no figure for a resource adds nothing to that resource.
export function capacityOf(
  machines: ReadonlyArray<{
    resources?: { cpus?: number; memoryMiB?: number; diskGiB?: number };
  }>,
): Capacity {
  const total: Capacity = { cpus: 0, memoryMiB: 0, diskGiB: 0 };
  for (const machine of machines) {
    total.cpus += machine.resources?.cpus ?? 0;
    total.memoryMiB += machine.resources?.memoryMiB ?? 0;
    total.diskGiB += machine.resources?.diskGiB ?? 0;
  }
  return total;
}

// capacitySummary writes the allocation as one line, and leaves out a
// resource nothing reports rather than printing a zero.
export function capacitySummary(capacity: Capacity): string | null {
  const parts: string[] = [];
  if (capacity.cpus > 0) parts.push(`${formatCount(capacity.cpus)} vCPU`);
  if (capacity.memoryMiB > 0) {
    parts.push(`${formatMemory(capacity.memoryMiB)} memory`);
  }
  if (capacity.diskGiB > 0) {
    parts.push(`${formatCount(capacity.diskGiB)} GiB disk`);
  }
  return parts.length === 0 ? null : parts.join(" · ");
}
