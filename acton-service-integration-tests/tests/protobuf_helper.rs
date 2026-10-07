#![cfg(feature = "grpc")]

use acton_service::build_utils::{compile_protos_from_dir, BuildError};
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn public_helper_generates_service_code_and_reflection_descriptor() {
    const CHILD_ENV: &str = "ACTON_TEST_PROTO_HELPER_DIR";
    if let Ok(proto_dir) = std::env::var(CHILD_ENV) {
        compile_protos_from_dir(proto_dir).expect("compile with public build helper");
        return;
    }

    let temp = tempfile::tempdir().expect("temporary build directory");
    let proto_dir = temp.path().join("proto");
    let output_dir = temp.path().join("generated");
    std::fs::create_dir_all(proto_dir.join("nested")).expect("create nested proto directory");
    std::fs::create_dir(&output_dir).expect("create output directory");
    assert!(matches!(
        compile_protos_from_dir(temp.path().join("missing")),
        Err(BuildError::InvalidProtoDir(_))
    ));
    assert!(matches!(
        compile_protos_from_dir(&proto_dir),
        Err(BuildError::NoProtoFiles(_))
    ));
    std::fs::write(
        proto_dir.join("nested/ping.proto"),
        include_str!("../proto/ping.proto"),
    )
    .expect("write nested service fixture");
    let compiled = Command::new(std::env::current_exe().expect("current test executable"))
        .args([
            "--exact",
            "public_helper_generates_service_code_and_reflection_descriptor",
            "--nocapture",
        ])
        .env(CHILD_ENV, &proto_dir)
        .env("OUT_DIR", &output_dir)
        .env("CARGO_PKG_NAME", "acton-proto-smoke")
        .output()
        .expect("run isolated build-helper process");
    assert!(
        compiled.status.success(),
        "public helper failed: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let generated = std::fs::read_to_string(output_dir.join("ping.v1.rs"))
        .expect("generated Rust service code");
    assert!(generated.contains("pub struct PingRequest"));
    assert!(generated.contains("pub mod ping_service_client"));
    assert!(generated.contains("pub mod ping_service_server"));
    let descriptor = std::fs::read(output_dir.join("acton_proto_smoke_descriptor.bin"))
        .expect("package-named reflection descriptor");
    let mut decoder = Command::new("protoc")
        .arg("--decode_raw")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("decode reflection descriptor");
    decoder
        .stdin
        .take()
        .expect("descriptor input")
        .write_all(&descriptor)
        .expect("send descriptor to protoc");
    let decoded = decoder.wait_with_output().expect("read descriptor fields");
    assert!(
        decoded.status.success(),
        "descriptor must decode as protobuf"
    );
    let fields = String::from_utf8(decoded.stdout).expect("UTF-8 descriptor fields");
    for identity in [
        "nested/ping.proto",
        "ping.v1",
        "PingService",
        ".ping.v1.PingRequest",
        ".ping.v1.PongResponse",
    ] {
        assert!(
            fields.contains(identity),
            "descriptor missing {identity}: {fields}"
        );
    }
}
