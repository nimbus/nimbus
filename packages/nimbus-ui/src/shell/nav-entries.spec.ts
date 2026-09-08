import { describe, expect, it } from "vitest";

import {
  DEVELOPER_NAV_ENTRIES,
  DEVELOPER_NAV_GROUPS,
  type NavEntry,
  type NavGroup,
  navEntriesForView,
  navGroupsForView,
  OPERATOR_NAV_ENTRIES,
  OPERATOR_NAV_GROUPS,
  viewFromPathname,
} from "./nav-entries";

const EXPECTED_DEVELOPER_GROUPS: Array<[string | null, string[]]> = [
  [null, ["overview"]],
  ["Build", ["compute", "storage", "files"]],
  ["Run", ["services", "schedules"]],
  ["Observe", ["observability"]],
  [null, ["settings"]],
];

const EXPECTED_OPERATOR_GROUPS: Array<[string | null, string[]]> = [
  [null, ["nodes"]],
  ["Fleet", ["machines", "network", "services"]],
  ["Access", ["tenants"]],
  ["Observe", ["observability"]],
  [null, ["settings"]],
];

function shape(groups: ReadonlyArray<NavGroup>) {
  return groups.map((group) => [
    group.label,
    group.entries.map((entry) => entry.id),
  ]);
}

describe("nav-entries", () => {
  it("groups the eight developer entries as Build, Run and Observe", () => {
    expect(shape(DEVELOPER_NAV_GROUPS)).toEqual(EXPECTED_DEVELOPER_GROUPS);
  });

  it("groups the seven operator entries as Fleet, Access and Observe", () => {
    expect(shape(OPERATOR_NAV_GROUPS)).toEqual(EXPECTED_OPERATOR_GROUPS);
  });

  it("flattens the groups into the entry lists in group order", () => {
    expect(DEVELOPER_NAV_ENTRIES.map((e) => e.id)).toEqual(
      EXPECTED_DEVELOPER_GROUPS.flatMap(([, ids]) => ids),
    );
    expect(OPERATOR_NAV_ENTRIES.map((e) => e.id)).toEqual(
      EXPECTED_OPERATOR_GROUPS.flatMap(([, ids]) => ids),
    );
  });

  it("puts the view's home page first and Settings last, both outside a group", () => {
    for (const groups of [DEVELOPER_NAV_GROUPS, OPERATOR_NAV_GROUPS]) {
      const first = groups[0];
      const last = groups[groups.length - 1];
      expect(first?.label).toBeNull();
      expect(first?.entries[0]?.to).toMatch(/^\/(developer|operator)$/);
      expect(last?.label).toBeNull();
      expect(last?.entries.map((e) => e.id)).toEqual(["settings"]);
    }
  });

  it("tags every entry with the view of its group list", () => {
    for (const entry of DEVELOPER_NAV_ENTRIES) {
      expect(entry.view).toBe("developer");
    }
    for (const entry of OPERATOR_NAV_ENTRIES) {
      expect(entry.view).toBe("operator");
    }
  });

  it("has unique ids within each view", () => {
    expectUniqueIds(DEVELOPER_NAV_ENTRIES);
    expectUniqueIds(OPERATOR_NAV_ENTRIES);
  });

  it("targets developer paths under /developer and operator paths under /operator", () => {
    for (const entry of DEVELOPER_NAV_ENTRIES) {
      expect(entry.to.startsWith("/developer")).toBe(true);
    }
    for (const entry of OPERATOR_NAV_ENTRIES) {
      expect(entry.to.startsWith("/operator")).toBe(true);
    }
  });

  it("carries no count on any entry", () => {
    // The sidebar shows no badges: a zero-count badge on every empty
    // section was the noise the redesign removed, so the entry shape has no
    // place to put one.
    for (const entry of [...DEVELOPER_NAV_ENTRIES, ...OPERATOR_NAV_ENTRIES]) {
      expect(entry).not.toHaveProperty("count");
      expect(entry).not.toHaveProperty("countKind");
    }
  });

  it("navGroupsForView and navEntriesForView return the matching lists", () => {
    expect(navGroupsForView("developer")).toBe(DEVELOPER_NAV_GROUPS);
    expect(navGroupsForView("operator")).toBe(OPERATOR_NAV_GROUPS);
    expect(navEntriesForView("developer")).toBe(DEVELOPER_NAV_ENTRIES);
    expect(navEntriesForView("operator")).toBe(OPERATOR_NAV_ENTRIES);
  });

  it("viewFromPathname maps /operator* to operator and everything else to developer", () => {
    expect(viewFromPathname("/operator")).toBe("operator");
    expect(viewFromPathname("/operator/")).toBe("operator");
    expect(viewFromPathname("/operator/machines")).toBe("operator");
    expect(viewFromPathname("/developer")).toBe("developer");
    expect(viewFromPathname("/developer/compute")).toBe("developer");
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
