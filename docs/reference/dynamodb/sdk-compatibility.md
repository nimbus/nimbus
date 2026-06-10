---
title: DynamoDB SDK compatibility
description: Which AWS SDKs work against the Nimbus DynamoDB endpoint, and the verification status of each.
sidebar:
  order: 3
---

The Nimbus DynamoDB endpoint speaks the standard DynamoDB JSON wire
protocol, so official AWS SDKs connect with an endpoint override and no
Nimbus-specific client code.

## Connection requirements

| Requirement | Value |
| --- | --- |
| Protocol | DynamoDB JSON 1.0: `POST /` with an `X-Amz-Target` header and `application/x-amz-json-1.0` body |
| Endpoint | The SDK's endpoint override, pointed at the Nimbus listener (default `http://127.0.0.1:8000`) |
| Credentials | An access key ID registered on the server; each key is bound to one Nimbus tenant |
| Authentication | Strict SigV4 verification by default; an insecure lookup-only mode exists for loopback development |
| Region | Any region string; it participates in the SigV4 credential scope but selects nothing |
| Request body | Capped at 16 MiB; larger requests get `413 Payload Too Large` |
| Transport | Plain HTTP from the listener; terminate TLS in front of it for any non-loopback deployment |

## SDK matrix

| Client | Wiring | Verification status |
| --- | --- | --- |
| Rust `aws-sdk-dynamodb` | Endpoint override | **Verified.** A 27-scenario parity suite in the Nimbus repository drives the official SDK (pinned at 1.111.0) against the endpoint on every change, covering the full T0–T7 surface in both strict SigV4 and lookup auth modes. |
| Rust `aws-sdk-dynamodbstreams` | Endpoint override | **Verified.** The same suite exercises `ListStreams`, `DescribeStream`, `GetShardIterator`, and `GetRecords` (SDK pinned at 1.99.0). |
| JavaScript `@aws-sdk/client-dynamodb` (v3) | `clientConfig()` from `@nimbus/dynamodb`, or a plain endpoint override | Wired identically to the verified Rust path; not yet part of the continuously executed verification matrix. |
| Python `boto3` | `endpoint_url` override | Same wire protocol; not yet independently verified against Nimbus. |
| AWS CLI v2 | `--endpoint-url` | Same wire protocol; not yet independently verified against Nimbus. |
| Java `software.amazon.awssdk:dynamodb` (v2) | Endpoint override | Same wire protocol; not yet independently verified against Nimbus. |

The verified rows are executed claims: the official SDK marshals and
unmarshals against Nimbus exactly as it would against AWS, with no
Nimbus-specific shimming. The recorded
[divergences](/reference/dynamodb/divergences/) are semantic differences,
not wire-protocol differences — no SDK call fails to parse because of
them.

## Helper package

`@nimbus/dynamodb` is a small optional package, provisioned into a Nimbus
project with `nimbus packages provision dynamodb`:

| Export | Behavior |
| --- | --- |
| `endpoint(options?)` | Returns the listener URL. Defaults to `http://127.0.0.1:8000`; an explicit `endpoint` option wins over `host`/`port`. |
| `clientConfig(options?)` | Returns a drop-in configuration object for `new DynamoDBClient(...)`: the endpoint above, region (default `us-east-1`), and credentials (default access key ID and secret `nimbus`). The access key ID you pass selects the tenant the server has bound it to. |

## Auth modes as the SDK sees them

| Mode | Behavior toward the SDK |
| --- | --- |
| Strict (default) | The SDK's real SigV4 signature is verified against the registered secret. A wrong secret yields `InvalidSignatureException`; a timestamp outside the ±15-minute window is rejected; an unregistered access key yields `UnrecognizedClientException`. |
| Lookup-only (development) | The access key is resolved to its tenant and the signature is not verified. The server refuses to run this mode on a non-loopback address. |
