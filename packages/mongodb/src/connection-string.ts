export interface ConnectionStringOptions {
  host?: string;
  port?: number;
  database?: string;
  username?: string;
  password?: string;
}

const DEFAULT_HOST = "127.0.0.1";
const DEFAULT_PORT = 27017;

export function buildConnectionString(
  options: ConnectionStringOptions = {},
): string {
  const host = options.host ?? DEFAULT_HOST;
  const port = options.port ?? DEFAULT_PORT;
  const db = options.database ?? "default";

  if (options.username && options.password) {
    const user = encodeURIComponent(options.username);
    const pass = encodeURIComponent(options.password);
    return `mongodb://${user}:${pass}@${host}:${port}/${db}?directConnection=true`;
  }

  return `mongodb://${host}:${port}/${db}?directConnection=true`;
}
