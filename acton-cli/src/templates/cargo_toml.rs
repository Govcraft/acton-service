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

    let mut content = format!(
r#"[package]
name = "{}"
version = "0.1.0"
edition = "2021"

[dependencies]
acton-service = {{ version = "0.2.0"{} }}
tokio = {{ version = "1", features = ["full"] }}
serde = {{ version = "1.0", features = ["derive"] }}
serde_json = "1.0"
anyhow = "1.0"
tracing = "0.1"
"#,
        template.name, features_str
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
