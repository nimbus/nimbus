import { MongoClient } from "mongodb";
import { uri } from "@neovex/mongodb";

declare const process: {
  env: Record<string, string | undefined>;
  exit(code: number): never;
};

const host = process.env.NEOVEX_MONGODB_HOST ?? "127.0.0.1";
const port = process.env.NEOVEX_MONGODB_PORT
  ? Number(process.env.NEOVEX_MONGODB_PORT)
  : 27017;

async function main() {
  console.log(`Connecting to Neovex MongoDB at ${host}:${port}...`);

  const client = new MongoClient(uri({ host, port }));
  await client.connect();

  try {
    const db = client.db("demo");
    const messages = db.collection("messages");

    // Insert a document.
    const insertResult = await messages.insertOne({
      author: "Node Demo",
      body: `Hello from Node at ${new Date().toISOString()}`,
      createdAt: new Date(),
    });
    console.log("Inserted document:", insertResult.insertedId.toString());

    // Query all documents.
    const allMessages = await messages.find().toArray();
    console.log("All messages after insert:", allMessages);

    // Update the document we just inserted.
    const updateResult = await messages.updateOne(
      { _id: insertResult.insertedId },
      { $set: { body: "Updated message from Node Demo" } },
    );
    console.log("Updated documents:", updateResult.modifiedCount);

    // Query again to show the update.
    const updated = await messages.find().toArray();
    console.log("All messages after update:", updated);
  } finally {
    await client.close();
    console.log("Connection closed.");
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
