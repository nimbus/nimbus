import { describe, expect, it } from "vitest";

import { groupLogs } from "./-log-groups";
import type { EventDoc, RunDoc } from "./-types";

const NOW = 1_700_000_000_000;

const RUNS: RunDoc[] = [
  { _id: "run-1", functionPath: "a:one", status: "error", startedAt: NOW + 3 },
  { _id: "run-2", functionPath: "a:two", status: "ok", startedAt: NOW + 2 },
  { _id: "run-3", functionPath: "a:three", status: "ok", startedAt: NOW + 1 },
];

const EVENTS: EventDoc[] = [
  { _id: "evt-1", createdAt: NOW + 4, level: "error", correlationId: "run-1" },
  { _id: "evt-2", createdAt: NOW, level: "info" },
  { _id: "evt-3", createdAt: NOW + 5, level: "info", correlationId: "run-1" },
];

describe("groupLogs", () => {
  it("makes one group per run, newest first, and parks free lines under the server group", () => {
    const groups = groupLogs(RUNS, EVENTS, { lineFiltered: false });
    expect(groups.map((g) => g.id)).toEqual([
      "run-1",
      "run-2",
      "run-3",
      "server",
    ]);
    expect(groups[0].events.map((e) => e._id)).toEqual(["evt-3", "evt-1"]);
    expect(groups[3].kind).toBe("server");
    expect(groups[3].events.map((e) => e._id)).toEqual(["evt-2"]);
  });

  it("omits the server group when every line belongs to a run", () => {
    const groups = groupLogs(RUNS, [EVENTS[0]], { lineFiltered: false });
    expect(groups.find((g) => g.kind === "server")).toBeUndefined();
  });

  it("keeps a run group with no lines so the run is still visible", () => {
    const groups = groupLogs(RUNS, [], { lineFiltered: false });
    expect(groups).toHaveLength(3);
    expect(groups.every((g) => g.events.length === 0)).toBe(true);
  });

  it("drops runs without a matching line while a line facet is set", () => {
    const groups = groupLogs(RUNS, [EVENTS[0]], { lineFiltered: true });
    expect(groups.map((g) => g.id)).toEqual(["run-1"]);
  });

  it("narrows to one run's group when a correlation id is set", () => {
    const groups = groupLogs(RUNS, EVENTS, {
      correlationId: "run-1",
      lineFiltered: false,
    });
    expect(groups.map((g) => g.id)).toEqual(["run-1"]);
    expect(groups[0].events).toHaveLength(2);
  });

  it("builds a run group without a doc for lines whose run was not loaded", () => {
    const orphan: EventDoc = {
      _id: "evt-9",
      createdAt: NOW + 10,
      correlationId: "run-gone",
    };
    const groups = groupLogs(RUNS, [orphan], { lineFiltered: false });
    expect(groups[0]).toMatchObject({
      kind: "run",
      id: "run-gone",
      at: NOW + 10,
    });
    expect((groups[0] as { run?: unknown }).run).toBeUndefined();
  });

  it("orders a run by its start and the server group by its newest line", () => {
    const late: EventDoc = { _id: "evt-late", createdAt: NOW + 100 };
    const groups = groupLogs(RUNS, [late], { lineFiltered: false });
    expect(groups[0].id).toBe("server");
  });
});
