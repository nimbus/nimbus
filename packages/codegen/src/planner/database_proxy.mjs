import { normalizeDocumentReference } from "./arg_validators.mjs";
import { createQueryBuilder, createQueryState } from "./query_builder.mjs";
import { recordOperation } from "./request_bindings.mjs";
import {
  normalizeRecord,
  normalizeTableName,
} from "./shared.mjs";

function createDatabaseProxy(filePath, schema, writable, operationLog, argsSchema) {
  const db = {
    query(tableName) {
      return createQueryBuilder(
        filePath,
        schema,
        createQueryState(normalizeTableName(tableName, filePath)),
      );
    },
    get(id) {
      const documentRef = normalizeDocumentReference(
        id,
        argsSchema,
        filePath,
        "ctx.db.get",
      );
      return recordOperation(operationLog, {
        type: "get",
        table: documentRef.table,
        id: documentRef.id,
      });
    },
  };

  if (writable) {
    db.insert = (tableName, fields) =>
      recordOperation(operationLog, {
        type: "insert",
        table: normalizeTableName(tableName, filePath),
        fields: normalizeRecord(fields, filePath, "ctx.db.insert"),
      });
    db.patch = (id, patch) => {
      const documentRef = normalizeDocumentReference(
        id,
        argsSchema,
        filePath,
        "ctx.db.patch",
      );
      return recordOperation(operationLog, {
        type: "update",
        table: documentRef.table,
        id: documentRef.id,
        patch: normalizeRecord(patch, filePath, "ctx.db.patch"),
      });
    };
    db.delete = (id) => {
      const documentRef = normalizeDocumentReference(
        id,
        argsSchema,
        filePath,
        "ctx.db.delete",
      );
      return recordOperation(operationLog, {
        type: "delete",
        table: documentRef.table,
        id: documentRef.id,
      });
    };
  }

  return db;
}

export { createDatabaseProxy };
