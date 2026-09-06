import { describe, it } from "vitest";

import {
  OBSERVABILITY_SUB_DRAWER,
  type ObservabilityTab,
} from "./observability";

type Equal<A, B> =
  (<T>() => T extends A ? 1 : 2) extends <T>() => T extends B ? 1 : 2
    ? true
    : false;

function assertEqual<T extends true>(_: T): void {}

describe("ObservabilityTab type derivation", () => {
  it("ObservabilityTab is derived from the spec items, not duplicated", () => {
    assertEqual<Equal<ObservabilityTab, "logs" | "runs" | "events" | "errors">>(
      true,
    );
  });

  it("derivation tracks spec changes at compile time", () => {
    type FromConst = (typeof OBSERVABILITY_SUB_DRAWER.items)[number]["id"];
    assertEqual<Equal<FromConst, ObservabilityTab>>(true);
  });
});
