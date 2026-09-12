import { assert } from "./support.mjs";

export async function testFieldPathOwnPropertySurface(firestoreModule, appModule) {
  const app = appModule.initializeApp({ projectId: "field-paths" }, "field-paths");
  const requests = [];
  let fields = {};
  const firestore = firestoreModule.initializeFirestore(app, {
    host: "field-paths.test",
    ssl: false,
    experimentalFetch: async (url, options) => {
      if (String(url).endsWith(":commit")) {
        const body = JSON.parse(String(options.body));
        requests.push(body);
        fields = body.writes[0].update.fields;
        return Response.json({ commitTime: "2026-09-12T00:00:00Z" });
      }
      assert.ok(String(url).endsWith(":batchGet"));
      return Response.json({
        found: {
          name: "projects/field-paths/databases/(default)/documents/items/one",
          fields,
        },
      });
    },
  });
  const reference = firestoreModule.doc(firestore, "items/one");
  const marker = "__nimbus_field_path_probe__";
  const originalPrototypeKeys = Reflect.ownKeys(Object.prototype);
  const cases = [
    ["setDoc", { ["__proto__"]: { [marker]: "set" } }, `__proto__.${marker}`, "set"],
    ["updateDoc", { [`__proto__.${marker}`]: "update" }, `__proto__.${marker}`, "update"],
    ["setDoc", { nested: { ["__proto__"]: { [marker]: "nested" } } }, `nested.__proto__.${marker}`, "nested"],
    ["updateDoc", { [`nested.__proto__.${marker}`]: "dotted" }, `nested.__proto__.${marker}`, "dotted"],
    ["setDoc", { ["__proto__"]: "leaf" }, "__proto__", "leaf"],
    ["updateDoc", { [`constructor.prototype.${marker}`]: "constructor" }, `constructor.prototype.${marker}`, "constructor"],
    ["setDoc", { toString: "own" }, "toString", "own"],
    ["setDoc", { ["__proto__"]: { [marker]: "merge" } }, `__proto__.${marker}`, "merge", { mergeFields: [`__proto__.${marker}`] }],
  ];

  assert.equal(Object.hasOwn(Object.prototype, marker), false);
  try {
    for (const [operation, data, fieldPath, expected, options] of cases) {
      const before = requests.length;
      await firestoreModule[operation](reference, data, options);
      assert.equal(requests.length, before + 1);
      assert.deepEqual(Reflect.ownKeys(Object.prototype), originalPrototypeKeys);
      assert.equal(Object.hasOwn(Object.prototype, marker), false);
      const snapshot = await firestoreModule.getDoc(reference);
      assert.equal(snapshot.get(fieldPath), expected, `${operation}: ${fieldPath}`);
      assert.equal(Object.getPrototypeOf(snapshot.data()), Object.prototype);
    }

    await firestoreModule.setDoc(reference, { normal: true });
    const snapshot = await firestoreModule.getDoc(reference);
    for (const fieldPath of ["__proto__", "constructor", "toString"]) {
      assert.equal(snapshot.get(fieldPath), undefined, fieldPath);
      const before = requests.length;
      await assert.rejects(
        firestoreModule.setDoc(reference, { normal: true }, { mergeFields: [fieldPath] }),
        /was not present in the provided data/,
      );
      assert.equal(requests.length, before);
    }
  } finally {
    delete Object.prototype[marker];
    await appModule.deleteApp(app);
  }
}
