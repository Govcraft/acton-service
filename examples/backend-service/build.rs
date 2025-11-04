fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Compile the user service proto file
    tonic_prost_build::compile_protos("../../proto/user_service.proto")?;

    // Rerun if proto files change
    println!("cargo:rerun-if-changed=../../proto/user_service.proto");
    println!("cargo:rerun-if-changed=build.rs");

    Ok(())
}
