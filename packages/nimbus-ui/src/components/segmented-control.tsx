import { Radio } from "@base-ui/react/radio";
import { RadioGroup } from "@base-ui/react/radio-group";
import type { ComponentType } from "react";

import { cn } from "@/lib/utils";

export type SegmentedControlOption<T extends string> = {
  value: T;
  label: string;
  // description is the longer phrase a screen reader hears after the label.
  description?: string;
  icon?: ComponentType<{
    size?: number;
    className?: string;
    "aria-hidden"?: boolean;
  }>;
};

// SegmentedControl is a radio group drawn as one bar of segments. Base UI
// owns the roving tab stop and the arrow keys; every segment is a real
// radio, so a screen reader hears "Light, radio button, 1 of 3, checked".
export function SegmentedControl<T extends string>({
  label,
  value,
  options,
  onChange,
  testid,
  className,
  segmentClassName,
}: {
  label: string;
  value: T;
  options: ReadonlyArray<SegmentedControlOption<T>>;
  onChange: (value: T) => void;
  testid?: string;
  className?: string;
  segmentClassName?: string;
}) {
  return (
    <RadioGroup
      aria-label={label}
      value={value}
      onValueChange={(next) => onChange(next as T)}
      data-testid={testid}
      className={cn(
        "inline-flex items-center gap-0.5 rounded-md border border-border-2 bg-bg-panel p-0.5 text-xs",
        className,
      )}
    >
      {options.map((option) => {
        const active = option.value === value;
        const Icon = option.icon;
        return (
          <Radio.Root
            key={option.value}
            value={option.value}
            aria-label={option.label}
            aria-description={option.description}
            data-testid={testid ? `${testid}-${option.value}` : undefined}
            data-active={active ? "true" : "false"}
            className={cn(
              "inline-flex h-6 items-center gap-1.5 rounded-sm px-2.5 font-medium text-text-3 transition-colors duration-150 ease-standard outline-none hover:text-text-1 data-checked:bg-bg-raised data-checked:text-text-1",
              segmentClassName,
            )}
          >
            {Icon ? <Icon size={14} aria-hidden className="shrink-0" /> : null}
            <span>{option.label}</span>
          </Radio.Root>
        );
      })}
    </RadioGroup>
  );
}
