/**
 * Connection options for talking to a Nimbus DynamoDB-compatible endpoint with
 * the official `@aws-sdk/client-dynamodb` client.
 */
export interface NimbusDynamoOptions {
  /** Full endpoint URL. Overrides `host`/`port` when set. */
  endpoint?: string;
  /** Host the Nimbus DynamoDB listener is bound to (default `127.0.0.1`). */
  host?: string;
  /** Port the listener is bound to (default `8000`, the DynamoDB Local port). */
  port?: number;
  /** AWS region for the credential scope (default `us-east-1`). */
  region?: string;
  /** Access key id — selects the Nimbus tenant the server binds it to. */
  accessKeyId?: string;
  /** Secret access key — only verified when the server runs in strict mode. */
  secretAccessKey?: string;
}

/**
 * A configuration object that is a drop-in for `new DynamoDBClient(config)`
 * from `@aws-sdk/client-dynamodb`.
 */
export interface NimbusDynamoConfig {
  endpoint: string;
  region: string;
  credentials: {
    accessKeyId: string;
    secretAccessKey: string;
  };
}

const DEFAULT_HOST = "127.0.0.1";
const DEFAULT_PORT = 8000;
const DEFAULT_REGION = "us-east-1";
const DEFAULT_CREDENTIAL = "nimbus";

/**
 * Build the endpoint URL for a Nimbus DynamoDB listener. An explicit `endpoint`
 * wins; otherwise it is `http://<host>:<port>` with the local defaults.
 */
export function endpoint(options: NimbusDynamoOptions = {}): string {
  if (options.endpoint) {
    return options.endpoint;
  }
  const host = options.host ?? DEFAULT_HOST;
  const port = options.port ?? DEFAULT_PORT;
  return `http://${host}:${port}`;
}

/**
 * Build a `DynamoDBClient` configuration pointed at a Nimbus endpoint.
 *
 * The access key id selects the tenant — Nimbus binds each access key to a
 * tenant server-side, so two clients with different keys are isolated. The
 * secret is only checked when the server runs in strict SigV4 mode; in the
 * default lookup mode any non-empty secret is accepted.
 *
 * @example
 * ```ts
 * import { DynamoDBClient } from "@aws-sdk/client-dynamodb";
 * import { clientConfig } from "@nimbus/dynamodb";
 *
 * const client = new DynamoDBClient(clientConfig({ accessKeyId: "AKIAACME" }));
 * ```
 */
export function clientConfig(
  options: NimbusDynamoOptions = {},
): NimbusDynamoConfig {
  return {
    endpoint: endpoint(options),
    region: options.region ?? DEFAULT_REGION,
    credentials: {
      accessKeyId: options.accessKeyId ?? DEFAULT_CREDENTIAL,
      secretAccessKey: options.secretAccessKey ?? DEFAULT_CREDENTIAL,
    },
  };
}
