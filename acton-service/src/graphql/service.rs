//! Internal Tower `Service` that wraps an `async_graphql::Executor` and
//! propagates Axum request extensions (notably [`Claims`]) into the GraphQL
//! request's per-request data.
//!
//! This is a deliberately thin wrapper around the official
//! `async_graphql_axum::GraphQL` service. The only behavior added is:
//!
//! * Claims and `CedarAuthz` (under `graphql-cedar`) are copied from the Axum
//!   `Extensions` map into the GraphQL `Request::data` map so resolvers can
//!   pull them out via `Context::data::<T>()`.
//! * For batch requests the same data is cloned into every batched query.

use std::{
    convert::Infallible,
    pin::Pin,
    task::{Context as TaskContext, Poll},
    time::Duration,
};

use async_graphql::{
    http::{create_multipart_mixed_stream, is_accept_multipart_mixed},
    BatchRequest, Executor,
};
use async_graphql_axum::{
    rejection::GraphQLRejection, GraphQLBatchRequest, GraphQLRequest, GraphQLResponse,
};
use axum::{
    body::{Body, HttpBody},
    extract::FromRequest,
    http::{Request as HttpRequest, Response as HttpResponse},
    response::IntoResponse,
    BoxError,
};
use bytes::Bytes;
use futures::{Future, StreamExt};
use tower::Service;

use crate::middleware::token::Claims;

#[cfg(feature = "graphql-cedar")]
use crate::middleware::cedar::CedarAuthz;

/// Wraps an `Executor` and forwards claims (and Cedar) from request extensions.
pub(crate) struct ActonGraphQL<E> {
    executor: E,
    #[cfg(feature = "graphql-cedar")]
    cedar: Option<CedarAuthz>,
}

impl<E: Clone> Clone for ActonGraphQL<E> {
    fn clone(&self) -> Self {
        Self {
            executor: self.executor.clone(),
            #[cfg(feature = "graphql-cedar")]
            cedar: self.cedar.clone(),
        }
    }
}

impl<E> ActonGraphQL<E> {
    pub(crate) fn new(
        executor: E,
        #[cfg(feature = "graphql-cedar")] cedar: Option<CedarAuthz>,
    ) -> Self {
        Self {
            executor,
            #[cfg(feature = "graphql-cedar")]
            cedar,
        }
    }
}

impl<B, E> Service<HttpRequest<B>> for ActonGraphQL<E>
where
    B: HttpBody<Data = Bytes> + Send + 'static,
    B::Data: Into<Bytes>,
    B::Error: Into<BoxError>,
    E: Executor,
{
    type Response = HttpResponse<Body>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut TaskContext<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: HttpRequest<B>) -> Self::Future {
        let executor = self.executor.clone();
        let claims = req.extensions().get::<Claims>().cloned();
        #[cfg(feature = "graphql-cedar")]
        let cedar = self.cedar.clone();
        let req = req.map(Body::new);

        Box::pin(async move {
            let is_accept_multipart_mixed = req
                .headers()
                .get("accept")
                .and_then(|value| value.to_str().ok())
                .map(is_accept_multipart_mixed)
                .unwrap_or_default();

            if is_accept_multipart_mixed {
                let graphql_req =
                    match GraphQLRequest::<GraphQLRejection>::from_request(req, &()).await {
                        Ok(r) => r,
                        Err(err) => return Ok(err.into_response()),
                    };
                let mut single = graphql_req.into_inner();
                if let Some(ref c) = claims {
                    single = single.data(c.clone());
                }
                #[cfg(feature = "graphql-cedar")]
                if let Some(ref ca) = cedar {
                    single = single.data(ca.clone());
                }
                let stream = executor.execute_stream(single, None);
                let body = Body::from_stream(
                    create_multipart_mixed_stream(stream, Duration::from_secs(30))
                        .map(Ok::<_, std::io::Error>),
                );
                Ok(HttpResponse::builder()
                    .header("content-type", "multipart/mixed; boundary=graphql")
                    .body(body)
                    .expect("BUG: invalid response"))
            } else {
                let graphql_req =
                    match GraphQLBatchRequest::<GraphQLRejection>::from_request(req, &()).await {
                        Ok(r) => r,
                        Err(err) => return Ok(err.into_response()),
                    };
                let batch = inject_data(
                    graphql_req.into_inner(),
                    claims,
                    #[cfg(feature = "graphql-cedar")]
                    cedar,
                );
                Ok(GraphQLResponse(executor.execute_batch(batch).await).into_response())
            }
        })
    }
}

fn inject_data(
    batch: BatchRequest,
    claims: Option<Claims>,
    #[cfg(feature = "graphql-cedar")] cedar: Option<CedarAuthz>,
) -> BatchRequest {
    let inject = move |mut req: async_graphql::Request| -> async_graphql::Request {
        if let Some(c) = claims.clone() {
            req = req.data(c);
        }
        #[cfg(feature = "graphql-cedar")]
        if let Some(ca) = cedar.clone() {
            req = req.data(ca);
        }
        req
    };
    match batch {
        BatchRequest::Single(single) => BatchRequest::Single(inject(single)),
        BatchRequest::Batch(batch) => BatchRequest::Batch(batch.into_iter().map(inject).collect()),
    }
}
