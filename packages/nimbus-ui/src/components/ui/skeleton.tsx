import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "cn";
import type * as React from "react";

// `shape` follows what the bar stands in for: `bar` is a line of text, `tile`
// is an icon or avatar tile, `circle` is a dot.
const skeletonVariants = cva(
  "animate-shimmer bg-bg-raised bg-linear-90 from-transparent from-35% via-bg-hover via-50% to-transparent to-65% bg-size-[200%_100%] motion-reduce:animate-none",
  {
    variants: {
      shape: {
        bar: "rounded-sm",
        tile: "rounded-md",
        circle: "rounded-full",
      },
    },
    defaultVariants: { shape: "bar" },
  },
);

// Skeleton takes the geometry of what it stands in for, sits on the raised
// ground, and carries a faint 1.8 s shimmer that reduced-motion turns off.
// A bare Skeleton is decoration and says nothing to a screen reader; the
// LoadingStatus wrapper in components/skeleton.tsx gives a group of them
// its one announcement.
function Skeleton({
  className,
  shape = "bar",
  ...props
}: React.ComponentProps<"div"> & VariantProps<typeof skeletonVariants>) {
  return (
    <div
      aria-hidden="true"
      data-slot="skeleton"
      data-shape={shape}
      className={cn(skeletonVariants({ shape, className }))}
      {...props}
    />
  );
}

export { Skeleton };
