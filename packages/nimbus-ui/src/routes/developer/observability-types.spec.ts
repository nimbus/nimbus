import { describe, it } from "vitest";
import type { ObservabilityTab } from "./observability";
import { OBSERVABILITY_TABS } from "./observability/-types";

type Equal<A, B> =
  (<T>() => T extends A ? 1 : 2) extends <T>() => T extends B ? 1 : 2
    ? true
    : false;

function assertEqual<T extends true>(_: T): void {}

describe("ObservabilityTab type derivation", () => {
  it("ObservabilityTab names only the tabs that exist", () => {
    assertEqual<Equal<ObservabilityTab, "logs" | "runs" | "traces" | "errors">>(
      true,
    );
  });

  it("derivation tracks the tab list at compile time", () => {
    type FromConst = (typeof OBSERVABILITY_TABS)[number]["id"];
    assertEqual<Equal<FromConst, ObservabilityTab>>(true);
  });
});
