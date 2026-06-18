import { describe, expect, it } from "vitest";

import { COMPUTE_VIEWS, parseComputeView } from "./compute-views";

describe("compute-views", () => {
  it("exposes functions and sandboxes as the two compute types", () => {
    expect(COMPUTE_VIEWS.map((v) => v.value)).toEqual([
      "functions",
      "sandboxes",
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

  it("parses the sandboxes view", () => {
    expect(parseComputeView("sandboxes")).toBe("sandboxes");
  });
});
