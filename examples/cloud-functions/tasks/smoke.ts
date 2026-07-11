const baseUrl = new URL(process.env.NIMBUS_CLOUD_FUNCTIONS_URL ?? "http://localhost:8080");
const tenantId = process.env.NIMBUS_TENANT_ID ?? "demo";
const adminToken = process.env.NIMBUS_ADMIN_TOKEN;
const firebaseToken = process.env.NIMBUS_FIREBASE_AUTH_TOKEN ?? JSON.stringify({
  sub: "cloud-functions-tasks-smoke",
  iss: `https://securetoken.google.com/${tenantId}`,
});
let activeAssertion = "setup";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function pass(assertion: string): void {
  console.log(`PASS ${assertion}`);
}

async function sleep(milliseconds: number): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, milliseconds));
}

function apiUrl(path: string): URL {
  return new URL(path, baseUrl);
}

function adminHeaders(json = false): Record<string, string> {
  return {
    ...(json ? { "content-type": "application/json" } : {}),
    ...(adminToken ? { authorization: `Bearer ${adminToken}` } : {}),
  };
}

async function createTenant(): Promise<void> {
  const response = await fetch(apiUrl("/api/tenants"), {
    method: "POST",
    headers: adminHeaders(true),
    body: JSON.stringify({ id: tenantId }),
  });
  assert(
    response.status === 201 || response.status === 409,
    `tenant setup expected HTTP 201 or 409, got ${response.status}`,
  );
}

function firestoreDocumentName(collectionName: string, documentId: string): string {
  return `projects/${tenantId}/databases/(default)/documents/${collectionName}/${documentId}`;
}

async function commitFirestoreWrites(writes: Record<string, unknown>[]): Promise<Response> {
  return await fetch(
    apiUrl(`/v1/projects/${tenantId}/databases/(default)/documents:commit`),
    {
      method: "POST",
      headers: {
        "content-type": "text/plain;charset=UTF-8",
        authorization: `Bearer ${firebaseToken}`,
      },
      body: JSON.stringify({
        database: `projects/${tenantId}/databases/(default)`,
        writes,
      }),
    },
  );
}

async function main(): Promise<void> {
  await createTenant();
  const task = {
    text: "Observe a durable derived write",
    completed: false,
    createdAt: Date.now(),
  };
  const taskId = `smoke-${Date.now()}-${Math.random().toString(16).slice(2)}`;
  const taskName = firestoreDocumentName("tasks", taskId);
  const derivationName = firestoreDocumentName("taskDerivations", taskId);

  try {
    // EX3.6 acceptance: tasks.create triggers an observable derived write.
    activeAssertion = "tasks.create-triggers-derived-write";
    const insertResponse = await commitFirestoreWrites([{
      update: {
        name: taskName,
        fields: {
          text: { stringValue: task.text },
          completed: { booleanValue: task.completed },
          createdAt: { integerValue: String(task.createdAt) },
        },
      },
      currentDocument: { exists: false },
    }]);
    assert(insertResponse.status === 200, `Firestore task insert expected HTTP 200, got ${insertResponse.status}`);

    const deadline = Date.now() + 10_000;
    let derived: Record<string, unknown> | undefined;
    while (Date.now() < deadline) {
      const handlerUrl = apiUrl("/taskDetails");
      handlerUrl.searchParams.set("taskId", taskId);
      const response = await fetch(handlerUrl);
      if (response.status === 200) {
        const payload = (await response.json()) as { derivation?: Record<string, unknown> | null };
        if (payload.derivation) {
          derived = payload.derivation;
          break;
        }
      }
      assert(response.status === 200, `taskDetails polling expected HTTP 200, got ${response.status}`);
      await sleep(200);
    }
    assert(derived, "timed out waiting for the taskDerivations write");
    assert(derived.sourceTaskId === taskId, "derived write must identify its source task");
    assert(derived.textLength === task.text.length, "derived write must contain the task text length");
    assert(
      derived.completedAtCreation === false,
      "derived write must preserve the task's creation-time completion state",
    );
    assert(derived.sourceCreatedAt === task.createdAt, "derived write must preserve createdAt");
    pass("tasks.create-triggers-derived-write");

    // EX3.6 acceptance: the directly curl-able HTTP handler returns current task data.
    activeAssertion = "cloud-functions.http-handler-response";
    const handlerUrl = apiUrl("/taskDetails");
    handlerUrl.searchParams.set("taskId", taskId);
    const response = await fetch(handlerUrl);
    const payload = (await response.json()) as {
      task?: { id?: unknown; text?: unknown; completed?: unknown; createdAt?: unknown };
      derivation?: Record<string, unknown> | null;
    };
    assert(response.status === 200, `taskDetails expected HTTP 200, got ${response.status}`);
    assert(payload.task?.id === taskId, "taskDetails must return the requested task id");
    assert(payload.task.text === task.text, "taskDetails must return the current task text");
    assert(payload.task.completed === false, "taskDetails must return completed === false");
    assert(payload.task.createdAt === task.createdAt, "taskDetails must return createdAt");
    assert(payload.derivation?.sourceTaskId === taskId, "taskDetails must return the derived write");
    pass("cloud-functions.http-handler-response");
  } finally {
    await commitFirestoreWrites([
      { delete: derivationName },
      { delete: taskName },
    ]).catch(() => undefined);
  }
}

main().catch((error) => {
  console.error(`FAIL ${activeAssertion}: ${(error as Error).message}`);
  process.exitCode = 1;
});
