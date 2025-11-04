//! gRPC server implementation

use crate::config::GrpcConfig;
use crate::error::Result;
use std::net::SocketAddr;
use tonic::transport::Server;
use tonic::server::NamedService;

/// gRPC server builder
///
/// Handles configuration and setup of the gRPC server, including
/// service registration, interceptors, and middleware.
#[derive(Debug)]
pub struct GrpcServer {
    config: GrpcConfig,
}

impl GrpcServer {
    /// Create a new gRPC server with the given configuration
    pub fn new(config: GrpcConfig) -> Self {
        Self {
            config,
        }
    }

    /// Build the tonic server
    ///
    /// Creates a fully configured tonic Server with all middleware,
    /// interceptors, and registered services.
    pub fn build(self) -> Result<Server> {
        let server = Server::builder()
            .max_frame_size(Some(self.config.max_message_size_bytes() as u32))
            .timeout(self.config.timeout())
            .tcp_keepalive(Some(std::time::Duration::from_secs(60)));

        Ok(server)
    }

    /// Get the socket address for the gRPC server
    pub fn socket_addr(&self, http_port: u16) -> SocketAddr {
        let port = self.config.effective_port(http_port);
        SocketAddr::from(([0, 0, 0, 0], port))
    }
}

/// Builder for gRPC services
///
/// Allows adding multiple gRPC services that will be served together.
pub struct GrpcServicesBuilder {
    router: Option<tonic::transport::server::Router>,
}

impl GrpcServicesBuilder {
    /// Create a new services builder
    pub fn new() -> Self {
        Self {
            router: None,
        }
    }

    /// Add a gRPC service to the builder
    ///
    /// # Example
    /// ```ignore
    /// use tonic::transport::Server;
    ///
    /// let services = GrpcServicesBuilder::new()
    ///     .add_service(UserServiceServer::new(user_service))
    ///     .build();
    /// ```
    pub fn add_service<S>(mut self, service: S) -> Self
    where
        S: tower::Service<
            http::Request<tonic::body::Body>,
            Response = http::Response<tonic::body::Body>,
            Error = std::convert::Infallible,
        > + NamedService + Clone + Send + Sync + 'static,
        S::Future: Send + 'static,
    {
        self.router = Some(match self.router {
            Some(router) => router.add_service(service),
            None => Server::builder().add_service(service),
        });
        self
    }

    /// Build the router
    ///
    /// Returns None if no services were added
    pub fn build(self) -> Option<tonic::transport::server::Router> {
        self.router
    }
}

impl Default for GrpcServicesBuilder {
    fn default() -> Self {
        Self::new()
    }
}
