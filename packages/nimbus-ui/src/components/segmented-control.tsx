import type { ComponentType, KeyboardEvent, ReactNode } from "react";
import { useCallback, useRef } from "react";

import { cn } from "../lib/cn";

export type SegmentedControlOption<T extends string> = {
  value: T;
  label: string;
  description?: string;
  icon?: ComponentType<{
    size?: number;
    className?: string;
    "aria-hidden"?: boolean;
  }>;
};

export type SegmentedControlProps<T extends string> = {
  label: string;
  value: T;
  options: ReadonlyArray<SegmentedControlOption<T>>;
  onChange: (value: T) => void;
  testid?: string;
  className?: string;
  segmentClassName?: string;
  renderSegment?: (
    option: SegmentedControlOption<T>,
    active: boolean,
  ) => ReactNode;
};

export function SegmentedControl<T extends string>({
  label,
  value,
  options,
  onChange,
  testid,
  className,
  segmentClassName,
  renderSegment,
}: SegmentedControlProps<T>) {
  const refs = useRef(new Map<T, HTMLButtonElement | null>());

  const focusByOffset = useCallback(
    (current: T, offset: number) => {
      if (options.length === 0) return;
      const idx = options.findIndex((o) => o.value === current);
      if (idx < 0) return;
      const nextIdx = (idx + offset + options.length) % options.length;
      const next = options[nextIdx].value;
      refs.current.get(next)?.focus();
    },
    [options],
  );

  const onKeyDown = useCallback(
    (e: KeyboardEvent<HTMLButtonElement>, current: T) => {
      switch (e.key) {
        case "ArrowLeft":
        case "ArrowUp":
          e.preventDefault();
          focusByOffset(current, -1);
          break;
        case "ArrowRight":
        case "ArrowDown":
          e.preventDefault();
          focusByOffset(current, 1);
          break;
        case "Home":
          e.preventDefault();
          if (options.length > 0) {
            refs.current.get(options[0].value)?.focus();
          }
          break;
        case "End":
          e.preventDefault();
          if (options.length > 0) {
            refs.current.get(options[options.length - 1].value)?.focus();
          }
          break;
        case "Enter":
        case " ":
          e.preventDefault();
          onChange(current);
          break;
      }
    },
    [focusByOffset, onChange, options],
  );

  return (
    <div
      role="radiogroup"
      aria-label={label}
      data-testid={testid}
      // No `overflow-hidden` here: the global focus style paints the ring 2px
      // outside each segment's box, and clipping it leaves the roving-tabindex
      // focus invisible while arrowing between segments. The end segments carry
      // the corner treatment themselves instead.
      className={cn(
        "inline-flex rounded-md border border-app text-xs",
        className,
      )}
    >
      {options.map((opt, idx) => {
        const active = opt.value === value;
        const Icon = opt.icon;
        return (
          <button
            key={opt.value}
            ref={(node) => {
              refs.current.set(opt.value, node);
            }}
            type="button"
            role="radio"
            aria-checked={active}
            aria-label={opt.description ?? opt.label}
            tabIndex={active ? 0 : -1}
            onClick={() => onChange(opt.value)}
            onKeyDown={(e) => onKeyDown(e, opt.value)}
            data-testid={testid ? `${testid}-${opt.value}` : undefined}
            data-active={active ? "true" : "false"}
            className={cn(
              // `max-h-full` keeps a segment inside the group now that nothing
              // clips it: a consumer that sets its own segment height (the
              // top-nav switcher asks for h-7 inside an h-7 group) would
              // otherwise paint its fill 1px past the group's bottom border.
              "flex max-h-full items-center gap-1.5 px-3 py-1.5 transition-colors",
              // 5px, not `rounded-l-md` (6px): the child radius has to match the
              // parent's inner radius across its 1px border, or the filled end
              // segments show a hairline corner mismatch.
              idx === 0 && "rounded-l-[5px]",
              idx === options.length - 1 && "rounded-r-[5px]",
              idx > 0 && "border-l border-app",
              active
                ? "bg-surface-2 text-default"
                : "text-muted hover:bg-surface-2 hover:text-default",
              segmentClassName,
            )}
          >
            {renderSegment ? (
              renderSegment(opt, active)
            ) : (
              <>
                {Icon ? (
                  <Icon size={14} aria-hidden className="shrink-0" />
                ) : null}
                <span>{opt.label}</span>
              </>
            )}
          </button>
        );
      })}
    </div>
  );
}
