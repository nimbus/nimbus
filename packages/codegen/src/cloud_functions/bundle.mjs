import path from "node:path";

import { build } from "esbuild";

import {
  cloudFunctionsAdminAppSource,
  cloudFunctionsAdminFirestoreSource,
  cloudFunctionsEntrySource,
  cloudFunctionsSharedSource,
} from "./runtime_sources.mjs";

async function buildCloudFunctionsRuntimeBundle(project) {
  const runtimeProject = withRelativeEntrypoints(project);
  const result = await build({
    absWorkingDir: runtimeProject.appDir,
    entryPoints: ["__nimbus_cloud_functions_entry__"],
    bundle: true,
    format: "esm",
    platform: "node",
    target: "node20",
    write: false,
    logLevel: "silent",
    plugins: [
      createCloudFunctionsVirtualModulePlugin(runtimeProject),
    ],
  });

  const outputFile = result.outputFiles?.[0];
  if (!outputFile) {
    throw new Error(`failed to bundle Cloud Functions sources for ${runtimeProject.appDir}`);
  }
  return outputFile.text;
}

function withRelativeEntrypoints(project) {
  if (project.kind === "firebase_project") {
    return {
      ...project,
      codebases: project.codebases.map((codebase) => ({
        ...codebase,
        relativeEntrypoint: path.relative(project.appDir, codebase.entrypoint),
      })),
    };
  }

  return {
    ...project,
    relativeEntrypoint: path.relative(project.appDir, project.entrypoint),
  };
}

function createCloudFunctionsVirtualModulePlugin(project) {
  return {
    name: "nimbus-cloud-functions-virtual-modules",
    setup(builder) {
      builder.onResolve({ filter: /^__nimbus_cloud_functions_entry__$/ }, () => ({
        path: "__nimbus_cloud_functions_entry__",
        namespace: "nimbus-cloud-functions",
      }));
      builder.onResolve({ filter: /^__nimbus_cloud_functions_shared__$/ }, () => ({
        path: "__nimbus_cloud_functions_shared__",
        namespace: "nimbus-cloud-functions",
      }));
      builder.onResolve({ filter: /^__nimbus_firebase_functions_v2__$/ }, () => ({
        path: "__nimbus_firebase_functions_v2__",
        namespace: "nimbus-cloud-functions",
      }));
      builder.onResolve({ filter: /^__nimbus_firebase_functions_v2_firestore__$/ }, () => ({
        path: "__nimbus_firebase_functions_v2_firestore__",
        namespace: "nimbus-cloud-functions",
      }));
      builder.onResolve({ filter: /^__nimbus_firebase_functions_v2_https__$/ }, () => ({
        path: "__nimbus_firebase_functions_v2_https__",
        namespace: "nimbus-cloud-functions",
      }));
      builder.onResolve({ filter: /^__nimbus_firebase_admin_app__$/ }, () => ({
        path: "__nimbus_firebase_admin_app__",
        namespace: "nimbus-cloud-functions",
      }));
      builder.onResolve({ filter: /^__nimbus_firebase_admin_firestore__$/ }, () => ({
        path: "__nimbus_firebase_admin_firestore__",
        namespace: "nimbus-cloud-functions",
      }));
      builder.onResolve({ filter: /^__nimbus_functions_framework__$/ }, () => ({
        path: "__nimbus_functions_framework__",
        namespace: "nimbus-cloud-functions",
      }));

      builder.onResolve({ filter: /^firebase-functions\/v2$/ }, () => ({
        path: "__nimbus_firebase_functions_v2__",
        namespace: "nimbus-cloud-functions",
      }));
      builder.onResolve({ filter: /^firebase-functions\/v2\/firestore$/ }, () => ({
        path: "__nimbus_firebase_functions_v2_firestore__",
        namespace: "nimbus-cloud-functions",
      }));
      builder.onResolve({ filter: /^firebase-functions\/v2\/https$/ }, () => ({
        path: "__nimbus_firebase_functions_v2_https__",
        namespace: "nimbus-cloud-functions",
      }));
      builder.onResolve({ filter: /^firebase-admin\/app$/ }, () => ({
        path: "__nimbus_firebase_admin_app__",
        namespace: "nimbus-cloud-functions",
      }));
      builder.onResolve({ filter: /^firebase-admin\/firestore$/ }, () => ({
        path: "__nimbus_firebase_admin_firestore__",
        namespace: "nimbus-cloud-functions",
      }));
      builder.onResolve({ filter: /^@google-cloud\/functions-framework$/ }, () => ({
        path: "__nimbus_functions_framework__",
        namespace: "nimbus-cloud-functions",
      }));

      builder.onLoad(
        { filter: /^__nimbus_cloud_functions_entry__$/, namespace: "nimbus-cloud-functions" },
        () => ({
          contents: cloudFunctionsEntrySource(project),
          loader: "js",
          resolveDir: project.appDir,
        }),
      );
      builder.onLoad(
        { filter: /^__nimbus_cloud_functions_shared__$/, namespace: "nimbus-cloud-functions" },
        () => ({
          contents: cloudFunctionsSharedSource(),
          loader: "js",
        }),
      );
      builder.onLoad(
        { filter: /^__nimbus_firebase_functions_v2__$/, namespace: "nimbus-cloud-functions" },
        () => ({
          contents: `
import { setGlobalOptions, onInit } from "__nimbus_cloud_functions_shared__";

export { onInit, setGlobalOptions };
`,
          loader: "js",
        }),
      );
      builder.onLoad(
        { filter: /^__nimbus_firebase_functions_v2_firestore__$/, namespace: "nimbus-cloud-functions" },
        () => ({
          contents: `
import { defineFirestoreDocumentTarget } from "__nimbus_cloud_functions_shared__";

export const onDocumentCreated = (documentOrOptions, handler) =>
  defineFirestoreDocumentTarget("google.cloud.firestore.document.v1.created", documentOrOptions, handler);
export const onDocumentUpdated = (documentOrOptions, handler) =>
  defineFirestoreDocumentTarget("google.cloud.firestore.document.v1.updated", documentOrOptions, handler);
export const onDocumentDeleted = (documentOrOptions, handler) =>
  defineFirestoreDocumentTarget("google.cloud.firestore.document.v1.deleted", documentOrOptions, handler);
export const onDocumentWritten = (documentOrOptions, handler) =>
  defineFirestoreDocumentTarget("google.cloud.firestore.document.v1.written", documentOrOptions, handler);
`,
          loader: "js",
        }),
      );
      builder.onLoad(
        { filter: /^__nimbus_firebase_functions_v2_https__$/, namespace: "nimbus-cloud-functions" },
        () => ({
          contents: `
import {
  defineFirebaseHttpsCallableTarget,
  defineFirebaseHttpsRequestTarget,
  HttpsError,
} from "__nimbus_cloud_functions_shared__";

export const onRequest = (optionsOrHandler, handler) =>
  defineFirebaseHttpsRequestTarget(optionsOrHandler, handler);
export const onCall = (optionsOrHandler, handler) =>
  defineFirebaseHttpsCallableTarget(optionsOrHandler, handler);
export { HttpsError };
`,
          loader: "js",
        }),
      );
      builder.onLoad(
        { filter: /^__nimbus_firebase_admin_app__$/, namespace: "nimbus-cloud-functions" },
        () => ({
          contents: cloudFunctionsAdminAppSource(),
          loader: "js",
        }),
      );
      builder.onLoad(
        { filter: /^__nimbus_firebase_admin_firestore__$/, namespace: "nimbus-cloud-functions" },
        () => ({
          contents: cloudFunctionsAdminFirestoreSource(),
          loader: "js",
        }),
      );
      builder.onLoad(
        { filter: /^__nimbus_functions_framework__$/, namespace: "nimbus-cloud-functions" },
        () => ({
          contents: `
import { registerFrameworkTarget } from "__nimbus_cloud_functions_shared__";

const functions = {
  cloudEvent(name, handler) {
    return registerFrameworkTarget("cloud_event", name, handler);
  },
  http(name, handler) {
    return registerFrameworkTarget("http", name, handler);
  },
};

export default functions;
`,
          loader: "js",
        }),
      );
    },
  };
}

export { buildCloudFunctionsRuntimeBundle };
