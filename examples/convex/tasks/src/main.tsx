import React from "react";
import ReactDOM from "react-dom/client";
import { ConvexProvider, ConvexReactClient } from "convex/react";

import App from "./App";

const nativeUrl = import.meta.env.VITE_NIMBUS_NATIVE_URL ?? "http://localhost:8080";
const deploymentUrl =
  import.meta.env.VITE_NIMBUS_URL ?? "http://localhost:8080/convex/demo";

const client = new ConvexReactClient(deploymentUrl, {
  skipConvexDeploymentUrlCheck: true,
});

async function ensureDemoTenant() {
  const response = await fetch(`${nativeUrl}/api/tenants`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ id: "demo" }),
  });
  if (!response.ok && response.status !== 409) {
    throw new Error(`Failed to provision the demo tenant (${response.status})`);
  }
}

function render() {
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <ConvexProvider client={client}>
        <App />
      </ConvexProvider>
    </React.StrictMode>,
  );
}

void ensureDemoTenant().then(render).catch((error: Error) => {
  document.getElementById("root")!.textContent = error.message;
});
