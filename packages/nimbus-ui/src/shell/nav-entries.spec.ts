import { describe, expect, it } from "vitest";

import {
  DEVELOPER_NAV_ENTRIES,
  type NavEntry,
  navEntriesForView,
  OPERATOR_NAV_ENTRIES,
  viewFromPathname,
} from "./nav-entries";

const EXPECTED_DEVELOPER_IDS = [
  "overview",
  "compute",
  "services",
  "schedules",
  "storage",
  "files",
  "observability",
  "settings",
];

const EXPECTED_OPERATOR_IDS = [
  "system",
  "tenants",
  "machines",
  "network",
  "services",
  "observability",
  "settings",
];

describe("nav-entries", () => {
  it("exports eight developer entries in the expected order", () => {
    expect(DEVELOPER_NAV_ENTRIES.map((e) => e.id)).toEqual(
      EXPECTED_DEVELOPER_IDS,
    );
  });

  it("exports seven operator entries in the expected order", () => {
    expect(OPERATOR_NAV_ENTRIES.map((e) => e.id)).toEqual(
      EXPECTED_OPERATOR_IDS,
    );
  });

  it("tags every developer entry with view='developer'", () => {
    for (const entry of DEVELOPER_NAV_ENTRIES) {
      expect(entry.view).toBe("developer");
    }
  });

  it("tags every operator entry with view='operator'", () => {
    for (const entry of OPERATOR_NAV_ENTRIES) {
      expect(entry.view).toBe("operator");
    }
  });

  it("has unique ids within each view", () => {
    expectUniqueIds(DEVELOPER_NAV_ENTRIES);
    expectUniqueIds(OPERATOR_NAV_ENTRIES);
  });

  it("targets developer paths under /app and operator paths under /admin", () => {
    for (const entry of DEVELOPER_NAV_ENTRIES) {
      expect(entry.to.startsWith("/app")).toBe(true);
    }
    for (const entry of OPERATOR_NAV_ENTRIES) {
      expect(entry.to.startsWith("/admin")).toBe(true);
    }
  });

  it("pairs countQuery with countArgs (both set or both null)", () => {
    for (const entry of [...DEVELOPER_NAV_ENTRIES, ...OPERATOR_NAV_ENTRIES]) {
      expect(entry.countQuery === null).toBe(entry.countArgs === null);
    }
  });

  it("navEntriesForView returns the matching list", () => {
    expect(navEntriesForView("developer")).toBe(DEVELOPER_NAV_ENTRIES);
    expect(navEntriesForView("operator")).toBe(OPERATOR_NAV_ENTRIES);
  });

  it("viewFromPathname maps /admin* to operator and everything else to developer", () => {
    expect(viewFromPathname("/admin")).toBe("operator");
    expect(viewFromPathname("/admin/")).toBe("operator");
    expect(viewFromPathname("/admin/machines")).toBe("operator");
    expect(viewFromPathname("/app")).toBe("developer");
    expect(viewFromPathname("/app/compute")).toBe("developer");
    expect(viewFromPathname("/")).toBe("developer");
  });
});

function expectUniqueIds(entries: NavEntry[]): void {
  const seen = new Set<string>();
  for (const entry of entries) {
    expect(seen.has(entry.id)).toBe(false);
    seen.add(entry.id);
  }
}
