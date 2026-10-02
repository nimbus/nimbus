//! SQL dialect seam for the PostgreSQL and MySQL providers.
//!
//! The two providers run the same statements against the same tenant tables.
//! They differ only in identifier quoting, bind-marker syntax, and the wire
//! types each driver binds and decodes. [`Dialect`] owns those differences, and
//! [`SqlSession`] runs a statement written once with `?` bind markers against
//! one driver connection. The shared statement modules (`table_lifecycle`,
//! `resource_paths`, `index_versions`, `read_store`) are generic over
//! [`SqlSession`], so each statement lives once.
//!
//! Each provider implements the seam in its own `dialect.rs`. Driver-specific
//! work that has no shared form (PostgreSQL index DDL, MySQL hashed lookup
//! keys, snapshot transaction setup) stays behind narrow hook traits next to
//! the shared code that calls it.

use std::borrow::Cow;
use std::future::Future;

use nimbus_core::{Result, SequenceNumber};
use tokio::runtime::Handle as TokioRuntimeHandle;

/// A value bound to a shared statement.
///
/// The variants name what the value means, not its wire type, so each dialect
/// can map it to the column type it declares.
#[derive(Debug, Clone, Copy)]
pub(crate) enum SqlValue<'a> {
    Text(&'a str),
    Bytes(&'a [u8]),
    /// A journal sequence. A dialect with signed integer columns rejects a
    /// sequence that does not fit.
    Sequence(SequenceNumber),
    /// An inclusive upper bound or a row limit. A dialect with signed integer
    /// columns saturates it.
    Bound(u64),
}

/// Identifier, bind-marker, and type-mapping rules of one SQL dialect.
pub(crate) trait Dialect {
    /// Provider name used in error messages.
    const NAME: &'static str;
    /// Character that opens and closes a quoted identifier.
    const IDENTIFIER_QUOTE: char;

    /// Driver-side parameter for one bound [`SqlValue`].
    type Param<'a>: Send;

    /// Quote `identifier`, doubling every embedded quote character.
    fn quote_identifier(identifier: &str) -> String {
        let mut quoted = String::with_capacity(identifier.len() + 2);
        quoted.push(Self::IDENTIFIER_QUOTE);
        for character in identifier.chars() {
            if character == Self::IDENTIFIER_QUOTE {
                quoted.push(Self::IDENTIFIER_QUOTE);
            }
            quoted.push(character);
        }
        quoted.push(Self::IDENTIFIER_QUOTE);
        quoted
    }

    /// Name `table` inside the tenant `namespace` (schema or database).
    fn qualified_table(namespace: &str, table: &str) -> String {
        format!(
            "{}.{}",
            Self::quote_identifier(namespace),
            Self::quote_identifier(table)
        )
    }

    /// Bind marker for the 1-based parameter `ordinal`.
    fn placeholder(ordinal: usize) -> Cow<'static, str>;

    /// Rewrite the `?` bind markers of a shared statement into this dialect's
    /// markers. Markers inside string literals and quoted identifiers stay.
    fn bind_markers(sql: &str) -> Cow<'_, str> {
        if Self::placeholder(1) == "?" || !sql.contains('?') {
            return Cow::Borrowed(sql);
        }
        let mut rewritten = String::with_capacity(sql.len() + 16);
        let mut ordinal = 0;
        let mut quote = None;
        for character in sql.chars() {
            match (quote, character) {
                (None, '\'') => quote = Some('\''),
                (None, character) if character == Self::IDENTIFIER_QUOTE => quote = Some(character),
                (Some(open), character) if character == open => quote = None,
                (None, '?') => {
                    ordinal += 1;
                    rewritten.push_str(&Self::placeholder(ordinal));
                    continue;
                }
                _ => {}
            }
            rewritten.push(character);
        }
        Cow::Owned(rewritten)
    }

    /// Map a shared value to this dialect's driver parameter.
    fn bind(value: SqlValue<'_>) -> Result<Self::Param<'_>>;
}

/// One result row. A column that does not decode to the requested type is a
/// corruption error.
pub(crate) trait SqlRow: Send {
    fn text(&self, index: usize) -> Result<String>;
    fn bytes(&self, index: usize) -> Result<Vec<u8>>;
    fn u64(&self, index: usize) -> Result<u64>;
    fn optional_u64(&self, index: usize) -> Result<Option<u64>>;
}

/// An open driver connection or transaction that runs shared statements.
///
/// Statements use `?` bind markers. The session rewrites them through
/// [`Dialect::bind_markers`] and binds each value through [`Dialect::bind`].
pub(crate) trait SqlSession: Send {
    type Dialect: Dialect;
    type Row: SqlRow;

    /// Run a statement and return the number of affected rows.
    fn execute_sql(
        &mut self,
        sql: &str,
        params: &[SqlValue<'_>],
    ) -> impl Future<Output = Result<u64>> + Send;

    fn fetch_all(
        &mut self,
        sql: &str,
        params: &[SqlValue<'_>],
    ) -> impl Future<Output = Result<Vec<Self::Row>>> + Send;

    fn fetch_optional(
        &mut self,
        sql: &str,
        params: &[SqlValue<'_>],
    ) -> impl Future<Output = Result<Option<Self::Row>>> + Send;

    /// Name a tenant table for this session's dialect.
    fn table(namespace: &str, table: &str) -> String {
        Self::Dialect::qualified_table(namespace, table)
    }
}

/// A write transaction that lends its open session to shared statement code.
pub(crate) trait SqlSessionTransaction {
    type Session<'s>: crate::sql::table_lifecycle::SqlTableLifecycleSession
        + crate::sql::resource_paths::SqlResourcePathSession
    where
        Self: 's;

    /// Borrow the open session with the tenant namespace and the runtime that
    /// drives it. Fails when the transaction is already closed.
    fn session_parts(&mut self) -> Result<(Self::Session<'_>, String, TokioRuntimeHandle)>;

    /// Drive a session future to completion from synchronous code.
    fn block_on_session<T: Send>(
        runtime_handle: &TokioRuntimeHandle,
        future: impl Future<Output = Result<T>> + Send,
    ) -> Result<T>;
}

/// A tenant store that opens pooled read sessions for shared statement code.
pub(crate) trait SqlSessionStore {
    type ReadSession: crate::sql::index_versions::SqlIndexVersionSession
        + crate::sql::read_store::SqlReadSession
        + crate::sql::resource_paths::SqlResourcePathSession
        + 'static;

    /// The tenant namespace: a PostgreSQL schema or a MySQL database.
    fn tenant_schema(&self) -> String;

    /// Check out a pooled session. The future owns everything it needs.
    fn open_read_session(&self)
    -> impl Future<Output = Result<Self::ReadSession>> + Send + 'static;

    /// Drive a read future to completion from synchronous code.
    fn block_on_read<T: Send + 'static>(
        &self,
        future: impl Future<Output = Result<T>> + Send + 'static,
    ) -> Result<T>;
}
