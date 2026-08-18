import { describe, expect, it } from "vitest";

import { resolveStateKind } from "../../../components/state-chip";
import { RUN_STATUSES } from "./-runs";

/**
 * The status dropdown is a closed list, so every option it offers is a promise
 * that rows exist behind it. `running` and `queued` were on that list and no
 * run has ever carried either: a row is written only after the invocation
 * returns, with `result.is_ok() ? "ok" : "error"`. Both options answered "No
 * runs" for every deployment that has ever existed, which reads as "your runs
 * are missing" rather than "this state does not occur here".
 */
describe("run status filter", () => {
  it("offers only the two values a run can carry", () => {
    expect([...RUN_STATUSES]).toEqual(["ok", "error"]);
  });

  it("names every offered value in the state palette", () => {
    for (const status of RUN_STATUSES) {
      expect(resolveStateKind(status)).toBe(status);
    }
  });
});
