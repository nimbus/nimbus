// Shared browser-safe protobuf foundation for the firebase package's transport.
// The source proto tree is vendored under crates/nimbus-firebase/proto/google/...
// and regenerated with `npm run codegen:proto --workspace firebase`.
export { create, fromBinary, fromJson, toBinary, toJson } from "@bufbuild/protobuf";

export * as firestoreV1 from "../gen/google/firestore/v1/firestore_pb.ts";
export * as firestoreDocumentV1 from "../gen/google/firestore/v1/document_pb.ts";
export * as firestoreQueryV1 from "../gen/google/firestore/v1/query_pb.ts";
export * as firestoreWriteV1 from "../gen/google/firestore/v1/write_pb.ts";
export * as protobufTimestamp from "../gen/google/protobuf/timestamp_pb.ts";
