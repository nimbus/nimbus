// Shared browser-safe protobuf foundation for @nimbus/firebase transport work.
// The source proto tree is vendored under crates/nimbus-server/proto/google/...
// and regenerated with `npm run codegen:proto --workspace @nimbus/firebase`.
export { create, fromBinary, fromJson, toBinary, toJson } from "@bufbuild/protobuf";

export * as firestoreV1 from "../gen/google/firestore/v1/firestore_pb";
export * as firestoreDocumentV1 from "../gen/google/firestore/v1/document_pb";
export * as firestoreQueryV1 from "../gen/google/firestore/v1/query_pb";
export * as firestoreWriteV1 from "../gen/google/firestore/v1/write_pb";
export * as protobufTimestamp from "../gen/google/protobuf/timestamp_pb";
