import { MongoClient, type MongoClientOptions } from "mongodb";

import {
  buildConnectionString,
  type ConnectionStringOptions,
} from "./connection-string.ts";

export interface NeovexMongoOptions extends ConnectionStringOptions {
  clientOptions?: MongoClientOptions;
}

export async function connectNeovex(
  options: NeovexMongoOptions = {},
): Promise<MongoClient> {
  const uri = buildConnectionString(options);
  const client = new MongoClient(uri, options.clientOptions);
  await client.connect();
  return client;
}
