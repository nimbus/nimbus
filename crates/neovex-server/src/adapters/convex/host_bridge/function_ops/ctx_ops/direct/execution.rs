use super::*;

impl ConvexHostBridge {
    pub(in crate::adapters::convex) async fn execute_query_with_execution_context_async_cancellable(
        &self,
        query: ConvexExecutableQuery,
        auth: Option<&InvocationAuth>,
        cancellation: &HostCallCancellation,
    ) -> Result<Value, Error> {
        if let Some(execution_unit) = self.mutation_execution_unit() {
            return execute_query_with_execution_unit_cancellable(
                execution_unit,
                query,
                cancellation,
            );
        }

        execute_query_result_async(
            &self.service,
            &self.tenant_id,
            query,
            auth,
            Some(cancellation.clone()),
        )
        .await
    }

    pub(in crate::adapters::convex) fn execute_query_with_execution_context_cancellable(
        &self,
        query: ConvexExecutableQuery,
        auth: Option<&InvocationAuth>,
        cancellation: &HostCallCancellation,
    ) -> Result<Value, Error> {
        if let Some(execution_unit) = self.mutation_execution_unit() {
            return execute_query_with_execution_unit_cancellable(
                execution_unit,
                query,
                cancellation,
            );
        }

        let mut check_cancel = || check_host_cancellation(cancellation);
        execute_query_result_cancellable_with_auth(
            &self.service,
            &self.tenant_id,
            query,
            auth,
            &mut check_cancel,
        )
    }

    pub(in crate::adapters::convex) async fn paginate_query_with_execution_context_async_cancellable(
        &self,
        query: Query,
        page_size: usize,
        after: Option<Cursor>,
        principal: &neovex_core::PrincipalContext,
        cancellation: &HostCallCancellation,
    ) -> Result<neovex_core::Page, Error> {
        if let Some(execution_unit) = self.mutation_execution_unit() {
            return paginate_query_with_execution_unit_cancellable(
                execution_unit,
                query,
                page_size,
                after,
                cancellation,
            );
        }

        let check_cancellation = cancellation.clone();
        self.service
            .paginate_documents_async_cancellable_with_principal(
                self.tenant_id.clone(),
                PaginatedQuery {
                    query,
                    page_size,
                    after,
                },
                principal.clone(),
                cancellation.cancelled(),
                move || check_host_cancellation(&check_cancellation),
            )
            .await
    }

    pub(in crate::adapters::convex) fn paginate_query_with_execution_context_cancellable(
        &self,
        query: Query,
        page_size: usize,
        after: Option<Cursor>,
        principal: &neovex_core::PrincipalContext,
        cancellation: &HostCallCancellation,
    ) -> Result<neovex_core::Page, Error> {
        if let Some(execution_unit) = self.mutation_execution_unit() {
            return paginate_query_with_execution_unit_cancellable(
                execution_unit,
                query,
                page_size,
                after,
                cancellation,
            );
        }

        let mut check_cancel = || check_host_cancellation(cancellation);
        self.service.paginate_documents_with_principal_cancellable(
            &self.tenant_id,
            &PaginatedQuery {
                query,
                page_size,
                after,
            },
            principal,
            &mut check_cancel,
        )
    }

    pub(in crate::adapters::convex) async fn dispatch_convex_mutation_with_execution_context_async_cancellable(
        &self,
        mutation: ConvexExecutableMutation,
        auth: Option<&InvocationAuth>,
        cancellation: &HostCallCancellation,
    ) -> Result<Value, Error> {
        if let Some(execution_unit) = self.mutation_execution_unit() {
            return match mutation {
                ConvexExecutableMutation::Mutation(mutation) => {
                    execute_document_mutation_with_execution_unit(execution_unit, mutation)
                }
                ConvexExecutableMutation::Query(query) => {
                    self.execute_query_with_execution_context_async_cancellable(
                        query,
                        auth,
                        cancellation,
                    )
                    .await
                }
                ConvexExecutableMutation::Scheduled(command) => {
                    self.execute_schedule_command_with_execution_context_async(
                        command,
                        cancellation,
                    )
                    .await
                }
            };
        }

        dispatch_convex_mutation_async(
            &self.service,
            &self.registry,
            &self.tenant_id,
            mutation,
            auth,
            Some(cancellation.clone()),
        )
        .await
    }

    pub(in crate::adapters::convex) fn dispatch_convex_mutation_with_execution_context_cancellable(
        &self,
        mutation: ConvexExecutableMutation,
        auth: Option<&InvocationAuth>,
        cancellation: &HostCallCancellation,
    ) -> Result<Value, Error> {
        if let Some(execution_unit) = self.mutation_execution_unit() {
            return match mutation {
                ConvexExecutableMutation::Mutation(mutation) => {
                    execute_document_mutation_with_execution_unit(execution_unit, mutation)
                }
                ConvexExecutableMutation::Query(query) => {
                    self.execute_query_with_execution_context_cancellable(query, auth, cancellation)
                }
                ConvexExecutableMutation::Scheduled(command) => {
                    self.execute_schedule_command_with_execution_context(command)
                }
            };
        }

        dispatch_convex_mutation_cancellable_with_auth(
            &self.service,
            &self.registry,
            &self.tenant_id,
            mutation,
            auth,
            cancellation,
        )
    }
}

fn execute_query_with_execution_unit_cancellable(
    execution_unit: &Arc<neovex_engine::MutationExecutionUnit>,
    query: ConvexExecutableQuery,
    cancellation: &HostCallCancellation,
) -> Result<Value, Error> {
    let mut check_cancel = || check_host_cancellation(cancellation);
    match query {
        ConvexExecutableQuery::Query(query) => execution_unit
            .query_documents_cancellable(&query, &mut check_cancel)
            .map(|documents| {
                Value::Array(
                    documents
                        .into_iter()
                        .map(|document| document.into_json())
                        .collect(),
                )
            }),
        ConvexExecutableQuery::Read(ConvexReadCommand::Get { table, id }) => {
            execution_unit.get_document(&table, id).map(|document| {
                document
                    .map(|document| document.into_json())
                    .unwrap_or(Value::Null)
            })
        }
        ConvexExecutableQuery::Read(ConvexReadCommand::First { query }) => {
            let mut documents =
                execution_unit.query_documents_cancellable(&query, &mut check_cancel)?;
            Ok(documents
                .drain(..)
                .next()
                .map(|document| document.into_json())
                .unwrap_or(Value::Null))
        }
        ConvexExecutableQuery::Read(ConvexReadCommand::Unique { query }) => {
            let mut documents =
                execution_unit.query_documents_cancellable(&query, &mut check_cancel)?;
            if documents.len() > 1 {
                return Err(Error::InvalidInput(
                    "convex unique query matched multiple documents".to_string(),
                ));
            }
            Ok(documents
                .drain(..)
                .next()
                .map(|document| document.into_json())
                .unwrap_or(Value::Null))
        }
    }
}

fn paginate_query_with_execution_unit_cancellable(
    execution_unit: &Arc<neovex_engine::MutationExecutionUnit>,
    query: Query,
    page_size: usize,
    after: Option<Cursor>,
    cancellation: &HostCallCancellation,
) -> Result<neovex_core::Page, Error> {
    let mut check_cancel = || check_host_cancellation(cancellation);
    execution_unit.paginate_documents_cancellable(
        &PaginatedQuery {
            query,
            page_size,
            after,
        },
        &mut check_cancel,
    )
}

fn execute_document_mutation_with_execution_unit(
    execution_unit: &Arc<neovex_engine::MutationExecutionUnit>,
    mutation: Mutation,
) -> Result<Value, Error> {
    match mutation {
        Mutation::Insert { table, fields } => execution_unit
            .insert_document(table, fields)
            .map(|id| Value::String(id.to_string())),
        Mutation::Update { table, id, patch } => execution_unit
            .update_document(table, id, patch)
            .map(|id| Value::String(id.to_string())),
        Mutation::Delete { table, id } => execution_unit
            .delete_document(table, id)
            .map(|_| Value::Null),
    }
}
