import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useUiStore } from "../../store/ui-store";
import { FunctionRunner } from "./function-runner";

// The Nimbus SDK records a validator as `{kind, fields, inner, ...}`.
const twoStrings = {
  kind: "object",
  fields: {
    conversationId: { kind: "string" },
    text: { kind: "string" },
  },
};

const sendFn = {
  _id: "functions:send",
  path: "agent:send",
  kind: "mutation",
  adapter: "nimbus",
  argsSchema: twoStrings,
};

function okResponse(body: unknown, correlationId = "corr-1") {
  return {
    ok: true,
    status: 200,
    headers: new Headers({ "x-nimbus-correlation-id": correlationId }),
    json: async () => body,
  } as unknown as Response;
}

const fetchMock = vi.fn();

beforeEach(() => {
  useUiStore.setState({ activeTenant: "demo" });
  fetchMock.mockReset();
  fetchMock.mockResolvedValue(okResponse({ tool: null }));
  vi.stubGlobal("fetch", fetchMock);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

function openRunner() {
  fireEvent.click(screen.getByTestId("function-runner-toggle"));
}

describe("FunctionRunner", () => {
  it("pre-fills one field per validator argument in form mode", () => {
    render(<FunctionRunner fn={sendFn} />);
    openRunner();

    const conversation = screen.getByTestId(
      "function-runner-field-conversationId",
    );
    const text = screen.getByTestId("function-runner-field-text");
    expect(conversation).toBeInstanceOf(HTMLInputElement);
    expect(text).toBeInstanceOf(HTMLInputElement);
    expect(screen.getByText("conversationId")).toBeTruthy();
    expect(screen.getByText("text")).toBeTruthy();
    // Form mode is the default when the validator is known.
    expect(
      screen
        .getByTestId("function-runner-mode-form")
        .getAttribute("data-active"),
    ).toBe("true");
    expect(screen.queryByTestId("function-runner-args-input")).toBeNull();
  });

  it("submits the form values as the argument object on ⌘⏎", async () => {
    render(<FunctionRunner fn={sendFn} />);
    openRunner();

    fireEvent.change(
      screen.getByTestId("function-runner-field-conversationId"),
      { target: { value: "c1" } },
    );
    const text = screen.getByTestId("function-runner-field-text");
    fireEvent.change(text, { target: { value: "hello" } });
    fireEvent.keyDown(text, { key: "Enter", metaKey: true });

    await screen.findByTestId("function-runner-result-ok");
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toBe("/convex/demo/mutation");
    expect(JSON.parse(String(init.body))).toEqual({
      name: "agent:send",
      args: { conversationId: "c1", text: "hello" },
    });
    expect(
      screen.getByTestId("function-runner-result-ok-correlation").textContent,
    ).toContain("corr-1");
  });

  it("labels the submit button Run function with the ⌘⏎ shortcut", () => {
    render(<FunctionRunner fn={sendFn} />);
    openRunner();

    const submit = screen.getByTestId("function-runner-submit");
    expect(submit.textContent).toContain("Run function");
    expect(submit.querySelectorAll("kbd").length).toBeGreaterThanOrEqual(2);
  });

  it("does not offer a tenant chooser; the active tenant is the target", () => {
    render(<FunctionRunner fn={sendFn} />);
    openRunner();

    expect(screen.queryByTestId("function-runner-tenant")).toBeNull();
    expect(screen.getByTestId("function-runner-target").textContent).toContain(
      "demo",
    );
  });

  it("falls back to JSON mode when the function has no validator", () => {
    render(
      <FunctionRunner
        fn={{ _id: "functions:list", path: "agent:list", kind: "query" }}
      />,
    );
    openRunner();

    expect(screen.getByTestId("function-runner-args-input")).toBeTruthy();
    expect(screen.queryByTestId("function-runner-mode")).toBeNull();
  });

  it("carries the form values into JSON mode and back", () => {
    render(<FunctionRunner fn={sendFn} />);
    openRunner();

    fireEvent.change(screen.getByTestId("function-runner-field-text"), {
      target: { value: "hello" },
    });
    fireEvent.click(screen.getByTestId("function-runner-mode-json"));
    const textarea = screen.getByTestId(
      "function-runner-args-input",
    ) as HTMLTextAreaElement;
    expect(JSON.parse(textarea.value)).toEqual({
      conversationId: "",
      text: "hello",
    });

    fireEvent.change(textarea, {
      target: { value: '{"conversationId":"c9","text":"bye"}' },
    });
    fireEvent.click(screen.getByTestId("function-runner-mode-form"));
    expect(
      (
        screen.getByTestId(
          "function-runner-field-conversationId",
        ) as HTMLInputElement
      ).value,
    ).toBe("c9");
  });

  it("shows the error envelope with its remediation and request id", async () => {
    fetchMock.mockResolvedValue({
      ok: false,
      status: 400,
      headers: new Headers(),
      json: async () => ({
        error: {
          code: "function.args",
          message: "Message text must not be empty",
          requestId: "req-9",
          remediation: { message: "Send a non-empty text." },
        },
      }),
    } as unknown as Response);
    render(<FunctionRunner fn={sendFn} />);
    openRunner();
    fireEvent.click(screen.getByTestId("function-runner-submit"));

    const error = await screen.findByTestId("function-runner-result-error");
    expect(error.textContent).toContain("Message text must not be empty");
    expect(error.textContent).toContain("Send a non-empty text.");
    expect(
      screen.getByTestId("function-runner-result-error-correlation")
        .textContent,
    ).toContain("req-9");
  });

  it("renders a thrown-error card with the function path, message, stack, and a runs link", async () => {
    fetchMock.mockResolvedValue({
      ok: false,
      status: 422,
      headers: new Headers(),
      json: async () => ({
        error: {
          code: "function.thrown",
          message: "Message text must not be empty (at agent:41)",
          requestId: "req-12",
          severity: "error",
          retryable: false,
          detail: {
            functionPath: "agent:send",
            stack:
              "Error: Message text must not be empty\n    at anonymous (<anonymous>:5:11)",
          },
          remediation: {
            action: "fix_function",
            message:
              "Read the message and the stack, then fix the function or the input it received.",
          },
        },
      }),
    } as unknown as Response);
    const onOpenRuns = vi.fn();
    render(<FunctionRunner fn={sendFn} onOpenRuns={onOpenRuns} />);
    openRunner();
    fireEvent.click(screen.getByTestId("function-runner-submit"));

    const card = await screen.findByTestId("function-runner-result-error");
    expect(card).toHaveAttribute("data-error-class", "function");
    expect(card.textContent).toContain("threw");
    expect(
      screen.getByTestId("function-runner-result-error-function").textContent,
    ).toContain("agent:send");
    expect(
      screen.getByTestId("function-runner-result-error-message").textContent,
    ).toBe("Message text must not be empty (at agent:41)");
    expect(
      screen.getByTestId("function-runner-result-error-stack").textContent,
    ).toContain("at anonymous (<anonymous>:5:11)");
    // The card itself is the remediation; the server's generic copy stays off.
    expect(card.textContent).not.toContain("fix the function or the input");
    expect(
      screen.getByTestId("function-runner-result-error-correlation")
        .textContent,
    ).toContain("req-12");

    fireEvent.click(screen.getByTestId("function-runner-result-error-runs"));
    expect(onOpenRuns).toHaveBeenCalledTimes(1);
  });

  it("keeps the operator copy and no stack for a service fault", async () => {
    fetchMock.mockResolvedValue({
      ok: false,
      status: 500,
      headers: new Headers(),
      json: async () => ({
        error: {
          code: "service.internal",
          message: "An internal server error occurred.",
          requestId: "req-500",
          severity: "fatal",
          retryable: false,
          remediation: {
            action: "contact_operator",
            message:
              "Correlate the request id with the server diagnostics, then contact the operator.",
          },
        },
      }),
    } as unknown as Response);
    render(<FunctionRunner fn={sendFn} onOpenRuns={vi.fn()} />);
    openRunner();
    fireEvent.click(screen.getByTestId("function-runner-submit"));

    const card = await screen.findByTestId("function-runner-result-error");
    expect(card).toHaveAttribute("data-error-class", "service");
    expect(card.textContent).toContain("service.internal");
    expect(card.textContent).toContain("contact the operator");
    expect(
      screen.queryByTestId("function-runner-result-error-stack"),
    ).toBeNull();
    expect(
      screen.queryByTestId("function-runner-result-error-runs"),
    ).toBeNull();
  });
});
