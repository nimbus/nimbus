import React from "react";
import ReactDOM from "react-dom/client";
import { ConvexProvider, ConvexReactClient } from "convex/react";

import App from "./App";

const deploymentUrl =
  import.meta.env.VITE_NIMBUS_URL ?? "http://localhost:8080/convex/demo";

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
