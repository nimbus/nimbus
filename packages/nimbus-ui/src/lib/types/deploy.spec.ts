import { describe, expect, it } from "vitest";

import {
  canRollBack,
  type DeployActivation,
  diffFunctionPaths,
} from "./deploy";

const fns = (...paths: string[]) =>
  paths.map((path) => ({ path, kind: "query" }));

function activation(over: Partial<DeployActivation>): DeployActivation {
  return {
    sha256: "a".repeat(64),
    generation: 1,
    activatedAt: 1_700_000_000_000,
    actor: "deploy-admin",
    sourceRef: "deploy:generation:1",
    kind: "deploy",
    retained: true,
    functions: [],
    ...over,
  };
}

describe("diffFunctionPaths", () => {
  it("sorts the paths that appear, vanish and stay", () => {
    const diff = diffFunctionPaths(
      fns("todos:list", "todos:add", "notes:list"),
      fns("todos:list", "notes:list", "notes:add", "archive:run"),
    );
    expect(diff).toEqual({
      added: ["archive:run", "notes:add"],
      removed: ["todos:add"],
      kept: ["notes:list", "todos:list"],
    });
  });

  it("reads an identical inventory as all kept", () => {
    expect(diffFunctionPaths(fns("a:b"), fns("a:b"))).toEqual({
      added: [],
      removed: [],
      kept: ["a:b"],
    });
  });
});

describe("canRollBack", () => {
  const sha = "b".repeat(64);
  it("allows a retained bundle that is not active", () => {
    expect(canRollBack(activation({ sha256: sha }), "a".repeat(64))).toBe(true);
  });
  it("refuses the active bundle and a bundle without retained files", () => {
    expect(canRollBack(activation({ sha256: sha }), sha)).toBe(false);
    expect(
      canRollBack(activation({ sha256: sha, retained: false }), null),
    ).toBe(false);
  });
});
