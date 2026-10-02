//! PostgreSQL implementation of the shared SQL dialect seam.

use std::borrow::Cow;

use crate::sql::dialect::{
    Dialect, SqlRow, SqlSession, SqlSessionStore, SqlSessionTransaction, SqlValue,
};

use super::*;

pub(crate) struct PostgresDialect;

impl Dialect for PostgresDialect {
    const NAME: &'static str = "PostgreSQL";
    const IDENTIFIER_QUOTE: char = '"';

    type Param<'a> = Box<dyn ToSql + Sync + Send + 'a>;

    fn placeholder(ordinal: usize) -> Cow<'static, str> {
        Cow::Owned(format!("${ordinal}"))
    }

    fn bind(value: SqlValue<'_>) -> Result<Self::Param<'_>> {
        let param: Self::Param<'_> = match value {
            SqlValue::Text(text) => Box::new(text),
            SqlValue::Bytes(bytes) => Box::new(bytes),
            SqlValue::Sequence(sequence) => Box::new(i64_from_sequence(sequence)?),
            SqlValue::Bound(bound) => Box::new(i64::try_from(bound).unwrap_or(i64::MAX)),
        };
        Ok(param)
    }
}

/// A pooled client or an open transaction that a [`PostgresSession`] drives.
pub(crate) trait PostgresHandle: Send + Sync {
    type Client: GenericClient + Sync;

    fn client(&self) -> &Self::Client;
}

impl PostgresHandle for Client {
    type Client = Client;

    fn client(&self) -> &Client {
        self
    }
}

impl<C: GenericClient + Sync> PostgresHandle for &C {
    type Client = C;

    fn client(&self) -> &C {
        self
    }
}

/// Shared-statement session over one PostgreSQL client or transaction.
pub(crate) struct PostgresSession<H>(pub(crate) H);

impl<H: PostgresHandle> PostgresSession<H> {
    pub(crate) fn driver(&self) -> &H::Client {
        self.0.client()
    }
}

fn bind_all<'a>(params: &[SqlValue<'a>]) -> Result<Vec<Box<dyn ToSql + Sync + Send + 'a>>> {
    params
        .iter()
        .map(|value| PostgresDialect::bind(*value))
        .collect()
}

fn as_refs<'b>(params: &'b [Box<dyn ToSql + Sync + Send + '_>]) -> Vec<&'b (dyn ToSql + Sync)> {
    params
        .iter()
        .map(|param| param.as_ref() as &(dyn ToSql + Sync))
        .collect()
}

impl<H: PostgresHandle> SqlSession for PostgresSession<H> {
    type Dialect = PostgresDialect;
    type Row = tokio_postgres::Row;

    async fn execute_sql(&mut self, sql: &str, params: &[SqlValue<'_>]) -> Result<u64> {
        let sql = PostgresDialect::bind_markers(sql);
        let params = bind_all(params)?;
        self.driver()
            .execute(sql.as_ref(), &as_refs(&params))
            .await
            .map_err(map_postgres_error)
    }

    async fn fetch_all(
        &mut self,
        sql: &str,
        params: &[SqlValue<'_>],
    ) -> Result<Vec<tokio_postgres::Row>> {
        let sql = PostgresDialect::bind_markers(sql);
        let params = bind_all(params)?;
        self.driver()
            .query(sql.as_ref(), &as_refs(&params))
            .await
            .map_err(map_postgres_error)
    }

    async fn fetch_optional(
        &mut self,
        sql: &str,
        params: &[SqlValue<'_>],
    ) -> Result<Option<tokio_postgres::Row>> {
        let sql = PostgresDialect::bind_markers(sql);
        let params = bind_all(params)?;
        self.driver()
            .query_opt(sql.as_ref(), &as_refs(&params))
            .await
            .map_err(map_postgres_error)
    }
}

fn decode_column<'r, T: tokio_postgres::types::FromSql<'r>>(
    row: &'r tokio_postgres::Row,
    index: usize,
) -> Result<T> {
    row.try_get(index).map_err(|error| {
        Error::storage(
            StorageErrorKind::Corruption,
            format!("invalid PostgreSQL column {index}: {error}"),
        )
    })
}

fn u64_from_column(value: i64, index: usize) -> Result<u64> {
    u64::try_from(value).map_err(|_| {
        Error::storage(
            StorageErrorKind::Corruption,
            format!("negative PostgreSQL value {value} in column {index}"),
        )
    })
}

impl SqlRow for tokio_postgres::Row {
    fn text(&self, index: usize) -> Result<String> {
        decode_column(self, index)
    }

    fn bytes(&self, index: usize) -> Result<Vec<u8>> {
        decode_column(self, index)
    }

    fn u64(&self, index: usize) -> Result<u64> {
        u64_from_column(decode_column(self, index)?, index)
    }

    fn optional_u64(&self, index: usize) -> Result<Option<u64>> {
        decode_column::<Option<i64>>(self, index)?
            .map(|value| u64_from_column(value, index))
            .transpose()
    }
}

impl SqlSessionTransaction for PostgresWriteTransaction {
    type Session<'s> = PostgresSession<&'s Client>;

    fn session_parts(&mut self) -> Result<(PostgresSession<&Client>, String, TokioRuntimeHandle)> {
        Ok((
            PostgresSession(self.session()?),
            self.schema_name.clone(),
            self.provider.runtime_handle.clone(),
        ))
    }

    fn block_on_session<T: Send>(
        runtime_handle: &TokioRuntimeHandle,
        future: impl Future<Output = Result<T>> + Send,
    ) -> Result<T> {
        runtime_handle.block_on(future)
    }
}

impl SqlSessionStore for PostgresTenantStore {
    type ReadSession = PostgresSession<Client>;

    fn tenant_schema(&self) -> String {
        self.schema_name.clone()
    }

    fn open_read_session(
        &self,
    ) -> impl Future<Output = Result<PostgresSession<Client>>> + Send + 'static {
        let provider = self.provider.clone();
        async move { provider.client().await.map(PostgresSession) }
    }

    fn block_on_read<T: Send + 'static>(
        &self,
        future: impl Future<Output = Result<T>> + Send + 'static,
    ) -> Result<T> {
        self.block_on(future)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_markers_number_each_marker_outside_quotes() {
        let sql = r#"SELECT '?', 'it''s ?', "a?b" FROM t WHERE x = ? AND y = ?"#;
        assert_eq!(
            PostgresDialect::bind_markers(sql),
            r#"SELECT '?', 'it''s ?', "a?b" FROM t WHERE x = $1 AND y = $2"#
        );
    }

    #[test]
    fn bind_markers_borrow_a_statement_without_markers() {
        assert!(matches!(
            PostgresDialect::bind_markers("SELECT 1"),
            Cow::Borrowed("SELECT 1")
        ));
    }

    #[test]
    fn qualified_table_doubles_embedded_quotes() {
        assert_eq!(
            PostgresDialect::qualified_table("ten\"ant", "jobs"),
            r#""ten""ant"."jobs""#
        );
    }

    #[test]
    fn bind_saturates_bounds_and_rejects_oversized_sequences() {
        assert!(PostgresDialect::bind(SqlValue::Bound(u64::MAX)).is_ok());
        let error = PostgresDialect::bind(SqlValue::Sequence(SequenceNumber(u64::MAX)))
            .expect_err("an oversized sequence must not bind");
        assert!(matches!(error, Error::InvalidInput(_)));
    }
}
