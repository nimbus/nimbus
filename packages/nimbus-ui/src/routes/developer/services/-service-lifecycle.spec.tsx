import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { StatePill } from "../../../components/pill";
import type { ServiceDoc } from "../../../lib/types/service";
import {
  actionsForState,
  BRANCHED_SERVICE_STATES,
  OPTIMISTIC_STATES,
  sourceGenerationOf,
} from "./-service-lifecycle";

// The services table renders `<StatePill state={shownState} />`, where
// `shownState` is a real service state or one this module invents while a
// lifecycle request is in flight. If the two vocabularies drift, the row
// answers the click with a muted `?` at the moment the operator needs to
// know their action landed.
describe("service states the pill must be able to name", () => {
  const glyphFor = (state: string): string | null => {
    const { container } = render(<StatePill state={state} />);
    return (
      container.querySelector("[data-state]")?.getAttribute("data-glyph") ??
      null
    );
  };

  it.each(
    Object.entries(OPTIMISTIC_STATES),
  )("%s puts the row into %s, which StatePill names", (_action, state) => {
    expect(glyphFor(state)).not.toBe("question");
  });

  it.each(
    BRANCHED_SERVICE_STATES,
  )("%s is a state actionsForState branches on, so StatePill must name it", (state) => {
    expect(glyphFor(state)).not.toBe("question");
  });
});

describe("actionsForState", () => {
  it("offers no actions while a lifecycle request is in flight", () => {
    for (const state of Object.values(OPTIMISTIC_STATES)) {
      expect(actionsForState(state)).toEqual([]);
    }
  });

  it("offers stop and restart on a ready service", () => {
    expect(actionsForState("ready")).toEqual(["stop", "restart"]);
    expect(actionsForState("running")).toEqual(["stop", "restart"]);
  });

  it("offers start on a stopped or not-ready service", () => {
    expect(actionsForState("stopped")).toEqual(["start"]);
    expect(actionsForState("not_ready")).toEqual(["start"]);
  });

  it("offers start and restart on a failed service", () => {
    expect(actionsForState("failed")).toEqual(["start", "restart"]);
  });

  it("falls back to the full row for an unknown or missing state", () => {
    expect(actionsForState("weird")).toEqual(["start", "stop", "restart"]);
    expect(actionsForState(undefined)).toEqual(["start", "stop", "restart"]);
  });
});

describe("sourceGenerationOf", () => {
  const doc = (sourceGeneration: unknown) =>
    ({ _id: "svc", sourceGeneration }) as unknown as ServiceDoc;

  it("reads the string the system schema stores", () => {
    expect(sourceGenerationOf(doc("7"))).toBe(7);
  });

  it("accepts a number and floors it", () => {
    expect(sourceGenerationOf(doc(2.9))).toBe(2);
  });

  it("falls back to the first generation when the row carries none", () => {
    expect(sourceGenerationOf(doc(undefined))).toBe(1);
    expect(sourceGenerationOf(doc("nope"))).toBe(1);
    expect(sourceGenerationOf(doc(0))).toBe(1);
  });
});
