import { Link } from "@tanstack/react-router";

import { cn } from "@/lib/utils";

export type PageTab<TId extends string = string> = {
  readonly id: TId;
  readonly label: string;
};

// PageTabs switches between the sub-views of one page through its `tab`
// search param, so every sub-view has an address and the back button
// works. It is the switch for a page with a short, fixed list of views. A
// list that the operator can grow, or that needs a search field, is a
// sub-panel instead.
//
// A tab links to "." — the page it is on — and rewrites only `tab` in the
// search, so the page's other search params (filters, tenant scope)
// survive the switch.
export function PageTabs<TId extends string>({
  label,
  tabs,
  active,
  testid,
  itemTestid,
}: {
  label: string;
  tabs: ReadonlyArray<PageTab<TId>>;
  active: TId;
  testid: string;
  itemTestid: string;
}) {
  return (
    <nav
      aria-label={label}
      className="flex shrink-0 gap-px self-start overflow-hidden rounded-md border border-border-2 bg-bg-raised"
      data-testid={testid}
    >
      {tabs.map((tab) => {
        const current = tab.id === active;
        return (
          <Link
            key={tab.id}
            to="."
            // The router types `tab` from the union of every route's search;
            // the caller's ids are that route's `tab` values by contract.
            search={(prev) => ({ ...prev, tab: tab.id as typeof prev.tab })}
            data-testid={`${itemTestid}-${tab.id}`}
            aria-current={current ? "page" : undefined}
            className={cn(
              "px-3 py-1.5 text-xs font-medium",
              current
                ? "bg-bg-panel text-text-1"
                : "text-text-3 hover:bg-bg-panel hover:text-text-1",
            )}
          >
            {tab.label}
          </Link>
        );
      })}
    </nav>
  );
}
