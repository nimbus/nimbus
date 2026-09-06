import React from "react";
import ReactDOM from "react-dom/client";
import { ConvexProvider, ConvexReactClient } from "convex/react";

import App from "./App";

const browserServerUrl = new URL(
  new URLSearchParams(window.location.search).get("server") ?? window.location.origin,
);
const deploymentUrl = import.meta.env.VITE_NIMBUS_URL
  ?? new URL("/convex/demo", browserServerUrl).toString().replace(/\/$/, "");

const client = new ConvexReactClient(deploymentUrl, {
  skipConvexDeploymentUrlCheck: true,
});

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ConvexProvider client={client}>
      <App />
    </ConvexProvider>
  </React.StrictMode>,
);
