import {
  DeleteObjectCommand,
  GetObjectCommand,
  ListObjectsV2Command,
  PutObjectCommand,
  S3Client,
} from "@aws-sdk/client-s3";

const endpoint = process.env.NIMBUS_S3_ENDPOINT ?? "http://127.0.0.1:19001";
const accessKeyId = process.env.NIMBUS_S3_ACCESS_KEY;
const secretAccessKey = process.env.NIMBUS_S3_SECRET_KEY;
const phase = process.env.NIMBUS_OBJECT_SMOKE_PHASE ?? "seed";
const bucket = "release-storage";
const objects = new Map([
  ["recovery/alpha.txt", "nimbus-object-recovery-alpha"],
  ["recovery/beta.txt", "nimbus-object-recovery-beta"],
]);

if (!accessKeyId || !secretAccessKey) {
  throw new Error("NIMBUS_S3_ACCESS_KEY and NIMBUS_S3_SECRET_KEY are required");
}

const client = new S3Client({
  endpoint,
  region: "us-east-1",
  forcePathStyle: true,
  requestChecksumCalculation: "WHEN_REQUIRED",
  responseChecksumValidation: "WHEN_REQUIRED",
  credentials: { accessKeyId, secretAccessKey },
});

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

async function readObject(key) {
  const result = await client.send(new GetObjectCommand({ Bucket: bucket, Key: key }));
  return result.Body.transformToString();
}

async function verifyObjects(anchor) {
  for (const [key, expected] of objects) {
    assert((await readObject(key)) === expected, `${key} must retain its bytes`);
  }
  const listed = await client.send(
    new ListObjectsV2Command({ Bucket: bucket, Prefix: "recovery/" }),
  );
  assert(listed.KeyCount === objects.size, "object list must contain both recovery objects");
  assert(
    listed.Contents?.map((entry) => entry.Key).join(",") === [...objects.keys()].join(","),
    "object list must retain stable key order",
  );
  console.log(`PASS ${anchor}`);
}

try {
  if (phase === "seed") {
    for (const [key, body] of objects) {
      await client.send(
        new PutObjectCommand({
          Bucket: bucket,
          Key: key,
          Body: body,
          ContentType: "text/plain",
          IfNoneMatch: "*",
        }),
      );
    }
    await verifyObjects("object-seed-read-list");
  } else if (phase === "verify") {
    await verifyObjects("object-restore-read-list");
  } else if (phase === "cleanup") {
    for (const key of objects.keys()) {
      await client.send(new DeleteObjectCommand({ Bucket: bucket, Key: key }));
    }
    const listed = await client.send(
      new ListObjectsV2Command({ Bucket: bucket, Prefix: "recovery/" }),
    );
    assert(listed.KeyCount === 0, "cleanup must remove all recovery objects");
    console.log("PASS object-delete-cleanup");
  } else {
    throw new Error(`unknown NIMBUS_OBJECT_SMOKE_PHASE: ${phase}`);
  }
} finally {
  client.destroy();
}
