//! Dialect-shared historical index-scan orchestration.
//!
//! Every historical index read — equality, prefix, range, composite range, and
//! the paged form of each — reduces to the same three steps: build a
//! [`HistoricalIndexScanPlan`], load the visible index entries for that plan's
//! tuple bounds, then hand the entries to `finish_historical_index_page` for
//! cursor and limit handling. Only the middle step touches a database, so it is
//! the single hook on [`SqlHistoricalIndexStore`]; the eight entry points around
//! it were byte-identical between the PostgreSQL and MySQL stores.
//!
//! What stays per-backend:
//!
//! - **The entry load itself.** Each store keeps its own
//!   `visible_historical_index_entries_for_tuple_bounds`: the storage-format
//!   validation, the tuple-bound SQL, parameter binding (`ToSql` boxes vs
//!   `MySqlValue`), and the row decoding are all dialect-owned.
//! - **The sqlite backend.** It has the same scan family but reaches its rows
//!   through a synchronous `Connection` rather than a pooled async session, and
//!   is out of scope for this unification pass.
//! - **The libsql replica.** It has no historical index-scan family at all.

use nimbus_core::{Document, HistoricalIndexCursor, HistoricalReadShape, IndexDefinition, Result};
use serde_json::Value;

use crate::IndexRangeBound;
use crate::index::history_scan::{
    HistoricalIndexDocumentEntry, HistoricalIndexPageRequest, HistoricalIndexScanPlan,
    finish_historical_index_page,
};
use crate::store::HistoricalIndexDocumentPage;

/// Store-level seam for historical index reads.
///
/// As elsewhere in [`crate::sql`], a default method here shares a name with the
/// inherent method the facade below generates. Inherent methods win method-call
/// resolution, so the facade is not recursive.
pub(crate) trait SqlHistoricalIndexStore {
    /// Loads the index entries visible at `read_shape`'s sequence whose encoded
    /// tuples fall within `[start_key, end_key]` and match `match_prefix`.
    fn visible_historical_index_entries(
        &self,
        read_shape: &HistoricalReadShape,
        index: &IndexDefinition,
        match_prefix: &[u8],
        start_key: Option<&[u8]>,
        end_key: Option<&[u8]>,
    ) -> Result<Vec<HistoricalIndexDocumentEntry>>;

    fn historical_index_scan_eq_cancellable(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        value: &Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        Ok(self
            .historical_index_scan_eq_page_cancellable(
                read_shape,
                index_name,
                value,
                None,
                usize::MAX,
                check_cancel,
            )?
            .documents)
    }

    fn historical_index_scan_eq_page_cancellable(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        value: &Value,
        after: Option<&HistoricalIndexCursor>,
        limit: usize,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<HistoricalIndexDocumentPage> {
        let plan = HistoricalIndexScanPlan::equal(read_shape, index_name, value)?;
        self.historical_index_scan_page_for_plan(
            read_shape,
            &plan,
            HistoricalIndexPageRequest {
                after,
                limit,
                check_cancel,
            },
        )
    }

    fn historical_index_scan_prefix_cancellable(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        prefix_values: &[Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        Ok(self
            .historical_index_scan_prefix_page_cancellable(
                read_shape,
                index_name,
                prefix_values,
                None,
                usize::MAX,
                check_cancel,
            )?
            .documents)
    }

    fn historical_index_scan_prefix_page_cancellable(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        prefix_values: &[Value],
        after: Option<&HistoricalIndexCursor>,
        limit: usize,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<HistoricalIndexDocumentPage> {
        let plan = HistoricalIndexScanPlan::prefix(read_shape, index_name, prefix_values)?;
        self.historical_index_scan_page_for_plan(
            read_shape,
            &plan,
            HistoricalIndexPageRequest {
                after,
                limit,
                check_cancel,
            },
        )
    }

    fn historical_index_scan_range_cancellable(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        Ok(self
            .historical_index_scan_range_page_cancellable(
                read_shape,
                index_name,
                start,
                end,
                HistoricalIndexPageRequest {
                    after: None,
                    limit: usize::MAX,
                    check_cancel,
                },
            )?
            .documents)
    }

    fn historical_index_scan_range_page_cancellable(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        page: HistoricalIndexPageRequest<'_, '_>,
    ) -> Result<HistoricalIndexDocumentPage> {
        let plan = HistoricalIndexScanPlan::range(read_shape, index_name, start, end)?;
        self.historical_index_scan_page_for_plan(read_shape, &plan, page)
    }

    fn historical_index_scan_composite_range_cancellable(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        exact_prefix: &[Value],
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        Ok(self
            .historical_index_scan_composite_range_page_cancellable(
                read_shape,
                index_name,
                exact_prefix,
                start,
                end,
                HistoricalIndexPageRequest {
                    after: None,
                    limit: usize::MAX,
                    check_cancel,
                },
            )?
            .documents)
    }

    fn historical_index_scan_composite_range_page_cancellable(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        exact_prefix: &[Value],
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        page: HistoricalIndexPageRequest<'_, '_>,
    ) -> Result<HistoricalIndexDocumentPage> {
        let plan = HistoricalIndexScanPlan::composite_range(
            read_shape,
            index_name,
            exact_prefix,
            start,
            end,
        )?;
        self.historical_index_scan_page_for_plan(read_shape, &plan, page)
    }

    fn historical_index_scan_page_for_plan(
        &self,
        read_shape: &HistoricalReadShape,
        plan: &HistoricalIndexScanPlan,
        page: HistoricalIndexPageRequest<'_, '_>,
    ) -> Result<HistoricalIndexDocumentPage> {
        let HistoricalIndexPageRequest {
            after,
            limit,
            check_cancel,
        } = page;
        plan.validate_page_request(read_shape, after, limit)?;
        if plan.empty {
            return finish_historical_index_page(read_shape, plan, after, limit, Vec::new());
        }
        let entries = self.visible_historical_index_entries(
            read_shape,
            &plan.index,
            &plan.match_prefix,
            plan.start_key.as_deref(),
            plan.end_key.as_deref(),
        )?;
        for _ in &entries {
            check_cancel()?;
        }
        finish_historical_index_page(read_shape, plan, after, limit, entries)
    }
}

/// Re-exposes the public [`SqlHistoricalIndexStore`] entry points as inherent
/// methods on `$ty`, keeping each store's public API exactly as it was before
/// the family moved here. The `*_range_page_cancellable` forms are omitted:
/// they are `pub(crate)`, had no caller outside the family itself on these two
/// stores, and the trait defaults reach them directly.
macro_rules! sql_historical_index_facade {
    ($ty:ty) => {
        impl $ty {
            pub fn historical_index_scan_eq_cancellable(
                &self,
                read_shape: &nimbus_core::HistoricalReadShape,
                index_name: &str,
                value: &serde_json::Value,
                check_cancel: &mut dyn FnMut() -> nimbus_core::Result<()>,
            ) -> nimbus_core::Result<Vec<nimbus_core::Document>> {
                <Self as crate::sql::index_history::SqlHistoricalIndexStore>::historical_index_scan_eq_cancellable(self, read_shape, index_name, value, check_cancel)
            }

            pub fn historical_index_scan_eq_page_cancellable(
                &self,
                read_shape: &nimbus_core::HistoricalReadShape,
                index_name: &str,
                value: &serde_json::Value,
                after: Option<&nimbus_core::HistoricalIndexCursor>,
                limit: usize,
                check_cancel: &mut dyn FnMut() -> nimbus_core::Result<()>,
            ) -> nimbus_core::Result<crate::store::HistoricalIndexDocumentPage> {
                <Self as crate::sql::index_history::SqlHistoricalIndexStore>::historical_index_scan_eq_page_cancellable(self, read_shape, index_name, value, after, limit, check_cancel)
            }

            pub fn historical_index_scan_prefix_cancellable(
                &self,
                read_shape: &nimbus_core::HistoricalReadShape,
                index_name: &str,
                prefix_values: &[serde_json::Value],
                check_cancel: &mut dyn FnMut() -> nimbus_core::Result<()>,
            ) -> nimbus_core::Result<Vec<nimbus_core::Document>> {
                <Self as crate::sql::index_history::SqlHistoricalIndexStore>::historical_index_scan_prefix_cancellable(self, read_shape, index_name, prefix_values, check_cancel)
            }

            pub fn historical_index_scan_prefix_page_cancellable(
                &self,
                read_shape: &nimbus_core::HistoricalReadShape,
                index_name: &str,
                prefix_values: &[serde_json::Value],
                after: Option<&nimbus_core::HistoricalIndexCursor>,
                limit: usize,
                check_cancel: &mut dyn FnMut() -> nimbus_core::Result<()>,
            ) -> nimbus_core::Result<crate::store::HistoricalIndexDocumentPage> {
                <Self as crate::sql::index_history::SqlHistoricalIndexStore>::historical_index_scan_prefix_page_cancellable(self, read_shape, index_name, prefix_values, after, limit, check_cancel)
            }

            pub fn historical_index_scan_range_cancellable(
                &self,
                read_shape: &nimbus_core::HistoricalReadShape,
                index_name: &str,
                start: crate::IndexRangeBound<'_>,
                end: crate::IndexRangeBound<'_>,
                check_cancel: &mut dyn FnMut() -> nimbus_core::Result<()>,
            ) -> nimbus_core::Result<Vec<nimbus_core::Document>> {
                <Self as crate::sql::index_history::SqlHistoricalIndexStore>::historical_index_scan_range_cancellable(self, read_shape, index_name, start, end, check_cancel)
            }

            pub fn historical_index_scan_composite_range_cancellable(
                &self,
                read_shape: &nimbus_core::HistoricalReadShape,
                index_name: &str,
                exact_prefix: &[serde_json::Value],
                start: crate::IndexRangeBound<'_>,
                end: crate::IndexRangeBound<'_>,
                check_cancel: &mut dyn FnMut() -> nimbus_core::Result<()>,
            ) -> nimbus_core::Result<Vec<nimbus_core::Document>> {
                <Self as crate::sql::index_history::SqlHistoricalIndexStore>::historical_index_scan_composite_range_cancellable(self, read_shape, index_name, exact_prefix, start, end, check_cancel)
            }
        }
    };
}

pub(crate) use sql_historical_index_facade;
