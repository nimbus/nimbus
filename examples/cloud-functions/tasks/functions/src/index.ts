import { initializeApp } from "firebase-admin/app";
import { getFirestore } from "firebase-admin/firestore";
import { onDocumentCreated } from "firebase-functions/v2/firestore";
import { onRequest } from "firebase-functions/v2/https";

initializeApp();

const firestore = getFirestore();

export const taskDetails = onRequest(async (req, res) => {
  const rawTaskId = req.query.taskId;
  const taskId = Array.isArray(rawTaskId) ? rawTaskId[0] : rawTaskId;
  if (typeof taskId !== "string" || taskId.length === 0) {
    res.status(400).json({ error: "taskId query parameter is required" });
    return;
  }

  const snapshot = await firestore.collection("tasks").doc(taskId).get();
  if (!snapshot.exists) {
    res.status(404).json({ error: "task not found", taskId });
    return;
  }

  const derivation = await firestore.collection("taskDerivations").doc(taskId).get();
  res.json({
    task: { id: snapshot.id, ...snapshot.data() },
    derivation: derivation.exists ? derivation.data() : null,
  });
});

export const deriveTask = onDocumentCreated({
  document: "tasks/{taskId}",
  retry: true,
}, async (event) => {
  const task = event.data?.data();
  if (!task) {
    return;
  }

  // At-least-once redelivery overwrites the same source-keyed document with
  // the same values, so retries cannot double-count or duplicate the effect.
  await firestore.collection("taskDerivations").doc(event.params.taskId).set({
    sourceTaskId: event.params.taskId,
    textLength: typeof task.text === "string" ? task.text.length : 0,
    completedAtCreation: task.completed === true,
    sourceCreatedAt: task.createdAt,
  });
});
