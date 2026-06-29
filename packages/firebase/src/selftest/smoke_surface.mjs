import { assert, path, pathToFileURL } from "./support.mjs";

function assertTimestampDate(value, fieldName) {
  assert.ok(value instanceof Date, `${fieldName} should decode to a Date`);
  assert.ok(!Number.isNaN(value.getTime()), `${fieldName} should be valid`);
}

async function runFieldValueSmokeFlow(
  firestoreModule,
  firestore,
  documentReference,
) {
  await firestoreModule.setDoc(documentReference, {
    batchDelete: "remove-batch",
    count: 1,
    legacy: "old",
    name: "Transform City",
    tags: ["seed"],
    txnDelete: "remove-txn",
  });

  await firestoreModule.setDoc(
    documentReference,
    {
      mergeDelete: firestoreModule.deleteField(),
      mergeStamp: firestoreModule.serverTimestamp(),
    },
    { merge: true },
  );
  let snapshot = await firestoreModule.getDoc(documentReference);
  let data = snapshot.data();
  assert.equal(data.count, 1);
  assert.equal(data.legacy, "old");
  assert.deepEqual(data.tags, ["seed"]);
  assert.equal(data.mergeDelete, undefined);
  assertTimestampDate(data.mergeStamp, "mergeStamp");

  await firestoreModule.updateDoc(documentReference, {
    count: firestoreModule.increment(2),
    legacy: firestoreModule.deleteField(),
    tags: firestoreModule.arrayUnion("north"),
    updatedAt: firestoreModule.serverTimestamp(),
  });
  snapshot = await firestoreModule.getDoc(documentReference);
  data = snapshot.data();
  assert.equal(data.count, 3);
  assert.equal(data.legacy, undefined);
  assert.deepEqual(data.tags, ["seed", "north"]);
  assertTimestampDate(data.updatedAt, "updatedAt");

  const batch = firestoreModule.writeBatch(firestore);
  batch.set(
    documentReference,
    {
      batchDelete: firestoreModule.deleteField(),
      batchStamp: firestoreModule.serverTimestamp(),
    },
    { merge: true },
  );
  batch.update(documentReference, {
    count: firestoreModule.increment(1),
    tags: firestoreModule.arrayRemove("seed"),
  });
  await batch.commit();
  snapshot = await firestoreModule.getDoc(documentReference);
  data = snapshot.data();
  assert.equal(data.count, 4);
  assert.equal(data.batchDelete, undefined);
  assert.deepEqual(data.tags, ["north"]);
  assertTimestampDate(data.batchStamp, "batchStamp");

  const priorCount = await firestoreModule.runTransaction(
    firestore,
    async (transaction) => {
      const current = await transaction.get(documentReference);
      transaction.set(
        documentReference,
        {
          txnDelete: firestoreModule.deleteField(),
          txnStamp: firestoreModule.serverTimestamp(),
        },
        { merge: true },
      );
      transaction.update(documentReference, {
        count: firestoreModule.increment(1),
        tags: firestoreModule.arrayUnion("txn"),
      });
      return Number(current.get("count") ?? 0);
    },
  );
  assert.equal(priorCount, 4);
  snapshot = await firestoreModule.getDoc(documentReference);
  data = snapshot.data();
  assert.equal(data.count, 5);
  assert.equal(data.txnDelete, undefined);
  assert.deepEqual(data.tags, ["north", "txn"]);
  assertTimestampDate(data.txnStamp, "txnStamp");
}

export async function testSmokeSurface(bundleDir, smokeBaseUrl) {
  const appModule = await import(pathToFileURL(path.join(bundleDir, "app.mjs")).href);
  const firestoreModule = await import(pathToFileURL(path.join(bundleDir, "firestore.mjs")).href);
  const baseUrl = new URL(smokeBaseUrl);
  const smokeAuthToken = process.env.NIMBUS_FIREBASE_SMOKE_MOCK_USER_TOKEN;
  assert.ok(baseUrl.hostname, "Smoke base URL must include a hostname.");
  assert.ok(baseUrl.port, "Smoke base URL must include an explicit port.");

  // The #24 verified-project gate refuses any caller without a verified Firebase
  // project, so every smoke flow runs through the dev-mode verification bypass.
  // The main/grpc flows use a non-owner subject so the owner-gated secureSmoke
  // sub-flow still observes the protected document as filtered (not as its owner).
  const smokeMainToken = {
    sub: "smoke-main",
    iss: "https://securetoken.google.com/demo",
  };

  const app = appModule.initializeApp({ projectId: "demo" }, "smoke");
  const firestore = firestoreModule.getFirestore(app);
  firestoreModule.connectFirestoreEmulator(
    firestore,
    baseUrl.hostname,
    Number.parseInt(baseUrl.port, 10),
    { mockUserToken: smokeMainToken },
  );

  const cities = firestoreModule.collection(firestore, "cities.v2");
  const city = firestoreModule.doc(cities, "日本語 __.SF");

  await firestoreModule.setDoc(city, {
    count: 1,
    displayName: "Tokyo",
    nested: {
      active: true,
    },
  });

  const initial = await firestoreModule.getDoc(city);
  assert.equal(initial.exists(), true);
  assert.deepEqual(initial.data(), {
    count: 1,
    displayName: "Tokyo",
    nested: {
      active: true,
    },
  });

  await firestoreModule.updateDoc(city, {
    "nested.active": false,
    count: 2,
  });
  const updated = await firestoreModule.getDoc(city);
  assert.deepEqual(updated.data(), {
    count: 2,
    displayName: "Tokyo",
    nested: {
      active: false,
    },
  });

  const landmarks = firestoreModule.collection(city, "landmarks.__");
  const landmark = await firestoreModule.addDoc(landmarks, {
    category: "tower",
    name: "Skytree",
  });
  assert.match(landmark.id, /^[A-Za-z0-9]{20}$/u);
  const landmarkSnapshot = await firestoreModule.getDoc(landmark);
  assert.equal(landmarkSnapshot.exists(), true);
  assert.deepEqual(landmarkSnapshot.data(), {
    category: "tower",
    name: "Skytree",
  });

  const cityResults = await firestoreModule.getDocs(
    firestoreModule.query(
      cities,
      firestoreModule.orderBy("count"),
      firestoreModule.limit(1),
      firestoreModule.startAt(2),
    ),
  );
  assert.equal(cityResults.empty, false);
  assert.equal(cityResults.size, 1);
  assert.equal(cityResults.metadata.fromCache, false);
  assert.equal(cityResults.metadata.hasPendingWrites, false);
  assert.equal(cityResults.docs[0].ref.path, "cities.v2/日本語 __.SF");
  assert.deepEqual(cityResults.docs[0].data(), {
    count: 2,
    displayName: "Tokyo",
    nested: {
      active: false,
    },
  });

  const landmarkResults = await firestoreModule.getDocs(
    firestoreModule.query(
      firestoreModule.collectionGroup(firestore, "landmarks.__"),
      firestoreModule.orderBy(firestoreModule.documentId()),
      firestoreModule.startAt(
        "projects/demo/databases/(default)/documents/cities.v2/日本語 __.SF/landmarks.__/00000000000000000000",
      ),
    ),
  );
  assert.equal(landmarkResults.size, 1);
  assert.equal(
    landmarkResults.docs[0].ref.path,
    `cities.v2/日本語 __.SF/landmarks.__/${landmark.id}`,
  );
  assert.deepEqual(landmarkResults.docs[0].data(), {
    category: "tower",
    name: "Skytree",
  });

  const emptyNestedResults = await firestoreModule.getDocs(
    firestoreModule.collection(
      firestoreModule.doc(firestoreModule.collection(firestore, "cities"), "missing"),
      "landmarks.__",
    ),
  );
  assert.equal(emptyNestedResults.empty, true);
  assert.equal(emptyNestedResults.size, 0);

  await firestoreModule.deleteDoc(city);
  const deleted = await firestoreModule.getDoc(city);
  assert.equal(deleted.exists(), false);

  const oakland = firestoreModule.doc(cities, "oak");
  const sanJose = firestoreModule.doc(cities, "sj");
  const smokeBatch = firestoreModule.writeBatch(firestore);
  smokeBatch.set(oakland, { name: "Oakland", visits: 1 });
  smokeBatch.set(sanJose, { name: "San Jose", visits: 2 });
  await smokeBatch.commit();
  const oaklandSnapshot = await firestoreModule.getDoc(oakland);
  const sanJoseSnapshot = await firestoreModule.getDoc(sanJose);
  assert.deepEqual(oaklandSnapshot.data(), { name: "Oakland", visits: 1 });
  assert.deepEqual(sanJoseSnapshot.data(), { name: "San Jose", visits: 2 });

  const transactionResult = await firestoreModule.runTransaction(
    firestore,
    async (transaction) => {
      const snapshot = await transaction.get(
        firestoreModule.query(cities, firestoreModule.where("name", "==", "Oakland")),
      );
      transaction.update(oakland, {
        visits: Number(snapshot.docs[0]?.data()?.visits ?? 0) + 1,
      });
      return snapshot.docs[0]?.data()?.name;
    },
    { maxAttempts: 2 },
  );
  assert.equal(transactionResult, "Oakland");
  const oaklandAfterTransaction = await firestoreModule.getDoc(oakland);
  assert.deepEqual(oaklandAfterTransaction.data(), { name: "Oakland", visits: 2 });

  const restTransformCity = firestoreModule.doc(cities, "transform-rest");
  await runFieldValueSmokeFlow(firestoreModule, firestore, restTransformCity);

  await assert.rejects(
    () =>
      firestoreModule.runTransaction(firestore, async (transaction) => {
        const snapshot = await transaction.get(sanJose);
        transaction.update(sanJose, {
          visits: Number(snapshot.data()?.visits ?? 0) + 5,
        });
        throw new Error("smoke rollback");
      }),
    /smoke rollback/u,
  );
  const sanJoseAfterRollback = await firestoreModule.getDoc(sanJose);
  assert.deepEqual(sanJoseAfterRollback.data(), { name: "San Jose", visits: 2 });

  await appModule.deleteApp(app);

  const grpcApp = appModule.initializeApp({ projectId: "demo" }, "smoke-grpc");
  const grpcFirestore = firestoreModule.initializeFirestore(grpcApp, {
    experimentalUnaryTransport: "grpc-web",
    experimentalAuthToken: JSON.stringify(smokeMainToken),
    host: baseUrl.host,
    ssl: false,
  });
  const grpcCities = firestoreModule.collection(grpcFirestore, "cities.v2");
  const grpcTransformCity = firestoreModule.doc(grpcCities, "transform-grpc");
  await runFieldValueSmokeFlow(firestoreModule, grpcFirestore, grpcTransformCity);
  await appModule.deleteApp(grpcApp);

  if (smokeAuthToken) {
    let parsedSmokeAuthToken;
    try {
      parsedSmokeAuthToken = JSON.parse(smokeAuthToken);
    } catch {
      parsedSmokeAuthToken = smokeAuthToken;
    }

    const secureCities = firestoreModule.collection(firestore, "secureSmoke");
    const secureCity = firestoreModule.doc(secureCities, "owned");
    const anonymousSecureSnapshot = await firestoreModule.getDoc(secureCity);
    assert.equal(
      anonymousSecureSnapshot.exists(),
      false,
      "Anonymous smoke client should not see protected documents.",
    );

    const authApp = appModule.initializeApp({ projectId: "demo" }, "smoke-auth");
    const authFirestore = firestoreModule.getFirestore(authApp);
    firestoreModule.connectFirestoreEmulator(
      authFirestore,
      baseUrl.hostname,
      Number.parseInt(baseUrl.port, 10),
      { mockUserToken: parsedSmokeAuthToken },
    );
    const authSecureCity = firestoreModule.doc(
      firestoreModule.collection(authFirestore, "secureSmoke"),
      "owned",
    );
    await firestoreModule.setDoc(authSecureCity, {
      owner: "user-1",
      name: "Authenticated Smoke City",
    });
    const authenticatedSecureSnapshot = await firestoreModule.getDoc(authSecureCity);
    assert.equal(authenticatedSecureSnapshot.exists(), true);
    assert.deepEqual(authenticatedSecureSnapshot.data(), {
      owner: "user-1",
      name: "Authenticated Smoke City",
    });

    const anonymousSecureSnapshotAfterWrite = await firestoreModule.getDoc(secureCity);
    assert.equal(
      anonymousSecureSnapshotAfterWrite.exists(),
      false,
      "Anonymous smoke client should stay filtered after authenticated writes.",
    );

    await appModule.deleteApp(authApp);
  }
}
