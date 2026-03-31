import { QUERY_STATE_MARKER } from "../constants.mjs";
import { unsupportedError } from "../errors.mjs";

import {
  collectConstraintFilters,
  createFilterExpressionBuilder,
  createIndexRangeBuilder,
  finalizeQueryState,
  lookupIndexFields,
} from "./query_filters.mjs";
import {
  normalizeLimit,
  normalizeOrderDirection,
  normalizeString,
} from "./shared.mjs";

function createQueryState(table) {
  return {
    table,
    filters: [],
    order: null,
    orderFieldHint: null,
  };
}

function createQueryBuilder(filePath, schema, state) {
  return Object.assign(
    {
      withIndex(indexName, builder) {
        const normalizedIndexName = normalizeString(indexName, filePath, "withIndex name");
        const indexFields = lookupIndexFields(schema, state.table, normalizedIndexName);
        const filters = builder
          ? collectConstraintFilters(
              builder,
              createIndexRangeBuilder(filePath),
              filePath,
              "withIndex",
            )
          : [];
        return createQueryBuilder(filePath, schema, {
          ...state,
          filters: [...state.filters, ...filters],
          orderFieldHint:
            state.orderFieldHint ??
            indexFields[0] ??
            filters[0]?.field ??
            null,
        });
      },
      filter(builder) {
        const filters = collectConstraintFilters(
          builder,
          createFilterExpressionBuilder(filePath),
          filePath,
          "filter",
        );
        return createQueryBuilder(filePath, schema, {
          ...state,
          filters: [...state.filters, ...filters],
          orderFieldHint: state.orderFieldHint ?? filters[0]?.field ?? null,
        });
      },
      order(direction) {
        const normalizedDirection = normalizeOrderDirection(direction, filePath);
        const orderField = state.orderFieldHint ?? state.filters[0]?.field ?? null;
        if (orderField === null) {
          throw unsupportedError(
            filePath,
            "ctx.db.query(...).order(...) requires withIndex(...) or filter(...) in 4B",
          );
        }
        return createQueryBuilder(filePath, schema, {
          ...state,
          order: {
            field: orderField,
            direction: normalizedDirection,
          },
        });
      },
      collect() {
        return finalizeQueryState(state, null);
      },
      take(limit) {
        return finalizeQueryState(state, normalizeLimit(limit, filePath));
      },
      first() {
        return {
          type: "first",
          query: finalizeQueryState(state, 1),
        };
      },
      unique() {
        return {
          type: "unique",
          query: finalizeQueryState(state, 2),
        };
      },
    },
    { [QUERY_STATE_MARKER]: state },
  );
}

export { createQueryBuilder, createQueryState };
