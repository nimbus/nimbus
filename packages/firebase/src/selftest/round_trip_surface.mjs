import { assert, path, pathToFileURL } from "./support.mjs";

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

async function withTimeout(promise, label, milliseconds = 15000) {
  let timer;
  const timeout = new Promise((_, reject) => {
    timer = setTimeout(
      () => reject(new Error(`${label} timed out after ${milliseconds}ms`)),
      milliseconds,
    );
  });
  try {
    return await Promise.race([promise, timeout]);
  } finally {
    clearTimeout(timer);
  }
}

// Live addDoc/getDocs/onSnapshot round-trip against a Nimbus dev-shaped
// server, addressing the project id `nimbus dev` discovered and mapped to a
// tenant — proves the projectId→tenant mapping end to end through
// connectFirestoreEmulator, exactly the way a client app reaches the
// emulator endpoint.
export async function testRoundTripSurface(bundleDir, baseUrlText, projectId) {
  const appModule = await import(pathToFileURL(path.join(bundleDir, "app.mjs")).href);
  const firestoreModule = await import(
    pathToFileURL(path.join(bundleDir, "firestore.mjs")).href,
  );
  const baseUrl = new URL(baseUrlText);
  assert.ok(baseUrl.hostname, "Round-trip base URL must include a hostname.");
  assert.ok(baseUrl.port, "Round-trip base URL must include an explicit port.");
  assert.ok(projectId, "Round-trip project id is required.");

  const app = appModule.initializeApp({ projectId }, "round-trip");
  const firestore = firestoreModule.getFirestore(app);
  firestoreModule.connectFirestoreEmulator(
    firestore,
    baseUrl.hostname,
    Number.parseInt(baseUrl.port, 10),
  );

  const notes = firestoreModule.collection(firestore, "notes");

  const first = await firestoreModule.addDoc(notes, {
    body: "first note",
    rank: 1,
  });
  assert.ok(first.id, "addDoc should mint a document id");

  const initial = await firestoreModule.getDocs(notes);
  assert.equal(initial.size, 1);
  assert.equal(initial.docs[0].id, first.id);
  assert.deepEqual(initial.docs[0].data(), { body: "first note", rank: 1 });

  const sawFirst = deferred();
  const sawSecond = deferred();
  const watchErrors = [];
  const unsubscribe = firestoreModule.onSnapshot(
    notes,
    (snapshot) => {
      if (snapshot.docs.some((doc) => doc.id === first.id)) {
        sawFirst.resolve(snapshot);
      }
      if (snapshot.size === 2) {
        sawSecond.resolve(snapshot);
      }
    },
    (error) => {
      watchErrors.push(error);
      sawFirst.reject(error);
      sawSecond.reject(error);
    },
  );

  const initialSnapshot = await withTimeout(sawFirst.promise, "initial onSnapshot");
  assert.equal(initialSnapshot.size, 1);

  const second = await firestoreModule.addDoc(notes, {
    body: "second note",
    rank: 2,
  });
  const updateSnapshot = await withTimeout(sawSecond.promise, "live onSnapshot update");
  const byId = new Map(updateSnapshot.docs.map((doc) => [doc.id, doc.data()]));
  assert.deepEqual(byId.get(second.id), { body: "second note", rank: 2 });

  unsubscribe();
  assert.deepEqual(watchErrors, []);
  await firestoreModule.terminate(firestore);
}
