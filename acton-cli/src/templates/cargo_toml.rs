use super::ServiceTemplate;

pub fn generate(template: &ServiceTemplate) -> String {
    let features = template.features();
    let features_str = if features.is_empty() {
        String::new()
    } else {
        format!(", features = [{}]",
            features.iter()
                .map(|f| format!("\"{}\"", f))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };

    // Compile-time detection of acton-service path for local development
    // env!("CARGO_MANIFEST_DIR") is set during compilation and baked into the binary
    let acton_service_dep = {
        let cli_manifest_dir = env!("CARGO_MANIFEST_DIR");
        if cli_manifest_dir.ends_with("/acton-cli") {
            let workspace_root = cli_manifest_dir.strip_suffix("/acton-cli").unwrap();
            let acton_service_path = format!("{}/acton-service", workspace_root);
            format!(r#"acton-service = {{ path = "{}"{} }}"#, acton_service_path, features_str)
        } else {
            format!(r#"acton-service = {{ version = "0.2.1"{} }}"#, features_str)
        }
    };

    let mut content = format!(
r#"[package]
name = "{}"
version = "0.1.0"
edition = "2021"

[dependencies]
{}
tokio = {{ version = "1", features = ["full"] }}
serde = {{ version = "1.0", features = ["derive"] }}
serde_json = "1.0"
anyhow = "1.0"
tracing = "0.1"
"#,
        template.name, acton_service_dep
    );

    // Add database dependencies
    if let Some(db) = &template.database {
        if db == "postgres" {
            content.push_str(
r#"sqlx = { version = "0.8", features = ["runtime-tokio", "postgres", "uuid", "chrono"] }
uuid = { version = "1.0", features = ["v4", "serde"] }
"#
            );
        }
    }

    // Add cache dependencies
    if let Some(cache) = &template.cache {
        if cache == "redis" {
            content.push_str("redis = { version = \"0.32\", features = [\"tokio-comp\"] }\n");
        }
    }

    // Add gRPC dependencies
    if template.grpc {
        content.push_str(
r#"
[build-dependencies]
tonic-build = "0.14"
"#
        );
    }

    content
}
