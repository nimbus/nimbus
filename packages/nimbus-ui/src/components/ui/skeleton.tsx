import { cn } from "cn";
import type * as React from "react";

// Skeleton takes the geometry of what it stands in for, sits on the raised
// ground, and carries a faint 1.8 s shimmer that reduced-motion turns off.
// A bare Skeleton is decoration and says nothing to a screen reader; the
// LoadingStatus wrapper in components/skeleton.tsx gives a group of them
// its one announcement.
function Skeleton({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      aria-hidden="true"
      data-slot="skeleton"
      className={cn(
        "animate-shimmer rounded-sm bg-bg-raised bg-linear-90 from-transparent from-35% via-bg-hover via-50% to-transparent to-65% bg-size-[200%_100%] motion-reduce:animate-none",
        className,
      )}
      {...props}
    />
  );
}

export { Skeleton };
