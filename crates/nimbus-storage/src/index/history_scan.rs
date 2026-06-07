use nimbus_core::{
    Document, Error, HistoricalIndexCursor, HistoricalIndexQuery, HistoricalIndexScalar,
    HistoricalIndexTuple, HistoricalReadShape, IndexDefinition, Result,
};
use serde_json::Value;

use crate::keys::prefix_end;
use crate::store::HistoricalIndexDocumentPage;

use super::{encode_index_tuple, encode_index_value};

#[derive(Debug, Clone)]
pub(crate) struct HistoricalIndexScanPlan {
    pub index: IndexDefinition,
    pub query: HistoricalIndexQuery,
    pub match_prefix: Vec<u8>,
    pub start_key: Option<Vec<u8>>,
    pub end_key: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub(crate) struct HistoricalIndexDocumentEntry {
    pub tuple: HistoricalIndexTuple,
    pub document: Document,
}

impl HistoricalIndexScanPlan {
    pub fn equal(
        read_shape: &HistoricalReadShape,
        index_name: &str,
        value: &Value,
    ) -> Result<Self> {
        let index = queryable_historical_index(read_shape, index_name)?;
        let encoded = encode_index_value(value)?;
        let query = HistoricalIndexQuery::Equal(HistoricalIndexTuple::from_values(
            std::slice::from_ref(value),
        )?);
        Ok(Self {
            index,
            query,
            match_prefix: encoded.clone(),
            start_key: Some(encoded.clone()),
            end_key: prefix_end(&encoded),
        })
    }

    pub fn prefix(
        read_shape: &HistoricalReadShape,
        index_name: &str,
        prefix_values: &[Value],
    ) -> Result<Self> {
        let index = queryable_historical_index(read_shape, index_name)?;
        let encoded_prefix = encode_index_tuple(prefix_values)?;
        let prefix = prefix_values
            .iter()
            .map(HistoricalIndexScalar::from_json)
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            index,
            query: HistoricalIndexQuery::Prefix(prefix),
            match_prefix: encoded_prefix.clone(),
            start_key: Some(encoded_prefix.clone()),
            end_key: prefix_end(&encoded_prefix),
        })
    }

    pub fn range(
        read_shape: &HistoricalReadShape,
        index_name: &str,
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
    ) -> Result<Self> {
        let index = queryable_historical_index(read_shape, index_name)?;
        let start_encoded = start.map(encode_index_value).transpose()?;
        let end_encoded = end.map(encode_index_value).transpose()?;
        Ok(Self {
            index,
            query: historical_range_query(start, end, start_inclusive, end_inclusive)?,
            match_prefix: Vec::new(),
            start_key: historical_range_start_key(start_encoded.as_deref(), start_inclusive),
            end_key: historical_range_end_key(end_encoded.as_deref(), end_inclusive),
        })
    }

    pub fn composite_range(
        read_shape: &HistoricalReadShape,
        index_name: &str,
        exact_prefix: &[Value],
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
    ) -> Result<Self> {
        let index = queryable_historical_index(read_shape, index_name)?;
        let encoded_prefix = encode_index_tuple(exact_prefix)?;
        let start_key = historical_composite_start_key(&encoded_prefix, start, start_inclusive)?;
        Ok(Self {
            index,
            query: historical_composite_range_query(
                exact_prefix,
                start,
                end,
                start_inclusive,
                end_inclusive,
            )?,
            match_prefix: encoded_prefix.clone(),
            start_key: Some(start_key),
            end_key: historical_composite_end_key(&encoded_prefix, end, end_inclusive)?,
        })
    }

    pub fn validate_page_request(
        &self,
        read_shape: &HistoricalReadShape,
        after: Option<&HistoricalIndexCursor>,
        limit: usize,
    ) -> Result<()> {
        if limit == 0 {
            return Err(Error::InvalidInput(
                "historical index page limit must be greater than zero".to_string(),
            ));
        }
        if let Some(cursor) = after {
            cursor.validate_context(read_shape, &self.index, &self.query)?;
        }
        Ok(())
    }
}

pub(crate) fn finish_historical_index_page(
    read_shape: &HistoricalReadShape,
    plan: &HistoricalIndexScanPlan,
    after: Option<&HistoricalIndexCursor>,
    limit: usize,
    mut entries: Vec<HistoricalIndexDocumentEntry>,
) -> Result<HistoricalIndexDocumentPage> {
    plan.validate_page_request(read_shape, after, limit)?;
    entries.sort_by(|left, right| {
        left.tuple
            .cmp(&right.tuple)
            .then_with(|| left.document.id.cmp(&right.document.id))
    });
    let start = after
        .and_then(|cursor| {
            entries.iter().position(|entry| {
                &entry.tuple == cursor.last_tuple()
                    && &entry.document.id == cursor.last_document_id()
            })
        })
        .map_or(0, |position| position.saturating_add(1));
    let selected = entries
        .into_iter()
        .skip(start)
        .take(limit)
        .collect::<Vec<_>>();
    let next_cursor = if selected.len() == limit {
        selected.last().map(|entry| {
            HistoricalIndexCursor::new(
                read_shape,
                &plan.index,
                plan.query.clone(),
                entry.tuple.clone(),
                entry.document.id.clone(),
            )
        })
    } else {
        None
    };
    Ok(HistoricalIndexDocumentPage {
        documents: selected.into_iter().map(|entry| entry.document).collect(),
        next_cursor,
    })
}

fn queryable_historical_index(
    read_shape: &HistoricalReadShape,
    index_name: &str,
) -> Result<IndexDefinition> {
    read_shape
        .queryable_indexes()
        .iter()
        .find(|index| index.name == index_name)
        .cloned()
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "enabled historical index not found for table {}: {}",
                read_shape.table(),
                index_name
            ))
        })
}

fn historical_range_start_key(start: Option<&[u8]>, start_inclusive: bool) -> Option<Vec<u8>> {
    let start = start?;
    if start_inclusive {
        Some(start.to_vec())
    } else {
        prefix_end(start).or_else(|| Some(Vec::new()))
    }
}

fn historical_range_end_key(end: Option<&[u8]>, end_inclusive: bool) -> Option<Vec<u8>> {
    let end = end?;
    if end_inclusive {
        prefix_end(end)
    } else {
        Some(end.to_vec())
    }
}

fn historical_composite_start_key(
    exact_prefix: &[u8],
    start: Option<&Value>,
    start_inclusive: bool,
) -> Result<Vec<u8>> {
    let Some(start) = start else {
        return Ok(exact_prefix.to_vec());
    };
    let mut key = exact_prefix.to_vec();
    key.extend_from_slice(&encode_index_value(start)?);
    if start_inclusive {
        Ok(key)
    } else {
        Ok(prefix_end(&key).unwrap_or_default())
    }
}

fn historical_composite_end_key(
    exact_prefix: &[u8],
    end: Option<&Value>,
    end_inclusive: bool,
) -> Result<Option<Vec<u8>>> {
    let Some(end) = end else {
        return Ok(prefix_end(exact_prefix));
    };
    let mut key = exact_prefix.to_vec();
    key.extend_from_slice(&encode_index_value(end)?);
    if end_inclusive {
        Ok(prefix_end(&key))
    } else {
        Ok(Some(key))
    }
}

fn historical_range_query(
    start: Option<&Value>,
    end: Option<&Value>,
    start_inclusive: bool,
    end_inclusive: bool,
) -> Result<HistoricalIndexQuery> {
    Ok(HistoricalIndexQuery::Range {
        start: start
            .map(|value| HistoricalIndexTuple::from_values(std::slice::from_ref(value)))
            .transpose()?,
        start_inclusive,
        end: end
            .map(|value| HistoricalIndexTuple::from_values(std::slice::from_ref(value)))
            .transpose()?,
        end_inclusive,
    })
}

fn historical_composite_range_query(
    exact_prefix: &[Value],
    start: Option<&Value>,
    end: Option<&Value>,
    start_inclusive: bool,
    end_inclusive: bool,
) -> Result<HistoricalIndexQuery> {
    if start.is_none() && end.is_none() {
        return Ok(HistoricalIndexQuery::Prefix(
            exact_prefix
                .iter()
                .map(HistoricalIndexScalar::from_json)
                .collect::<Result<Vec<_>>>()?,
        ));
    }
    Ok(HistoricalIndexQuery::Range {
        start: composite_bound_tuple(exact_prefix, start)?,
        start_inclusive,
        end: composite_bound_tuple(exact_prefix, end)?,
        end_inclusive,
    })
}

fn composite_bound_tuple(
    exact_prefix: &[Value],
    bound: Option<&Value>,
) -> Result<Option<HistoricalIndexTuple>> {
    if exact_prefix.is_empty() && bound.is_none() {
        return Ok(None);
    }
    let mut values = exact_prefix.to_vec();
    if let Some(bound) = bound {
        values.push(bound.clone());
    }
    HistoricalIndexTuple::from_values(&values).map(Some)
}
