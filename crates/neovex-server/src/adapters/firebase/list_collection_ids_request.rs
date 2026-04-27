use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedListCollectionIdsRequest {
    pub page_size: Option<usize>,
    pub page_offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PaginatedCollectionIds {
    pub collection_ids: Vec<String>,
    pub next_page_token: String,
}

#[derive(Debug, Error)]
pub(crate) enum FirestoreListCollectionIdsRequestError {
    #[error("invalid Firestore ListCollectionIds request: {0}")]
    InvalidRequest(String),
    #[error("unsupported Firestore ListCollectionIds feature: {0}")]
    Unsupported(String),
}

pub(crate) fn parse_list_collection_ids_request(
    request: &Value,
) -> Result<ParsedListCollectionIdsRequest, FirestoreListCollectionIdsRequestError> {
    let request: ListCollectionIdsRequestJson = serde_json::from_value(request.clone())
        .map_err(|error| invalid_request(format!("malformed JSON body: {error}")))?;
    let page_size = parse_page_size(request.page_size)?;
    let page_offset = decode_page_token(request.page_token.as_deref().unwrap_or_default())?;
    if request.read_time.is_some() {
        return Err(unsupported_request("`readTime`"));
    }

    Ok(ParsedListCollectionIdsRequest {
        page_size,
        page_offset,
    })
}

pub(crate) fn paginate_collection_ids(
    mut collection_ids: Vec<String>,
    request: &ParsedListCollectionIdsRequest,
) -> Result<PaginatedCollectionIds, FirestoreListCollectionIdsRequestError> {
    collection_ids.sort();
    collection_ids.dedup();

    if request.page_offset > collection_ids.len() {
        return Err(invalid_request(
            "`pageToken` does not point at a valid collection-id page boundary",
        ));
    }

    let remaining = &collection_ids[request.page_offset..];
    let page_len = request.page_size.unwrap_or(remaining.len());
    let page_len = page_len.min(remaining.len());
    let page_end = request.page_offset + page_len;
    let next_page_token = if page_end < collection_ids.len() {
        encode_page_token(page_end)
    } else {
        String::new()
    };

    Ok(PaginatedCollectionIds {
        collection_ids: remaining[..page_len].to_vec(),
        next_page_token,
    })
}

pub(crate) fn parse_list_collection_ids_page_token(
    page_token: &str,
) -> Result<usize, FirestoreListCollectionIdsRequestError> {
    decode_page_token(page_token)
}

fn parse_page_size(
    page_size: Option<i32>,
) -> Result<Option<usize>, FirestoreListCollectionIdsRequestError> {
    let Some(page_size) = page_size else {
        return Ok(None);
    };
    if page_size < 0 {
        return Err(invalid_request("`pageSize` must not be negative"));
    }
    if page_size == 0 {
        return Ok(None);
    }
    usize::try_from(page_size)
        .map(Some)
        .map_err(|_| invalid_request("`pageSize` exceeds supported range"))
}

fn decode_page_token(page_token: &str) -> Result<usize, FirestoreListCollectionIdsRequestError> {
    if page_token.is_empty() {
        return Ok(0);
    }
    let decoded = URL_SAFE_NO_PAD
        .decode(page_token)
        .map_err(|error| invalid_request(format!("invalid `pageToken`: {error}")))?;
    let decoded = String::from_utf8(decoded)
        .map_err(|error| invalid_request(format!("invalid `pageToken`: {error}")))?;
    decoded
        .parse::<usize>()
        .map_err(|error| invalid_request(format!("invalid `pageToken`: {error}")))
}

fn encode_page_token(page_offset: usize) -> String {
    URL_SAFE_NO_PAD.encode(page_offset.to_string().as_bytes())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListCollectionIdsRequestJson {
    page_size: Option<i32>,
    page_token: Option<String>,
    read_time: Option<String>,
}

fn invalid_request(reason: impl Into<String>) -> FirestoreListCollectionIdsRequestError {
    FirestoreListCollectionIdsRequestError::InvalidRequest(reason.into())
}

fn unsupported_request(feature: impl Into<String>) -> FirestoreListCollectionIdsRequestError {
    FirestoreListCollectionIdsRequestError::Unsupported(feature.into())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        paginate_collection_ids, parse_list_collection_ids_page_token,
        parse_list_collection_ids_request,
    };

    #[test]
    fn parses_paging_options_and_roundtrips_page_tokens() {
        let parsed = parse_list_collection_ids_request(&json!({
            "pageSize": 2,
            "pageToken": "Mg"
        }))
        .expect("request should parse");

        assert_eq!(parsed.page_size, Some(2));
        assert_eq!(parsed.page_offset, 2);
        assert_eq!(
            parse_list_collection_ids_page_token("Mg").expect("page token should decode"),
            2
        );
    }

    #[test]
    fn paginates_collection_ids_in_sorted_deduped_order() {
        let request = parse_list_collection_ids_request(&json!({
            "pageSize": 2
        }))
        .expect("request should parse");

        let page = paginate_collection_ids(
            vec![
                "regions".to_string(),
                "cities".to_string(),
                "countries".to_string(),
                "cities".to_string(),
            ],
            &request,
        )
        .expect("pagination should succeed");

        assert_eq!(page.collection_ids, vec!["cities", "countries"]);
        assert_eq!(page.next_page_token, "Mg");
    }

    #[test]
    fn rejects_invalid_tokens_and_read_time() {
        assert!(
            parse_list_collection_ids_request(&json!({
                "pageToken": "not-base64!"
            }))
            .is_err()
        );
        assert!(
            parse_list_collection_ids_request(&json!({
                "readTime": "2024-01-01T00:00:00Z"
            }))
            .is_err()
        );
    }
}
