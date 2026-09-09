import { render, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

// `useApiRead` hands its `select` the raw API result; the spec drives it
// with a 404 and with a source payload so the mapping is what is proven.
const { resultRef } = vi.hoisted(() => ({
  resultRef: { current: null as unknown },
}));

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => config,
  useNavigate: () => vi.fn(),
  useSearch: () => ({}),
  useRouter: () => ({
    buildLocation: () => ({ href: "#" }),
    navigate: vi.fn(),
  }),
  Link: ({
    to,
    children,
    "data-testid": testId,
  }: {
    to?: string;
    children?: ReactNode;
    "data-testid"?: string;
  }) => (
    <a href={to ?? "#"} data-testid={testId}>
      {children}
    </a>
  ),
}));
vi.mock("@nimbus/nimbus/react", () => ({ useQuery: () => undefined }));
vi.mock("../../shell/sub-panel", () => ({
  useContributeSubPanel: () => undefined,
  useSubPanelSearch: () => "",
}));
vi.mock("../../hooks/use-api-read", () => ({
  useApiRead: (_path: string, select?: (result: unknown) => unknown) =>
    select ? select(resultRef.current) : { kind: "loading" },
}));
vi.mock("../../components/code-block", () => ({
  CodeBlock: ({ code }: { code: string }) => (
    <pre data-testid="code-block">{code}</pre>
  ),
}));

import { SOURCE_CAPTURE_COMMAND, SourceTab } from "./compute_.$function";

const fn = { _id: "functions:1", path: "agent:send", kind: "mutation" };

beforeEach(() => {
  resultRef.current = null;
});

describe("SourceTab", () => {
  it("shows the exact nimbus dev command with a copy button when source is missing", () => {
    resultRef.current = {
      ok: false,
      status: 404,
      error: "no source for module",
    };
    render(<SourceTab fn={fn} />);

    expect(screen.getByTestId("function-source-missing")).toBeTruthy();
    expect(screen.getByText("Source not available")).toBeTruthy();
    const snippet = screen.getByTestId("function-source-missing-snippet");
    expect(snippet.textContent).toContain(SOURCE_CAPTURE_COMMAND);
    expect(SOURCE_CAPTURE_COMMAND).toBe("nimbus dev --app-dir .");
    expect(screen.getByRole("button", { name: /copy command/i })).toBeTruthy();
  });

  it("reports a failed read as an error, not as missing source", () => {
    resultRef.current = { ok: false, status: 500, error: "boom" };
    render(<SourceTab fn={fn} />);

    expect(screen.getByText("Could not load source")).toBeTruthy();
    expect(screen.queryByTestId("function-source-missing")).toBeNull();
  });

  it("renders the module source and its symbols when present", () => {
    resultRef.current = {
      ok: true,
      status: 200,
      data: {
        source: "export const send = mutation({});",
        digest: "sha256:abcdef1234567890",
        analysis: {
          exports: [{ name: "send", line: 1 }],
          imports: [],
          references: [{ target: "agent:list", line: 1 }],
        },
        called_by: [{ target: "agent:send", caller: "chat:post" }],
      },
    };
    render(<SourceTab fn={fn} />);

    expect(screen.getByTestId("function-tab-source")).toBeTruthy();
    expect(screen.getByTestId("code-block").textContent).toContain(
      "export const send",
    );
    expect(screen.getByTestId("function-source-define-send")).toBeTruthy();
    expect(screen.getByTestId("function-source-call-agent:list")).toBeTruthy();
    expect(
      screen.getByTestId("function-source-calledby-chat:post"),
    ).toBeTruthy();
  });
});
