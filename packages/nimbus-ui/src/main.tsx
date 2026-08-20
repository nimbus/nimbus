import { NimbusProvider } from "@nimbus/nimbus/react";
import { createRouter, RouterProvider } from "@tanstack/react-router";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import "./styles/globals.css";
import { NotFound } from "./components/not-found";
import { RouteError } from "./components/route-error";
import { getNimbusClient } from "./lib/nimbus-client";
import { routeTree } from "./route-tree.gen";

const router = createRouter({
  routeTree,
  basepath: window.location.pathname.startsWith("/ui") ? "/ui" : undefined,
  defaultPreload: "intent",
  // Both render inside the root `<Outlet/>`, so a missing route or a crashing
  // view degrades the content pane only: nav, tenant selector and status bar
  // stay mounted and clickable.
  defaultNotFoundComponent: NotFound,
  defaultErrorComponent: RouteError,
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}

const container = document.getElementById("root");
if (!container) {
  throw new Error("nimbus-ui: missing #root element");
}

createRoot(container).render(
  <StrictMode>
    <NimbusProvider client={getNimbusClient()}>
      <RouterProvider router={router} />
    </NimbusProvider>
  </StrictMode>,
);
