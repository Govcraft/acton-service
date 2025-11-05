//! gRPC server implementation

use crate::config::GrpcConfig;
use crate::error::Result;
use crate::state::AppState;
use std::net::SocketAddr;
use tonic::transport::Server;
use tonic::server::NamedService;
// Reflection types are not used directly, we use the Builder API

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
/// Supports optional health check and reflection services.
pub struct GrpcServicesBuilder {
    routes: tonic::service::Routes,
    reflection_enabled: bool,
    health_enabled: bool,
    file_descriptor_sets: Vec<&'static [u8]>,
}

impl GrpcServicesBuilder {
    /// Create a new services builder
    pub fn new() -> Self {
        Self {
            routes: tonic::service::Routes::default(),
            reflection_enabled: false,
            health_enabled: false,
            file_descriptor_sets: Vec::new(),
        }
    }

    /// Enable gRPC reflection service
    ///
    /// This allows tools like `grpcurl` and Postman to discover available services.
    /// Requires that file descriptor sets are registered using `add_file_descriptor_set()`.
    ///
    /// # Example
    /// ```ignore
    /// use tonic::include_file_descriptor_set;
    ///
    /// const FILE_DESCRIPTOR_SET: &[u8] = include_file_descriptor_set!("my_service_descriptor");
    ///
    /// let services = GrpcServicesBuilder::new()
    ///     .with_reflection()
    ///     .add_file_descriptor_set(FILE_DESCRIPTOR_SET)
    ///     .add_service(MyServiceServer::new(my_service))
    ///     .build();
    /// ```
    pub fn with_reflection(mut self) -> Self {
        self.reflection_enabled = true;
        self
    }

    /// Enable gRPC health check service
    ///
    /// Adds the standard gRPC health checking protocol to the server.
    /// The health service will check all configured dependencies (database, Redis, NATS).
    pub fn with_health(mut self) -> Self {
        self.health_enabled = true;
        self
    }

    /// Add a file descriptor set for reflection
    ///
    /// This is required when reflection is enabled. File descriptor sets are
    /// generated at build time using `tonic-build`.
    ///
    /// # Example
    /// ```ignore
    /// const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("my_service_descriptor");
    ///
    /// builder.add_file_descriptor_set(FILE_DESCRIPTOR_SET);
    /// ```
    pub fn add_file_descriptor_set(mut self, file_descriptor_set: &'static [u8]) -> Self {
        self.file_descriptor_sets.push(file_descriptor_set);
        self
    }

    /// Add a gRPC service to the builder
    ///
    /// # Example
    /// ```ignore
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
        self.routes = self.routes.add_service(service);
        self
    }

    /// Build the routes
    ///
    /// If health or reflection are enabled, they will be added automatically.
    ///
    /// # Arguments
    /// * `state` - Optional AppState, required if health checks are enabled
    pub fn build(mut self, state: Option<AppState>) -> tonic::service::Routes {
        // Add health service if enabled
        if self.health_enabled {
            if let Some(app_state) = state.clone() {
                let health_service = crate::grpc::HealthService::new(app_state);
                let health_server = tonic_health::pb::health_server::HealthServer::new(health_service);

                self.routes = self.routes.add_service(health_server);

                tracing::info!("gRPC health service enabled");
            } else {
                tracing::warn!("Health service enabled but no AppState provided, skipping health service");
            }
        }

        // Add reflection service if enabled
        if self.reflection_enabled {
            if self.file_descriptor_sets.is_empty() {
                tracing::warn!("Reflection enabled but no file descriptor sets registered. Use add_file_descriptor_set() to register services.");
            } else {
                // Build the reflection service with all registered file descriptor sets
                let mut reflection_builder = tonic_reflection::server::Builder::configure();

                for file_descriptor_set in self.file_descriptor_sets {
                    reflection_builder = reflection_builder
                        .register_encoded_file_descriptor_set(file_descriptor_set);
                }

                match reflection_builder.build_v1() {
                    Ok(reflection_service) => {
                        self.routes = self.routes.add_service(reflection_service);

                        tracing::info!("gRPC reflection service enabled");
                    }
                    Err(e) => {
                        tracing::error!("Failed to build reflection service: {}", e);
                    }
                }
            }
        }

        self.routes
    }
}

impl Default for GrpcServicesBuilder {
    fn default() -> Self {
        Self::new()
    }
}
