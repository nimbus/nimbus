import { describe, expect, it } from "vitest";

import { COMPUTE_VIEWS, parseComputeView } from "./compute-views";

describe("compute-views", () => {
  it("exposes functions, sandboxes, and the call graph as compute views", () => {
    expect(COMPUTE_VIEWS.map((v) => v.value)).toEqual([
      "functions",
      "sandboxes",
      "graph",
    ]);
  });

  it("gives every compute type a label and an icon", () => {
    for (const view of COMPUTE_VIEWS) {
      expect(view.label.length).toBeGreaterThan(0);
      expect(view.icon).toBeTruthy();
    }
  });

  it("defaults unknown or missing view params to functions", () => {
    expect(parseComputeView(undefined)).toBe("functions");
    expect(parseComputeView("nope")).toBe("functions");
    expect(parseComputeView("functions")).toBe("functions");
  });

  it("parses the sandboxes and graph views", () => {
    expect(parseComputeView("sandboxes")).toBe("sandboxes");
    expect(parseComputeView("graph")).toBe("graph");
  });
});
