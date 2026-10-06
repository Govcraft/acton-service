fn main() {
    // Which XML-DSig backend the SAML service provider is compiled against.
    // The saml-rs target tables in Cargo.toml select aws-lc on Linux
    // x86_64/aarch64 and RustCrypto everywhere else; RustCrypto's RSA
    // key-transport decryption needs an explicit opt-in (RUSTSEC-2023-0071),
    // so the code needs to know which one it got. Keep this predicate in step
    // with those tables.
    println!("cargo::rustc-check-cfg=cfg(saml_backend_rustcrypto)");
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let aws_lc_capable =
        target_os == "linux" && matches!(target_arch.as_str(), "x86_64" | "aarch64");
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
}
