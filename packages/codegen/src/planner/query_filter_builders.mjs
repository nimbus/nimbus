import { unsupportedError } from "../errors.mjs";

import { normalizeString } from "./shared.mjs";

function createIndexRangeBuilder(filePath) {
  const filters = [];
  const builder = {
    eq(field, value) {
      filters.push(createFilter("eq", field, value, filePath));
      return builder;
    },
    neq(field, value) {
      filters.push(createFilter("neq", field, value, filePath));
      return builder;
    },
    gt(field, value) {
      filters.push(createFilter("gt", field, value, filePath));
      return builder;
    },
    gte(field, value) {
      filters.push(createFilter("gte", field, value, filePath));
      return builder;
    },
    lt(field, value) {
      filters.push(createFilter("lt", field, value, filePath));
      return builder;
    },
    lte(field, value) {
      filters.push(createFilter("lte", field, value, filePath));
      return builder;
    },
  };
  return Object.assign(builder, { __filters: filters });
}

function createFilterExpressionBuilder(filePath) {
  const filters = [];
  const builder = {
    field(name) {
      return {
        __fieldName: normalizeString(name, filePath, "field path"),
      };
    },
    eq(field, value) {
      filters.push(createFilter("eq", field, value, filePath));
      return builder;
    },
    neq(field, value) {
      filters.push(createFilter("neq", field, value, filePath));
      return builder;
    },
    gt(field, value) {
      filters.push(createFilter("gt", field, value, filePath));
      return builder;
    },
    gte(field, value) {
      filters.push(createFilter("gte", field, value, filePath));
      return builder;
    },
    lt(field, value) {
      filters.push(createFilter("lt", field, value, filePath));
      return builder;
    },
    lte(field, value) {
      filters.push(createFilter("lte", field, value, filePath));
      return builder;
    },
  };
  return Object.assign(builder, { __filters: filters });
}

function collectConstraintFilters(builderFn, builder, filePath, label) {
  const result = builderFn(builder);
  if (result !== undefined && result !== builder && result?.__filters !== builder.__filters) {
    throw unsupportedError(filePath, `ctx.db.${label}(...) must return the provided builder`);
  }
  return [...builder.__filters];
}

function createFilter(op, field, value, filePath) {
  return {
    field: normalizeFieldName(field, filePath),
    op,
    value,
  };
}

function normalizeFieldName(field, filePath) {
  if (typeof field === "string") {
    return normalizeString(field, filePath, "field name");
  }
  if (
    field &&
    typeof field === "object" &&
    typeof field.__fieldName === "string"
  ) {
    return field.__fieldName;
  }
  throw unsupportedError(filePath, "filter fields must be string literals or q.field(...)");
}

export {
  collectConstraintFilters,
  createFilterExpressionBuilder,
  createIndexRangeBuilder,
};
