use super::*;

impl ConvexHostBridge {
    pub(in crate::adapters::convex) async fn dispatch_host_call_async(
        &self,
        request: HostCallRequest,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let HostCallRequest { operation, payload } = request;
        let operation = ConvexHostCallOperation::from(operation);
        match operation.family() {
            ConvexHostCallFamily::Function => {
                self.dispatch_function_host_call_async(operation, payload, cancellation)
                    .await
            }
            ConvexHostCallFamily::QueryBuilder => {
                self.dispatch_query_builder_host_call_async(operation, payload, cancellation)
                    .await
            }
            ConvexHostCallFamily::QueryRead => {
                self.dispatch_query_read_host_call_async(operation, payload, cancellation)
                    .await
            }
            ConvexHostCallFamily::Document => {
                self.dispatch_document_host_call_async(operation, payload, cancellation)
                    .await
            }
            ConvexHostCallFamily::Scheduler => {
                self.dispatch_scheduler_host_call_async(operation, payload, cancellation)
                    .await
            }
        }
    }

    pub(in crate::adapters::convex) fn dispatch_host_call_cancellable(
        &self,
        request: HostCallRequest,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let HostCallRequest { operation, payload } = request;
        let operation = ConvexHostCallOperation::from(operation);
        match operation.family() {
            ConvexHostCallFamily::Function => {
                self.dispatch_function_host_call_cancellable(operation, payload, cancellation)
            }
            ConvexHostCallFamily::QueryBuilder => {
                self.dispatch_query_builder_host_call_cancellable(operation, payload, cancellation)
            }
            ConvexHostCallFamily::QueryRead => {
                self.dispatch_query_read_host_call_cancellable(operation, payload, cancellation)
            }
            ConvexHostCallFamily::Document => {
                self.dispatch_document_host_call_cancellable(operation, payload, cancellation)
            }
            ConvexHostCallFamily::Scheduler => {
                self.dispatch_scheduler_host_call_cancellable(operation, payload, cancellation)
            }
        }
    }

    pub(in crate::adapters::convex) fn dispatch_host_call(
        &self,
        request: HostCallRequest,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let HostCallRequest { operation, payload } = request;
        let operation = ConvexHostCallOperation::from(operation);
        match operation.family() {
            ConvexHostCallFamily::Function => self.dispatch_function_host_call(operation, payload),
            ConvexHostCallFamily::QueryBuilder => {
                self.dispatch_query_builder_host_call(operation, payload)
            }
            ConvexHostCallFamily::QueryRead => {
                self.dispatch_query_read_host_call(operation, payload)
            }
            ConvexHostCallFamily::Document => self.dispatch_document_host_call(operation, payload),
            ConvexHostCallFamily::Scheduler => {
                self.dispatch_scheduler_host_call(operation, payload)
            }
        }
    }
}
