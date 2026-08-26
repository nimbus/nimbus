---
title: S3 compatibility
description: The object and multipart operation surface of the Nimbus S3 wire adapter, its SigV4 authentication, tenant model, and Convex file-storage face.
sidebar:
  order: 1
---

Served by default on `127.0.0.1:9000` when the port is free; opt out with
`--no-s3`; per-tenant signed access keys with SigV4 verification. The
adapter speaks the S3 REST wire protocol through `s3s`, so official S3
SDKs and tools connect with an endpoint override and no Nimbus-specific
client code. Anything not listed on this page should be assumed
unsupported.

## Maturity

The S3 adapter is default-on but bounded. It implements object-level
reads and writes, listing, and multipart upload against implicit,
per-tenant buckets. It does not implement bucket lifecycle, object
copying, batch delete, tagging, ACLs, or versioning. Object integrity
covers CRC64NVME and Content-MD5 only. See [Not
implemented](#not-implemented) for the full exclusion list.

## Enablement and binding

The listener is served by default. It binds `127.0.0.1:9000` when that
port is free.

| Behavior | Rule |
| --- | --- |
| Default bind | `127.0.0.1:9000` |
| Port busy, no `--s3-port` | The listener is skipped with a startup warning; the rest of the server starts |
| `--s3-port <port>` on a busy port | Hard startup error |
| `--no-s3` | Disables the listener |
| `--no-s3` with `--s3-port` or `--s3-access-key` | Rejected as a conflicting configuration |

### Flags

| Flag | Default | What it does |
| --- | --- | --- |
| `--no-s3` | listener on | Disables the S3 listener. |
| `--s3-port <port>` | `9000` when free | Pins an explicit port. A busy explicit port is a hard error instead of a skip. |
| `--s3-host <host>` | `127.0.0.1` | Host interface. A non-loopback host requires `--allow-network`. |
| `--s3-access-key KEY_ID:SECRET:TENANT` | generated key | Signed access-key binding (repeatable). Also read from `NIMBUS_S3_ACCESS_KEYS` (comma-separated) when no flag is given. |

The S3 adapter uses its own credential registry. It never reuses the
DynamoDB or RESP KV credentials. With no bindings supplied, a generated
wire-credential key is bound to the `default` tenant and persisted with
the other generated wire credentials in the data directory. Supplying
one or more bindings replaces the generated key entirely. A binding whose
tenant begins with the reserved `_nimbus` prefix is rejected at startup.
The listener refuses to start with an empty access-key registry.

## Authentication

Every request must be signed with AWS Signature Version 4. Signature
verification, canonicalization, and REST/XML response shaping are handled
by `s3s`.

| Property | Behavior |
| --- | --- |
| Signature | SigV4 verified against the registered secret for the request's access key ID |
| Unsigned request | Rejected with `AccessDenied` |
| Unknown access key ID | Rejected with `InvalidAccessKeyId` |
| Presigned URLs | SigV4 query-parameter presigned `GET` URLs are accepted |
| Region | Any region string; it is part of the SigV4 credential scope and selects nothing |
| Access key → tenant | Each access key ID resolves to exactly one tenant; the request operates only in that tenant's object store |

The listener serves plain HTTP; a non-loopback deployment needs TLS
terminated in front of it.

## Object operations

| Operation | Request surface | Response surface | Notes |
| --- | --- | --- | --- |
| `PutObject` | body, `Content-Type`, user metadata (`x-amz-meta-*`), `Content-MD5`, `x-amz-checksum-crc64nvme`, `Content-Length`, `If-Match`, `If-None-Match` | `ETag`, `x-amz-checksum-crc64nvme`, size | Content-MD5 and CRC64NVME are verified against the received bytes; `Content-Length`, if sent, must match. Preconditions are checked against the current object |
| `GetObject` | `Range`, `If-Match`, `If-None-Match`, `If-Modified-Since`, `If-Unmodified-Since` | body, `ETag`, `Content-Type`, `Last-Modified`, user metadata, `Accept-Ranges` | A `Range` request returns `206 Partial Content` with `Content-Range`. `If-None-Match`/`If-Modified-Since` yield `304 Not Modified`; a failed `If-Match`/`If-Unmodified-Since` yields `412 Precondition Failed` |
| `HeadObject` | `Range`, same preconditions as `GetObject` | headers only: `Content-Length`, `ETag`, `Content-Type`, `Last-Modified`, user metadata | `Range` reports the selected length and `Content-Range` |
| `DeleteObject` | bucket, key, `If-Match`, `x-amz-if-match-size`, `x-amz-if-match-last-modified-time` | empty | A condition mismatch on an existing object yields `412 Precondition Failed`. The conditions are checked atomically with deletion. Deleting an absent key still succeeds |
| `ListObjectsV2` | `prefix`, `delimiter`, `max-keys`, `continuation-token`, `start-after` | `Contents`, `CommonPrefixes`, `KeyCount`, `IsTruncated`, `NextContinuationToken` | `max-keys` defaults to 1000 and is clamped to 1000. `delimiter` rolls up `CommonPrefixes` |

Buckets are implicit. There is no bucket lifecycle operation: a
`PutObject` writes an object under its bucket and key directly, and the
bucket name is a namespace within the tenant.

## Multipart upload

| Operation | Request surface | Response surface | Notes |
| --- | --- | --- | --- |
| `CreateMultipartUpload` | bucket, key, `Content-Type`, user metadata | `UploadId` | |
| `UploadPart` | `UploadId`, `PartNumber`, body, `Content-MD5`, `x-amz-checksum-crc64nvme` | `ETag`, `x-amz-checksum-crc64nvme` | `PartNumber` must be 1–10000. Re-uploading a part number replaces it |
| `CompleteMultipartUpload` | `UploadId`, ordered `Part` list with `PartNumber` and optional `ETag`, optional full-object `x-amz-checksum-crc64nvme` | `ETag`, `Location`, verified CRC64NVME when supplied | Parts must be strictly ascending; a supplied per-part `ETag` must match; completion requires at least one part. A supplied full-object CRC64NVME is recomputed across the selected parts and a mismatch is rejected |
| `AbortMultipartUpload` | `UploadId` | empty | Releases the uploaded part bytes |

The completed-object `ETag` is the standard multipart form: the MD5 of the
concatenated part MD5s, suffixed with the part count.

## Integrity and checksums

| Checksum | Behavior |
| --- | --- |
| `Content-MD5` | Verified against the received bytes; a mismatch is rejected |
| CRC64NVME (`x-amz-checksum-crc64nvme`, header or trailer) | Verified against the received bytes; returned on `PutObject`, `UploadPart`, `GetObject`, and `HeadObject` |
| CRC32, CRC32C, SHA1, SHA256 | Rejected — a request carrying one of these checksum headers, or `x-amz-sdk-checksum-algorithm` naming one, fails with an error naming the algorithm |

This rejection is deliberate. Nimbus does not accept a checksum that it
cannot verify. Configure clients that add CRC32 or CRC32C automatically to
use CRC64NVME or Content-MD5 for this endpoint.

Whole-object `ETag` values are the MD5 of the object bytes. Object bytes
are stored in the tenant's content-addressed blob plane; object metadata
(bucket, key, size, `ETag`, checksums, content type, user metadata) is
stored in the tenant's metadata plane through the engine.

## Tenant model

Each signed access key is bound to exactly one tenant. A request operates
only within that tenant's object namespace: buckets and keys created under
one access key are not visible to a key bound to a different tenant. A
tenant's object store is created on first use — the first `PutObject` or
`CreateMultipartUpload` ensures the tenant exists; a read against a tenant
that was never created fails closed.

## Convex file storage

The same crate implements the Convex file-storage surface over the same
byte and metadata planes. Convex exposes opaque `_storage` document ids
rather than bucket and key names, backed by the reserved `_storage`
bucket. Downloads are served from the `/_nimbus/convex/storage/` path.

| Property | Behavior |
| --- | --- |
| Storage ids | Opaque `_storage`-scoped document ids |
| Download path | `/_nimbus/convex/storage/{token}` |
| Download token | HMAC-SHA256 over a versioned payload of tenant, storage id, and expiry; verified with a constant-time comparison |
| Token expiry | An expired token is rejected; a missing object returns `404`, an invalid or expired token returns `403` |
| Export/import | Objects export to, and import from, a zip archive with a `_storage/documents.jsonl` manifest |

The download route is mounted only when the listener runs with the
generated default wire credential — the CLI wires that generated secret as
the Convex download-signing secret. Supplying explicit `--s3-access-key`
or `NIMBUS_S3_ACCESS_KEYS` bindings does not configure a Convex download
secret, and the download route is not mounted in that configuration.

## Not implemented

The following S3 surfaces are not served. A request for one is rejected,
not silently accepted.

- Bucket lifecycle: `CreateBucket`, `DeleteBucket`, `ListBuckets`, `HeadBucket`.
- `ListObjects` (v1) and batch `DeleteObjects`.
- `CopyObject` and `UploadPartCopy`.
- `ListMultipartUploads` and `ListParts`.
- Object, bucket, and part tagging; ACLs; bucket policies.
- Versioning, object lock, and lifecycle configuration.
- `GetObjectAttributes`.
- Checksum algorithms other than CRC64NVME and Content-MD5.
