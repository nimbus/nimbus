import { describe, expect, it } from "vitest";

import { SCHEDULES_SUB_PANEL } from "./schedules";

function assertStatic<T extends { kind: string }>(
  spec: T,
  name: string,
): asserts spec is Extract<T, { kind: "static" }> {
  if (spec.kind !== "static") {
    throw new Error(
      `${name} expected to be a static sub-panel, got kind="${spec.kind}"`,
    );
  }
}

describe("Schedules section nav (DR3 / F4)", () => {
  it("sub-panel is static with exactly the two stable sections", () => {
    assertStatic(SCHEDULES_SUB_PANEL, "SCHEDULES_SUB_PANEL");
    expect(SCHEDULES_SUB_PANEL.items.map((i) => i.id)).toEqual([
      "scheduled",
      "cron",
    ]);
  });

  it("each Schedules item routes through the ?section= query, not a path segment", () => {
    assertStatic(SCHEDULES_SUB_PANEL, "SCHEDULES_SUB_PANEL");
    for (const item of SCHEDULES_SUB_PANEL.items) {
      expect(item.to).toBe("/developer/schedules");
      expect(item.search).toEqual({ section: item.id });
    }
  });
});
