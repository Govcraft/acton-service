fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "grpc")]
    {
        let out = std::env::var("OUT_DIR")?;
        for (proto, descriptor) in [
            ("ping", "ping_descriptor"),
            ("orders", "orders_descriptor"),
            ("hello", "hello_descriptor"),
        ] {
            let path = format!("proto/{proto}.proto");
            println!("cargo:rerun-if-changed={path}");
            tonic_prost_build::configure()
                .file_descriptor_set_path(format!("{out}/{descriptor}.bin"))
                .compile_protos(&[path], &["proto"])?;
        }
    }
    Ok(())
}
