import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { RouterProvider, createRouter } from "@tanstack/react-router";
import { NimbusProvider } from "nimbus/react";

import "./styles/globals.css";
import { routeTree } from "./route-tree.gen";
import { getNimbusClient } from "./lib/nimbus-client";

const router = createRouter({
  routeTree,
  basepath: window.location.pathname.startsWith("/ui") ? "/ui" : undefined,
  defaultPreload: "intent",
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
