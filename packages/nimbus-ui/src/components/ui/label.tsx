/* biome-ignore-all lint/a11y/noLabelWithoutControl: shadcn registry markup; the call site supplies htmlFor or the nested control */
import { cn } from "cn";
import * as React from "react";

function Label({ className, ...props }: React.ComponentProps<"label">) {
  return (
    <label
      data-slot="label"
      // Nimbus alignment: a label is the sentence-case sans at `text-xs
      // font-medium text-text-3` (DESIGN.md → Typography), one step under
      // the registry's `text-sm` and on the tertiary ink.
      className={cn(
        "flex items-center gap-2 text-xs leading-none font-medium text-text-3 select-none group-data-[disabled=true]:pointer-events-none group-data-[disabled=true]:opacity-50 peer-disabled:cursor-not-allowed peer-disabled:opacity-50",
        className,
      )}
      {...props}
    />
  );
}

export { Label };
