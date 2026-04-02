use super::*;

impl ConvexHostBridge {
    pub(in crate::adapters::convex) async fn execute_query_with_execution_context_async_cancellable(
        &self,
        query: ConvexExecutableQuery,
        auth: Option<&InvocationAuth>,
        cancellation: &HostCallCancellation,
    ) -> Result<Value, Error> {
        if let Some(execution_unit) = self.mutation_execution_unit() {
            let mut check_cancel = || check_host_cancellation(cancellation);
            return match query {
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
            };
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
            let mut check_cancel = || check_host_cancellation(cancellation);
            return match query {
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
            };
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
            let mut check_cancel = || check_host_cancellation(cancellation);
            return execution_unit.paginate_documents_cancellable(
                &PaginatedQuery {
                    query,
                    page_size,
                    after,
                },
                &mut check_cancel,
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
            let mut check_cancel = || check_host_cancellation(cancellation);
            return execution_unit.paginate_documents_cancellable(
                &PaginatedQuery {
                    query,
                    page_size,
                    after,
                },
                &mut check_cancel,
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
                ConvexExecutableMutation::Mutation(mutation) => match mutation {
                    Mutation::Insert { table, fields } => execution_unit
                        .insert_document(table, fields)
                        .map(|id| Value::String(id.to_string())),
                    Mutation::Update { table, id, patch } => execution_unit
                        .update_document(table, id, patch)
                        .map(|id| Value::String(id.to_string())),
                    Mutation::Delete { table, id } => execution_unit
                        .delete_document(table, id)
                        .map(|_| Value::Null),
                },
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
                ConvexExecutableMutation::Mutation(mutation) => match mutation {
                    Mutation::Insert { table, fields } => execution_unit
                        .insert_document(table, fields)
                        .map(|id| Value::String(id.to_string())),
                    Mutation::Update { table, id, patch } => execution_unit
                        .update_document(table, id, patch)
                        .map(|id| Value::String(id.to_string())),
                    Mutation::Delete { table, id } => execution_unit
                        .delete_document(table, id)
                        .map(|_| Value::Null),
                },
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

    pub(in crate::adapters::convex) async fn invoke_ctx_query_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeQueryPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        self.record_executable_query_read(&payload.query);
        let query = payload.query;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = self
            .execute_query_with_execution_context_async_cancellable(
                query.clone(),
                self.auth.as_ref(),
                cancellation,
            )
            .await
            .inspect(|value| self.record_query_result_value(&query, value));
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_query(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_query_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_query_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeQueryPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        self.record_executable_query_read(&payload.query);
        let query = payload.query;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = self
            .execute_query_with_execution_context_cancellable(
                query.clone(),
                self.auth.as_ref(),
                cancellation,
            )
            .inspect(|value| self.record_query_result_value(&query, value));
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) async fn invoke_ctx_paginated_query_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimePaginatedQueryPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        let table = payload.query.table.clone();
        let query = payload.query;
        let after = payload.cursor.map(Cursor);
        let page_size = payload.page_size;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = self
            .paginate_query_with_execution_context_async_cancellable(
                query.clone(),
                page_size,
                after.clone(),
                &self.principal,
                cancellation,
            )
            .await
            .and_then(|mut page| {
                synthesize_runtime_paginate_cursor(&query, page_size, &mut page)?;
                self.record_paginated_window_read(&query, page_size, after.as_ref(), &page);
                let value = serde_json::to_value(page)
                    .map_err(|error| Error::Serialization(error.to_string()))?;
                self.record_result_documents(&table, &value);
                Ok(value)
            });
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_paginated_query(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_paginated_query_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_paginated_query_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimePaginatedQueryPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        let table = payload.query.table.clone();
        let query = payload.query;
        let after = payload.cursor.map(Cursor);
        let page_size = payload.page_size;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = self
            .paginate_query_with_execution_context_cancellable(
                query.clone(),
                page_size,
                after.clone(),
                &self.principal,
                cancellation,
            )
            .and_then(|mut page| {
                synthesize_runtime_paginate_cursor(&query, page_size, &mut page)?;
                self.record_paginated_window_read(&query, page_size, after.as_ref(), &page);
                let value = serde_json::to_value(page)
                    .map_err(|error| Error::Serialization(error.to_string()))?;
                self.record_result_documents(&table, &value);
                Ok(value)
            });
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) async fn invoke_ctx_mutation_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeMutationPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = self
            .dispatch_convex_mutation_with_execution_context_async_cancellable(
                payload.mutation,
                self.auth.as_ref(),
                cancellation,
            )
            .await;
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_mutation(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_mutation_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_mutation_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeMutationPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = self.dispatch_convex_mutation_with_execution_context_cancellable(
            payload.mutation,
            self.auth.as_ref(),
            cancellation,
        );
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) async fn invoke_ctx_action_async_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeActionPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = execute_convex_action_async(
            &self.service,
            &self.registry,
            &self.tenant_id,
            payload.action,
            self.auth.as_ref(),
            Some(cancellation.clone()),
        )
        .await;
        encode_runtime_core_result(response)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_action(
        &self,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let cancellation = HostCallCancellation::default();
        self.invoke_ctx_action_cancellable(payload, &cancellation)
    }

    pub(in crate::adapters::convex) fn invoke_ctx_action_cancellable(
        &self,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let payload: ConvexRuntimeActionPayload = serde_json::from_value(payload)?;
        self.validate_session(payload.session_id.as_deref())?;
        ensure_runtime_host_not_cancelled(cancellation)?;
        let response = execute_convex_action_cancellable_with_auth(
            &self.service,
            &self.registry,
            &self.tenant_id,
            payload.action,
            self.auth.as_ref(),
            cancellation,
        );
        encode_runtime_core_result(response)
    }
}
