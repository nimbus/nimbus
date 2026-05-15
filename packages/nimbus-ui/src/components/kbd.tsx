import { cn } from "../lib/cn";

type KbdProps = React.HTMLAttributes<HTMLElement> & {
  children: React.ReactNode;
};

export function Kbd({ className, children, ...rest }: KbdProps) {
  return (
    <kbd
      {...rest}
      className={cn(
        "inline-flex h-[18px] min-w-[18px] items-center justify-center rounded border px-1 text-[11px] leading-none border-app bg-surface-2 text-muted font-mono",
        className,
      )}
    >
      {children}
    </kbd>
  );
}
