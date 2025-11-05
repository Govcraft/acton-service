fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "grpc")]
    {
        let out_dir = std::env::var("OUT_DIR")?;

        // Compile ping.proto for ping-pong example
        tonic_prost_build::configure()
            .file_descriptor_set_path(format!("{}/ping_descriptor.bin", out_dir))
            .compile_protos(&["proto/ping.proto"], &["proto"])?;

        // Compile orders.proto for event-driven example
        tonic_prost_build::configure()
            .file_descriptor_set_path(format!("{}/orders_descriptor.bin", out_dir))
            .compile_protos(&["proto/orders.proto"], &["proto"])?;
    }
    Ok(())
}
