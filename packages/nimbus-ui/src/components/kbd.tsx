import { cn } from "@/lib/utils";

type KbdProps = React.HTMLAttributes<HTMLElement> & {
  children: React.ReactNode;
};

export function Kbd({ className, children, ...rest }: KbdProps) {
  return (
    <kbd
      {...rest}
      className={cn(
        "inline-flex h-[18px] min-w-[18px] items-center justify-center rounded-xs border px-1 text-xs leading-none border-border-2 bg-bg-raised text-text-3 font-mono",
        className,
      )}
    >
      {children}
    </kbd>
  );
}
