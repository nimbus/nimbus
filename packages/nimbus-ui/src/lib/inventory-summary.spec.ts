import { describe, expect, it } from "vitest";

import {
  adapterSummary,
  capacityOf,
  capacitySummary,
  stateSummary,
} from "./inventory-summary";

describe("stateSummary", () => {
  it("names the states behind a count, largest first, ties by name", () => {
    expect(
      stateSummary([
        { state: "stopped" },
        { state: "running" },
        { state: "running" },
        { state: "Failed" },
      ]),
    ).toBe("2 running · 1 failed · 1 stopped");
  });

  it("stops at three states and calls a missing state unknown", () => {
    expect(
      stateSummary([
        { state: "a" },
        { state: "b" },
        { state: "c" },
        { state: null },
      ]),
    ).toBe("1 a · 1 b · 1 c");
    expect(stateSummary([{}])).toBe("1 unknown");
    expect(stateSummary([])).toBeNull();
  });
});

describe("adapterSummary", () => {
  it("lists each adapter once, sorted", () => {
    expect(
      adapterSummary([
        { adapter: "ws" },
        { adapter: "http" },
        { adapter: "ws" },
      ]),
    ).toBe("http, ws");
    expect(adapterSummary([{}])).toBeNull();
  });
});

describe("capacity", () => {
  it("adds up what every machine allocates", () => {
    expect(
      capacityOf([
        { resources: { cpus: 4, memoryMiB: 8192, diskGiB: 60 } },
        { resources: { cpus: 4, memoryMiB: 8192 } },
        {},
      ]),
    ).toEqual({ cpus: 8, memoryMiB: 16384, diskGiB: 60 });
  });

  it("writes the allocation as one line and leaves out what is zero", () => {
    expect(capacitySummary({ cpus: 8, memoryMiB: 16384, diskGiB: 120 })).toBe(
      "8 vCPU · 16 GiB memory · 120 GiB disk",
    );
    expect(capacitySummary({ cpus: 2, memoryMiB: 0, diskGiB: 0 })).toBe(
      "2 vCPU",
    );
    expect(capacitySummary({ cpus: 0, memoryMiB: 0, diskGiB: 0 })).toBeNull();
  });
});
