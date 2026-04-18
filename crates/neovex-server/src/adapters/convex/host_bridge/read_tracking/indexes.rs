use super::*;

impl ConvexHostBridge {
    pub(in crate::adapters::convex) fn lookup_index_primary_field(
        &self,
        table: &TableName,
        index_name: &str,
    ) -> Result<Option<String>, Error> {
        let schema = self.service.get_table_schema(&self.tenant_id, table)?;
        Ok(schema
            .indexes
            .iter()
            .find(|index| index.name == index_name)
            .and_then(|index| index.single_field().map(|field| field.to_string())))
    }

    pub(in crate::adapters::convex) fn derive_index_read(
        &self,
        query: &Query,
        preferred_index_name: Option<&str>,
    ) -> Option<RuntimeIndexRangeRead> {
        let table_schema = self
            .service
            .get_table_schema(&self.tenant_id, &query.table)
            .ok()?;
        let index = if let Some(index_name) = preferred_index_name {
            table_schema
                .indexes
                .iter()
                .find(|index| index.name == index_name)
        } else {
            table_schema.indexes.iter().find(|index| {
                query.filters.iter().any(|filter| {
                    index.single_field() == Some(filter.field.as_str())
                        && is_scalar_filter_value(&filter.value)
                })
            })
        }?;
        let field = index.single_field()?.to_string();
        let mut start: Option<Value> = None;
        let mut end: Option<Value> = None;
        let mut start_inclusive = false;
        let mut end_inclusive = false;
        let mut has_bound = false;

        for filter in query.filters.iter().filter(|filter| filter.field == field) {
            match filter.op {
                FilterOp::Eq if is_scalar_filter_value(&filter.value) => {
                    start = Some(filter.value.clone());
                    end = Some(filter.value.clone());
                    start_inclusive = true;
                    end_inclusive = true;
                    has_bound = true;
                }
                FilterOp::Gt
                    if is_scalar_filter_value(&filter.value)
                        && should_replace_lower_bound(
                            start.as_ref(),
                            Some(&filter.value),
                            false,
                        ) =>
                {
                    start = Some(filter.value.clone());
                    start_inclusive = false;
                    has_bound = true;
                }
                FilterOp::Gte
                    if is_scalar_filter_value(&filter.value)
                        && should_replace_lower_bound(
                            start.as_ref(),
                            Some(&filter.value),
                            true,
                        ) =>
                {
                    start = Some(filter.value.clone());
                    start_inclusive = true;
                    has_bound = true;
                }
                FilterOp::Lt
                    if is_scalar_filter_value(&filter.value)
                        && should_replace_upper_bound(end.as_ref(), Some(&filter.value), false) =>
                {
                    end = Some(filter.value.clone());
                    end_inclusive = false;
                    has_bound = true;
                }
                FilterOp::Lte
                    if is_scalar_filter_value(&filter.value)
                        && should_replace_upper_bound(end.as_ref(), Some(&filter.value), true) =>
                {
                    end = Some(filter.value.clone());
                    end_inclusive = true;
                    has_bound = true;
                }
                _ => {}
            }
        }

        if !has_bound {
            return None;
        }

        Some(RuntimeIndexRangeRead {
            table: query.table.clone(),
            index_name: index.name.clone(),
            field,
            start,
            end,
            start_inclusive,
            end_inclusive,
        })
    }
}
