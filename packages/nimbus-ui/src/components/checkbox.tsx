import { useEffect, useRef } from "react";

import { cn } from "../lib/cn";

/**
 * The console's checkbox.
 *
 * A native `input` keeps keyboard behaviour, form semantics, and assistive-tech
 * support. `appearance: none` replaces the platform control, whose default
 * rendering is actively misleading on a dark surface: an *unchecked* box paints
 * a light fill and reads as checked. The replacement paints an empty box in the
 * surface colour and fills it with `--brand` only when it is genuinely on.
 *
 * `indeterminate` is a DOM property rather than an attribute, so React will not
 * forward it — it is applied through a ref. Use it for a select-all control
 * when only part of the page is selected; a plain unchecked box there claims
 * nothing is selected, which is false.
 */
export function Checkbox({
  checked,
  indeterminate = false,
  onChange,
  label,
  testid,
  className,
}: {
  checked: boolean;
  indeterminate?: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  testid?: string;
  className?: string;
}) {
  const ref = useRef<HTMLInputElement>(null);
  const showIndeterminate = indeterminate && !checked;

  useEffect(() => {
    if (ref.current) {
      ref.current.indeterminate = showIndeterminate;
    }
  }, [showIndeterminate]);

  return (
    <input
      ref={ref}
      type="checkbox"
      aria-label={label}
      checked={checked}
      onChange={(event) => onChange(event.target.checked)}
      data-testid={testid}
      className={cn(
        "nimbus-checkbox size-3.5 shrink-0 cursor-pointer align-middle",
        className,
      )}
    />
  );
}
