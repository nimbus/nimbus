import type { ReactNode, Ref } from "react";

import { cn } from "../lib/cn";

/**
 * A scroll box whose contents are read-only.
 *
 * Scrolling with a keyboard means focusing something inside the box and
 * letting the arrow keys carry the view along with it. A table of links gets
 * that for free. A table of plain rows — 28 scheduled jobs, 200 log lines, a
 * routing table — has nothing to focus, so every row past the fold is
 * unreachable without a mouse, and nothing on screen says so.
 *
 * The fix is to make the box itself the thing you focus: `tabIndex` puts it in
 * the tab order, and a named `<section>` — a region landmark as soon as it
 * carries an accessible name — tells a screen reader what it just landed in
 * rather than announcing an anonymous group. The console-wide
 * `:focus-visible` rule paints the ring.
 *
 * Use this only where the content is read-only. A box that already holds links
 * or buttons is reachable through them, and a tab stop in front of them would
 * be one more press between the user and the row they want.
 *
 * Sizing stays with the caller. `overflow-auto` alone does not scroll — inside
 * a panel that is `min-h-0 flex-1 overflow-hidden` but not itself a flex
 * container, a bare `overflow-auto` div resolves to `height: auto`, grows past
 * the panel, and is clipped with no scrollbar. `h-full overflow-auto` is the
 * shape that holds under either parent; the log list reaches the same place as
 * a flex child with `min-h-0 flex-1`.
 */
export function ScrollRegion({
  label,
  className,
  children,
  ref,
  ...rest
}: {
  /** Names the region for a screen reader: "Scheduled jobs", not "scroll area". */
  label: string;
  className?: string;
  children: ReactNode;
  ref?: Ref<HTMLElement>;
  "data-testid"?: string;
}) {
  return (
    <section
      ref={ref}
      aria-label={label}
      /* The rule reads a tab stop on a non-interactive element as a mistake.
         Here it is the whole point: with no focusable descendant the box
         cannot be reached or scrolled by keyboard at all, and WCAG 2.1.1 is
         satisfied only by giving the box itself the stop. */
      // biome-ignore lint/a11y/noNoninteractiveTabindex: a read-only scroll box has no focusable descendant to reach it through
      tabIndex={0}
      className={cn("overflow-auto", className)}
      {...rest}
    >
      {children}
    </section>
  );
}
