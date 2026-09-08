import { describe, expect, it } from "vitest";

import { OBSERVABILITY_SUB_PANEL } from "./observability";
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

describe("Observability section nav (DR3 / F3)", () => {
  it("sub-panel is the single source of truth: 4 items (logs, runs, events, errors)", () => {
    assertStatic(OBSERVABILITY_SUB_PANEL, "OBSERVABILITY_SUB_PANEL");
    expect(OBSERVABILITY_SUB_PANEL.items.map((i) => i.id)).toEqual([
      "logs",
      "runs",
      "events",
      "errors",
    ]);
  });

  it("events and errors are flagged disabled until their backends land", () => {
    assertStatic(OBSERVABILITY_SUB_PANEL, "OBSERVABILITY_SUB_PANEL");
    const byId = Object.fromEntries(
      OBSERVABILITY_SUB_PANEL.items.map((i) => [i.id, i]),
    );
    expect(byId.logs?.disabled).toBeFalsy();
    expect(byId.runs?.disabled).toBeFalsy();
    expect(byId.events?.disabled).toBe(true);
    expect(byId.errors?.disabled).toBe(true);
  });
});

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
