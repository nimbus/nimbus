//! MySQL implementation of the shared SQL dialect seam.

use std::borrow::Cow;

use mysql_async::FromValueError;
use mysql_async::prelude::FromValue;

use crate::sql::dialect::{
    Dialect, SqlRow, SqlSession, SqlSessionStore, SqlSessionTransaction, SqlValue,
};

use super::*;

pub(crate) struct MySqlDialect;

impl Dialect for MySqlDialect {
    const NAME: &'static str = "MySQL";
    const IDENTIFIER_QUOTE: char = '`';

    type Param<'a> = MySqlValue;

    fn placeholder(_ordinal: usize) -> Cow<'static, str> {
        Cow::Borrowed("?")
    }

    fn bind(value: SqlValue<'_>) -> Result<MySqlValue> {
        Ok(match value {
            SqlValue::Text(text) => MySqlValue::Bytes(text.as_bytes().to_vec()),
            SqlValue::Bytes(bytes) => MySqlValue::Bytes(bytes.to_vec()),
            SqlValue::Sequence(sequence) => MySqlValue::UInt(sequence.0),
            SqlValue::Bound(bound) => MySqlValue::UInt(bound),
        })
    }
}

/// A pooled connection or an open transaction that a [`MySqlSession`] drives.
pub(crate) trait MySqlHandle: Send {
    type Conn: Queryable;

    fn conn(&mut self) -> &mut Self::Conn;
}

impl MySqlHandle for Conn {
    type Conn = Conn;

    fn conn(&mut self) -> &mut Conn {
        self
    }
}

impl<C: Queryable> MySqlHandle for &mut C {
    type Conn = C;

    fn conn(&mut self) -> &mut C {
        self
    }
}

/// Shared-statement session over one MySQL connection or transaction.
pub(crate) struct MySqlSession<H>(pub(crate) H);

impl<H: MySqlHandle> MySqlSession<H> {
    pub(crate) fn driver(&mut self) -> &mut H::Conn {
        self.0.conn()
    }
}

fn bind_all(params: &[SqlValue<'_>]) -> Result<Params> {
    if params.is_empty() {
        return Ok(Params::Empty);
    }
    params
        .iter()
        .map(|value| MySqlDialect::bind(*value))
        .collect::<Result<Vec<_>>>()
        .map(Params::Positional)
}

impl<H: MySqlHandle> SqlSession for MySqlSession<H> {
    type Dialect = MySqlDialect;
    type Row = Row;

    async fn execute_sql(&mut self, sql: &str, params: &[SqlValue<'_>]) -> Result<u64> {
        let params = bind_all(params)?;
        let affected_rows = if matches!(params, Params::Empty) {
            let result = self
                .driver()
                .query_iter(sql)
                .await
                .map_err(map_mysql_error)?;
            let affected_rows = result.affected_rows();
            result.drop_result().await.map_err(map_mysql_error)?;
            affected_rows
        } else {
            let result = self
                .driver()
                .exec_iter(sql, params)
                .await
                .map_err(map_mysql_error)?;
            let affected_rows = result.affected_rows();
            result.drop_result().await.map_err(map_mysql_error)?;
            affected_rows
        };
        Ok(affected_rows)
    }

    async fn fetch_all(&mut self, sql: &str, params: &[SqlValue<'_>]) -> Result<Vec<Row>> {
        let params = bind_all(params)?;
        let rows = if matches!(params, Params::Empty) {
            self.driver().query(sql).await
        } else {
            self.driver().exec(sql, params).await
        };
        rows.map_err(map_mysql_error)
    }

    async fn fetch_optional(&mut self, sql: &str, params: &[SqlValue<'_>]) -> Result<Option<Row>> {
        let params = bind_all(params)?;
        let row = if matches!(params, Params::Empty) {
            self.driver().query_first(sql).await
        } else {
            self.driver().exec_first(sql, params).await
        };
        row.map_err(map_mysql_error)
    }
}

fn decode_column<T: FromValue>(row: &Row, index: usize) -> Result<T> {
    row.get_opt::<T, usize>(index)
        .unwrap_or_else(|| Err(FromValueError(MySqlValue::NULL)))
        .map_err(|error| {
            Error::storage(
                StorageErrorKind::Corruption,
                format!("invalid MySQL column {index}: {error}"),
            )
        })
}

impl SqlRow for Row {
    fn text(&self, index: usize) -> Result<String> {
        decode_column(self, index)
    }

    fn bytes(&self, index: usize) -> Result<Vec<u8>> {
        decode_column(self, index)
    }

    fn u64(&self, index: usize) -> Result<u64> {
        decode_column(self, index)
    }

    fn optional_u64(&self, index: usize) -> Result<Option<u64>> {
        decode_column(self, index)
    }
}

impl SqlSessionTransaction for MySqlWriteTransaction {
    type Session<'s> = MySqlSession<&'s mut Conn>;

    fn session_parts(&mut self) -> Result<(MySqlSession<&mut Conn>, String, TokioRuntimeHandle)> {
        let database_name = self.database_name.clone();
        let runtime_handle = self.provider.runtime_handle.clone();
        Ok((MySqlSession(self.session()?), database_name, runtime_handle))
    }

    fn block_on_session<T: Send>(
        runtime_handle: &TokioRuntimeHandle,
        future: impl Future<Output = Result<T>> + Send,
    ) -> Result<T> {
        Self::block_on(runtime_handle, future)
    }
}

impl SqlSessionStore for MySqlTenantStore {
    type ReadSession = MySqlSession<Conn>;

    fn tenant_schema(&self) -> String {
        self.database_name.clone()
    }

    fn open_read_session(
        &self,
    ) -> impl Future<Output = Result<MySqlSession<Conn>>> + Send + 'static {
        let provider = self.provider.clone();
        async move { provider.conn().await.map(MySqlSession) }
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
    fn bind_markers_keep_native_markers() {
        let sql = "SELECT 1 FROM t WHERE x = ? AND y = ?";
        assert!(matches!(MySqlDialect::bind_markers(sql), Cow::Borrowed(kept) if kept == sql));
    }

    #[test]
    fn qualified_table_doubles_embedded_backticks() {
        assert_eq!(
            MySqlDialect::qualified_table("ten`ant", "jobs"),
            "`ten``ant`.`jobs`"
        );
    }

    #[test]
    fn bind_keeps_unsigned_values() {
        assert!(matches!(
            MySqlDialect::bind(SqlValue::Bound(u64::MAX)),
            Ok(MySqlValue::UInt(u64::MAX))
        ));
        assert!(matches!(
            MySqlDialect::bind(SqlValue::Sequence(SequenceNumber(u64::MAX))),
            Ok(MySqlValue::UInt(u64::MAX))
        ));
    }
}
