import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { JsonEditorForm } from "./json-editor-form";

function mount(onSubmit: (json: string) => Promise<void>, onCancel = vi.fn()) {
  render(
    <JsonEditorForm
      initialJson='{"a":1}'
      label="Document"
      fieldId="doc"
      submitLabel="save"
      submittingLabel="saving…"
      testidPrefix="doc"
      onSubmit={onSubmit}
      onCancel={onCancel}
    />,
  );
  return { onCancel };
}

describe("JsonEditorForm", () => {
  it("labels the textarea and seeds it with the initial JSON", () => {
    mount(vi.fn().mockResolvedValue(undefined));
    const field = screen.getByLabelText("Document");
    expect(field).toBe(screen.getByTestId("doc-textarea"));
    expect(field).toHaveValue('{"a":1}');
  });

  it("submits the edited draft", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    mount(onSubmit);
    await user.clear(screen.getByTestId("doc-textarea"));
    await user.type(screen.getByTestId("doc-textarea"), "{{}");
    await user.click(screen.getByTestId("doc-submit"));
    expect(onSubmit).toHaveBeenCalledWith("{}");
    expect(screen.queryByTestId("doc-error")).toBeNull();
  });

  it("shows the submitting label while the handler is pending", async () => {
    const user = userEvent.setup();
    let release: () => void = () => {};
    const onSubmit = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        }),
    );
    mount(onSubmit);
    await user.click(screen.getByTestId("doc-submit"));
    expect(screen.getByTestId("doc-submit")).toHaveTextContent("saving…");
    expect(screen.getByTestId("doc-submit")).toBeDisabled();
    release();
    await waitFor(() =>
      expect(screen.getByTestId("doc-submit")).toHaveTextContent("save"),
    );
  });

  it("surfaces a thrown error in the error line and stays open", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn().mockRejectedValue(new Error("invalid JSON"));
    mount(onSubmit);
    await user.click(screen.getByTestId("doc-submit"));
    expect(await screen.findByTestId("doc-error")).toHaveTextContent(
      "invalid JSON",
    );
    expect(screen.getByTestId("doc-submit")).not.toBeDisabled();
  });

  it("cancel calls the caller without submitting", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    const { onCancel } = mount(onSubmit);
    await user.click(screen.getByRole("button", { name: "cancel" }));
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onSubmit).not.toHaveBeenCalled();
  });
});
