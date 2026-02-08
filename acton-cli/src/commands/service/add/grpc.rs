use anyhow::Result;
use colored::Colorize;

#[allow(clippy::too_many_arguments)]
pub async fn execute(
    service_name: String,
    package: Option<String>,
    method: Option<String>,
    request: Option<String>,
    response: Option<String>,
    health: bool,
    reflection: bool,
    streaming: bool,
    handler: bool,
    client: bool,
    interceptor: Option<String>,
    dry_run: bool,
) -> Result<()> {
    if dry_run {
        show_dry_run(&service_name, &package, &method);
        return Ok(());
    }

    println!(
        "{}",
        format!("Adding gRPC service: {}", service_name).bold()
    );
    println!();

    // Determine package name
    let package_name = package
        .clone()
        .unwrap_or_else(|| format!("{}.v1", service_name.to_lowercase()));

    println!("{}: {}", "Package".cyan().bold(), package_name.cyan());
    println!();

    // Step 1: Show proto file setup
    println!("{}", "1. Create proto file:".green().bold());
    println!();
    show_proto_file(
        &service_name,
        &package_name,
        &method,
        &request,
        &response,
        streaming,
    );
    println!();

    // Step 2: Show build.rs setup
    println!("{}", "2. Set up build.rs:".green().bold());
    println!();
    show_build_setup();
    println!();

    // Step 3: Show Cargo.toml feature setup
    println!("{}", "3. Enable gRPC feature in Cargo.toml:".green().bold());
    println!();
    show_cargo_setup();
    println!();

    // Step 4: Show service implementation
    if handler {
        println!("{}", "4. Implement gRPC service:".green().bold());
        println!();
        show_service_implementation(&service_name, &package_name, &method, streaming);
        println!();
    }

    // Step 5: Show server setup
    let server_step = if handler { "5" } else { "4" };
    println!(
        "{}",
        format!("{}. Set up gRPC server:", server_step)
            .green()
            .bold()
    );
    println!();
    show_server_setup(&service_name, &package_name, health, reflection);
    println!();

    // Step 6: Show client setup if requested
    if client {
        let client_step = if handler { "6" } else { "5" };
        println!(
            "{}",
            format!("{}. Create gRPC client:", client_step)
                .green()
                .bold()
        );
        println!();
        show_client_setup(&service_name, &package_name);
        println!();
    }

    // Step 7: Show interceptor setup if requested
    if let Some(ref interceptor_type) = interceptor {
        let step = if client && handler {
            "7"
        } else if client || handler {
            "6"
        } else {
            "5"
        };
        println!("{}", format!("{}. Add interceptor:", step).green().bold());
        println!();
        show_interceptor_setup(interceptor_type);
        println!();
    }

    // Final notes
    println!("{}", "Build and test:".cyan().bold());
    println!("  cargo build --features grpc");
    println!("  cargo run --features grpc");
    println!();
    println!("{}", "Test with grpcurl:".cyan().bold());
    println!("  grpcurl -plaintext localhost:9090 list");
    println!(
        "  grpcurl -plaintext -d '{{}}' localhost:9090 {}.{}/MethodName",
        package_name, service_name
    );
    println!();

    println!("{}", "Learn more:".yellow().bold());
    println!("  See acton-service/examples/ping-pong.rs for a complete example");
    println!("  See acton-service/examples/event-driven.rs for HTTP + gRPC");

    Ok(())
}

fn show_dry_run(service_name: &str, package: &Option<String>, method: &Option<String>) {
    println!("\n{}", "Dry run - would show:".bold());
    println!();
    println!("Service: {}", service_name.cyan());
    if let Some(pkg) = package {
        println!("Package: {}", pkg.cyan());
    }
    if let Some(mtd) = method {
        println!("Method: {}", mtd.cyan());
    }
    println!();
    println!("Instructions for adding gRPC service {}", service_name);
}

fn show_proto_file(
    service_name: &str,
    package_name: &str,
    method: &Option<String>,
    request: &Option<String>,
    response: &Option<String>,
    streaming: bool,
) {
    let method_name = method.as_deref().unwrap_or("Execute");

    let default_request = format!("{}Request", method_name);
    let default_response = format!("{}Response", method_name);

    let request_type = request.as_deref().unwrap_or(&default_request);
    let response_type = response.as_deref().unwrap_or(&default_response);

    println!("   Create proto/{}.proto:", service_name.to_lowercase());
    println!();
    println!("   syntax = \"proto3\";");
    println!();
    println!("   package {};", package_name);
    println!();
    println!("   service {} {{", service_name);

    if streaming {
        println!("     // Server streaming");
        println!(
            "     rpc {}({}) returns (stream {});",
            method_name, request_type, response_type
        );
        println!("     // Or bidirectional streaming:");
        println!(
            "     // rpc {}(stream {}) returns (stream {});",
            method_name, request_type, response_type
        );
    } else {
        println!(
            "     rpc {}({}) returns ({});",
            method_name, request_type, response_type
        );
    }

    println!("   }}");
    println!();
    println!("   message {} {{", request_type);
    println!("     string id = 1;");
    println!("     // Add your fields");
    println!("   }}");
    println!();
    println!("   message {} {{", response_type);
    println!("     string result = 1;");
    println!("     // Add your fields");
    println!("   }}");
}

fn show_build_setup() {
    println!("   Create or update build.rs:");
    println!();
    println!("   fn main() -> Result<(), Box<dyn std::error::Error>> {{");
    println!("       #[cfg(feature = \"grpc\")]");
    println!("       {{");
    println!("           acton_service::build_utils::compile_service_protos()?;");
    println!("       }}");
    println!("       Ok(())");
    println!("   }}");
    println!();
    println!(
        "   {}: See acton-service/examples/build.rs.example",
        "Example".yellow()
    );
}

fn show_cargo_setup() {
    println!("   [dependencies]");
    println!(r#"   acton-service = {{ version = "0.2", features = ["grpc"] }}"#);
    println!();
    println!("   [build-dependencies]");
    println!(r#"   acton-service = {{ version = "0.2", features = ["build-utils"] }}"#);
}

fn show_service_implementation(
    service_name: &str,
    package_name: &str,
    method: &Option<String>,
    streaming: bool,
) {
    let method_name = method.as_deref().unwrap_or("Execute");
    let method_snake = method_name.to_lowercase();
    let pkg_mod = package_name.replace('.', "_");

    println!("   // Include generated code");
    println!("   pub mod {} {{", pkg_mod);
    println!(r#"       tonic::include_proto!("{}");"#, package_name);
    println!();
    println!("       pub const FILE_DESCRIPTOR_SET: &[u8] =");
    println!(
        r#"           tonic::include_file_descriptor_set!("{}_descriptor");"#,
        pkg_mod
    );
    println!("   }}");
    println!();
    println!("   use {}::{{", pkg_mod);
    println!(
        "       {}_service_server::{{{}Service, {}ServiceServer}},",
        service_name.to_lowercase(),
        service_name,
        service_name
    );
    println!("       {}Request, {}Response,", method_name, method_name);
    println!("   }};");
    println!();
    println!("   #[derive(Default)]");
    println!("   struct {}ServiceImpl {{}}", service_name);
    println!();
    println!("   #[tonic::async_trait]");
    println!(
        "   impl {}Service for {}ServiceImpl {{",
        service_name, service_name
    );

    if streaming {
        println!(
            "       type {}Stream = tokio_stream::wrappers::ReceiverStream<",
            method_name
        );
        println!("           Result<{}Response, tonic::Status>", method_name);
        println!("       >;");
        println!();
        println!("       async fn {}(", method_snake);
        println!("           &self,");
        println!(
            "           request: tonic::Request<{}Request>,",
            method_name
        );
        println!(
            "       ) -> Result<tonic::Response<Self::{}Stream>, tonic::Status> {{",
            method_name
        );
        println!("           let req = request.into_inner();");
        println!("           let (tx, rx) = tokio::sync::mpsc::channel(128);");
        println!();
        println!("           // Stream responses");
        println!("           tokio::spawn(async move {{");
        println!("               for i in 0..10 {{");
        println!(
            "                   let response = {}Response {{",
            method_name
        );
        println!(r#"                       result: format!("Item {{}}", i),"#);
        println!("                   }};");
        println!("                   if tx.send(Ok(response)).await.is_err() {{");
        println!("                       break;");
        println!("                   }}");
        println!("               }}");
        println!("           }});");
        println!();
        println!("           Ok(tonic::Response::new(");
        println!("               tokio_stream::wrappers::ReceiverStream::new(rx)");
        println!("           ))");
        println!("       }}");
    } else {
        println!("       async fn {}(", method_snake);
        println!("           &self,");
        println!(
            "           request: tonic::Request<{}Request>,",
            method_name
        );
        println!(
            "       ) -> Result<tonic::Response<{}Response>, tonic::Status> {{",
            method_name
        );
        println!("           let req = request.into_inner();");
        println!();
        println!("           tracing::info!(id = %req.id, \"Processing request\");");
        println!();
        println!("           let response = {}Response {{", method_name);
        println!(r#"               result: "Success".to_string(),"#);
        println!("           }};");
        println!();
        println!("           Ok(tonic::Response::new(response))");
        println!("       }}");
    }

    println!("   }}");
}

fn show_server_setup(service_name: &str, package_name: &str, health: bool, reflection: bool) {
    let pkg_mod = package_name.replace('.', "_");

    println!("   use acton_service::grpc::server::GrpcServicesBuilder;");
    println!();
    println!("   let grpc_addr = \"0.0.0.0:9090\".parse().unwrap();");
    println!();
    println!("   let service = {}ServiceImpl::default();", service_name);
    println!();
    println!("   let mut builder = GrpcServicesBuilder::new()");

    if health {
        println!("       .with_health()");
    }
    if reflection {
        println!("       .with_reflection()");
        println!(
            "       .add_file_descriptor_set({}::FILE_DESCRIPTOR_SET)",
            pkg_mod
        );
    }

    println!(
        "       .add_service({}ServiceServer::new(service));",
        service_name
    );
    println!();
    println!("   let router = builder.build(None).unwrap();");
    println!();
    println!("   // Serve");
    println!("   router.serve(grpc_addr).await?;");
}

fn show_client_setup(service_name: &str, package_name: &str) {
    let pkg_mod = package_name.replace('.', "_");

    println!(
        "   use {}::{}_service_client::{}ServiceClient;",
        pkg_mod,
        service_name.to_lowercase(),
        service_name
    );
    println!();
    println!("   // Connect to gRPC server");
    println!(
        r#"   let mut client = {}ServiceClient::connect("http://localhost:9090")"#,
        service_name
    );
    println!("       .await?;");
    println!();
    println!("   // Make request");
    println!(
        "   let request = tonic::Request::new({}Request {{",
        service_name
    );
    println!(r#"       id: "123".to_string(),"#);
    println!("   }});");
    println!();
    println!("   let response = client.execute(request).await?;");
    println!("   println!(\"Response: {{:?}}\", response.into_inner());");
}

fn show_interceptor_setup(interceptor_type: &str) {
    match interceptor_type.to_lowercase().as_str() {
        "auth" | "jwt" => {
            println!("   use acton_service::grpc::interceptors::JwtAuthInterceptor;");
            println!();
            println!("   let interceptor = JwtAuthInterceptor::new(");
            println!(r#"       "path/to/public.pem".into(),"#);
            println!(r#"       "RS256".to_string()"#);
            println!("   );");
            println!();
            println!("   // Add to server");
            println!("   .add_service(");
            println!("       YourServiceServer::with_interceptor(service, interceptor)");
            println!("   )");
        }
        "logging" | "tracing" => {
            println!("   use acton_service::grpc::interceptors::TracingInterceptor;");
            println!();
            println!("   let interceptor = TracingInterceptor;");
            println!();
            println!("   // Add to server");
            println!("   .add_service(");
            println!("       YourServiceServer::with_interceptor(service, interceptor)");
            println!("   )");
        }
        "metrics" => {
            println!("   use acton_service::grpc::interceptors::MetricsInterceptor;");
            println!();
            println!("   let interceptor = MetricsInterceptor::new();");
            println!();
            println!("   // Add to server");
            println!("   .add_service(");
            println!("       YourServiceServer::with_interceptor(service, interceptor)");
            println!("   )");
        }
        _ => {
            println!("   // Custom interceptor");
            println!("   use tonic::{{Request, Status}};");
            println!();
            println!("   fn my_interceptor(req: Request<()>) -> Result<Request<()>, Status> {{");
            println!("       // Add your logic");
            println!("       Ok(req)");
            println!("   }}");
            println!();
            println!("   // Add to server");
            println!("   .add_service(");
            println!("       YourServiceServer::with_interceptor(service, my_interceptor)");
            println!("   )");
        }
    }
}
