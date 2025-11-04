//! gRPC middleware utilities
//!
//! Provides Tower middleware that can be used with gRPC services.

use std::time::Instant;
use tonic::{Request, Response, Status};
use tower::{Layer, Service};
use std::task::{Context, Poll};
use std::pin::Pin;
use std::future::Future;

/// Logging middleware for gRPC requests
#[derive(Clone)]
pub struct LoggingLayer;

impl<S> Layer<S> for LoggingLayer {
    type Service = LoggingService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        LoggingService { inner }
    }
}

/// Logging service implementation
#[derive(Clone)]
pub struct LoggingService<S> {
    inner: S,
}

impl<S, ReqBody> Service<Request<ReqBody>> for LoggingService<S>
where
    S: Service<Request<ReqBody>, Response = Response<tonic::body::Body>, Error = Status> + Clone + Send + 'static,
    S::Future: Send + 'static,
    ReqBody: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let start = Instant::now();

            tracing::debug!("gRPC request started");

            let result = inner.call(req).await;

            let duration = start.elapsed();

            match &result {
                Ok(_) => {
                    tracing::info!(
                        duration_ms = duration.as_millis(),
                        "gRPC request completed"
                    );
                }
                Err(status) => {
                    tracing::warn!(
                        duration_ms = duration.as_millis(),
                        status_code = ?status.code(),
                        "gRPC request failed"
                    );
                }
            }

            result
        })
    }
}
