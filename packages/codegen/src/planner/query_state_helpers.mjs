function lookupIndexFields(schema, tableName, indexName) {
  const tableSchema = schema.tables?.[tableName];
  if (!tableSchema) {
    return [];
  }
  const index = tableSchema.indexes.find((entry) => entry.name === indexName);
  if (!index) {
    throw new Error(`unknown index "${indexName}" on table "${tableName}"`);
  }
  return index.fields;
}

function finalizeQueryState(state, limit) {
  return {
    table: state.table,
    filters: state.filters,
    order: state.order,
    limit,
  };
}

export { finalizeQueryState, lookupIndexFields };
