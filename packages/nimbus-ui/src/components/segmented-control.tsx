import type { ComponentType, KeyboardEvent, ReactNode } from "react";
import { useCallback, useRef } from "react";

import { cn } from "../lib/cn";

export type SegmentedControlOption<T extends string> = {
  value: T;
  label: string;
  description?: string;
  icon?: ComponentType<{ size?: number; className?: string; "aria-hidden"?: boolean }>;
};

export type SegmentedControlProps<T extends string> = {
  label: string;
  value: T;
  options: ReadonlyArray<SegmentedControlOption<T>>;
  onChange: (value: T) => void;
  testid?: string;
  className?: string;
  segmentClassName?: string;
  renderSegment?: (option: SegmentedControlOption<T>, active: boolean) => ReactNode;
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
      className={cn(
        "inline-flex overflow-hidden rounded-md border border-app text-xs",
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
              "flex items-center gap-1.5 px-3 py-1.5 transition-colors",
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
                {Icon ? <Icon size={14} aria-hidden className="shrink-0" /> : null}
                <span>{opt.label}</span>
              </>
            )}
          </button>
        );
      })}
    </div>
  );
}
