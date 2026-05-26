import { assert } from "./support.mjs";

class FakeWebSocket {
  constructor(url, protocols = []) {
    this.url = url;
    this.protocols = Array.isArray(protocols)
      ? [...protocols]
      : protocols
        ? [protocols]
        : [];
    this.binaryType = "blob";
    this.closed = false;
    this.closeCalls = [];
    this.listeners = new Map();
    this.sentFrames = [];
  }

  addEventListener(type, listener) {
    const listeners = this.listeners.get(type) ?? [];
    listeners.push(listener);
    this.listeners.set(type, listeners);
  }

  send(data) {
    this.sentFrames.push(normalizeWebSocketBinaryFrame(data));
  }

  close(code, reason) {
    this.closed = true;
    this.closeCalls.push({ code: code ?? null, reason: reason ?? null });
  }

  emitOpen() {
    this.#emit("open", { type: "open" });
  }

  emitBinary(data) {
    this.#emit("message", { data });
  }

  emitClose(code = 1000, reason = "") {
    this.closed = true;
    this.#emit("close", { code, reason });
  }

  emitError(error = new Error("socket error")) {
    this.#emit("error", error);
  }

  #emit(type, event) {
    for (const listener of this.listeners.get(type) ?? []) {
      listener(event);
    }
  }
}

function normalizeWebSocketBinaryFrame(data) {
  if (data instanceof Uint8Array) {
    return data;
  }
  if (ArrayBuffer.isView(data)) {
    return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  }
  if (data instanceof ArrayBuffer) {
    return new Uint8Array(data);
  }
  throw new Error(`Expected binary WebSocket frame, received ${typeof data}.`);
}

function decodeListenAuthSubprotocol(protocols) {
  const offered = protocols.find((protocol) =>
    protocol.startsWith("nimbus.firebase.auth."),
  );
  if (!offered) {
    return null;
  }
  const encoded = offered.slice("nimbus.firebase.auth.".length);
  return new TextDecoder().decode(Buffer.from(encoded, "base64url"));
}

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, reject, resolve };
}

async function flushMicrotasks() {
  await Promise.resolve();
  await Promise.resolve();
}

async function withImmediateTimeouts(run) {
  const originalSetTimeout = globalThis.setTimeout;
  const originalClearTimeout = globalThis.clearTimeout;
  const cancelled = new Set();
  const scheduledDelays = [];
  let nextId = 1;

  globalThis.setTimeout = ((handler, delay = 0, ...args) => {
    const id = nextId;
    nextId += 1;
    scheduledDelays.push(Number(delay));
    queueMicrotask(() => {
      if (cancelled.has(id)) {
        return;
      }
      if (typeof handler === "function") {
        handler(...args);
        return;
      }
      throw new Error("String-based timeouts are not supported in the Firebase selftest.");
    });
    return id;
  });
  globalThis.clearTimeout = ((id) => {
    cancelled.add(id);
  });

  try {
    return await run(scheduledDelays);
  } finally {
    globalThis.setTimeout = originalSetTimeout;
    globalThis.clearTimeout = originalClearTimeout;
  }
}


export async function testListenWatchSurface(firestoreModule, appModule, protobufModule) {
  await withImmediateTimeouts(async (scheduledRetryDelays) => {
    const { create, fromBinary, toBinary, firestoreV1 } = protobufModule;

    const app = appModule.initializeApp(
      {
        apiKey: "listen-api-key",
        appId: "listen-app-id",
        projectId: "listen-project",
      },
      "listen-runtime",
    );

    const sockets = [];
    const firestore = firestoreModule.initializeFirestore(
      app,
      {
        experimentalWebSocketFactory: (url, protocols) => {
          const socket = new FakeWebSocket(url, protocols);
          sockets.push(socket);
          return socket;
        },
        host: "listen.test",
        ssl: false,
      },
      "listen",
    );

    const cities = firestoreModule.collection(firestore, "cities");
    const city = firestoreModule.doc(cities, "SF");

  const documentSnapshotResult = deferred();
  const documentErrors = [];
  const unsubscribeDocument = firestoreModule.onSnapshot(
    city,
    (snapshot) => documentSnapshotResult.resolve(snapshot),
    (error) => documentErrors.push(error),
  );
  await flushMicrotasks();
  assert.equal(sockets.length, 1);
  const documentSocket = sockets[0];
  assert.equal(
    documentSocket.url,
    "ws://listen.test/google.firestore.v1.Firestore/Listen",
  );
  assert.deepEqual(documentSocket.protocols, ["nimbus.firebase.listen.v1"]);

  documentSocket.emitOpen();
  await flushMicrotasks();
  assert.equal(documentSocket.sentFrames.length, 1);
  const addDocumentRequest = fromBinary(
    firestoreV1.ListenRequestSchema,
    documentSocket.sentFrames[0],
  );
  assert.equal(
    addDocumentRequest.database,
    "projects/listen-project/databases/listen",
  );
  assert.equal(addDocumentRequest.targetChange.case, "addTarget");
  assert.equal(addDocumentRequest.targetChange.value.targetId, 1);
  assert.equal(addDocumentRequest.targetChange.value.targetType.case, "documents");
  assert.deepEqual(addDocumentRequest.targetChange.value.targetType.value.documents, [
    "projects/listen-project/databases/listen/documents/cities/SF",
  ]);

  documentSocket.emitBinary(
    toBinary(
      firestoreV1.ListenResponseSchema,
      create(firestoreV1.ListenResponseSchema, {
        responseType: {
          case: "targetChange",
          value: {
            targetChangeType: firestoreV1.TargetChange_TargetChangeType.ADD,
            targetIds: [1],
          },
        },
      }),
    ),
  );
  documentSocket.emitBinary(
    toBinary(
      firestoreV1.ListenResponseSchema,
      create(firestoreV1.ListenResponseSchema, {
        responseType: {
          case: "documentChange",
          value: {
            document: {
              name: "projects/listen-project/databases/listen/documents/cities/SF",
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
            targetIds: [1],
          },
        },
      }),
    ),
  );
  documentSocket.emitBinary(
    toBinary(
      firestoreV1.ListenResponseSchema,
      create(firestoreV1.ListenResponseSchema, {
        responseType: {
          case: "targetChange",
          value: {
            targetChangeType: firestoreV1.TargetChange_TargetChangeType.CURRENT,
            targetIds: [1],
          },
        },
      }),
    ),
  );

  const documentSnapshot = await documentSnapshotResult.promise;
  assert.equal(documentErrors.length, 0);
  assert.equal(documentSnapshot.exists(), true);
  assert.equal(documentSnapshot.ref.path, "cities/SF");
  assert.deepEqual(documentSnapshot.data(), {
    name: "San Francisco",
    population: 883305,
  });
  assert.equal(documentSnapshot.metadata.fromCache, false);
  assert.equal(documentSnapshot.metadata.hasPendingWrites, false);

  unsubscribeDocument();
  await flushMicrotasks();
  assert.equal(documentSocket.closed, true);
  assert.equal(documentSocket.sentFrames.length, 2);
  const removeDocumentRequest = fromBinary(
    firestoreV1.ListenRequestSchema,
    documentSocket.sentFrames[1],
  );
  assert.equal(removeDocumentRequest.targetChange.case, "removeTarget");
  assert.equal(removeDocumentRequest.targetChange.value, 1);

  const querySnapshotResult = deferred();
  const queryErrors = [];
  const unsubscribeQuery = firestoreModule.onSnapshot(
    firestoreModule.query(cities, firestoreModule.orderBy("name")),
    (snapshot) => querySnapshotResult.resolve(snapshot),
    (error) => queryErrors.push(error),
  );
  await flushMicrotasks();
  assert.equal(sockets.length, 2);
  const querySocket = sockets[1];
  assert.deepEqual(querySocket.protocols, ["nimbus.firebase.listen.v1"]);

  querySocket.emitOpen();
  await flushMicrotasks();
  assert.equal(querySocket.sentFrames.length, 1);
  const addQueryRequest = fromBinary(
    firestoreV1.ListenRequestSchema,
    querySocket.sentFrames[0],
  );
  assert.equal(addQueryRequest.targetChange.case, "addTarget");
  assert.equal(addQueryRequest.targetChange.value.targetType.case, "query");
  assert.equal(
    addQueryRequest.targetChange.value.targetType.value.parent,
    "projects/listen-project/databases/listen/documents",
  );
  assert.equal(
    addQueryRequest.targetChange.value.targetType.value.queryType.case,
    "structuredQuery",
  );
  assert.equal(
    addQueryRequest.targetChange.value.targetType.value.queryType.value.orderBy[0]?.field
      ?.fieldPath,
    "name",
  );

  querySocket.emitBinary(
    toBinary(
      firestoreV1.ListenResponseSchema,
      create(firestoreV1.ListenResponseSchema, {
        responseType: {
          case: "targetChange",
          value: {
            targetChangeType: firestoreV1.TargetChange_TargetChangeType.ADD,
            targetIds: [1],
          },
        },
      }),
    ),
  );
  querySocket.emitBinary(
    toBinary(
      firestoreV1.ListenResponseSchema,
      create(firestoreV1.ListenResponseSchema, {
        responseType: {
          case: "documentChange",
          value: {
            document: {
              name: "projects/listen-project/databases/listen/documents/cities/SF",
              fields: {
                name: {
                  valueType: {
                    case: "stringValue",
                    value: "San Francisco",
                  },
                },
              },
            },
            targetIds: [1],
          },
        },
      }),
    ),
  );
  querySocket.emitBinary(
    toBinary(
      firestoreV1.ListenResponseSchema,
      create(firestoreV1.ListenResponseSchema, {
        responseType: {
          case: "documentChange",
          value: {
            document: {
              name: "projects/listen-project/databases/listen/documents/cities/LA",
              fields: {
                name: {
                  valueType: {
                    case: "stringValue",
                    value: "Los Angeles",
                  },
                },
              },
            },
            targetIds: [1],
          },
        },
      }),
    ),
  );
  querySocket.emitBinary(
    toBinary(
      firestoreV1.ListenResponseSchema,
      create(firestoreV1.ListenResponseSchema, {
        responseType: {
          case: "targetChange",
          value: {
            targetChangeType: firestoreV1.TargetChange_TargetChangeType.CURRENT,
            targetIds: [1],
          },
        },
      }),
    ),
  );

  const querySnapshot = await querySnapshotResult.promise;
  assert.equal(queryErrors.length, 0);
  assert.equal(querySnapshot.size, 2);
  assert.deepEqual(
    querySnapshot.docs.map((snapshot) => snapshot.data().name),
    ["Los Angeles", "San Francisco"],
  );
  assert.equal(querySnapshot.metadata.fromCache, false);
  assert.equal(querySnapshot.metadata.hasPendingWrites, false);

  unsubscribeQuery();
  await flushMicrotasks();
  assert.equal(querySocket.closed, true);
  assert.equal(querySocket.sentFrames.length, 2);
  const removeQueryRequest = fromBinary(
    firestoreV1.ListenRequestSchema,
    querySocket.sentFrames[1],
  );
  assert.equal(removeQueryRequest.targetChange.case, "removeTarget");
  assert.equal(removeQueryRequest.targetChange.value, 1);

  const reconnectSnapshots = [];
  const reconnectErrors = [];
  const unsubscribeReconnectQuery = firestoreModule.onSnapshot(
    firestoreModule.query(cities, firestoreModule.orderBy("name")),
    (snapshot) => reconnectSnapshots.push(snapshot),
    (error) => reconnectErrors.push(error),
  );
  await flushMicrotasks();
  assert.equal(sockets.length, 3);
  const reconnectQuerySocket = sockets[2];

  reconnectQuerySocket.emitOpen();
  await flushMicrotasks();
  const initialReconnectRequest = fromBinary(
    firestoreV1.ListenRequestSchema,
    reconnectQuerySocket.sentFrames[0],
  );
  assert.equal(
    initialReconnectRequest.targetChange.value.resumeType.case,
    undefined,
  );

  reconnectQuerySocket.emitBinary(
    toBinary(
      firestoreV1.ListenResponseSchema,
      create(firestoreV1.ListenResponseSchema, {
        responseType: {
          case: "targetChange",
          value: {
            targetChangeType: firestoreV1.TargetChange_TargetChangeType.ADD,
            targetIds: [1],
          },
        },
      }),
    ),
  );
  reconnectQuerySocket.emitBinary(
    toBinary(
      firestoreV1.ListenResponseSchema,
      create(firestoreV1.ListenResponseSchema, {
        responseType: {
          case: "documentChange",
          value: {
            document: {
              name: "projects/listen-project/databases/listen/documents/cities/SF",
              fields: {
                name: {
                  valueType: {
                    case: "stringValue",
                    value: "San Francisco",
                  },
                },
              },
            },
            targetIds: [1],
          },
        },
      }),
    ),
  );
  reconnectQuerySocket.emitBinary(
    toBinary(
      firestoreV1.ListenResponseSchema,
      create(firestoreV1.ListenResponseSchema, {
        responseType: {
          case: "targetChange",
          value: {
            resumeToken: new Uint8Array([1, 2, 3]),
            readTime: {
              seconds: 1_745_452_800n,
              nanos: 123,
            },
            targetChangeType: firestoreV1.TargetChange_TargetChangeType.CURRENT,
            targetIds: [1],
          },
        },
      }),
    ),
  );
  await flushMicrotasks();
  assert.equal(reconnectErrors.length, 0);
  assert.equal(reconnectSnapshots.length, 1);
  assert.deepEqual(
    reconnectSnapshots[0].docs.map((snapshot) => snapshot.data().name),
    ["San Francisco"],
  );

  reconnectQuerySocket.emitClose(1006, "dropped");
  await flushMicrotasks();
  assert.equal(sockets.length, 4);
  const resumedQuerySocket = sockets[3];

  resumedQuerySocket.emitOpen();
  await flushMicrotasks();
  const resumedQueryRequest = fromBinary(
    firestoreV1.ListenRequestSchema,
    resumedQuerySocket.sentFrames[0],
  );
  assert.equal(
    resumedQueryRequest.targetChange.value.resumeType.case,
    "resumeToken",
  );
  assert.deepEqual(
    Array.from(resumedQueryRequest.targetChange.value.resumeType.value),
    [1, 2, 3],
  );

  resumedQuerySocket.emitBinary(
    toBinary(
      firestoreV1.ListenResponseSchema,
      create(firestoreV1.ListenResponseSchema, {
        responseType: {
          case: "documentChange",
          value: {
            document: {
              name: "projects/listen-project/databases/listen/documents/cities/LA",
              fields: {
                name: {
                  valueType: {
                    case: "stringValue",
                    value: "Los Angeles",
                  },
                },
              },
            },
            targetIds: [1],
          },
        },
      }),
    ),
  );
  await flushMicrotasks();
  assert.equal(reconnectSnapshots.length, 1);

  resumedQuerySocket.emitBinary(
    toBinary(
      firestoreV1.ListenResponseSchema,
      create(firestoreV1.ListenResponseSchema, {
        responseType: {
          case: "targetChange",
          value: {
            resumeToken: new Uint8Array([4, 5, 6]),
            readTime: {
              seconds: 1_745_452_801n,
              nanos: 456,
            },
            targetChangeType: firestoreV1.TargetChange_TargetChangeType.CURRENT,
            targetIds: [1],
          },
        },
      }),
    ),
  );
  await flushMicrotasks();
  assert.equal(reconnectErrors.length, 0);
  assert.equal(reconnectSnapshots.length, 2);
  assert.deepEqual(
    reconnectSnapshots[1].docs.map((snapshot) => snapshot.data().name),
    ["Los Angeles", "San Francisco"],
  );

  unsubscribeReconnectQuery();
  await flushMicrotasks();
  assert.equal(resumedQuerySocket.closed, true);
  resumedQuerySocket.emitClose(1006, "after unsubscribe");
  await flushMicrotasks();
  assert.equal(sockets.length, 4);

  const readTimeSnapshots = [];
  const readTimeErrors = [];
  const unsubscribeReadTimeDocument = firestoreModule.onSnapshot(
    city,
    (snapshot) => readTimeSnapshots.push(snapshot),
    (error) => readTimeErrors.push(error),
  );
  await flushMicrotasks();
  assert.equal(sockets.length, 5);
  const readTimeDocumentSocket = sockets[4];

  readTimeDocumentSocket.emitOpen();
  await flushMicrotasks();
  const initialReadTimeRequest = fromBinary(
    firestoreV1.ListenRequestSchema,
    readTimeDocumentSocket.sentFrames[0],
  );
  assert.equal(
    initialReadTimeRequest.targetChange.value.resumeType.case,
    undefined,
  );

  readTimeDocumentSocket.emitBinary(
    toBinary(
      firestoreV1.ListenResponseSchema,
      create(firestoreV1.ListenResponseSchema, {
        responseType: {
          case: "targetChange",
          value: {
            targetChangeType: firestoreV1.TargetChange_TargetChangeType.ADD,
            targetIds: [1],
          },
        },
      }),
    ),
  );
  readTimeDocumentSocket.emitBinary(
    toBinary(
      firestoreV1.ListenResponseSchema,
      create(firestoreV1.ListenResponseSchema, {
        responseType: {
          case: "documentChange",
          value: {
            document: {
              name: "projects/listen-project/databases/listen/documents/cities/SF",
              fields: {
                name: {
                  valueType: {
                    case: "stringValue",
                    value: "San Francisco",
                  },
                },
              },
            },
            targetIds: [1],
          },
        },
      }),
    ),
  );
  readTimeDocumentSocket.emitBinary(
    toBinary(
      firestoreV1.ListenResponseSchema,
      create(firestoreV1.ListenResponseSchema, {
        responseType: {
          case: "targetChange",
          value: {
            readTime: {
              seconds: 1_745_452_802n,
              nanos: 789,
            },
            targetChangeType: firestoreV1.TargetChange_TargetChangeType.CURRENT,
            targetIds: [1],
          },
        },
      }),
    ),
  );
  await flushMicrotasks();
  assert.equal(readTimeErrors.length, 0);
  assert.equal(readTimeSnapshots.length, 1);
  assert.equal(readTimeSnapshots[0].exists(), true);

  readTimeDocumentSocket.emitClose(1006, "dropped");
  await flushMicrotasks();
  assert.equal(sockets.length, 6);
  const resumedReadTimeSocket = sockets[5];

  resumedReadTimeSocket.emitOpen();
  await flushMicrotasks();
  const resumedReadTimeRequest = fromBinary(
    firestoreV1.ListenRequestSchema,
    resumedReadTimeSocket.sentFrames[0],
  );
  assert.equal(
    resumedReadTimeRequest.targetChange.value.resumeType.case,
    "readTime",
  );
  assert.equal(
    resumedReadTimeRequest.targetChange.value.resumeType.value.seconds,
    1_745_452_802n,
  );
  assert.equal(
    resumedReadTimeRequest.targetChange.value.resumeType.value.nanos,
    789,
  );

  unsubscribeReadTimeDocument();
  await flushMicrotasks();
  assert.equal(resumedReadTimeSocket.closed, true);

    const fatalPolicyErrors = [];
    const unsubscribeFatalPolicy = firestoreModule.onSnapshot(
      city,
      () => {
        throw new Error("Policy-close watch should not deliver a snapshot.");
      },
      (error) => fatalPolicyErrors.push(error),
    );
    await flushMicrotasks();
    assert.equal(sockets.length, 7);
    const fatalPolicySocket = sockets[6];
    fatalPolicySocket.emitOpen();
    await flushMicrotasks();
    fatalPolicySocket.emitClose(1008, "request-level policy failure");
    await flushMicrotasks();
    assert.equal(fatalPolicyErrors.length, 1);
    assert.equal(fatalPolicyErrors[0].code, "FAILED_PRECONDITION");
    assert.equal(fatalPolicyErrors[0].status, 400);
    assert.equal(sockets.length, 7);
    unsubscribeFatalPolicy();
    await flushMicrotasks();

    const unsupportedFrameErrors = [];
    const unsubscribeUnsupportedFrame = firestoreModule.onSnapshot(
      city,
      () => {
        throw new Error("Unsupported-close watch should not deliver a snapshot.");
      },
      (error) => unsupportedFrameErrors.push(error),
    );
    await flushMicrotasks();
    assert.equal(sockets.length, 8);
    const unsupportedFrameSocket = sockets[7];
    unsupportedFrameSocket.emitOpen();
    await flushMicrotasks();
    unsupportedFrameSocket.emitClose(1003, "binary protobuf required");
    await flushMicrotasks();
    assert.equal(unsupportedFrameErrors.length, 1);
    assert.equal(unsupportedFrameErrors[0].code, "INVALID_ARGUMENT");
    assert.equal(unsupportedFrameErrors[0].status, 400);
    assert.equal(sockets.length, 8);
    unsubscribeUnsupportedFrame();
    await flushMicrotasks();

    const retryErrors = [];
    const unsubscribeRetryBudget = firestoreModule.onSnapshot(
      city,
      () => {
        throw new Error("Retry-budget watch should not deliver a snapshot.");
      },
      (error) => retryErrors.push(error),
    );
    await flushMicrotasks();
    assert.equal(sockets.length, 9);
    const retrySocketOne = sockets[8];
    retrySocketOne.emitOpen();
    await flushMicrotasks();
    retrySocketOne.emitClose(1011, "backpressure 1");
    await flushMicrotasks();
    assert.equal(sockets.length, 10);

    const retrySocketTwo = sockets[9];
    retrySocketTwo.emitOpen();
    await flushMicrotasks();
    retrySocketTwo.emitClose(1011, "backpressure 2");
    await flushMicrotasks();
    assert.equal(sockets.length, 11);

    const retrySocketThree = sockets[10];
    retrySocketThree.emitOpen();
    await flushMicrotasks();
    retrySocketThree.emitClose(1011, "backpressure 3");
    await flushMicrotasks();
    assert.equal(sockets.length, 12);
    assert.deepEqual(scheduledRetryDelays.slice(-3), [0, 50, 250]);

    const retrySocketFour = sockets[11];
    retrySocketFour.emitOpen();
    await flushMicrotasks();
    retrySocketFour.emitClose(1011, "backpressure exhausted");
    await flushMicrotasks();
    assert.equal(retryErrors.length, 1);
    assert.equal(retryErrors[0].code, "UNAVAILABLE");
    assert.equal(retryErrors[0].status, 503);
    assert.equal(retryErrors[0].message, "backpressure exhausted");
    assert.equal(sockets.length, 12);
    unsubscribeRetryBudget();
    await flushMicrotasks();

    const listenAuthCalls = [];
    const authSockets = [];
    const authFirestore = firestoreModule.initializeFirestore(
      app,
      {
        experimentalAuthToken: async ({ forceRefresh }) => {
          listenAuthCalls.push(forceRefresh);
          return forceRefresh ? "fresh-listen-token" : "stale-listen-token";
        },
        experimentalWebSocketFactory: (url, protocols) => {
          const socket = new FakeWebSocket(url, protocols);
          authSockets.push(socket);
          return socket;
        },
        host: "listen-auth.test",
        ssl: false,
      },
      "listen-auth",
    );
    const authCity = firestoreModule.doc(
      firestoreModule.collection(authFirestore, "cities"),
      "SFO",
    );
    const authSnapshots = [];
    const authErrors = [];
    const unsubscribeAuthWatch = firestoreModule.onSnapshot(
      authCity,
      (snapshot) => authSnapshots.push(snapshot),
      (error) => authErrors.push(error),
    );
    await flushMicrotasks();
    assert.equal(authSockets.length, 1);
    assert.deepEqual(authSockets[0].protocols[0], "nimbus.firebase.listen.v1");
    assert.equal(
      decodeListenAuthSubprotocol(authSockets[0].protocols),
      "stale-listen-token",
    );

    authSockets[0].emitOpen();
    await flushMicrotasks();
    authSockets[0].emitClose(1008, "Firestore Listen unauthenticated.");
    await flushMicrotasks();
    await flushMicrotasks();
    assert.equal(authSockets.length, 2);
    assert.deepEqual(listenAuthCalls, [false, true]);
    assert.equal(
      decodeListenAuthSubprotocol(authSockets[1].protocols),
      "fresh-listen-token",
    );

    authSockets[1].emitOpen();
    await flushMicrotasks();
    authSockets[1].emitBinary(
      toBinary(
        firestoreV1.ListenResponseSchema,
        create(firestoreV1.ListenResponseSchema, {
          responseType: {
            case: "targetChange",
            value: {
              targetChangeType: firestoreV1.TargetChange_TargetChangeType.ADD,
              targetIds: [1],
            },
          },
        }),
      ),
    );
    authSockets[1].emitBinary(
      toBinary(
        firestoreV1.ListenResponseSchema,
        create(firestoreV1.ListenResponseSchema, {
          responseType: {
            case: "documentChange",
            value: {
              document: {
                name: "projects/listen-project/databases/listen-auth/documents/cities/SFO",
                fields: {
                  name: {
                    valueType: {
                      case: "stringValue",
                      value: "San Francisco Authenticated",
                    },
                  },
                },
              },
              targetIds: [1],
            },
          },
        }),
      ),
    );
    authSockets[1].emitBinary(
      toBinary(
        firestoreV1.ListenResponseSchema,
        create(firestoreV1.ListenResponseSchema, {
          responseType: {
            case: "targetChange",
            value: {
              targetChangeType: firestoreV1.TargetChange_TargetChangeType.CURRENT,
              targetIds: [1],
            },
          },
        }),
      ),
    );
    await flushMicrotasks();
    assert.equal(authErrors.length, 0);
    assert.equal(authSnapshots.length, 1);
    assert.equal(authSnapshots[0].data().name, "San Francisco Authenticated");

    authSockets[1].emitClose(1008, "Firestore Listen unauthenticated.");
    await flushMicrotasks();
    await flushMicrotasks();
    assert.equal(authSockets.length, 3);
    assert.deepEqual(listenAuthCalls, [false, true, true]);
    assert.equal(
      decodeListenAuthSubprotocol(authSockets[2].protocols),
      "fresh-listen-token",
    );
    authSockets[2].emitOpen();
    await flushMicrotasks();
    authSockets[2].emitClose(1008, "Firestore Listen unauthenticated.");
    await flushMicrotasks();
    await flushMicrotasks();
    assert.equal(authErrors.length, 1);
    assert.equal(authErrors[0].code, "UNAUTHENTICATED");
    assert.equal(authErrors[0].status, 401);
    unsubscribeAuthWatch();
    await flushMicrotasks();

    await appModule.deleteApp(app);
  });
}

