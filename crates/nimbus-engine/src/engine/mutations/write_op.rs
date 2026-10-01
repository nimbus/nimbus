use nimbus_core::{
    AccessAction, Document, Error, IndexDefinition, PrincipalContext, ResourcePathBinding, Result,
    TableId, TableSchema, WriteOp, WriteOpType,
};

use super::enforce_mutation_authorization;

/// One validated and authorized single-document write. The owning commit path
/// still assigns the table identity, sequence, and lifecycle times.
#[derive(Debug)]
pub(in crate::engine) struct PreparedWriteOp<'schema> {
    pub(in crate::engine) op_type: WriteOpType,
    pub(in crate::engine) previous: Option<Document>,
    pub(in crate::engine) current: Option<Document>,
    pub(in crate::engine) indexes: &'schema [IndexDefinition],
    pub(in crate::engine) resource_path_binding: Option<ResourcePathBinding>,
}

impl<'schema> PreparedWriteOp<'schema> {
    /// Returns the image that names the written document: the new image, or
    /// the removed one for a delete.
    pub(in crate::engine) fn document(&self) -> Result<&Document> {
        self.current
            .as_ref()
            .or(self.previous.as_ref())
            .ok_or_else(|| {
                Error::Internal("a document write needs a previous or a current image".to_string())
            })
    }

    /// Returns the durable write and the index work that its commit carries.
    pub(in crate::engine) fn into_write_op(
        self,
        table_id: TableId,
    ) -> Result<(WriteOp, &'schema [IndexDefinition])> {
        let document = self.document()?;
        let write = WriteOp {
            table: document.table.clone(),
            table_id,
            op_type: self.op_type,
            doc_id: document.id.clone(),
            resource_path_binding: self.resource_path_binding,
            // Trigger origin belongs to the commit that carries this write,
            // not to the client write that the preparer builds.
            trigger_write_origin: None,
            previous: self.previous,
            current: self.current,
        };
        Ok((write, self.indexes))
    }
}

/// Builds the one durable write that every client mutation path commits for a
/// single document. The caller reads `previous` and builds `current` from its
/// own snapshot or staged image. This function performs no I/O.
///
/// `existing_binding` is the resource path that `previous` holds now.
/// `requested_binding` is a path that the client named for this write. An
/// insert or update keeps the requested path, else the existing one. A delete
/// carries the existing path so that collection-group conflicts and trigger
/// dispatch still see the removed document. The execution unit passes no
/// existing path for a delete. Its commit reads that path from the unit
/// snapshot, because a staged path can belong to an uncommitted write.
pub(in crate::engine) fn prepare_write_op<'schema>(
    table_schema: Option<&'schema TableSchema>,
    principal: &PrincipalContext,
    previous: Option<Document>,
    current: Option<Document>,
    existing_binding: Option<ResourcePathBinding>,
    requested_binding: Option<ResourcePathBinding>,
) -> Result<PreparedWriteOp<'schema>> {
    let (op_type, action, resource_path_binding) = match (&previous, &current) {
        (None, Some(_)) => (
            WriteOpType::Insert,
            AccessAction::Create,
            requested_binding.or(existing_binding),
        ),
        (Some(_), Some(_)) => (
            WriteOpType::Update,
            AccessAction::Update,
            requested_binding.or(existing_binding),
        ),
        (Some(_), None) => (WriteOpType::Delete, AccessAction::Delete, existing_binding),
        (None, None) => {
            return Err(Error::Internal(
                "a document write needs a previous or a current image".to_string(),
            ));
        }
    };
    if let (Some(table_schema), Some(current)) = (table_schema, current.as_ref()) {
        table_schema.validate(&current.fields)?;
    }
    enforce_mutation_authorization(
        table_schema,
        action,
        principal,
        current.as_ref(),
        previous.as_ref(),
    )?;
    Ok(PreparedWriteOp {
        op_type,
        previous,
        current,
        indexes: table_schema.map_or(&[], |table_schema| table_schema.indexes.as_slice()),
        resource_path_binding,
    })
}
