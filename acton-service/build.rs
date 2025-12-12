fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Propagate ACTON_DATABASE_URL to DATABASE_URL for SQLx compile-time query checks.
    // This allows users to use a single environment variable (ACTON_DATABASE_URL) for both
    // runtime connections and SQLx's compile-time verification macros (query!, query_as!).
    // Only set DATABASE_URL if it's not already set (user's explicit DATABASE_URL takes precedence).
    #[cfg(feature = "database")]
    {
        if std::env::var("DATABASE_URL").is_err() {
            if let Ok(acton_url) = std::env::var("ACTON_DATABASE_URL") {
                println!("cargo:rustc-env=DATABASE_URL={}", acton_url);
            }
        }
    }

    #[cfg(feature = "grpc")]
    {
        // Compile example protos
        // NOTE: acton-service's build.rs must use tonic_build directly,
        // since it can't reference the crate being built.
        //
        // ⚠️  CONSUMING PROJECTS should use: acton_service::build_utils::compile_service_protos()
        //    This is demonstrated in the example comments below.
        let out_dir = std::env::var("OUT_DIR")?;

        // Ping-pong example
        tonic_prost_build::configure()
            .file_descriptor_set_path(format!("{}/ping_descriptor.bin", out_dir))
            .compile_protos(&["proto/ping.proto"], &["proto"])?;

        println!("cargo:warning=Compiled ping.proto -> {}/ping_descriptor.bin", out_dir);

        // Event-driven example
        tonic_prost_build::configure()
            .file_descriptor_set_path(format!("{}/orders_descriptor.bin", out_dir))
            .compile_protos(&["proto/orders.proto"], &["proto"])?;

        println!(
            "cargo:warning=Compiled orders.proto -> {}/orders_descriptor.bin",
            out_dir
        );

        // Single-port example
        tonic_prost_build::configure()
            .file_descriptor_set_path(format!("{}/hello_descriptor.bin", out_dir))
            .compile_protos(&["proto/hello.proto"], &["proto"])?;

        println!(
            "cargo:warning=Compiled hello.proto -> {}/hello_descriptor.bin",
            out_dir
        );

        println!("cargo:warning=");
        println!("cargo:warning=💡 In YOUR project's build.rs, use:");
        println!("cargo:warning=   acton_service::build_utils::compile_service_protos()");
        println!("cargo:warning=   This will automatically compile all protos in proto/");
        println!("cargo:warning=");
        println!("cargo:warning=   Example build.rs:");
        println!("cargo:warning=   fn main() -> Result<(), Box<dyn std::error::Error>> {{");
        println!("cargo:warning=       #[cfg(feature = \"grpc\")]");
        println!("cargo:warning=       acton_service::build_utils::compile_service_protos()?;");
        println!("cargo:warning=       Ok(())");
        println!("cargo:warning=   }}");
    }
    Ok(())
}
