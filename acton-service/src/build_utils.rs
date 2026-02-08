//! Build-time utilities for compiling protocol buffers
//!
//! This module provides helpers for `build.rs` scripts in projects using acton-service.
//!
//! ## Important Note
//!
//! These utilities run at **compile time** (in build.rs), not at runtime.
//! They cannot access XDG config files or other runtime configuration.
//!
//! ## Configuration Priority
//!
//! Proto location is determined in this order:
//! 1. `ACTON_PROTO_DIR` environment variable
//! 2. `proto/` directory (convention)
//!
//! ## Usage in Your Project
//!
//! ```rust,no_run
//! // In your project's build.rs
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     #[cfg(feature = "grpc")]
//!     {
//!         // Use default convention (proto/ directory)
//!         acton_service::build_utils::compile_service_protos()?;
//!
//!         // Or specify a custom directory
//!         acton_service::build_utils::compile_protos_from_dir("my-protos")?;
//!     }
//!     Ok(())
//! }
//! ```
//!
//! ## Environment Variables
//!
//! - `ACTON_PROTO_DIR`: Override the default proto directory location
//! - `CARGO_PKG_NAME`: Used to generate descriptor file names (automatically set by Cargo)
//! - `OUT_DIR`: Where generated files are placed (automatically set by Cargo)

use std::path::{Path, PathBuf};

/// Error type for build utilities
#[derive(Debug)]
pub enum BuildError {
    Io(std::io::Error),
    ProtoBuild(Box<dyn std::error::Error>),
    NoProtoFiles(PathBuf),
    InvalidProtoDir(PathBuf),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::Io(e) => write!(f, "IO error: {}", e),
            BuildError::ProtoBuild(e) => write!(f, "Proto compilation error: {}", e),
            BuildError::NoProtoFiles(dir) => {
                write!(f, "No .proto files found in directory: {}", dir.display())
            }
            BuildError::InvalidProtoDir(dir) => write!(
                f,
                "Proto directory does not exist or is not a directory: {}",
                dir.display()
            ),
        }
    }
}

impl std::error::Error for BuildError {}

impl From<std::io::Error> for BuildError {
    fn from(e: std::io::Error) -> Self {
        BuildError::Io(e)
    }
}

pub type BuildResult<T> = Result<T, BuildError>;

/// Compile all `.proto` files from a directory
///
/// This function:
/// - Discovers all `.proto` files in the specified directory
/// - Compiles them using `tonic_build`
/// - Generates a file descriptor set for gRPC reflection
/// - Names the descriptor based on the crate name
///
/// # Arguments
///
/// * `proto_dir` - Directory containing `.proto` files
///
/// # Example
///
/// ```rust,no_run
/// // In build.rs
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     acton_service::build_utils::compile_protos_from_dir("proto")?;
///     Ok(())
/// }
/// ```
pub fn compile_protos_from_dir<P: AsRef<Path>>(proto_dir: P) -> BuildResult<()> {
    let proto_dir = proto_dir.as_ref();

    // Validate directory exists
    if !proto_dir.exists() || !proto_dir.is_dir() {
        return Err(BuildError::InvalidProtoDir(proto_dir.to_path_buf()));
    }

    // Discover all .proto files
    let proto_files: Vec<PathBuf> = discover_proto_files(proto_dir)?;

    if proto_files.is_empty() {
        return Err(BuildError::NoProtoFiles(proto_dir.to_path_buf()));
    }

    // Get output directory and package name from cargo
    let out_dir = std::env::var("OUT_DIR").map_err(|e| {
        BuildError::ProtoBuild(Box::new(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("OUT_DIR not set: {}", e),
        )))
    })?;

    let pkg_name = std::env::var("CARGO_PKG_NAME")
        .map_err(|e| {
            BuildError::ProtoBuild(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("CARGO_PKG_NAME not set: {}", e),
            )))
        })?
        .replace('-', "_");

    // Build descriptor set path
    let descriptor_path = format!("{}/{}_descriptor.bin", out_dir, pkg_name);

    println!(
        "cargo:warning=Compiling {} proto files from {}",
        proto_files.len(),
        proto_dir.display()
    );
    for proto_file in &proto_files {
        println!("cargo:warning=  - {}", proto_file.display());
    }

    // Compile protos
    compile_protos_with_descriptor(&proto_files, proto_dir, &descriptor_path)?;

    println!("cargo:warning=Generated descriptor: {}", descriptor_path);

    Ok(())
}

/// Compile service protos using convention-based or environment-configured location
///
/// This is the recommended entry point for acton-service projects.
///
/// Location priority:
/// 1. `ACTON_PROTO_DIR` environment variable
/// 2. `proto/` directory (convention)
///
/// # Example
///
/// ```rust,no_run
/// // In build.rs - uses default "proto/" directory
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     acton_service::build_utils::compile_service_protos()?;
///     Ok(())
/// }
/// ```
///
/// ```bash
/// # Override proto location at build time
/// ACTON_PROTO_DIR=../shared/protos cargo build
/// ```
pub fn compile_service_protos() -> BuildResult<()> {
    let proto_dir = std::env::var("ACTON_PROTO_DIR").unwrap_or_else(|_| "proto".to_string());

    println!("cargo:warning=Using proto directory: {}", proto_dir);

    compile_protos_from_dir(proto_dir)
}

/// Discover all `.proto` files recursively in a directory
fn discover_proto_files(dir: &Path) -> BuildResult<Vec<PathBuf>> {
    let mut proto_files = Vec::new();

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext == "proto" {
                    proto_files.push(path);
                }
            }
        } else if path.is_dir() {
            // Recursively search subdirectories
            proto_files.extend(discover_proto_files(&path)?);
        }
    }

    // Sort for deterministic builds
    proto_files.sort();

    Ok(proto_files)
}

/// Compile proto files with descriptor generation
fn compile_protos_with_descriptor(
    proto_files: &[PathBuf],
    proto_include_dir: &Path,
    descriptor_path: &str,
) -> BuildResult<()> {
    // Convert proto files to string refs
    let proto_paths: Vec<&str> = proto_files
        .iter()
        .map(|p| p.to_str().expect("Invalid UTF-8 in proto path"))
        .collect();

    let include_dirs = vec![proto_include_dir
        .to_str()
        .expect("Invalid UTF-8 in include path")];

    // Use tonic-prost-build to compile protos with file descriptor support
    #[cfg(feature = "grpc")]
    {
        tonic_prost_build::configure()
            .file_descriptor_set_path(descriptor_path)
            .compile_protos(&proto_paths, &include_dirs)
            .map_err(|e| BuildError::ProtoBuild(Box::new(e)))?;
        Ok(())
    }

    #[cfg(not(feature = "grpc"))]
    {
        let _ = (proto_paths, include_dirs, descriptor_path);
        Err(BuildError::ProtoBuild(Box::new(std::io::Error::other(
            "grpc feature not enabled",
        ))))
    }
}

/// Advanced: Compile specific proto files with custom configuration
///
/// Use this when you need fine-grained control over proto compilation.
///
/// # Example
///
/// ```rust,no_run
/// // In build.rs
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     acton_service::build_utils::compile_specific_protos(
///         &["proto/orders.proto", "proto/users.proto"],
///         &["proto"],
///         "my_descriptor.bin"
///     )?;
///     Ok(())
/// }
/// ```
pub fn compile_specific_protos<P: AsRef<Path>>(
    proto_files: &[P],
    include_dirs: &[P],
    descriptor_name: &str,
) -> BuildResult<()> {
    let out_dir = std::env::var("OUT_DIR").map_err(|e| {
        BuildError::ProtoBuild(Box::new(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("OUT_DIR not set: {}", e),
        )))
    })?;

    let descriptor_path = format!("{}/{}", out_dir, descriptor_name);

    let proto_paths: Vec<&str> = proto_files
        .iter()
        .map(|p| p.as_ref().to_str().expect("Invalid UTF-8 in proto path"))
        .collect();

    let include_paths: Vec<&str> = include_dirs
        .iter()
        .map(|p| p.as_ref().to_str().expect("Invalid UTF-8 in include path"))
        .collect();

    compile_protos_with_descriptor(
        &proto_paths.iter().map(PathBuf::from).collect::<Vec<_>>(),
        Path::new(include_paths[0]),
        &descriptor_path,
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discover_proto_files() {
        // This test would require setting up a test proto directory
        // For now, just ensure the function signature is correct
        let _result: BuildResult<Vec<PathBuf>> = discover_proto_files(Path::new("nonexistent"));
    }
}
