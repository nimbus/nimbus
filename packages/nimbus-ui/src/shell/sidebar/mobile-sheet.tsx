import { Link, useRouterState } from "@tanstack/react-router";
import { Menu } from "lucide-react";
import { useState } from "react";

import { Mascot } from "@/components/mascot";
import { Button } from "@/components/ui/button";
import { Sheet, SheetContent, SheetTrigger } from "@/components/ui/sheet";
import { viewFromPathname } from "../nav-entries";
import { SidebarBody } from "./sidebar";

// MobileTopBar replaces the sidebar below 640px: a 48px bar with the menu
// button and the mascot, and the whole sidebar body in a sheet behind the
// button. The sheet closes on every navigation because the page the user
// asked for is now behind it: a row tap closes it directly, and a change
// of pathname closes it for the view switcher and anything else that
// navigates without being a link.
export function MobileTopBar() {
  const [open, setOpen] = useState(false);
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const view = viewFromPathname(pathname);
  const close = () => setOpen(false);
  // State adjusted during render, so the sheet is already closed in the
  // frame that paints the new page instead of one frame later.
  const [seenPathname, setSeenPathname] = useState(pathname);
  if (seenPathname !== pathname) {
    setSeenPathname(pathname);
    setOpen(false);
  }
  return (
    <header
      data-testid="mobile-top-bar"
      className="flex h-12 shrink-0 items-center gap-1.5 border-b border-border-2 bg-bg-panel px-1"
    >
      <Sheet open={open} onOpenChange={setOpen}>
        <SheetTrigger
          render={
            <Button
              variant="ghost"
              size="icon-lg"
              aria-label="Open navigation"
              data-testid="mobile-menu-button"
              className="size-11 text-text-2"
            />
          }
        >
          <Menu size={20} aria-hidden />
        </SheetTrigger>
        <SheetContent
          side="left"
          aria-label="Navigation"
          data-testid="sidebar-sheet"
          showCloseButton={false}
          className="w-64 gap-0 overflow-y-auto bg-bg-panel p-0 text-text-1"
        >
          <SidebarBody
            collapsed={false}
            inSheet
            onCollapse={close}
            onExpand={() => {}}
            onNavigate={close}
          />
        </SheetContent>
      </Sheet>
      <Link
        to={`/${view}`}
        aria-label="Nimbus home"
        data-testid="mobile-brand"
        className="flex items-center gap-2 rounded-sm px-1 text-text-1 outline-none"
      >
        <Mascot size={26} decorative />
        <span className="text-base font-semibold tracking-tight">nimbus</span>
      </Link>
    </header>
  );
}
