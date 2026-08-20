import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { StateChip } from "../../components/state-chip";
import {
  actionsForState,
  BRANCHED_MACHINE_STATES,
  OPTIMISTIC_STATES,
} from "./-use-machine-actions";

// The machines table renders `<StateChip state={optimisticState} />`, where
// `optimisticState` is either a real machine state or one this hook invents
// while a lifecycle request is in flight. If the two files drift, the row
// answers the operator's click with a muted `?` — "the console lost track of
// this machine" — at the exact moment they need to know their action landed.
describe("machine states the badge must be able to name", () => {
  const glyphFor = (state: string): string | null => {
    const { container } = render(<StateChip state={state} />);
    return (
      container.querySelector("[data-state]")?.getAttribute("data-glyph") ??
      null
    );
  };

  it.each(
    Object.entries(OPTIMISTIC_STATES),
  )("%s puts the row into %s, which StateChip names", (_action, state) => {
    expect(glyphFor(state)).not.toBe("question");
  });

  it.each(
    BRANCHED_MACHINE_STATES,
  )("%s is a state actionsForState branches on, so StateChip must name it", (state) => {
    expect(glyphFor(state)).not.toBe("question");
  });

  it("shows restarting and deleting as transitional half-filled dots", () => {
    expect(glyphFor("restarting")).toBe("half");
    expect(glyphFor("deleting")).toBe("half");
  });
});

describe("actionsForState", () => {
  it("offers no actions while a lifecycle request is in flight", () => {
    for (const state of Object.values(OPTIMISTIC_STATES)) {
      expect(actionsForState(state)).toEqual([]);
    }
  });

  it("suppresses the action row for a machine being deleted", () => {
    // Previously fell through to the catch-all and offered the full
    // Start / Stop / Restart / Delete row on a machine already going away.
    expect(actionsForState("deleting")).toEqual([]);
  });

  it("still offers stop and restart on a live machine", () => {
    expect(actionsForState("running")).toEqual(["stop", "restart"]);
  });

  it("falls back to the full action row for an unrecognized state", () => {
    expect(actionsForState("quantum")).toEqual([
      "start",
      "stop",
      "restart",
      "delete",
    ]);
  });
});
