import { describe, expect, it } from "vitest";

import { OBSERVABILITY_SUB_DRAWER } from "./observability";
import { SCHEDULES_SUB_DRAWER } from "./schedules";

describe("Observability section nav (DR3 / F3)", () => {
  it("sub-drawer is the single source of truth: 4 items (logs, runs, events, errors)", () => {
    expect(OBSERVABILITY_SUB_DRAWER.kind).toBe("static");
    if (OBSERVABILITY_SUB_DRAWER.kind !== "static") return;
    expect(OBSERVABILITY_SUB_DRAWER.items.map((i) => i.id)).toEqual([
      "logs",
      "runs",
      "events",
      "errors",
    ]);
  });

  it("events and errors are flagged disabled until their backends land", () => {
    if (OBSERVABILITY_SUB_DRAWER.kind !== "static") return;
    const byId = Object.fromEntries(
      OBSERVABILITY_SUB_DRAWER.items.map((i) => [i.id, i]),
    );
    expect(byId.logs?.disabled).toBeFalsy();
    expect(byId.runs?.disabled).toBeFalsy();
    expect(byId.events?.disabled).toBe(true);
    expect(byId.errors?.disabled).toBe(true);
  });
});

describe("Schedules section nav (DR3 / F4)", () => {
  it("sub-drawer is static with exactly the two stable sections", () => {
    expect(SCHEDULES_SUB_DRAWER.kind).toBe("static");
    if (SCHEDULES_SUB_DRAWER.kind !== "static") return;
    expect(SCHEDULES_SUB_DRAWER.items.map((i) => i.id)).toEqual([
      "scheduled",
      "cron",
    ]);
  });

  it("each Schedules item routes through the ?section= query, not a path segment", () => {
    if (SCHEDULES_SUB_DRAWER.kind !== "static") return;
    for (const item of SCHEDULES_SUB_DRAWER.items) {
      expect(item.to).toBe("/app/schedules");
      expect(item.search).toEqual({ section: item.id });
    }
  });
});
