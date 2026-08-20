import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

// The highlighter is irrelevant to the typography contract under test, and
// loading shiki's grammars in the test environment is slow; stub it so the
// component renders its plain fallback deterministically.
vi.mock("shiki", () => ({
  createHighlighter: () => Promise.reject(new Error("stubbed")),
}));
vi.mock("shiki/engine/javascript", () => ({
  createJavaScriptRegexEngine: () => ({}),
}));

import { CodeBlock } from "./code-block";

describe("CodeBlock", () => {
  it("uses 12px monospace at 1.5 leading, per DESIGN.md", () => {
    render(<CodeBlock code={"const a = 1;\n"} testid="cb" />);
    const pre = screen.getByTestId("cb");
    expect(pre).toHaveClass("font-mono", "text-sm", "leading-[1.5]");
    expect(pre).not.toHaveClass("leading-5");
  });

  it("sits on the surface-2 background with 12px padding", () => {
    render(<CodeBlock code={"const a = 1;\n"} testid="cb" />);
    const pre = screen.getByTestId("cb");
    expect(pre).toHaveClass("bg-surface-2", "p-3");
  });
});
