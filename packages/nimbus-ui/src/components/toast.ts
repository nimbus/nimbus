import type { ReactNode } from "react";

import { toast as stack } from "@/components/ui/toast";

/** How long a toast that only confirms an action the operator took stays up. */
export const TRANSIENT_TOAST_MS = 4000;

/**
 * How many toasts stay on screen at once. DESIGN.md: "Never stack more than
 * three; collapse the rest into '+N more.'"
 */
export const VISIBLE_TOAST_LIMIT = 3;

export type ToastOptions = {
  description?: ReactNode;
  /**
   * Milliseconds before the toast closes itself; `0` keeps it up until the
   * operator closes it. Left unset, a confirmation takes the Toaster's
   * transient lifetime and an error stays.
   */
  timeout?: number;
  /**
   * The one follow-up the toast offers (`Update`, `Undo`). Taking it closes
   * the toast.
   */
  action?: { label: string; onClick: () => void };
  /**
   * Runs when the toast closes without its action: the close button, a
   * swipe, or the clock.
   */
  onDismiss?: () => void;
};

type ToastType = "success" | "error" | undefined;

/**
 * The console's toast policy over the registry stack.
 *
 * DESIGN.md: "Errors show until dismissed; never auto-disappear." The stack
 * runs one clock per toast, so the split lives here: an error gets no clock
 * and a high announcement priority, everything else takes the Toaster's
 * transient lifetime, and a caller that named its own lifetime has already
 * answered.
 */
function show(
  type: ToastType,
  title: ReactNode,
  options: ToastOptions = {},
): string {
  const { action, onDismiss } = options;
  let acted = false;
  const id = stack.add({
    type,
    title,
    description: options.description,
    timeout: options.timeout ?? (type === "error" ? 0 : undefined),
    priority: type === "error" ? "high" : "low",
    actionProps: action
      ? {
          children: action.label,
          onClick: () => {
            acted = true;
            action.onClick();
            stack.close(id);
          },
        }
      : undefined,
    onClose: onDismiss
      ? () => {
          if (!acted) onDismiss();
        }
      : undefined,
  });
  return id;
}

export const toast = {
  /** A neutral note with no icon: a copy confirmation, a request accepted. */
  message: (title: ReactNode, options?: ToastOptions): string =>
    show(undefined, title, options),
  /** A mutation that landed. Expires on its own. */
  success: (title: ReactNode, options?: ToastOptions): string =>
    show("success", title, options),
  /** A refusal or failure. Stays until the operator closes it. */
  error: (title: ReactNode, options?: ToastOptions): string =>
    show("error", title, options),
  /** Closes one toast, or every toast when no id is given. */
  close: (id?: string): void => stack.close(id),
};
