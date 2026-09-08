import { describe, expect, it } from "vitest";

import { COMPUTE_TABS, parseComputeTab } from "./-compute-tabs";

describe("compute-tabs", () => {
  it("exposes functions, sandboxes, and the graph as page tabs", () => {
    expect(COMPUTE_TABS.map((t) => t.id)).toEqual([
      "functions",
      "sandboxes",
      "graph",
    ]);
    for (const tab of COMPUTE_TABS) expect(tab.label.length).toBeGreaterThan(0);
  });

  it("drops an unknown or missing tab so the page falls back to functions", () => {
    expect(parseComputeTab(undefined)).toBeUndefined();
    expect(parseComputeTab("nope")).toBeUndefined();
    expect(parseComputeTab("functions")).toBe("functions");
  });

  it("parses the sandboxes and graph tabs", () => {
    expect(parseComputeTab("sandboxes")).toBe("sandboxes");
    expect(parseComputeTab("graph")).toBe("graph");
  });
});
