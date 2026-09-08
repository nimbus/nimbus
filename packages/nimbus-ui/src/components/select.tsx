import { useMemo } from "react";

import {
  SelectContent,
  SelectItem,
  Select as SelectRoot,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

export type SelectOption<T extends string> = {
  value: T;
  label: string;
};

export type SelectProps<T extends string> = {
  label: string;
  value: T;
  options: ReadonlyArray<SelectOption<T>>;
  onChange: (value: T) => void;
  placeholder?: string;
  testid?: string;
};

// Select is a labelled single choice for a filter bar: the label at the
// label size, the current value in mono because it is data, and one option
// per row. Base UI owns the listbox, typeahead, and keyboard paths.
export function Select<T extends string>({
  label,
  value,
  options,
  onChange,
  placeholder,
  testid,
}: SelectProps<T>) {
  const items = useMemo(
    () =>
      options.map((option) => ({ value: option.value, label: option.label })),
    [options],
  );
  return (
    <span className="inline-flex items-center gap-1.5 text-xs font-medium text-text-3">
      <span>{label}</span>
      <SelectRoot
        value={value}
        items={items}
        onValueChange={(next) => {
          if (next !== null && next !== undefined) onChange(next as T);
        }}
      >
        <SelectTrigger
          size="sm"
          aria-label={label}
          data-testid={testid}
          className="h-[26px] rounded-xs border-border-2 bg-bg-panel px-2 font-mono text-xs text-text-1"
        >
          <SelectValue placeholder={placeholder ?? "Select…"} />
        </SelectTrigger>
        <SelectContent align="start" alignItemWithTrigger={false}>
          {options.map((option) => (
            <SelectItem
              key={option.value}
              value={option.value}
              data-testid={
                testid ? `${testid}-option-${option.value}` : undefined
              }
              className="font-mono text-xs"
            >
              {option.label}
            </SelectItem>
          ))}
        </SelectContent>
      </SelectRoot>
    </span>
  );
}
