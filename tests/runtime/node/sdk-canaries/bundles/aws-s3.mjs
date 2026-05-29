import http from "node:http";
import { ListBucketsCommand, S3Client } from "@aws-sdk/client-s3";
import { NodeHttpHandler } from "@smithy/node-http-handler";

async function withServer(handler, callback) {
  const server = http.createServer(handler);
  await new Promise((resolve, reject) => {
    server.listen(0, "127.0.0.1", (error) => {
      if (error) reject(error);
      else resolve();
    });
  });
  const address = server.address();
  const port = typeof address === "object" && address ? address.port : null;
  try {
    return await callback(`http://127.0.0.1:${port}`);
  } finally {
    await new Promise((resolve, reject) => {
      server.close((error) => {
        if (error) reject(error);
        else resolve();
      });
    });
  }
}

globalThis.__nimbusInvoke = async function () {
  let requestPath = null;
  let authHeaderPresent = false;

  const output = await withServer((request, response) => {
    requestPath = request.url;
    authHeaderPresent = Boolean(request.headers.authorization);
    response.writeHead(200, { "content-type": "application/xml" });
    response.end(`<?xml version="1.0" encoding="UTF-8"?>
<ListAllMyBucketsResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Owner><ID>owner</ID><DisplayName>nimbus</DisplayName></Owner>
  <Buckets>
    <Bucket><Name>nimbus-canary</Name><CreationDate>2026-05-28T00:00:00.000Z</CreationDate></Bucket>
  </Buckets>
</ListAllMyBucketsResult>`);
  }, async (endpoint) => {
    const client = new S3Client({
      authSchemePreference: [],
      defaultUserAgentProvider: async () => [["nimbus-sdk-canary", "1"]],
      disableS3ExpressSessionAuth: true,
      defaultsMode: "standard",
      endpoint,
      forcePathStyle: true,
      maxAttempts: 1,
      region: "us-east-1",
      requestChecksumCalculation: "WHEN_SUPPORTED",
      responseChecksumValidation: "WHEN_SUPPORTED",
      retryMode: "standard",
      requestHandler: new NodeHttpHandler(),
      sigv4aSigningRegionSet: [],
      useArnRegion: false,
      useDualstackEndpoint: false,
      useFipsEndpoint: false,
      userAgentAppId: "nimbus",
      credentials: {
        accessKeyId: "AKIA_NIMBUS",
        secretAccessKey: "secret",
      },
    });
    return await client.send(new ListBucketsCommand({}));
  });

  return {
    bucketName: output.Buckets?.[0]?.Name ?? null,
    requestPath,
    authHeaderPresent,
  };
};

export {};
