import { assert } from "./support.mjs";

function createJsonResponse(status, body) {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      "content-type": "application/json",
    },
  });
}

function createJsonLinesResponse(lines) {
  return new Response(lines.map((line) => JSON.stringify(line)).join("\n"), {
    status: 200,
    headers: {
      "content-type": "application/json",
    },
  });
}

async function recordRequest(url, options) {
  return {
    body: options?.body ? JSON.parse(String(options.body)) : undefined,
    headers: new Headers(options?.headers ?? {}),
    method: options?.method ?? "GET",
    url: String(url),
  };
}


export async function testCrudTransportSurface(firestoreModule, appModule) {
  const app = appModule.initializeApp(
    {
      apiKey: "sdk-api-key",
      appId: "sdk-app-id",
      projectId: "sdk-project",
    },
    "crud-runtime",
  );
  const requests = [];
  const queuedResponses = [];
  const fetch = async (url, options) => {
    requests.push(await recordRequest(url, options));
    const nextResponse = queuedResponses.shift();
    assert.ok(nextResponse, `Unexpected Firestore request to ${url}`);
    return nextResponse();
  };

  const firestore = firestoreModule.initializeFirestore(
    app,
    {
      experimentalAuthToken: "unit-token",
      experimentalFetch: fetch,
      experimentalHeaders: {
        "x-sdk-test": "enabled",
      },
      host: "sdk.test",
      ssl: false,
    },
    "sdk",
  );

  const cities = firestoreModule.collection(firestore, "cities.v2");
  const city = firestoreModule.doc(cities, "日本語 __.SF");
  const cityName =
    "projects/sdk-project/databases/sdk/documents/cities.v2/日本語 __.SF";

  queuedResponses.push(() => createJsonResponse(200, { commitTime: "2026-04-25T00:00:00Z" }));
  await firestoreModule.setDoc(city, {
    name: "Tokyo",
    nested: { active: true },
    visits: 3,
  });

  assert.equal(
    requests[0].url,
    "http://sdk.test/v1/projects/sdk-project/databases/sdk/documents:commit",
  );
  assert.equal(requests[0].method, "POST");
  assert.equal(requests[0].headers.get("authorization"), "Bearer unit-token");
  assert.equal(requests[0].headers.get("x-goog-api-key"), "sdk-api-key");
  assert.equal(requests[0].headers.get("x-firebase-gmpid"), "sdk-app-id");
  assert.equal(requests[0].headers.get("x-sdk-test"), "enabled");
  assert.deepEqual(requests[0].body, {
    database: "projects/sdk-project/databases/sdk",
    writes: [
      {
        update: {
          fields: {
            name: { stringValue: "Tokyo" },
            nested: {
              mapValue: {
                fields: {
                  active: { booleanValue: true },
                },
              },
            },
            visits: { integerValue: "3" },
          },
          name: cityName,
        },
      },
    ],
  });

  queuedResponses.push(() =>
    createJsonLinesResponse([
      {
        found: {
          fields: {
            count: { integerValue: "3" },
            nested: {
              mapValue: {
                fields: {
                  active: { booleanValue: true },
                },
              },
            },
            score: { doubleValue: "-0" },
            title: { stringValue: "Tokyo" },
          },
          name: cityName,
        },
      },
    ]),
  );
  const snapshot = await firestoreModule.getDoc(city);
  assert.equal(snapshot.exists(), true);
  assert.deepEqual(snapshot.data(), {
    count: 3,
    nested: { active: true },
    score: -0,
    title: "Tokyo",
  });
  assert.equal(Object.is(snapshot.get("score"), -0), true);
  assert.equal(snapshot.get("nested.active"), true);
  assert.deepEqual(requests[1].body, {
    database: "projects/sdk-project/databases/sdk",
    documents: [cityName],
  });

  queuedResponses.push(() => createJsonResponse(200, { commitTime: "2026-04-25T00:00:01Z" }));
  await firestoreModule.updateDoc(city, {
    "nested.active": false,
    visits: 4,
  });
  assert.deepEqual(requests[2].body, {
    database: "projects/sdk-project/databases/sdk",
    writes: [
      {
        currentDocument: {
          exists: true,
        },
        update: {
          fields: {
            nested: {
              mapValue: {
                fields: {
                  active: { booleanValue: false },
                },
              },
            },
            visits: { integerValue: "4" },
          },
          name: cityName,
        },
        updateMask: {
          fieldPaths: ["nested.active", "visits"],
        },
      },
    ],
  });

  queuedResponses.push(() => createJsonResponse(200, { commitTime: "2026-04-25T00:00:02Z" }));
  await firestoreModule.deleteDoc(city);
  assert.deepEqual(requests[3].body, {
    database: "projects/sdk-project/databases/sdk",
    writes: [
      {
        delete: cityName,
      },
    ],
  });

  const landmarks = firestoreModule.collection(city, "landmarks.__");
  queuedResponses.push(() => createJsonResponse(200, { commitTime: "2026-04-25T00:00:03Z" }));
  const landmark = await firestoreModule.addDoc(landmarks, {
    category: "tower",
    name: "Skytree",
  });
  assert.match(landmark.id, /^[A-Za-z0-9]{20}$/u);
  assert.equal(landmark.parent.path, "cities.v2/日本語 __.SF/landmarks.__");
  assert.deepEqual(requests[4].body, {
    database: "projects/sdk-project/databases/sdk",
    writes: [
      {
        currentDocument: {
          exists: false,
        },
        update: {
          fields: {
            category: { stringValue: "tower" },
            name: { stringValue: "Skytree" },
          },
          name: `projects/sdk-project/databases/sdk/documents/cities.v2/日本語 __.SF/landmarks.__/${landmark.id}`,
        },
      },
    ],
  });

  queuedResponses.push(() => createJsonLinesResponse([{ missing: cityName }]));
  const missingSnapshot = await firestoreModule.getDoc(city);
  assert.equal(missingSnapshot.exists(), false);
  assert.equal(missingSnapshot.data(), undefined);

  queuedResponses.push(() => createJsonResponse(200, { commitTime: "2026-04-25T00:00:04Z" }));
  const oakland = firestoreModule.doc(cities, "OAK");
  const sanJose = firestoreModule.doc(cities, "SJC");
  const batch = firestoreModule.writeBatch(firestore);
  assert.equal(
    batch
      .set(oakland, { name: "Oakland" })
      .set(sanJose, { name: "San Jose" })
      .delete(city),
    batch,
  );
  await batch.commit();
  assert.deepEqual(requests[6].body, {
    database: "projects/sdk-project/databases/sdk",
    writes: [
      {
        update: {
          fields: {
            name: { stringValue: "Oakland" },
          },
          name: "projects/sdk-project/databases/sdk/documents/cities.v2/OAK",
        },
      },
      {
        update: {
          fields: {
            name: { stringValue: "San Jose" },
          },
          name: "projects/sdk-project/databases/sdk/documents/cities.v2/SJC",
        },
      },
      {
        delete: cityName,
      },
    ],
  });
  await assert.rejects(() => batch.commit(), /cannot be used after commit/i);

  await appModule.deleteApp(app);
}

export async function testTransactionSurface(firestoreModule, appModule) {
  const app = appModule.initializeApp({ projectId: "txn-project" }, "transaction-runtime");
  const requests = [];
  let beginCalls = 0;
  let batchGetCalls = 0;
  let runQueryCalls = 0;
  let commitCalls = 0;
  let rollbackCalls = 0;
  const transactionTokens = [
    Buffer.from("txn-rest-1").toString("base64"),
    Buffer.from("txn-rest-2").toString("base64"),
    Buffer.from("txn-rest-3").toString("base64"),
    Buffer.from("txn-rest-4").toString("base64"),
  ];
  const fetch = async (url, options) => {
    requests.push(await recordRequest(url, options));
    const request = requests.at(-1);
    assert.ok(request, "transaction request should be recorded");

    if (String(url).endsWith(":beginTransaction")) {
      const transaction = transactionTokens[beginCalls];
      beginCalls += 1;
      return createJsonResponse(200, { transaction });
    }

    if (String(url).endsWith(":batchGet")) {
      const expectedTransaction = transactionTokens[batchGetCalls + 2];
      batchGetCalls += 1;
      assert.equal(request.body.transaction, expectedTransaction);
      return createJsonLinesResponse([
        {
          found: {
            fields: {
              name: { stringValue: "San Francisco" },
              visits: { integerValue: "1" },
            },
            name: "projects/txn-project/databases/txn/documents/cities/SF",
          },
        },
      ]);
    }

    if (String(url).endsWith(":runQuery")) {
      const expectedTransaction = [
        transactionTokens[0],
        transactionTokens[1],
        transactionTokens[3],
      ][runQueryCalls];
      runQueryCalls += 1;
      assert.equal(request.body.transaction, expectedTransaction);
      return createJsonLinesResponse([
        {
          document: {
            fields: {
              name: { stringValue: "San Francisco" },
              visits: { integerValue: "1" },
            },
            name: "projects/txn-project/databases/txn/documents/cities/SF",
          },
        },
      ]);
    }

    if (String(url).endsWith(":commit")) {
      const expectedTransaction = commitCalls === 0 ? transactionTokens[0] : transactionTokens[1];
      commitCalls += 1;
      assert.equal(request.body.transaction, expectedTransaction);
      if (commitCalls === 1) {
        return createJsonResponse(409, {
          error: {
            message: "transaction conflict",
            status: "ABORTED",
          },
        });
      }
      return createJsonResponse(200, { commitTime: "2026-04-25T00:00:05Z" });
    }

    if (String(url).endsWith(":rollback")) {
      rollbackCalls += 1;
      const expectedTransaction = rollbackCalls === 1 ? transactionTokens[2] : transactionTokens[3];
      assert.equal(request.body.transaction, expectedTransaction);
      return createJsonResponse(200, {});
    }

    throw new Error(`Unexpected Firestore transaction request to ${url}`);
  };

  const firestore = firestoreModule.initializeFirestore(
    app,
    {
      experimentalFetch: fetch,
      host: "txn.test",
      ssl: false,
    },
    "txn",
  );
  const city = firestoreModule.doc(firestoreModule.collection(firestore, "cities"), "SF");
  const citiesQuery = firestoreModule.query(
    firestoreModule.collection(firestore, "cities"),
    firestoreModule.where("name", "==", "San Francisco"),
  );

  let attempts = 0;
  const result = await firestoreModule.runTransaction(
    firestore,
    async (transaction) => {
      attempts += 1;
      const snapshot = await transaction.get(citiesQuery);
      transaction.update(city, {
        visits: Number(snapshot.docs[0]?.data()?.visits ?? 0) + 1,
      });
      return attempts;
    },
    { maxAttempts: 2 },
  );

  assert.equal(result, 2);
  assert.equal(attempts, 2);
  assert.equal(beginCalls, 2);
  assert.equal(commitCalls, 2);
  assert.equal(rollbackCalls, 0);
  assert.equal(
    requests[0].url,
    "http://txn.test/v1/projects/txn-project/databases/txn/documents:beginTransaction",
  );
  assert.equal(
    requests[1].url,
    "http://txn.test/v1/projects/txn-project/databases/txn/documents:runQuery",
  );
  assert.equal(
    requests[2].url,
    "http://txn.test/v1/projects/txn-project/databases/txn/documents:commit",
  );

  await assert.rejects(
    () =>
      firestoreModule.runTransaction(firestore, async (transaction) => {
        const snapshot = await transaction.get(city);
        transaction.set(city, {
          name: snapshot.data()?.name,
          visits: 99,
        });
        throw new Error("abort transaction");
      }),
    /abort transaction/u,
  );
  assert.equal(rollbackCalls, 1);
  assert.equal(
    requests.at(-1)?.url,
    "http://txn.test/v1/projects/txn-project/databases/txn/documents:rollback",
  );

  const readOnlyCount = await firestoreModule.runTransaction(firestore, async (transaction) => {
    const snapshot = await transaction.get(citiesQuery);
    return snapshot.size;
  });
  assert.equal(readOnlyCount, 1);
  assert.equal(rollbackCalls, 2);
  assert.equal(
    requests.at(-1)?.url,
    "http://txn.test/v1/projects/txn-project/databases/txn/documents:rollback",
  );

  await appModule.deleteApp(app);
}

export async function testFieldValueSentinelWriteSurface(firestoreModule, appModule) {
  const app = appModule.initializeApp({ projectId: "field-value-project" }, "field-value-runtime");
  const requests = [];
  const transactionToken = Buffer.from("field-value-txn").toString("base64");
  const fetch = async (url, options) => {
    const request = await recordRequest(url, options);
    requests.push(request);

    if (String(url).endsWith(":beginTransaction")) {
      return createJsonResponse(200, { transaction: transactionToken });
    }

    if (String(url).endsWith(":commit")) {
      const writes = Array.isArray(request.body?.writes) ? request.body.writes : [];
      return createJsonResponse(200, {
        commitTime: "2026-04-25T00:00:06Z",
        writeResults: writes.map((write) => ({
          transformResults: Array.isArray(write.updateTransforms)
            ? write.updateTransforms.map((transform) => {
                if (transform.setToServerValue === "REQUEST_TIME") {
                  return { timestampValue: "2026-04-25T00:00:06Z" };
                }
                if (transform.increment) {
                  return transform.increment;
                }
                if (transform.appendMissingElements) {
                  return {
                    arrayValue: transform.appendMissingElements,
                  };
                }
                if (transform.removeAllFromArray) {
                  return {
                    arrayValue: transform.removeAllFromArray,
                  };
                }
                return { nullValue: null };
              })
            : [],
        })),
      });
    }

    throw new Error(`Unexpected FieldValue request to ${url}`);
  };

  const firestore = firestoreModule.initializeFirestore(
    app,
    {
      experimentalFetch: fetch,
      host: "field-value.test",
      ssl: false,
    },
    "field-values",
  );
  const city = firestoreModule.doc(firestoreModule.collection(firestore, "cities"), "SF");
  const cityName =
    "projects/field-value-project/databases/field-values/documents/cities/SF";

  await firestoreModule.setDoc(
    city,
    {
      name: "San Francisco",
      obsolete: firestoreModule.deleteField(),
      tags: firestoreModule.arrayUnion("west", "coast"),
      updatedAt: firestoreModule.serverTimestamp(),
      visits: firestoreModule.increment(1),
    },
    {
      mergeFields: ["name", "obsolete", "tags", "updatedAt", "visits"],
    },
  );
  assert.deepEqual(requests[0].body, {
    database: "projects/field-value-project/databases/field-values",
    writes: [
      {
        update: {
          fields: {
            name: { stringValue: "San Francisco" },
          },
          name: cityName,
        },
        updateMask: {
          fieldPaths: ["name", "obsolete"],
        },
        updateTransforms: [
          {
            appendMissingElements: {
              values: [{ stringValue: "west" }, { stringValue: "coast" }],
            },
            fieldPath: "tags",
          },
          {
            fieldPath: "updatedAt",
            setToServerValue: "REQUEST_TIME",
          },
          {
            fieldPath: "visits",
            increment: { integerValue: "1" },
          },
        ],
      },
    ],
  });

  await firestoreModule.updateDoc(city, {
    title: "Updated",
    "stats.legacy": firestoreModule.deleteField(),
    "stats.visits": firestoreModule.increment(2),
    tags: firestoreModule.arrayRemove("stale"),
    updatedAt: firestoreModule.serverTimestamp(),
  });
  assert.deepEqual(requests[1].body, {
    database: "projects/field-value-project/databases/field-values",
    writes: [
      {
        currentDocument: {
          exists: true,
        },
        update: {
          fields: {
            title: { stringValue: "Updated" },
          },
          name: cityName,
        },
        updateMask: {
          fieldPaths: ["stats.legacy", "title"],
        },
        updateTransforms: [
          {
            fieldPath: "stats.visits",
            increment: { integerValue: "2" },
          },
          {
            fieldPath: "tags",
            removeAllFromArray: {
              values: [{ stringValue: "stale" }],
            },
          },
          {
            fieldPath: "updatedAt",
            setToServerValue: "REQUEST_TIME",
          },
        ],
      },
    ],
  });

  const batch = firestoreModule.writeBatch(firestore);
  batch.set(
    city,
    {
      archivedAt: firestoreModule.serverTimestamp(),
      obsolete: firestoreModule.deleteField(),
    },
    { merge: true },
  );
  batch.update(city, {
    "stats.tags": firestoreModule.arrayUnion("north"),
  });
  await batch.commit();
  assert.deepEqual(requests[2].body, {
    database: "projects/field-value-project/databases/field-values",
    writes: [
      {
        update: {
          fields: {},
          name: cityName,
        },
        updateMask: {
          fieldPaths: ["obsolete"],
        },
        updateTransforms: [
          {
            fieldPath: "archivedAt",
            setToServerValue: "REQUEST_TIME",
          },
        ],
      },
      {
        currentDocument: {
          exists: true,
        },
        update: {
          fields: {},
          name: cityName,
        },
        updateMask: {
          fieldPaths: [],
        },
        updateTransforms: [
          {
            appendMissingElements: {
              values: [{ stringValue: "north" }],
            },
            fieldPath: "stats.tags",
          },
        ],
      },
    ],
  });

  const transactionResult = await firestoreModule.runTransaction(
    firestore,
    async (transaction) => {
      transaction.set(
        city,
        {
          clearedAt: firestoreModule.deleteField(),
          status: "active",
          updatedAt: firestoreModule.serverTimestamp(),
        },
        { merge: true },
      );
      transaction.update(city, {
        "stats.visits": firestoreModule.increment(3),
      });
      return "ok";
    },
  );
  assert.equal(transactionResult, "ok");
  assert.deepEqual(requests[3].body, {
    database: "projects/field-value-project/databases/field-values",
  });
  assert.deepEqual(requests[4].body, {
    database: "projects/field-value-project/databases/field-values",
    transaction: transactionToken,
    writes: [
      {
        update: {
          fields: {
            status: { stringValue: "active" },
          },
          name: cityName,
        },
        updateMask: {
          fieldPaths: ["status", "clearedAt"],
        },
        updateTransforms: [
          {
            fieldPath: "updatedAt",
            setToServerValue: "REQUEST_TIME",
          },
        ],
      },
      {
        currentDocument: {
          exists: true,
        },
        update: {
          fields: {},
          name: cityName,
        },
        updateMask: {
          fieldPaths: [],
        },
        updateTransforms: [
          {
            fieldPath: "stats.visits",
            increment: { integerValue: "3" },
          },
        ],
      },
    ],
  });

  await assert.rejects(
    () =>
      firestoreModule.setDoc(city, {
        obsolete: firestoreModule.deleteField(),
      }),
    /deleteField\(\) can only be used/u,
  );
  await assert.rejects(
    () =>
      firestoreModule.setDoc(
        city,
        {
          profile: {
            updatedAt: firestoreModule.serverTimestamp(),
          },
        },
        {
          mergeFields: ["profile"],
        },
      ),
    /cannot target a subtree containing FieldValue sentinels/u,
  );
  await assert.rejects(
    () =>
      firestoreModule.updateDoc(city, {
        stats: {
          visits: 1,
        },
        "stats.visits": firestoreModule.increment(1),
      }),
    /cannot apply both a regular value and a transform to "stats\.visits"/u,
  );
  await assert.rejects(
    () =>
      firestoreModule.updateDoc(city, {
        tags: [firestoreModule.serverTimestamp()],
      }),
    /FieldValue sentinels must be used as direct document field values/u,
  );

  await appModule.deleteApp(app);
}

export async function testAuthRefreshAndErrorMapping(firestoreModule, appModule) {
  const app = appModule.initializeApp({ projectId: "auth-project" }, "crud-auth");
  const authCalls = [];
  const requests = [];
  const queuedResponses = [
    createJsonResponse(401, {
      error: {
        message: "expired",
        status: "UNAUTHENTICATED",
      },
    }),
    createJsonResponse(200, { commitTime: "2026-04-25T00:00:04Z" }),
    createJsonResponse(409, {
      error: {
        message: "duplicate write",
        status: "ALREADY_EXISTS",
      },
    }),
  ];
  const fetch = async (url, options) => {
    requests.push(await recordRequest(url, options));
    const nextResponse = queuedResponses.shift();
    assert.ok(nextResponse, `Unexpected Firestore request to ${url}`);
    return nextResponse;
  };

  const firestore = firestoreModule.initializeFirestore(
    app,
    {
      experimentalAuthToken: async ({ forceRefresh }) => {
        authCalls.push(forceRefresh);
        return forceRefresh ? "fresh-token" : "stale-token";
      },
      experimentalFetch: fetch,
      host: "auth.test",
      ssl: false,
    },
    "auth",
  );

  const city = firestoreModule.doc(firestoreModule.collection(firestore, "cities"), "SF");
  await firestoreModule.setDoc(city, { name: "San Francisco" });
  assert.deepEqual(authCalls, [false, true]);
  assert.equal(requests[0].headers.get("authorization"), "Bearer stale-token");
  assert.equal(requests[1].headers.get("authorization"), "Bearer fresh-token");

  await assert.rejects(
    () => firestoreModule.setDoc(city, { name: "Duplicate" }),
    (error) => {
      assert.ok(error instanceof firestoreModule.FirestoreError);
      assert.equal(error.code, "ALREADY_EXISTS");
      assert.equal(error.message, "duplicate write");
      assert.equal(error.status, 409);
      return true;
    },
  );

  await appModule.deleteApp(app);
}

export async function testQueryConstraintSurface(firestoreModule, appModule) {
  const app = appModule.initializeApp({ projectId: "query-project" }, "query-runtime");
  const firestore = firestoreModule.getFirestore(app, "queries");
  const cities = firestoreModule.collection(firestore, "cities");

  const constrainedQuery = firestoreModule.query(
    cities,
    firestoreModule.where("state", "==", "CA"),
    firestoreModule.orderBy("name", "desc"),
    firestoreModule.limit(5),
    firestoreModule.startAt("Los Angeles"),
  );
  assert.deepEqual(constrainedQuery.structuredQuery, {
    from: [{ collectionId: "cities" }],
    limit: 5,
    orderBy: [
      {
        direction: "DESCENDING",
        field: { fieldPath: "name" },
      },
    ],
    startAt: {
      before: true,
      values: ["Los Angeles"],
    },
    where: {
      fieldFilter: {
        field: { fieldPath: "state" },
        op: "EQUAL",
        value: "CA",
      },
    },
  });

  const collectionGroupQuery = firestoreModule.query(
    firestoreModule.collectionGroup(firestore, "landmarks"),
    firestoreModule.where(
      firestoreModule.documentId(),
      "==",
      "projects/demo/databases/(default)/documents/cities/SF/landmarks/coit",
    ),
    firestoreModule.orderBy(firestoreModule.documentId()),
    firestoreModule.endBefore(
      "projects/demo/databases/(default)/documents/cities/SEA/landmarks/needle",
    ),
  );
  assert.deepEqual(collectionGroupQuery.structuredQuery, {
    endAt: {
      before: true,
      values: [
        "projects/demo/databases/(default)/documents/cities/SEA/landmarks/needle",
      ],
    },
    from: [{ allDescendants: true, collectionId: "landmarks" }],
    orderBy: [
      {
        direction: "ASCENDING",
        field: { fieldPath: "__name__" },
      },
    ],
    where: {
      fieldFilter: {
        field: { fieldPath: "__name__" },
        op: "EQUAL",
        value: "projects/demo/databases/(default)/documents/cities/SF/landmarks/coit",
      },
    },
  });

  const chainedQuery = firestoreModule.query(
    constrainedQuery,
    firestoreModule.where("capital", "==", true),
  );
  assert.deepEqual(chainedQuery.structuredQuery.where, {
    compositeFilter: {
      filters: [
        {
          fieldFilter: {
            field: { fieldPath: "state" },
            op: "EQUAL",
            value: "CA",
          },
        },
        {
          fieldFilter: {
            field: { fieldPath: "capital" },
            op: "EQUAL",
            value: true,
          },
        },
      ],
      op: "AND",
    },
  });

  assert.throws(
    () => firestoreModule.where("nested.active", "==", true),
    /nested field paths are not supported/,
  );
  assert.throws(
    () =>
      firestoreModule.query(
        cities,
        firestoreModule.limit(1),
        firestoreModule.limit(2),
      ),
    /at most one limit/,
  );
  assert.throws(
    () => firestoreModule.startAfter(),
    /requires at least one cursor value/,
  );

  await appModule.deleteApp(app);
}

export async function testQueryExecutionSurface(firestoreModule, appModule) {
  const app = appModule.initializeApp({ projectId: "query-results-project" }, "query-results");
  const requests = [];
  const queuedResponses = [];
  const fetch = async (url, options) => {
    requests.push(await recordRequest(url, options));
    const nextResponse = queuedResponses.shift();
    assert.ok(nextResponse, `Unexpected Firestore query request to ${url}`);
    return nextResponse();
  };
  const firestore = firestoreModule.initializeFirestore(
    app,
    {
      experimentalFetch: fetch,
      host: "query.test",
      ssl: false,
    },
    "queries",
  );

  const cities = firestoreModule.collection(firestore, "cities");
  queuedResponses.push(() =>
    createJsonLinesResponse([
      {
        document: {
          fields: {
            name: { stringValue: "Alpha" },
            rank: { integerValue: "1" },
          },
          name: "projects/query-results-project/databases/queries/documents/cities/alpha",
        },
        readTime: "2026-04-25T00:00:00Z",
        skippedResults: 1,
      },
      {
        document: {
          fields: {
            name: { stringValue: "Bravo" },
            rank: { integerValue: "2" },
          },
          name: "projects/query-results-project/databases/queries/documents/cities/bravo",
        },
        readTime: "2026-04-25T00:00:00Z",
      },
    ]),
  );

  const cityResults = await firestoreModule.getDocs(
    firestoreModule.query(
      cities,
      firestoreModule.where("name", ">=", "Alpha"),
      firestoreModule.orderBy("name"),
      firestoreModule.limit(2),
      firestoreModule.startAt("Alpha"),
    ),
  );
  assert.equal(cityResults.empty, false);
  assert.equal(cityResults.size, 2);
  assert.equal(cityResults.metadata.fromCache, false);
  assert.equal(cityResults.metadata.hasPendingWrites, false);
  assert.equal(cityResults.docs[0].ref.path, "cities/alpha");
  assert.deepEqual(cityResults.docs[0].data(), { name: "Alpha", rank: 1 });
  assert.equal(cityResults.docs[1].get("rank"), 2);
  assert.deepEqual(requests[0].body, {
    parent: "projects/query-results-project/databases/queries/documents",
    structuredQuery: {
      from: [{ collectionId: "cities" }],
      limit: 2,
      orderBy: [{ direction: "ASCENDING", field: { fieldPath: "name" } }],
      startAt: {
        before: true,
        values: [{ stringValue: "Alpha" }],
      },
      where: {
        fieldFilter: {
          field: { fieldPath: "name" },
          op: "GREATER_THAN_OR_EQUAL",
          value: { stringValue: "Alpha" },
        },
      },
    },
  });
  assert.equal(
    requests[0].url,
    "http://query.test/v1/projects/query-results-project/databases/queries/documents:runQuery",
  );

  const landmarks = firestoreModule.collectionGroup(firestore, "landmarks");
  queuedResponses.push(() =>
    createJsonLinesResponse([
      {
        document: {
          fields: {
            name: { stringValue: "Coit Tower" },
          },
          name: "projects/query-results-project/databases/queries/documents/cities/SF/landmarks/coit",
        },
        readTime: "2026-04-25T00:00:01Z",
      },
    ]),
  );

  const landmarkResults = await firestoreModule.getDocs(
    firestoreModule.query(
      landmarks,
      firestoreModule.where(
        firestoreModule.documentId(),
        "==",
        "projects/query-results-project/databases/queries/documents/cities/SF/landmarks/coit",
      ),
    ),
  );
  assert.equal(landmarkResults.size, 1);
  assert.equal(landmarkResults.docs[0].ref.path, "cities/SF/landmarks/coit");
  assert.deepEqual(landmarkResults.docs[0].data(), { name: "Coit Tower" });
  assert.deepEqual(requests[1].body, {
    parent: "projects/query-results-project/databases/queries/documents",
    structuredQuery: {
      from: [{ allDescendants: true, collectionId: "landmarks" }],
      where: {
        fieldFilter: {
          field: { fieldPath: "__name__" },
          op: "EQUAL",
          value: {
            referenceValue:
              "projects/query-results-project/databases/queries/documents/cities/SF/landmarks/coit",
          },
        },
      },
    },
  });

  const sfLandmarks = firestoreModule.collection(firestoreModule.doc(cities, "SF"), "landmarks");
  queuedResponses.push(() =>
    createJsonLinesResponse([
      {
        readTime: "2026-04-25T00:00:02Z",
      },
    ]),
  );
  const emptyResults = await firestoreModule.getDocs(sfLandmarks);
  assert.equal(emptyResults.empty, true);
  assert.equal(emptyResults.size, 0);
  assert.deepEqual(emptyResults.docs, []);
  assert.equal(
    requests[2].url,
    "http://query.test/v1/projects/query-results-project/databases/queries/documents/cities/SF:runQuery",
  );
  assert.deepEqual(requests[2].body, {
    parent: "projects/query-results-project/databases/queries/documents/cities/SF",
    structuredQuery: {
      from: [{ collectionId: "landmarks" }],
    },
  });

  const iterated = [];
  cityResults.forEach((snapshot) => {
    iterated.push(snapshot.id);
  });
  assert.deepEqual(iterated, ["alpha", "bravo"]);

  await appModule.deleteApp(app);
}

export async function testEqualityHelpers(firestoreModule, appModule) {
  const app = appModule.initializeApp({ projectId: "eq-project" }, "eq-runtime");
  const firestore = firestoreModule.getFirestore(app, "eq");
  const cities = firestoreModule.collection(firestore, "cities");
  const cityA = firestoreModule.doc(cities, "alpha");
  const cityACopy = firestoreModule.doc(cities, "alpha");
  const cityB = firestoreModule.doc(cities, "bravo");
  const landmarksA = firestoreModule.collectionGroup(firestore, "landmarks");
  const landmarksACopy = firestoreModule.collectionGroup(firestore, "landmarks");
  const landmarksB = firestoreModule.collectionGroup(firestore, "districts");

  assert.equal(firestoreModule.refEqual(cityA, cityACopy), true);
  assert.equal(firestoreModule.refEqual(cityA, cityB), false);
  assert.equal(firestoreModule.refEqual(cities, firestoreModule.collection(firestore, "cities")), true);
  assert.equal(firestoreModule.refEqual(landmarksA, landmarksACopy), true);
  assert.equal(firestoreModule.refEqual(landmarksA, landmarksB), false);

  const queryA = firestoreModule.query(
    cities,
    firestoreModule.where("state", "==", "CA"),
    firestoreModule.orderBy("name"),
  );
  const queryACopy = firestoreModule.query(
    firestoreModule.collection(firestore, "cities"),
    firestoreModule.where("state", "==", "CA"),
    firestoreModule.orderBy("name"),
  );
  const queryB = firestoreModule.query(
    cities,
    firestoreModule.where("state", "==", "NV"),
    firestoreModule.orderBy("name"),
  );
  assert.equal(firestoreModule.queryEqual(queryA, queryACopy), true);
  assert.equal(firestoreModule.queryEqual(queryA, queryB), false);

  const requests = [];
  const queuedResponses = [];
  const fetch = async (url, options) => {
    requests.push(await recordRequest(url, options));
    const nextResponse = queuedResponses.shift();
    assert.ok(nextResponse, `Unexpected Firestore equality request to ${url}`);
    return nextResponse();
  };
  const eqFirestore = firestoreModule.initializeFirestore(
    app,
    {
      experimentalFetch: fetch,
      host: "eq.test",
      ssl: false,
    },
    "eq-data",
  );
  const eqCities = firestoreModule.collection(eqFirestore, "cities");
  const eqCity = firestoreModule.doc(eqCities, "alpha");

  queuedResponses.push(() =>
    createJsonLinesResponse([
      {
        found: {
          fields: {
            name: { stringValue: "Alpha" },
          },
          name: "projects/eq-project/databases/eq-data/documents/cities/alpha",
        },
      },
    ]),
  );
  queuedResponses.push(() =>
    createJsonLinesResponse([
      {
        found: {
          fields: {
            name: { stringValue: "Alpha" },
          },
          name: "projects/eq-project/databases/eq-data/documents/cities/alpha",
        },
      },
    ]),
  );
  queuedResponses.push(() =>
    createJsonLinesResponse([
      {
        found: {
          fields: {
            name: { stringValue: "Bravo" },
          },
          name: "projects/eq-project/databases/eq-data/documents/cities/alpha",
        },
      },
    ]),
  );

  const snapshotA = await firestoreModule.getDoc(eqCity);
  const snapshotACopy = await firestoreModule.getDoc(eqCity);
  const snapshotB = await firestoreModule.getDoc(eqCity);
  assert.equal(firestoreModule.snapshotEqual(snapshotA, snapshotACopy), true);
  assert.equal(firestoreModule.snapshotEqual(snapshotA, snapshotB), false);

  queuedResponses.push(() =>
    createJsonLinesResponse([
      {
        document: {
          fields: {
            name: { stringValue: "Alpha" },
          },
          name: "projects/eq-project/databases/eq-data/documents/cities/alpha",
        },
        readTime: "2026-04-25T00:00:00Z",
      },
    ]),
  );
  queuedResponses.push(() =>
    createJsonLinesResponse([
      {
        document: {
          fields: {
            name: { stringValue: "Alpha" },
          },
          name: "projects/eq-project/databases/eq-data/documents/cities/alpha",
        },
        readTime: "2026-04-25T00:00:00Z",
      },
    ]),
  );
  queuedResponses.push(() =>
    createJsonLinesResponse([
      {
        readTime: "2026-04-25T00:00:00Z",
      },
    ]),
  );

  const resultA = await firestoreModule.getDocs(eqCities);
  const resultACopy = await firestoreModule.getDocs(eqCities);
  const resultB = await firestoreModule.getDocs(eqCities);
  assert.equal(firestoreModule.snapshotEqual(resultA, resultACopy), true);
  assert.equal(firestoreModule.snapshotEqual(resultA, resultB), false);
  assert.ok(requests.length >= 6);

  await appModule.deleteApp(app);
}

export async function testConverterSurface(firestoreModule, appModule) {
  class CityView {
    constructor(name, population, slug) {
      this.name = name;
      this.population = population;
      this.slug = slug;
    }
  }

  const app = appModule.initializeApp({ projectId: "converter-project" }, "converter-runtime");
  const requests = [];
  const queuedResponses = [];
  const fetch = async (url, options) => {
    requests.push(await recordRequest(url, options));
    const nextResponse = queuedResponses.shift();
    assert.ok(nextResponse, `Unexpected Firestore converter request to ${url}`);
    return nextResponse();
  };
  const firestore = firestoreModule.initializeFirestore(
    app,
    {
      experimentalFetch: fetch,
      host: "converter.test",
      ssl: false,
    },
    "typed",
  );

  const cityConverter = {
    toFirestore(city) {
      return {
        name: city.name,
        population: city.population,
        slug: city.slug,
      };
    },
    fromFirestore(snapshot) {
      const data = snapshot.data();
      return new CityView(data.name, data.population, data.slug);
    },
  };

  const cities = firestoreModule.collection(firestore, "cities").withConverter(cityConverter);
  const city = firestoreModule.doc(cities, "alpha");
  const rawCity = city.withConverter(null);
  assert.equal(city.converter, cityConverter);
  assert.equal(rawCity.converter, null);

  queuedResponses.push(() =>
    createJsonLinesResponse([
      {
        found: {
          fields: {
            name: { stringValue: "Alpha" },
            population: { integerValue: "7" },
            slug: { stringValue: "alpha" },
          },
          name: "projects/converter-project/databases/typed/documents/cities/alpha",
        },
      },
    ]),
  );
  queuedResponses.push(() =>
    createJsonLinesResponse([
      {
        found: {
          fields: {
            name: { stringValue: "Alpha" },
            population: { integerValue: "7" },
            slug: { stringValue: "alpha" },
          },
          name: "projects/converter-project/databases/typed/documents/cities/alpha",
        },
      },
    ]),
  );
  queuedResponses.push(() =>
    createJsonLinesResponse([
      {
        document: {
          fields: {
            name: { stringValue: "Alpha" },
            population: { integerValue: "7" },
            slug: { stringValue: "alpha" },
          },
          name: "projects/converter-project/databases/typed/documents/cities/alpha",
        },
        readTime: "2026-04-25T00:00:00Z",
      },
    ]),
  );
  queuedResponses.push(() =>
    createJsonResponse(200, {
      commitTime: "2026-04-25T00:00:01Z",
      writeResults: [],
    }),
  );
  queuedResponses.push(() =>
    createJsonResponse(200, {
      commitTime: "2026-04-25T00:00:02Z",
      writeResults: [],
    }),
  );

  const convertedSnapshot = await firestoreModule.getDoc(city);
  assert.ok(convertedSnapshot.data() instanceof CityView);
  assert.equal(convertedSnapshot.data()?.slug, "alpha");

  const rawSnapshot = await firestoreModule.getDoc(rawCity);
  assert.deepEqual(rawSnapshot.data(), {
    name: "Alpha",
    population: 7,
    slug: "alpha",
  });

  const convertedQuery = firestoreModule.query(
    firestoreModule.collection(firestore, "cities"),
    firestoreModule.where("population", ">=", 1),
  ).withConverter(cityConverter);
  const queryResults = await firestoreModule.getDocs(convertedQuery);
  assert.equal(queryResults.size, 1);
  assert.ok(queryResults.docs[0].data() instanceof CityView);

  await firestoreModule.setDoc(city, new CityView("Bravo", 9, "bravo"));
  assert.deepEqual(requests[3].body.writes[0].update.fields, {
    name: { stringValue: "Bravo" },
    population: { integerValue: "9" },
    slug: { stringValue: "bravo" },
  });

  await firestoreModule.addDoc(cities, new CityView("Charlie", 11, "charlie"));
  assert.deepEqual(requests[4].body.writes[0].update.fields, {
    name: { stringValue: "Charlie" },
    population: { integerValue: "11" },
    slug: { stringValue: "charlie" },
  });

  await appModule.deleteApp(app);
}

export async function testTemporalAndBytesCodecSurface(firestoreModule, appModule) {
  const app = appModule.initializeApp({ projectId: "codec-project" }, "codec-runtime");
  const requests = [];
  const queuedResponses = [];
  const fetch = async (url, options) => {
    requests.push(await recordRequest(url, options));
    const nextResponse = queuedResponses.shift();
    assert.ok(nextResponse, `Unexpected Firestore codec request to ${url}`);
    return nextResponse();
  };
  const firestore = firestoreModule.initializeFirestore(
    app,
    {
      experimentalFetch: fetch,
      host: "codec.test",
      ssl: false,
    },
    "codec",
  );
  const events = firestoreModule.collection(firestore, "events");
  const event = firestoreModule.doc(events, "launch");
  const eventName = "projects/codec-project/databases/codec/documents/events/launch";

  const occurredAt = new Date("2026-04-25T12:34:56.000Z");
  const payload = new Uint8Array([1, 2, 3, 255]);

  // Encode: Date -> timestampValue (RFC 3339), Uint8Array -> bytesValue (base64).
  queuedResponses.push(() => createJsonResponse(200, { commitTime: "2026-04-25T00:00:00Z" }));
  await firestoreModule.setDoc(event, { occurredAt, payload });
  const encodedFields = requests[0].body.writes[0].update.fields;
  assert.deepEqual(encodedFields.occurredAt, {
    timestampValue: "2026-04-25T12:34:56.000Z",
  });
  assert.equal(typeof encodedFields.payload.bytesValue, "string");
  assert.ok(encodedFields.payload.bytesValue.length > 0);

  // Decode: the same wire fields round-trip back to Date / Uint8Array.
  queuedResponses.push(() =>
    createJsonLinesResponse([
      {
        found: {
          fields: {
            occurredAt: { timestampValue: encodedFields.occurredAt.timestampValue },
            payload: { bytesValue: encodedFields.payload.bytesValue },
          },
          name: eventName,
        },
      },
    ]),
  );
  const snapshot = await firestoreModule.getDoc(event);
  const data = snapshot.data();
  assert.ok(data.occurredAt instanceof Date, "timestampValue decodes to a Date");
  assert.equal(data.occurredAt.getTime(), occurredAt.getTime());
  assert.ok(data.payload instanceof Uint8Array, "bytesValue decodes to a Uint8Array");
  assert.deepEqual(Array.from(data.payload), Array.from(payload));

  // Unsupported value kinds reject on decode rather than leaking the raw wire
  // shape, keeping the supported value set symmetric with the encoder.
  queuedResponses.push(() =>
    createJsonLinesResponse([
      {
        found: {
          fields: {
            location: { geoPointValue: { latitude: 1, longitude: 2 } },
          },
          name: eventName,
        },
      },
    ]),
  );
  await assert.rejects(
    () => firestoreModule.getDoc(event),
    /geo point values are not supported/,
  );

  queuedResponses.push(() =>
    createJsonLinesResponse([
      {
        found: {
          fields: {
            parent: {
              referenceValue: "projects/codec-project/databases/codec/documents/cities/SF",
            },
          },
          name: eventName,
        },
      },
    ]),
  );
  await assert.rejects(
    () => firestoreModule.getDoc(event),
    /reference values are not supported/,
  );

  // Encoding an invalid Date must fail loudly rather than emit a bad timestamp.
  await assert.rejects(
    () => firestoreModule.setDoc(event, { occurredAt: new Date(Number.NaN) }),
    /timestamp values must be valid Date instances/,
  );

  await appModule.deleteApp(app);
}

