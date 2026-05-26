import { assert, encodeEnvelope, trailerFlag, trailerSerialize } from "./support.mjs";

function concatBinaryChunks(chunks) {
  const totalLength = chunks.reduce((sum, chunk) => sum + chunk.byteLength, 0);
  const combined = new Uint8Array(totalLength);
  let offset = 0;
  for (const chunk of chunks) {
    combined.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return combined;
}

function binaryBodyToUint8Array(body) {
  if (body instanceof Uint8Array) {
    return body;
  }
  if (body instanceof ArrayBuffer) {
    return new Uint8Array(body);
  }
  if (ArrayBuffer.isView(body)) {
    return new Uint8Array(body.buffer, body.byteOffset, body.byteLength);
  }
  throw new Error(`Unsupported binary body type: ${Object.prototype.toString.call(body)}.`);
}

function decodeGrpcWebUnaryRequest(body) {
  const bytes = binaryBodyToUint8Array(body);
  assert.ok(bytes.byteLength >= 5, "gRPC-Web unary requests must include one envelope.");
  assert.equal(bytes[0], 0);
  const messageLength =
    (bytes[1] << 24) | (bytes[2] << 16) | (bytes[3] << 8) | bytes[4];
  return bytes.slice(5, 5 + messageLength);
}

function createGrpcWebResponse(messageBytes, trailerHeaders = new Headers({ "grpc-status": "0" })) {
  const body = concatBinaryChunks([
    ...messageBytes.map((message) => encodeEnvelope(0, message)),
    encodeEnvelope(trailerFlag, trailerSerialize(trailerHeaders)),
  ]);
  return new Response(body, {
    status: 200,
    headers: {
      "content-type": "application/grpc-web+proto",
    },
  });
}


export async function testProtobufFoundation(protobufModule) {
  const {
    create,
    fromBinary,
    toBinary,
    firestoreDocumentV1,
    firestoreV1,
  } = protobufModule;

  const commitRequest = create(firestoreV1.CommitRequestSchema, {
    database: "projects/demo-project/databases/(default)",
    writes: [
      {
        operation: {
          case: "update",
          value: {
            name: "projects/demo-project/databases/(default)/documents/cities/SF",
            fields: {
              name: {
                valueType: {
                  case: "stringValue",
                  value: "San Francisco",
                },
              },
              population: {
                valueType: {
                  case: "integerValue",
                  value: 883305n,
                },
              },
            },
          },
        },
      },
    ],
  });
  const commitBytes = toBinary(firestoreV1.CommitRequestSchema, commitRequest);
  const commitRoundTrip = fromBinary(firestoreV1.CommitRequestSchema, commitBytes);
  assert.equal(commitRoundTrip.database, "projects/demo-project/databases/(default)");
  assert.equal(commitRoundTrip.writes.length, 1);
  assert.equal(commitRoundTrip.writes[0]?.operation.case, "update");
  assert.equal(
    commitRoundTrip.writes[0]?.operation.value.fields.name.valueType.case,
    "stringValue",
  );
  assert.equal(
    commitRoundTrip.writes[0]?.operation.value.fields.name.valueType.value,
    "San Francisco",
  );
  assert.equal(
    commitRoundTrip.writes[0]?.operation.value.fields.population.valueType.case,
    "integerValue",
  );
  assert.equal(
    commitRoundTrip.writes[0]?.operation.value.fields.population.valueType.value,
    883305n,
  );

  const listenRequest = create(firestoreV1.ListenRequestSchema, {
    database: "projects/demo-project/databases/(default)",
    labels: {
      "goog-listen-tags": "browser-selftest",
    },
    targetChange: {
      case: "addTarget",
      value: {
        targetId: 7,
        once: true,
        targetType: {
          case: "documents",
          value: {
            documents: [
              "projects/demo-project/databases/(default)/documents/cities/SF",
              "projects/demo-project/databases/(default)/documents/cities/NYC",
            ],
          },
        },
      },
    },
  });
  const listenBytes = toBinary(firestoreV1.ListenRequestSchema, listenRequest);
  const listenRoundTrip = fromBinary(firestoreV1.ListenRequestSchema, listenBytes);
  assert.equal(listenRoundTrip.database, "projects/demo-project/databases/(default)");
  assert.equal(listenRoundTrip.targetChange.case, "addTarget");
  assert.equal(listenRoundTrip.targetChange.value.targetId, 7);
  assert.equal(listenRoundTrip.targetChange.value.once, true);
  assert.equal(listenRoundTrip.targetChange.value.targetType.case, "documents");
  assert.deepEqual(listenRoundTrip.targetChange.value.targetType.value.documents, [
    "projects/demo-project/databases/(default)/documents/cities/SF",
    "projects/demo-project/databases/(default)/documents/cities/NYC",
  ]);
  assert.equal(listenRoundTrip.labels["goog-listen-tags"], "browser-selftest");

  const document = create(firestoreDocumentV1.DocumentSchema, {
    name: "projects/demo-project/databases/(default)/documents/cities/SF",
    fields: {
      nickname: {
        valueType: {
          case: "stringValue",
          value: "Golden Gate",
        },
      },
    },
  });
  const documentBytes = toBinary(firestoreDocumentV1.DocumentSchema, document);
  const documentRoundTrip = fromBinary(firestoreDocumentV1.DocumentSchema, documentBytes);
  assert.equal(documentRoundTrip.fields.nickname.valueType.case, "stringValue");
  assert.equal(documentRoundTrip.fields.nickname.valueType.value, "Golden Gate");
}

export async function testGrpcWebUnaryTransportSurface(
  firestoreModule,
  appModule,
  protobufModule,
) {
  const {
    create,
    fromBinary,
    fromJson,
    toBinary,
    toJson,
    firestoreV1,
  } = protobufModule;

  const app = appModule.initializeApp(
    {
      apiKey: "grpc-api-key",
      appId: "grpc-app-id",
      projectId: "grpc-project",
    },
    "grpc-web-runtime",
  );

  const requests = [];
  const tokenCalls = [];
  let commitAttempts = 0;
  const fetch = async (url, options) => {
    const headers = new Headers(options?.headers ?? {});
    requests.push({
      body: options?.body ? decodeGrpcWebUnaryRequest(options.body) : null,
      headers,
      method: options?.method ?? "GET",
      url: String(url),
    });

    if (String(url).endsWith("/Commit")) {
      const request = fromBinary(
        firestoreV1.CommitRequestSchema,
        requests.at(-1).body,
      );
      const requestJson = toJson(firestoreV1.CommitRequestSchema, request);
      assert.equal(requestJson.database, "projects/grpc-project/databases/grpc");
      assert.equal(requestJson.writes.length, 1);
      if (commitAttempts === 0) {
        commitAttempts += 1;
        return new Response(null, {
          status: 401,
          headers: {
            "content-type": "application/grpc-web+proto",
          },
        });
      }
      return createGrpcWebResponse([
        toBinary(
          firestoreV1.CommitResponseSchema,
          create(firestoreV1.CommitResponseSchema, {
            commitTime: {
              nanos: 123456000,
              seconds: 1_777_068_800n,
            },
            writeResults: [],
          }),
        ),
      ]);
    }

    if (String(url).endsWith("/BatchGetDocuments")) {
      const request = fromBinary(
        firestoreV1.BatchGetDocumentsRequestSchema,
        requests.at(-1).body,
      );
      const requestJson = toJson(firestoreV1.BatchGetDocumentsRequestSchema, request);
      assert.equal(requestJson.database, "projects/grpc-project/databases/grpc");
      assert.deepEqual(requestJson.documents, [
        "projects/grpc-project/databases/grpc/documents/cities/SF",
      ]);
      return createGrpcWebResponse([
        toBinary(
          firestoreV1.BatchGetDocumentsResponseSchema,
          fromJson(firestoreV1.BatchGetDocumentsResponseSchema, {
            found: {
              fields: {
                name: { stringValue: "San Francisco" },
                population: { integerValue: "883305" },
              },
              name: "projects/grpc-project/databases/grpc/documents/cities/SF",
            },
          }),
        ),
      ]);
    }

    if (String(url).endsWith("/RunQuery")) {
      const request = fromBinary(firestoreV1.RunQueryRequestSchema, requests.at(-1).body);
      const requestJson = toJson(firestoreV1.RunQueryRequestSchema, request);
      assert.equal(requestJson.parent, "projects/grpc-project/databases/grpc/documents");
      assert.equal(requestJson.structuredQuery.from[0]?.collectionId, "cities");
      return createGrpcWebResponse([
        toBinary(
          firestoreV1.RunQueryResponseSchema,
          fromJson(firestoreV1.RunQueryResponseSchema, {
            document: {
              fields: {
                name: { stringValue: "San Francisco" },
              },
              name: "projects/grpc-project/databases/grpc/documents/cities/SF",
            },
          }),
        ),
        toBinary(
          firestoreV1.RunQueryResponseSchema,
          fromJson(firestoreV1.RunQueryResponseSchema, {
            done: true,
          }),
        ),
      ]);
    }

    throw new Error(`Unexpected gRPC-Web request to ${url}`);
  };

  const firestore = firestoreModule.initializeFirestore(
    app,
    {
      experimentalAuthToken: async ({ forceRefresh }) => {
        tokenCalls.push(forceRefresh);
        return forceRefresh ? "fresh-token" : "stale-token";
      },
      experimentalFetch: fetch,
      experimentalUnaryTransport: "grpc-web",
      host: "grpc-web.test",
      ssl: false,
    },
    "grpc",
  );
  const city = firestoreModule.doc(firestoreModule.collection(firestore, "cities"), "SF");

  await firestoreModule.setDoc(city, {
    name: "San Francisco",
    population: 883305,
  });
  const snapshot = await firestoreModule.getDoc(city);
  assert.deepEqual(snapshot.data(), {
    name: "San Francisco",
    population: 883305,
  });
  const querySnapshot = await firestoreModule.getDocs(
    firestoreModule.query(
      firestoreModule.collection(firestore, "cities"),
      firestoreModule.orderBy("name"),
    ),
  );
  assert.equal(querySnapshot.size, 1);
  assert.equal(querySnapshot.docs[0].data().name, "San Francisco");

  assert.deepEqual(tokenCalls, [false, true, false, false]);
  assert.equal(requests[0].url, "http://grpc-web.test/google.firestore.v1.Firestore/Commit");
  assert.equal(requests[0].headers.get("authorization"), "Bearer stale-token");
  assert.equal(requests[1].headers.get("authorization"), "Bearer fresh-token");
  assert.equal(requests[1].headers.get("x-goog-api-key"), "grpc-api-key");
  assert.equal(requests[1].headers.get("x-firebase-gmpid"), "grpc-app-id");
  assert.equal(requests[1].headers.get("x-grpc-web"), "1");
  assert.match(
    requests[1].headers.get("content-type") ?? "",
    /^application\/grpc-web\+proto/i,
  );
  assert.equal(
    requests[2].url,
    "http://grpc-web.test/google.firestore.v1.Firestore/BatchGetDocuments",
  );
  assert.equal(
    requests[3].url,
    "http://grpc-web.test/google.firestore.v1.Firestore/RunQuery",
  );

  const errorFirestore = firestoreModule.initializeFirestore(
    app,
    {
      experimentalFetch: async () =>
        createGrpcWebResponse([], new Headers({
          "grpc-message": "permission denied",
          "grpc-status": "7",
        })),
      experimentalUnaryTransport: "grpc-web",
      host: "grpc-web.test",
      ssl: false,
    },
    "grpc-error",
  );

  await assert.rejects(
    () =>
      firestoreModule.setDoc(
        firestoreModule.doc(
          firestoreModule.collection(errorFirestore, "cities"),
          "DENIED",
        ),
        { name: "Denied" },
      ),
    (error) =>
      error instanceof firestoreModule.FirestoreError &&
      error.code === "PERMISSION_DENIED" &&
      error.status === 403,
  );

  await appModule.deleteApp(app);
}

export async function testGrpcWebTransactionSurface(
  firestoreModule,
  appModule,
  protobufModule,
) {
  const {
    create,
    fromBinary,
    fromJson,
    toBinary,
    toJson,
    firestoreV1,
  } = protobufModule;

  const app = appModule.initializeApp({ projectId: "grpc-txn-project" }, "grpc-web-transaction");
  const requests = [];
  const firstTransaction = Uint8Array.from([1, 2, 3]);
  const secondTransaction = Uint8Array.from([4, 5, 6]);
  let beginCalls = 0;
  let commitCalls = 0;
  let batchGetCalls = 0;
  let runQueryCalls = 0;
  let rollbackCalls = 0;
  const fetch = async (url, options) => {
    const headers = new Headers(options?.headers ?? {});
    requests.push({
      body: options?.body ? decodeGrpcWebUnaryRequest(options.body) : null,
      headers,
      method: options?.method ?? "GET",
      url: String(url),
    });
    const request = requests.at(-1);
    assert.ok(request, "gRPC-Web transaction request should be recorded");

    if (String(url).endsWith("/BeginTransaction")) {
      const transaction = beginCalls === 0 ? firstTransaction : secondTransaction;
      beginCalls += 1;
      return createGrpcWebResponse([
        toBinary(
          firestoreV1.BeginTransactionResponseSchema,
          create(firestoreV1.BeginTransactionResponseSchema, {
            transaction,
          }),
        ),
      ]);
    }

    if (String(url).endsWith("/BatchGetDocuments")) {
      const requestMessage = fromBinary(
        firestoreV1.BatchGetDocumentsRequestSchema,
        request.body,
      );
      const requestJson = toJson(firestoreV1.BatchGetDocumentsRequestSchema, requestMessage);
      const expectedTransaction = batchGetCalls === 0
        ? Buffer.from(firstTransaction).toString("base64")
        : Buffer.from(secondTransaction).toString("base64");
      batchGetCalls += 1;
      assert.equal(requestJson.transaction, expectedTransaction);
      return createGrpcWebResponse([
        toBinary(
          firestoreV1.BatchGetDocumentsResponseSchema,
          fromJson(firestoreV1.BatchGetDocumentsResponseSchema, {
            found: {
              fields: {
                visits: { integerValue: "7" },
              },
              name: "projects/grpc-txn-project/databases/grpc-txn/documents/cities/SF",
            },
          }),
        ),
      ]);
    }

    if (String(url).endsWith("/RunQuery")) {
      const requestMessage = fromBinary(firestoreV1.RunQueryRequestSchema, request.body);
      const requestJson = toJson(firestoreV1.RunQueryRequestSchema, requestMessage);
      const expectedTransaction = runQueryCalls === 0
        ? Buffer.from(firstTransaction).toString("base64")
        : Buffer.from(secondTransaction).toString("base64");
      runQueryCalls += 1;
      assert.equal(requestJson.transaction, expectedTransaction);
      return createGrpcWebResponse([
        toBinary(
          firestoreV1.RunQueryResponseSchema,
          fromJson(firestoreV1.RunQueryResponseSchema, {
            document: {
              fields: {
                visits: { integerValue: "7" },
              },
              name: "projects/grpc-txn-project/databases/grpc-txn/documents/cities/SF",
            },
          }),
        ),
      ]);
    }

    if (String(url).endsWith("/Commit")) {
      const requestMessage = fromBinary(firestoreV1.CommitRequestSchema, request.body);
      const requestJson = toJson(firestoreV1.CommitRequestSchema, requestMessage);
      const expectedTransaction = commitCalls === 0
        ? Buffer.from(firstTransaction).toString("base64")
        : Buffer.from(secondTransaction).toString("base64");
      commitCalls += 1;
      assert.equal(requestJson.transaction, expectedTransaction);
      if (commitCalls === 1) {
        return createGrpcWebResponse([], new Headers({
          "grpc-message": "transaction conflict",
          "grpc-status": "10",
        }));
      }
      return createGrpcWebResponse([
        toBinary(
          firestoreV1.CommitResponseSchema,
          create(firestoreV1.CommitResponseSchema, {
            commitTime: {
              nanos: 0,
              seconds: 1_777_068_801n,
            },
            writeResults: [],
          }),
        ),
      ]);
    }

    if (String(url).endsWith("/Rollback")) {
      const requestMessage = fromBinary(firestoreV1.RollbackRequestSchema, request.body);
      const requestJson = toJson(firestoreV1.RollbackRequestSchema, requestMessage);
      rollbackCalls += 1;
      assert.equal(
        requestJson.transaction,
        Buffer.from(secondTransaction).toString("base64"),
      );
      return createGrpcWebResponse([new Uint8Array()]);
    }

    throw new Error(`Unexpected gRPC-Web transaction request to ${url}`);
  };

  const firestore = firestoreModule.initializeFirestore(
    app,
    {
      experimentalFetch: fetch,
      experimentalUnaryTransport: "grpc-web",
      host: "grpc-web.test",
      ssl: false,
    },
    "grpc-txn",
  );
  const city = firestoreModule.doc(firestoreModule.collection(firestore, "cities"), "SF");
  const citiesQuery = firestoreModule.query(
    firestoreModule.collection(firestore, "cities"),
    firestoreModule.where("visits", ">=", 1),
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

  const readOnlyResult = await firestoreModule.runTransaction(firestore, async (transaction) => {
    const snapshot = await transaction.get(citiesQuery);
    return snapshot.docs[0]?.data()?.visits;
  });
  assert.equal(readOnlyResult, 7);
  assert.equal(rollbackCalls, 1);
  assert.equal(
    requests.at(-1)?.url,
    "http://grpc-web.test/google.firestore.v1.Firestore/Rollback",
  );

  await appModule.deleteApp(app);
}

export async function testGrpcWebFieldValueSentinelSurface(
  firestoreModule,
  appModule,
  protobufModule,
) {
  const {
    create,
    fromBinary,
    fromJson,
    toBinary,
    toJson,
    firestoreV1,
  } = protobufModule;

  const app = appModule.initializeApp({ projectId: "grpc-field-value-project" }, "grpc-field-values");
  const requests = [];
  const transactionBytes = Uint8Array.from([9, 8, 7]);
  let beginCalls = 0;
  let commitCalls = 0;
  const fetch = async (url, options) => {
    const headers = new Headers(options?.headers ?? {});
    const body = options?.body ? decodeGrpcWebUnaryRequest(options.body) : null;
    requests.push({
      body,
      headers,
      method: options?.method ?? "GET",
      url: String(url),
    });

    if (String(url).endsWith("/BeginTransaction")) {
      beginCalls += 1;
      return createGrpcWebResponse([
        toBinary(
          firestoreV1.BeginTransactionResponseSchema,
          create(firestoreV1.BeginTransactionResponseSchema, {
            transaction: transactionBytes,
          }),
        ),
      ]);
    }

    if (String(url).endsWith("/Commit")) {
      const requestMessage = fromBinary(firestoreV1.CommitRequestSchema, body);
      const requestJson = toJson(firestoreV1.CommitRequestSchema, requestMessage);
      let responseJson;

      if (commitCalls === 0) {
        assert.deepEqual(requestJson.writes, [
          {
            update: {
              fields: {
                name: { stringValue: "San Francisco" },
              },
              name: "projects/grpc-field-value-project/databases/(default)/documents/cities/SF",
            },
            updateMask: {
              fieldPaths: ["name", "clearedAt"],
            },
            updateTransforms: [
              {
                fieldPath: "updatedAt",
                setToServerValue: "REQUEST_TIME",
              },
            ],
          },
        ]);
        responseJson = {
          commitTime: "2026-04-25T00:00:06Z",
          writeResults: [
            {
              transformResults: [
                {
                  timestampValue: "2026-04-25T00:00:06Z",
                },
              ],
            },
          ],
        };
      } else if (commitCalls === 1) {
        assert.deepEqual(requestJson.writes, [
          {
            update: {
              name: "projects/grpc-field-value-project/databases/(default)/documents/cities/SF",
            },
            updateMask: {
              fieldPaths: ["batchDeleted"],
            },
            updateTransforms: [
              {
                fieldPath: "batchStamp",
                setToServerValue: "REQUEST_TIME",
              },
            ],
          },
          {
            currentDocument: {
              exists: true,
            },
            update: {
              name: "projects/grpc-field-value-project/databases/(default)/documents/cities/SF",
            },
            updateMask: {},
            updateTransforms: [
              {
                appendMissingElements: {
                  values: [{ stringValue: "west" }],
                },
                fieldPath: "tags",
              },
            ],
          },
        ]);
        responseJson = {
          commitTime: "2026-04-25T00:00:06Z",
          writeResults: [
            {
              transformResults: [
                {
                  timestampValue: "2026-04-25T00:00:06Z",
                },
              ],
            },
            {
              transformResults: [
                {
                  arrayValue: {
                    values: [{ stringValue: "west" }],
                  },
                },
              ],
            },
          ],
        };
      } else {
        assert.equal(
          requestJson.transaction,
          Buffer.from(transactionBytes).toString("base64"),
        );
        assert.deepEqual(requestJson.writes, [
          {
            update: {
              name: "projects/grpc-field-value-project/databases/(default)/documents/cities/SF",
            },
            updateMask: {
              fieldPaths: ["txnDeleted"],
            },
            updateTransforms: [
              {
                fieldPath: "txnStamp",
                setToServerValue: "REQUEST_TIME",
              },
            ],
          },
          {
            currentDocument: {
              exists: true,
            },
            update: {
              name: "projects/grpc-field-value-project/databases/(default)/documents/cities/SF",
            },
            updateMask: {},
            updateTransforms: [
              {
                fieldPath: "visits",
                increment: { integerValue: "1" },
              },
            ],
          },
        ]);
        responseJson = {
          commitTime: "2026-04-25T00:00:06Z",
          writeResults: [
            {
              transformResults: [
                {
                  timestampValue: "2026-04-25T00:00:06Z",
                },
              ],
            },
            {
              transformResults: [
                {
                  integerValue: "1",
                },
              ],
            },
          ],
        };
      }

      commitCalls += 1;
      return createGrpcWebResponse([
        toBinary(
          firestoreV1.CommitResponseSchema,
          fromJson(firestoreV1.CommitResponseSchema, responseJson),
        ),
      ]);
    }

    throw new Error(`Unexpected gRPC-Web FieldValue request to ${url}`);
  };

  const firestore = firestoreModule.initializeFirestore(
    app,
    {
      experimentalFetch: fetch,
      experimentalUnaryTransport: "grpc-web",
      host: "grpc-web.test",
      ssl: false,
    },
  );
  const city = firestoreModule.doc(firestoreModule.collection(firestore, "cities"), "SF");

  await firestoreModule.setDoc(
    city,
    {
      clearedAt: firestoreModule.deleteField(),
      name: "San Francisco",
      updatedAt: firestoreModule.serverTimestamp(),
    },
    { merge: true },
  );

  const batch = firestoreModule.writeBatch(firestore);
  batch.set(
    city,
    {
      batchDeleted: firestoreModule.deleteField(),
      batchStamp: firestoreModule.serverTimestamp(),
    },
    { merge: true },
  );
  batch.update(city, {
    tags: firestoreModule.arrayUnion("west"),
  });
  await batch.commit();

  const transactionResult = await firestoreModule.runTransaction(
    firestore,
    async (transaction) => {
      transaction.set(
        city,
        {
          txnDeleted: firestoreModule.deleteField(),
          txnStamp: firestoreModule.serverTimestamp(),
        },
        { merge: true },
      );
      transaction.update(city, {
        visits: firestoreModule.increment(1),
      });
      return "grpc-field-values";
    },
  );

  assert.equal(transactionResult, "grpc-field-values");
  assert.equal(beginCalls, 1);
  assert.equal(commitCalls, 3);
  await appModule.deleteApp(app);
}

