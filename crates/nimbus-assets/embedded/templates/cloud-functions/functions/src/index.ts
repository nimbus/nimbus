import { onRequest } from "firebase-functions/v2/https";
import { onDocumentCreated } from "firebase-functions/v2/firestore";

export const hello = onRequest(async (req, res) => {
  res.json({ message: "Hello from Nimbus Cloud Functions!" });
});

export const onMessageCreated = onDocumentCreated(
  "messages/{messageId}",
  async (event) => {
    console.log("New message:", event.data?.data());
  },
);
