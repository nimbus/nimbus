import { assert, build, fileURLToPath, fs, os, packageJsonPath, packageRoot, path, spawnSync, tscPath } from "./support.mjs";

export async function assertPackageExports() {
  const packageJson = JSON.parse(await fs.readFile(packageJsonPath, "utf8"));
  // The stock npm name: provisioned apps install this package as `firebase`
  // so stock `firebase/app` + `firebase/firestore` imports resolve unchanged.
  assert.equal(packageJson.name, "firebase");
  assert.deepEqual(packageJson.exports, {
    ".": "./src/index.ts",
    "./app": "./src/app.ts",
    "./firestore": "./src/firestore.ts",
  });
}

export async function buildPackageSurface() {
  const bundleDir = await fs.mkdtemp(path.join(os.tmpdir(), "nimbus-firebase-package-"));
  for (const entry of [
    { name: "app", source: "./app.ts" },
    { name: "firestore", source: "./firestore.ts" },
    { name: "index", source: "./index.ts" },
    { name: "internal-protobuf", source: "./internal/protobuf.ts" },
  ]) {
    const source = path.join(packageRoot, "src", entry.source.replace(/^\.\//u, ""));
    await buildEntry(source, path.join(bundleDir, `${entry.name}.mjs`), "esm");
    await buildEntry(source, path.join(bundleDir, `${entry.name}.cjs`), "cjs");
  }
  return bundleDir;
}

async function buildEntry(entryPoint, outfile, format) {
  await build({
    entryPoints: [entryPoint],
    bundle: true,
    format,
    outfile,
    logLevel: "silent",
    platform: "neutral",
    target: "es2022",
  });
}


export async function assertGeneratedProtoSurface() {
  for (const relativePath of [
    "src/gen/google/firestore/v1/document_pb.ts",
    "src/gen/google/firestore/v1/firestore_pb.ts",
    "src/gen/google/firestore/v1/query_pb.ts",
    "src/gen/google/firestore/v1/write_pb.ts",
    "src/gen/google/protobuf/timestamp_pb.ts",
  ]) {
    try {
      await fs.access(path.join(packageRoot, relativePath));
    } catch {
      throw new Error(
        `Missing generated Firestore protobuf output at ${relativePath}. Run "npm run codegen:proto --workspace firebase" first.`,
      );
    }
  }
}


export async function typecheckFirebaseSurface() {
  const fixtureDir = await fs.mkdtemp(path.join(os.tmpdir(), "nimbus-firebase-ts-"));
  const normalize = (target) => path.relative(fixtureDir, target).replaceAll("\\", "/");
  const rootEntry = normalize(path.join(packageRoot, "src", "index.ts"));
  const appEntry = normalize(path.join(packageRoot, "src", "app.ts"));
  const firestoreEntry = normalize(path.join(packageRoot, "src", "firestore.ts"));

  await fs.writeFile(
    path.join(fixtureDir, "tsconfig.json"),
    JSON.stringify(
      {
        compilerOptions: {
          strict: true,
          noEmit: true,
          target: "ES2022",
          module: "ESNext",
          moduleResolution: "Bundler",
          allowImportingTsExtensions: true,
          lib: ["ES2022", "DOM"],
          paths: {
            firebase: [rootEntry],
            "firebase/app": [appEntry],
            "firebase/firestore": [firestoreEntry],
          },
        },
        files: ["fixture.ts"],
      },
      null,
      2,
    ),
    "utf8",
  );

  await fs.writeFile(
    path.join(fixtureDir, "fixture.ts"),
    `
import {
  deleteApp,
  getApp,
  getApps,
  initializeApp,
  type FirebaseApp,
  type FirebaseOptions,
} from "firebase/app";
import {
  addDoc,
  arrayRemove,
  arrayUnion,
  collection,
  collectionGroup,
  connectFirestoreEmulator,
  deleteField,
  deleteDoc,
  documentId,
  doc,
  endAt,
  endBefore,
  type FieldValue,
  getDoc,
  getDocs,
  getFirestore,
  increment,
  initializeFirestore,
  limit,
  onSnapshot,
  orderBy,
  query,
  runTransaction,
  serverTimestamp,
  setDoc,
  startAfter,
  startAt,
  terminate,
  updateDoc,
  writeBatch,
  where,
  type CollectionGroup,
  type CollectionReference,
  type DocumentSnapshot,
  type QueryDocumentSnapshot,
  type DocumentReference,
  FirestoreError,
  type FirestoreDataConverter,
  type Firestore,
  type Transaction,
  type TransactionOptions,
  type QuerySnapshot,
  type Query,
  type QueryConstraint,
  type FirestoreSettings,
  type SnapshotObserver,
  type SetOptions,
  type Unsubscribe,
  type WriteBatch,
} from "firebase/firestore";
import {
  getFirestore as getFirestoreFromRoot,
  initializeApp as initializeAppFromRoot,
} from "firebase";

const options: FirebaseOptions = {
  apiKey: "demo-key",
  projectId: "demo-project",
};

const app: FirebaseApp = initializeApp(options);
const namedApp = initializeAppFromRoot({ projectId: "other-project" }, "other");
const firestore: Firestore = getFirestore(app);
const settings: FirestoreSettings = {
  ignoreUndefinedProperties: true,
};
const setOptions: SetOptions = {
  merge: true,
};
const initialized: Firestore = initializeFirestore(namedApp, settings, "analytics");
const cities: CollectionReference = collection(firestore, "cities");
const city: DocumentReference = doc(cities, "SF");
const landmarks: CollectionReference = collection(city, "landmarks");
const group: CollectionGroup = collectionGroup(firestore, "landmarks");
const queryConstraint: QueryConstraint = where("state", "==", "CA");
const citiesQuery: Query = query(
  cities,
  queryConstraint,
  orderBy(documentId()),
  limit(10),
  startAt("projects/demo/databases/(default)/documents/cities/SF"),
  startAfter("projects/demo/databases/(default)/documents/cities/SEA"),
  endAt("projects/demo/databases/(default)/documents/cities/LA"),
  endBefore("projects/demo/databases/(default)/documents/cities/NYC"),
);
const snapshotPromise: Promise<DocumentSnapshot> = getDoc(city);
const querySnapshotPromise: Promise<QuerySnapshot> = getDocs(citiesQuery);
const transformValue: FieldValue = serverTimestamp();
const documentObserver: SnapshotObserver<DocumentSnapshot> = {
  next(snapshot) {
    void snapshot.exists();
  },
};
const queryObserver: SnapshotObserver<QuerySnapshot> = {
  next(snapshot) {
    void snapshot.size;
  },
};
const unsubscribeDocument: Unsubscribe = onSnapshot(city, documentObserver);
const unsubscribeQuery: Unsubscribe = onSnapshot(citiesQuery, queryObserver);
const addDocPromise: Promise<DocumentReference> = addDoc(cities, { name: "Paris" });
const setDocPromise: Promise<void> = setDoc(city, {
  deletedAt: deleteField(),
  name: "Paris",
  updatedAt: transformValue,
}, setOptions);
const updateDocPromise: Promise<void> = updateDoc(city, {
  "stats.tags": arrayUnion("metro"),
  "stats.visits": increment(1),
  archivedTags: arrayRemove("legacy"),
});
const deleteDocPromise: Promise<void> = deleteDoc(city);
const batch: WriteBatch = writeBatch(firestore);
const batchCommitPromise: Promise<void> = batch
  .set(city, {
    deletedAt: deleteField(),
    name: "Paris",
  }, setOptions)
  .update(city, { "stats.updatedAt": serverTimestamp() })
  .commit();
const transactionOptions: TransactionOptions = {
  maxAttempts: 2,
};
const transactionPromise: Promise<string> = runTransaction(
  firestore,
  async (transaction: Transaction) => {
    const snapshot = await transaction.get(city);
    const querySnapshot = await transaction.get(citiesQuery);
    transaction.update(city, {
      "stats.updatedAt": serverTimestamp(),
      "stats.visits": Number(snapshot.get("stats.visits") ?? 0) + 1,
    });
    return String(querySnapshot.docs[0]?.get("name") ?? snapshot.get("name") ?? "");
  },
  transactionOptions,
);
const firestoreError: FirestoreError = new FirestoreError("UNKNOWN", "message", 500);
const queryDocumentSnapshot: QueryDocumentSnapshot | null = null;
type City = {
  name: string;
  population: number;
};
const cityConverter: FirestoreDataConverter<City> = {
  toFirestore(model) {
    return {
      name: model.name,
      population: model.population,
    };
  },
  fromFirestore(snapshot) {
    const data = snapshot.data();
    return {
      name: String(data.name),
      population: Number(data.population),
    };
  },
};
const convertedCities: CollectionReference<City> = cities.withConverter(cityConverter);
const convertedCity: DocumentReference<City> = doc(convertedCities, "SEA");
const rawConvertedCity: DocumentReference = convertedCity.withConverter(null);
const convertedQuery: Query<City> = query(convertedCities, where("population", ">=", 1));
const reconvertedQuery: Query<City> = citiesQuery.withConverter(cityConverter);
const convertedSnapshotPromise: Promise<DocumentSnapshot<City>> = getDoc(convertedCity);
const convertedQuerySnapshotPromise: Promise<QuerySnapshot<City>> = getDocs(convertedQuery);
const convertedAddDocPromise: Promise<DocumentReference<City>> = addDoc(convertedCities, {
  name: "Seattle",
  population: 733000,
});
const convertedSetDocPromise: Promise<void> = setDoc(convertedCity, {
  name: "Seattle",
  population: 733000,
});

connectFirestoreEmulator(firestore, "127.0.0.1", 8080, {
  mockUserToken: {
    sub: "user-1",
  },
});

void getApp;
void getApps;
void deleteApp;
void terminate;
void initialized;
void cities;
void city;
void landmarks;
void group;
void citiesQuery;
void snapshotPromise;
void querySnapshotPromise;
void transformValue;
void unsubscribeDocument;
void unsubscribeQuery;
void addDocPromise;
void setDocPromise;
void updateDocPromise;
void deleteDocPromise;
void batchCommitPromise;
void transactionPromise;
void firestoreError;
void queryDocumentSnapshot;
void convertedCities;
void convertedCity;
void rawConvertedCity;
void convertedQuery;
void reconvertedQuery;
void convertedSnapshotPromise;
void convertedQuerySnapshotPromise;
void convertedAddDocPromise;
void convertedSetDocPromise;
void getFirestoreFromRoot(namedApp);
`,
    "utf8",
  );

  const result = spawnSync(process.execPath, [tscPath, "-p", path.join(fixtureDir, "tsconfig.json")], {
    encoding: "utf8",
    cwd: fixtureDir,
  });
  assert.equal(result.status, 0, result.stderr || result.stdout);
}

