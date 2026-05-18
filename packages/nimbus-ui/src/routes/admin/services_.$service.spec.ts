import { describe, expect, it } from "vitest";

import { isTab, TABS } from "./services_.$service";

describe("Admin service detail tabs (DR6 / F6)", () => {
  it("TABS has exactly one entry: Placement", () => {
    expect(TABS).toHaveLength(1);
    expect(TABS[0]).toEqual({ id: "placement", label: "Placement" });
  });

  it("isTab accepts only 'placement'", () => {
    expect(isTab("placement")).toBe(true);
    expect(isTab("restarts")).toBe(false);
    expect(isTab("density")).toBe(false);
    expect(isTab("drift")).toBe(false);
    expect(isTab(undefined)).toBe(false);
    expect(isTab(null)).toBe(false);
    expect(isTab(42)).toBe(false);
  });
});
