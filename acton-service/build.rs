fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Which XML-DSig backend the SAML service provider is compiled against.
    // The saml-rs target tables in Cargo.toml select aws-lc on Linux
    // x86_64/aarch64 and RustCrypto everywhere else; RustCrypto's RSA
    // key-transport decryption needs an explicit opt-in (RUSTSEC-2023-0071),
    // so the code needs to know which one it got. Keep this predicate in step
    // with those tables.
    println!("cargo::rustc-check-cfg=cfg(saml_backend_rustcrypto)");
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let aws_lc_capable = target_os == "linux" && matches!(target_arch.as_str(), "x86_64" | "aarch64");
    if !aws_lc_capable {
        println!("cargo::rustc-cfg=saml_backend_rustcrypto");
    }

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

        println!(
            "cargo:warning=Compiled ping.proto -> {}/ping_descriptor.bin",
            out_dir
        );

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
