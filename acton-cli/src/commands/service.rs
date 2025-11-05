use anyhow::Result;
use clap::Subcommand;

mod new;
mod add;
mod generate;
mod validate;
mod dev;

#[derive(Subcommand)]
pub enum ServiceCommands {
    /// Create a new microservice
    New {
        /// Service name (kebab-case)
        #[arg(value_name = "SERVICE_NAME")]
        name: String,

        /// Enable HTTP REST API
        #[arg(long, default_value = "true")]
        http: bool,

        /// Enable gRPC service
        #[arg(long)]
        grpc: bool,

        /// Enable both HTTP and gRPC
        #[arg(long)]
        full: bool,

        /// Add database support
        #[arg(long, value_name = "TYPE")]
        database: Option<String>,

        /// Add caching support
        #[arg(long, value_name = "TYPE")]
        cache: Option<String>,

        /// Add event streaming support
        #[arg(long, value_name = "TYPE")]
        events: Option<String>,

        /// Add authentication
        #[arg(long, value_name = "TYPE")]
        auth: Option<String>,

        /// Enable observability (OpenTelemetry)
        #[arg(long)]
        observability: bool,

        /// Enable resilience patterns
        #[arg(long)]
        resilience: bool,

        /// Enable rate limiting
        #[arg(long, name = "rate-limit")]
        rate_limit: bool,

        /// Generate OpenAPI/Swagger
        #[arg(long)]
        openapi: bool,

        /// Use organization template
        #[arg(long, value_name = "NAME")]
        template: Option<String>,

        /// Create in specific directory
        #[arg(long, value_name = "DIR")]
        path: Option<String>,

        /// Skip git initialization
        #[arg(long, name = "no-git")]
        no_git: bool,

        /// Interactive mode (prompt for options)
        #[arg(short, long)]
        interactive: bool,

        /// Accept all defaults
        #[arg(short, long)]
        yes: bool,

        /// Show what would be generated
        #[arg(long, name = "dry-run")]
        dry_run: bool,
    },

    /// Add components to existing service
    #[command(subcommand)]
    Add(AddCommands),

    /// Generate configuration files
    #[command(subcommand)]
    Generate(GenerateCommands),

    /// Validate service against best practices
    Validate {
        /// Path to service directory
        #[arg(value_name = "PATH", default_value = ".")]
        path: String,

        /// Run specific check
        #[arg(long, value_name = "TYPE")]
        check: Option<String>,

        /// Run all checks
        #[arg(long)]
        all: bool,

        /// Focus on deployment readiness
        #[arg(long)]
        deployment: bool,

        /// Focus on security checks
        #[arg(long)]
        security: bool,

        /// Output format
        #[arg(long, value_name = "FORMAT", default_value = "text")]
        format: String,

        /// Show detailed output
        #[arg(short, long)]
        verbose: bool,

        /// Only show errors and score
        #[arg(short, long)]
        quiet: bool,

        /// CI-friendly output
        #[arg(long)]
        ci: bool,

        /// Minimum passing score
        #[arg(long, value_name = "SCORE", default_value = "8.0")]
        min_score: f32,

        /// Treat warnings as errors
        #[arg(long)]
        strict: bool,

        /// Auto-fix issues where possible
        #[arg(long)]
        fix: bool,

        /// Write report to file
        #[arg(long, value_name = "FILE")]
        report: Option<String>,
    },

    /// Development tools
    #[command(subcommand)]
    Dev(DevCommands),
}

#[derive(Subcommand)]
pub enum AddCommands {
    /// Add HTTP endpoint
    #[command(disable_version_flag = true)]
    Endpoint {
        /// HTTP method
        #[arg(value_name = "METHOD")]
        method: String,

        /// Route path
        #[arg(value_name = "PATH")]
        path: String,

        /// API version
        #[arg(long, value_name = "VERSION", default_value = "v1")]
        version: String,

        /// Handler function name
        #[arg(long, value_name = "NAME")]
        handler: Option<String>,

        /// Require authentication
        #[arg(long, value_name = "TYPE")]
        auth: Option<String>,

        /// Rate limit (requests per minute)
        #[arg(long, value_name = "LIMIT")]
        rate_limit: Option<u32>,

        /// Generate associated model struct
        #[arg(long, value_name = "NAME")]
        model: Option<String>,

        /// Add request validation
        #[arg(long)]
        validate: bool,

        /// Response type
        #[arg(long, value_name = "TYPE", default_value = "json")]
        response: String,

        /// Add caching layer
        #[arg(long)]
        cache: bool,

        /// Publish event after success
        #[arg(long, value_name = "NAME")]
        event: Option<String>,

        /// Add OpenAPI annotations
        #[arg(long)]
        openapi: bool,

        /// Show what would be generated
        #[arg(long, name = "dry-run")]
        dry_run: bool,
    },

    /// Add gRPC service
    Grpc {
        /// Service name (PascalCase)
        #[arg(value_name = "SERVICE_NAME")]
        service_name: String,

        /// Proto package
        #[arg(long, value_name = "NAME")]
        package: Option<String>,

        /// Add RPC method
        #[arg(long, value_name = "NAME")]
        method: Option<String>,

        /// Request message type
        #[arg(long, value_name = "TYPE")]
        request: Option<String>,

        /// Response message type
        #[arg(long, value_name = "TYPE")]
        response: Option<String>,

        /// Enable health checks
        #[arg(long, default_value = "true")]
        health: bool,

        /// Enable server reflection
        #[arg(long, default_value = "true")]
        reflection: bool,

        /// Add streaming support
        #[arg(long)]
        streaming: bool,

        /// Generate handler implementation
        #[arg(long)]
        handler: bool,

        /// Generate client code
        #[arg(long)]
        client: bool,

        /// Add interceptor
        #[arg(long, value_name = "TYPE")]
        interceptor: Option<String>,

        /// Show what would be generated
        #[arg(long, name = "dry-run")]
        dry_run: bool,
    },

    /// Add background worker
    Worker {
        /// Worker name
        #[arg(value_name = "NAME")]
        name: String,

        /// Event source
        #[arg(long, value_name = "SOURCE")]
        source: String,

        /// Stream name
        #[arg(long, value_name = "NAME")]
        stream: String,

        /// NATS subject pattern
        #[arg(long, value_name = "PATTERN")]
        subject: Option<String>,

        /// Show what would be generated
        #[arg(long, name = "dry-run")]
        dry_run: bool,
    },

    /// Add middleware
    Middleware {
        /// Middleware type
        #[arg(value_name = "TYPE")]
        middleware_type: String,

        /// Show what would be generated
        #[arg(long, name = "dry-run")]
        dry_run: bool,
    },

    /// Add API version
    #[command(disable_version_flag = true)]
    Version {
        /// Version name
        #[arg(value_name = "VERSION")]
        version: String,

        /// Copy routes from version
        #[arg(long, value_name = "FROM")]
        from: Option<String>,

        /// Show what would be generated
        #[arg(long, name = "dry-run")]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
pub enum GenerateCommands {
    /// Generate deployment configurations
    Deployment {
        /// Target platform
        #[arg(long, value_name = "TYPE")]
        platform: Option<String>,

        /// Generate for all platforms
        #[arg(long)]
        all: bool,

        /// Number of replicas
        #[arg(long, value_name = "N", default_value = "3")]
        replicas: u32,

        /// Enable HorizontalPodAutoscaler
        #[arg(long)]
        hpa: bool,

        /// Memory limit
        #[arg(long, value_name = "SIZE", default_value = "512Mi")]
        memory: String,

        /// CPU limit
        #[arg(long, value_name = "MILLICORES", default_value = "500m")]
        cpu: String,

        /// Kubernetes namespace
        #[arg(long, value_name = "NAME")]
        namespace: Option<String>,

        /// Generate ServiceMonitor for Prometheus
        #[arg(long)]
        monitoring: bool,

        /// Generate PrometheusRule alerts
        #[arg(long)]
        alerts: bool,

        /// Generate Ingress resource
        #[arg(long)]
        ingress: bool,

        /// Enable TLS/HTTPS
        #[arg(long)]
        tls: bool,

        /// Environment
        #[arg(long, value_name = "STAGE")]
        env: Option<String>,

        /// Container registry URL
        #[arg(long, value_name = "URL")]
        registry: Option<String>,

        /// Image tag
        #[arg(long, value_name = "TAG", default_value = "latest")]
        image_tag: String,

        /// Show what would be generated
        #[arg(long, name = "dry-run")]
        dry_run: bool,

        /// Output directory
        #[arg(long, value_name = "DIR", default_value = "./deployment")]
        output: String,
    },

    /// Generate configuration file
    Config {
        /// Output path
        #[arg(long, value_name = "PATH")]
        output: Option<String>,

        /// Include examples
        #[arg(long)]
        examples: bool,

        /// Show what would be generated
        #[arg(long, name = "dry-run")]
        dry_run: bool,
    },

    /// Generate proto file
    Proto {
        /// Service name
        #[arg(value_name = "SERVICE")]
        service: String,

        /// Output path
        #[arg(long, value_name = "PATH")]
        output: Option<String>,

        /// Show what would be generated
        #[arg(long, name = "dry-run")]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
pub enum DevCommands {
    /// Run development server
    Run {
        /// Watch for changes and reload
        #[arg(long)]
        watch: bool,

        /// Port to listen on
        #[arg(long, value_name = "PORT")]
        port: Option<u16>,
    },

    /// Check service health
    Health {
        /// Show detailed output
        #[arg(long)]
        verbose: bool,

        /// Service URL
        #[arg(long, value_name = "URL", default_value = "http://localhost:8080")]
        url: String,
    },

    /// View logs
    Logs {
        /// Follow log output
        #[arg(short, long)]
        follow: bool,

        /// Filter by log level
        #[arg(long, value_name = "LEVEL")]
        level: Option<String>,

        /// Filter by pattern
        #[arg(long, value_name = "PATTERN")]
        filter: Option<String>,
    },
}

pub async fn execute(command: ServiceCommands) -> Result<()> {
    match command {
        ServiceCommands::New {
            name,
            http,
            grpc,
            full,
            database,
            cache,
            events,
            auth,
            observability,
            resilience,
            rate_limit,
            openapi,
            template,
            path,
            no_git,
            interactive,
            yes,
            dry_run,
        } => {
            new::execute(
                name,
                http,
                grpc,
                full,
                database,
                cache,
                events,
                auth,
                observability,
                resilience,
                rate_limit,
                openapi,
                template,
                path,
                no_git,
                interactive,
                yes,
                dry_run,
            )
            .await
        }
        ServiceCommands::Add(add_command) => match add_command {
            AddCommands::Endpoint {
                method,
                path,
                version,
                handler,
                auth,
                rate_limit,
                model,
                validate,
                response,
                cache,
                event,
                openapi,
                dry_run,
            } => {
                add::endpoint::execute(
                    method, path, version, handler, auth, rate_limit, model, validate, response,
                    cache, event, openapi, dry_run,
                )
                .await
            }
            AddCommands::Grpc {
                service_name,
                package,
                method,
                request,
                response,
                health,
                reflection,
                streaming,
                handler,
                client,
                interceptor,
                dry_run,
            } => {
                add::grpc::execute(
                    service_name,
                    package,
                    method,
                    request,
                    response,
                    health,
                    reflection,
                    streaming,
                    handler,
                    client,
                    interceptor,
                    dry_run,
                )
                .await
            }
            AddCommands::Worker {
                name,
                source,
                stream,
                subject,
                dry_run,
            } => add::worker::execute(name, source, stream, subject, dry_run).await,
            AddCommands::Middleware {
                middleware_type,
                dry_run,
            } => add::middleware::execute(middleware_type, dry_run).await,
            AddCommands::Version {
                version,
                from,
                dry_run,
            } => add::version::execute(version, from, dry_run).await,
        },
        ServiceCommands::Generate(gen_command) => match gen_command {
            GenerateCommands::Deployment {
                platform,
                all,
                replicas,
                hpa,
                memory,
                cpu,
                namespace,
                monitoring,
                alerts,
                ingress,
                tls,
                env,
                registry,
                image_tag,
                dry_run,
                output,
            } => {
                generate::deployment::execute(
                    platform, all, replicas, hpa, memory, cpu, namespace, monitoring, alerts,
                    ingress, tls, env, registry, image_tag, dry_run, output,
                )
                .await
            }
            GenerateCommands::Config {
                output,
                examples,
                dry_run,
            } => generate::config::execute(output, examples, dry_run).await,
            GenerateCommands::Proto {
                service,
                output,
                dry_run,
            } => generate::proto::execute(service, output, dry_run).await,
        },
        ServiceCommands::Validate {
            path,
            check,
            all,
            deployment,
            security,
            format,
            verbose,
            quiet,
            ci,
            min_score,
            strict,
            fix,
            report,
        } => {
            validate::execute(
                path, check, all, deployment, security, format, verbose, quiet, ci, min_score,
                strict, fix, report,
            )
            .await
        }
        ServiceCommands::Dev(dev_command) => match dev_command {
            DevCommands::Run { watch, port } => dev::run::execute(watch, port).await,
            DevCommands::Health { verbose, url } => dev::health::execute(verbose, url).await,
            DevCommands::Logs {
                follow,
                level,
                filter,
            } => dev::logs::execute(follow, level, filter).await,
        },
    }
}
