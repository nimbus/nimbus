import { describe, expect, it } from "vitest";

import { isMac, metaGlyph } from "./platform";

describe("platform", () => {
  it("exports a boolean isMac flag", () => {
    expect(typeof isMac).toBe("boolean");
  });

  it("uses ⌘ glyph on mac, Ctrl elsewhere", () => {
    expect(metaGlyph).toBe(isMac ? "⌘" : "Ctrl");
  });
});
