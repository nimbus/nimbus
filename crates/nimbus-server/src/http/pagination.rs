//! Page-token pagination shared by the session, service definition, and sandbox
//! resource list routes: clamp the requested limit, truncate to the page, and
//! derive the next page token from the last retained item's key.

use serde::Serialize;

const DEFAULT_PAGE_LIMIT: usize = 100;
const MAX_PAGE_LIMIT: usize = 100;

/// Collection-level pagination metadata, identical across every resource list
/// response regardless of item type.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::http) struct CollectionMetadataResponse {
    pub(in crate::http) tenant_id: String,
    pub(in crate::http) resource_version: String,
    pub(in crate::http) limit: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(in crate::http) next_page_token: Option<String>,
    pub(in crate::http) remaining_count: usize,
}

/// Limit, next-page-token, and remaining-count for a single page, before the
/// caller adds the resource-specific `tenant_id` and `resource_version`.
pub(in crate::http) struct PageMeta {
    pub(in crate::http) limit: usize,
    pub(in crate::http) next_page_token: Option<String>,
    pub(in crate::http) remaining_count: usize,
}

/// Applies page-token filtering to `items` (already sorted and domain-filtered
/// by the caller in the same order as `key`), clamps `limit` to
/// `[1, MAX_PAGE_LIMIT]`, and truncates to that page.
///
/// `key` must extract the same field `items` is sorted by, since it is used
/// both to re-apply the page-token filter and to derive the next page token.
pub(in crate::http) fn paginate_by_key<T>(
    mut items: Vec<T>,
    page_token: Option<&str>,
    limit: Option<usize>,
    key: impl Fn(&T) -> &str,
) -> (Vec<T>, PageMeta) {
    if let Some(token) = page_token {
        items.retain(|item| key(item) > token);
    }
    let limit = limit.unwrap_or(DEFAULT_PAGE_LIMIT).clamp(1, MAX_PAGE_LIMIT);
    let remaining_count = items.len().saturating_sub(limit);
    let next_page_token = if remaining_count > 0 {
        items
            .get(limit.saturating_sub(1))
            .map(|item| key(item).to_owned())
    } else {
        None
    };
    items.truncate(limit);
    (
        items,
        PageMeta {
            limit,
            next_page_token,
            remaining_count,
        },
    )
}
