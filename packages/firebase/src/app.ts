const DEFAULT_APP_NAME = "[DEFAULT]";

export interface FirebaseOptions {
  apiKey?: string;
  appId?: string;
  authDomain?: string;
  databaseURL?: string;
  measurementId?: string;
  messagingSenderId?: string;
  projectId?: string;
  storageBucket?: string;
}

export interface FirebaseAppSettings {
  name?: string;
}

export interface FirebaseApp {
  readonly name: string;
  readonly options: Readonly<FirebaseOptions>;
  readonly automaticDataCollectionEnabled: boolean;
}

class FirebaseAppImpl implements FirebaseApp {
  readonly #options: Readonly<FirebaseOptions>;
  readonly name: string;
  automaticDataCollectionEnabled = false;
  #deleted = false;

  constructor(options: FirebaseOptions, name: string) {
    this.#options = Object.freeze({ ...options });
    this.name = name;
  }

  get options(): Readonly<FirebaseOptions> {
    return this.#options;
  }

  get deleted(): boolean {
    return this.#deleted;
  }

  markDeleted(): void {
    this.#deleted = true;
  }
}

const apps = new Map<string, FirebaseAppImpl>();

function normalizedAppName(name?: string): string {
  const candidate = name?.trim() ?? DEFAULT_APP_NAME;
  if (candidate.length === 0) {
    throw new Error("Firebase app name must not be empty.");
  }
  return candidate;
}

function assertKnownApp(app: FirebaseApp): FirebaseAppImpl {
  const known = apps.get(app.name);
  if (!known || known !== app) {
    throw new Error(`Firebase app "${app.name}" is not registered by @neovex/firebase.`);
  }
  if (known.deleted) {
    throw new Error(`Firebase app "${app.name}" has already been deleted.`);
  }
  return known;
}

function requireInitializedApp(name: string): FirebaseAppImpl {
  const existing = apps.get(name);
  if (!existing || existing.deleted) {
    throw new Error(`Firebase app "${name}" has not been initialized.`);
  }
  return existing;
}

export function initializeApp(
  options: FirebaseOptions,
  configOrName?: FirebaseAppSettings | string,
): FirebaseApp {
  const name =
    typeof configOrName === "string"
      ? normalizedAppName(configOrName)
      : normalizedAppName(configOrName?.name);
  if (apps.has(name)) {
    throw new Error(`Firebase app "${name}" already exists.`);
  }
  const app = new FirebaseAppImpl(options, name);
  apps.set(name, app);
  return app;
}

export function getApp(name?: string): FirebaseApp {
  return requireInitializedApp(normalizedAppName(name));
}

export function getApps(): FirebaseApp[] {
  return Array.from(apps.values()).filter((app) => !app.deleted);
}

export async function deleteApp(app: FirebaseApp): Promise<void> {
  const known = assertKnownApp(app);
  known.markDeleted();
  apps.delete(known.name);
}
