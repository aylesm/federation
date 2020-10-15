use crate::request_pipeline::executor::ExecutionContext;
use crate::transports::http::{GraphQLRequest, GraphQLResponse};
use crate::Result;
use async_trait::async_trait;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::iter::FromIterator;

#[derive(Debug)]
pub struct ServiceDefinition {
    pub url: String,
}

#[async_trait]
pub trait Service {
    async fn send_operation<'schema, 'request>(
        &self,
        context: &ExecutionContext<'schema, 'request>,
        operation: String,
        variables: HashMap<String, Value>,
    ) -> Result<Value>;
}

#[async_trait]
impl Service for ServiceDefinition {
    async fn send_operation<'schema, 'request>(
        &self,
        context: &ExecutionContext<'schema, 'request>,
        operation: String,
        variables: HashMap<String, Value>,
    ) -> Result<Value> {
        let request = GraphQLRequest {
            query: operation,
            operation_name: None,
            variables: Some(Map::from_iter(variables.into_iter()).into()),
        };

        let headers = &context.request_context.header_map;

        let mut request_builder = surf::post(&self.url).header("userId", "1");
        for (&name, &value_bytes) in headers.into_iter() {
            match std::str::from_utf8(value_bytes) {
                Ok(value) => {
                    request_builder = request_builder.header(name, value);
                }
                Err(_e) => {
                    unreachable!("Unexpected UTF-8 error");
                }
            }
        }

        // TODO(ran) FIXME: use a single client, reuse connections.
        let GraphQLResponse { data } = request_builder
            .body(surf::Body::from_json(&request)?)
            .recv_json()
            .await?;

        data.ok_or_else(|| unimplemented!("Handle error cases in send_operation"))
    }
}

// TODO: Add tests

#[cfg(test)]
mod tests {}
