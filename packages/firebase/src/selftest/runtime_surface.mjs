import { assert, path, pathToFileURL, require } from "./support.mjs";
import { testConverterSurface, testCrudTransportSurface, testEqualityHelpers, testAuthRefreshAndErrorMapping, testFieldValueSentinelWriteSurface, testQueryConstraintSurface, testQueryExecutionSurface, testTransactionSurface } from "./rest_surface.mjs";
import { testGrpcWebFieldValueSentinelSurface, testGrpcWebTransactionSurface, testGrpcWebUnaryTransportSurface, testProtobufFoundation } from "./grpc_surface.mjs";
import { testListenWatchSurface } from "./watch_surface.mjs";

export async function testRuntimeSurface(bundleDir) {
  const appModule = await import(pathToFileURL(path.join(bundleDir, "app.mjs")).href);
  const firestoreModule = await import(pathToFileURL(path.join(bundleDir, "firestore.mjs")).href);
  const indexModule = await import(pathToFileURL(path.join(bundleDir, "index.mjs")).href);
  const protobufModule = await import(
    pathToFileURL(path.join(bundleDir, "internal-protobuf.mjs")).href,
  );

  await testAppLifecycle(appModule);
  await testFirestoreLifecycle(firestoreModule, appModule);
  await testCrudTransportSurface(firestoreModule, appModule);
  await testTransactionSurface(firestoreModule, appModule);
  await testFieldValueSentinelWriteSurface(firestoreModule, appModule);
  await testAuthRefreshAndErrorMapping(firestoreModule, appModule);
  await testQueryConstraintSurface(firestoreModule, appModule);
  await testQueryExecutionSurface(firestoreModule, appModule);
  await testEqualityHelpers(firestoreModule, appModule);
  await testConverterSurface(firestoreModule, appModule);
  await testProtobufFoundation(protobufModule);
  await testGrpcWebUnaryTransportSurface(firestoreModule, appModule, protobufModule);
  await testGrpcWebTransactionSurface(firestoreModule, appModule, protobufModule);
  await testGrpcWebFieldValueSentinelSurface(firestoreModule, appModule, protobufModule);
  await testListenWatchSurface(firestoreModule, appModule, protobufModule);
  await testRootReexports(indexModule);
  testCommonJsSurface(bundleDir);
}


async function testAppLifecycle(appModule) {
  const app = appModule.initializeApp({ projectId: "demo-project", apiKey: "demo-key" });
  assert.equal(app.name, "[DEFAULT]");
  assert.equal(app.options.projectId, "demo-project");
  assert.equal(appModule.getApps().length, 1);
  assert.equal(appModule.getApp().name, "[DEFAULT]");

  const named = appModule.initializeApp({ projectId: "named-project" }, "staging");
  assert.equal(named.name, "staging");
  assert.equal(appModule.getApp("staging").options.projectId, "named-project");

  await appModule.deleteApp(named);
  assert.throws(() => appModule.getApp("staging"), /has not been initialized/);
}

async function testFirestoreLifecycle(firestoreModule, appModule) {
  const app = appModule.getApp();
  const firestore = firestoreModule.getFirestore(app);
  assert.equal(firestore.databaseId, "(default)");
  assert.equal(firestore.settings.host, "firestore.googleapis.com");
  assert.equal(firestore.settings.ssl, true);

  const cities = firestoreModule.collection(firestore, "cities");
  assert.equal(cities.path, "cities");
  assert.equal(cities.parent, null);

  const sanFrancisco = firestoreModule.doc(cities, "SF");
  assert.equal(sanFrancisco.path, "cities/SF");
  assert.equal(sanFrancisco.parent.path, "cities");

  const landmarks = firestoreModule.collection(sanFrancisco, "landmarks");
  assert.equal(landmarks.path, "cities/SF/landmarks");
  assert.equal(landmarks.parent?.path, "cities/SF");

  const parks = firestoreModule.collection(firestore, "cities/SF/parks");
  assert.equal(parks.path, "cities/SF/parks");

  const collectionGroup = firestoreModule.collectionGroup(firestore, "landmarks");
  assert.equal(collectionGroup.id, "landmarks");

  const analytics = firestoreModule.initializeFirestore(
    app,
    { ignoreUndefinedProperties: true },
    "analytics",
  );
  assert.equal(analytics.databaseId, "analytics");
  assert.equal(analytics.settings.ignoreUndefinedProperties, true);

  firestoreModule.connectFirestoreEmulator(firestore, "127.0.0.1", 8080, {
    mockUserToken: { sub: "user-1" },
  });
  assert.equal(firestore.settings.host, "127.0.0.1:8080");
  assert.equal(firestore.settings.ssl, false);
  assert.equal(firestore.settings.useFetchStreams, false);

  assert.throws(
    () => firestoreModule.collection(firestore, "cities/SF"),
    /odd number of path segments/,
  );
  assert.throws(
    () => firestoreModule.doc(firestore, "cities"),
    /even number of path segments/,
  );
  assert.throws(
    () => firestoreModule.collectionGroup(firestore, "cities/landmarks"),
    /single collection segment/,
  );

  await firestoreModule.terminate(analytics);
  assert.equal(firestoreModule.getFirestore(app, "analytics").databaseId, "analytics");
}

async function testRootReexports(indexModule) {
  const clientApp = indexModule.initializeApp({ projectId: "root-project" }, "root");
  const firestore = indexModule.getFirestore(clientApp);
  assert.equal(firestore.app.name, "root");
  assert.equal(typeof indexModule.connectFirestoreEmulator, "function");
  assert.equal(indexModule.collection(firestore, "cities").path, "cities");
  assert.equal(typeof indexModule.documentId, "function");
  assert.equal(typeof indexModule.getDoc, "function");
  assert.equal(typeof indexModule.getDocs, "function");
  assert.equal(typeof indexModule.onSnapshot, "function");
  assert.equal(typeof indexModule.refEqual, "function");
  assert.equal(typeof indexModule.queryEqual, "function");
  assert.equal(typeof indexModule.snapshotEqual, "function");
  assert.equal(typeof indexModule.setDoc, "function");
  assert.equal(typeof indexModule.updateDoc, "function");
  assert.equal(typeof indexModule.deleteDoc, "function");
  assert.equal(typeof indexModule.addDoc, "function");
  assert.equal(typeof indexModule.arrayRemove, "function");
  assert.equal(typeof indexModule.arrayUnion, "function");
  assert.equal(typeof indexModule.deleteField, "function");
  assert.equal(typeof indexModule.runTransaction, "function");
  assert.equal(typeof indexModule.serverTimestamp, "function");
  assert.equal(typeof indexModule.writeBatch, "function");
  assert.equal(typeof indexModule.increment, "function");
  assert.equal(typeof indexModule.query, "function");
  assert.equal(typeof indexModule.where, "function");
  assert.equal(typeof indexModule.orderBy, "function");
  assert.equal(typeof indexModule.limit, "function");
  assert.equal(typeof indexModule.startAt, "function");
  assert.equal(typeof indexModule.startAfter, "function");
  assert.equal(typeof indexModule.endAt, "function");
  assert.equal(typeof indexModule.endBefore, "function");
  await indexModule.deleteApp(clientApp);
}

function testCommonJsSurface(bundleDir) {
  const appModule = require(path.join(bundleDir, "app.cjs"));
  const firestoreModule = require(path.join(bundleDir, "firestore.cjs"));
  const indexModule = require(path.join(bundleDir, "index.cjs"));
  assert.equal(typeof appModule.initializeApp, "function");
  assert.equal(typeof firestoreModule.getFirestore, "function");
  assert.equal(typeof firestoreModule.getDoc, "function");
  assert.equal(typeof firestoreModule.getDocs, "function");
  assert.equal(typeof firestoreModule.refEqual, "function");
  assert.equal(typeof firestoreModule.queryEqual, "function");
  assert.equal(typeof firestoreModule.snapshotEqual, "function");
  assert.equal(typeof firestoreModule.setDoc, "function");
  assert.equal(typeof firestoreModule.updateDoc, "function");
  assert.equal(typeof firestoreModule.deleteDoc, "function");
  assert.equal(typeof firestoreModule.addDoc, "function");
  assert.equal(typeof firestoreModule.arrayRemove, "function");
  assert.equal(typeof firestoreModule.arrayUnion, "function");
  assert.equal(typeof firestoreModule.deleteField, "function");
  assert.equal(typeof firestoreModule.runTransaction, "function");
  assert.equal(typeof firestoreModule.increment, "function");
  assert.equal(typeof firestoreModule.serverTimestamp, "function");
  assert.equal(typeof firestoreModule.writeBatch, "function");
  assert.equal(typeof firestoreModule.query, "function");
  assert.equal(typeof firestoreModule.where, "function");
  assert.equal(typeof indexModule.initializeApp, "function");
}

