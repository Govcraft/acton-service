pub struct WorkerTemplate {
    pub name: String,
    pub source: String,
    pub stream: String,
    pub subject: Option<String>,
}

/// Generate a background worker module
pub fn generate_worker(template: &WorkerTemplate) -> String {
    match template.source.as_str() {
        "nats" => generate_nats_worker(template),
        "redis-stream" | "redis" => generate_redis_worker(template),
        _ => generate_generic_worker(template),
    }
}

fn generate_nats_worker(template: &WorkerTemplate) -> String {
    let subject = template.subject.as_deref().unwrap_or("events.>");

    format!(
        r#"use anyhow::Result;
use async_nats::jetstream;
use futures::StreamExt;
use tracing::{{info, error}};

pub struct {}Worker {{
    context: jetstream::Context,
}}

impl {}Worker {{
    pub fn new(context: jetstream::Context) -> Self {{
        Self {{ context }}
    }}

    pub async fn run(&self) -> Result<()> {{
        info!("Starting {} worker");

        // Get or create stream
        let stream = self.context
            .get_or_create_stream(jetstream::stream::Config {{
                name: "{}".to_string(),
                subjects: vec!["{}".to_string()],
                ..Default::default()
            }})
            .await?;

        // Create consumer
        let consumer = stream
            .get_or_create_consumer(
                "{}",
                jetstream::consumer::pull::Config {{
                    durable_name: Some("{}".to_string()),
                    ..Default::default()
                }},
            )
            .await?;

        // Process messages
        let mut messages = consumer.messages().await?;

        while let Some(message) = messages.next().await {{
            match message {{
                Ok(msg) => {{
                    if let Err(e) = self.process_message(&msg).await {{
                        error!("Failed to process message: {{:?}}", e);
                        // Optionally: msg.ack_with(AckKind::Nak).await?;
                    }} else {{
                        msg.ack().await?;
                    }}
                }}
                Err(e) => {{
                    error!("Error receiving message: {{:?}}", e);
                }}
            }}
        }}

        Ok(())
    }}

    async fn process_message(&self, message: &jetstream::Message) -> Result<()> {{
        // TODO: Implement message processing logic
        let payload = std::str::from_utf8(&message.payload)?;
        info!("Processing message: {{}}", payload);

        // Your business logic here

        Ok(())
    }}
}}
"#,
        to_pascal_case(&template.name),
        to_pascal_case(&template.name),
        template.name,
        template.stream,
        subject,
        template.name,
        template.name,
    )
}

fn generate_redis_worker(template: &WorkerTemplate) -> String {
    format!(
        r#"use anyhow::Result;
use redis::{{AsyncCommands, Client}};
use tracing::{{info, error}};

pub struct {}Worker {{
    client: Client,
}}

impl {}Worker {{
    pub fn new(client: Client) -> Self {{
        Self {{ client }}
    }}

    pub async fn run(&self) -> Result<()> {{
        info!("Starting {} worker for stream {{}}", "{}");

        let mut conn = self.client.get_multiplexed_async_connection().await?;

        loop {{
            // Read from Redis stream
            let result: Vec<Vec<(String, Vec<(String, String)>)>> = conn
                .xread_options(
                    &["{}"],
                    &[">"],
                    &redis::streams::StreamReadOptions::default()
                        .count(10)
                        .block(5000),
                )
                .await?;

            for stream_data in result {{
                for (_stream_key, entries) in stream_data {{
                    for (message_id, fields) in entries {{
                        if let Err(e) = self.process_message(&message_id, &fields).await {{
                            error!("Failed to process message {{}}: {{:?}}", message_id, e);
                        }}
                    }}
                }}
            }}
        }}
    }}

    async fn process_message(&self, message_id: &str, fields: &[(String, String)]) -> Result<()> {{
        // TODO: Implement message processing logic
        info!("Processing message {{}}: {{:?}}", message_id, fields);

        // Your business logic here

        Ok(())
    }}
}}
"#,
        to_pascal_case(&template.name),
        to_pascal_case(&template.name),
        template.name,
        template.stream,
        template.stream,
    )
}

fn generate_generic_worker(template: &WorkerTemplate) -> String {
    let pascal_name = to_pascal_case(&template.name);

    format!(
        r#"use anyhow::Result;
use tracing::info;

pub struct {pascal_name}Worker {{
    // Add your dependencies here
}}

impl {pascal_name}Worker {{
    pub fn new() -> Self {{
        Self {{}}
    }}

    pub async fn run(&self) -> Result<()> {{
        info!("Starting {name} worker for source: {source}");

        // TODO: Implement worker loop
        loop {{
            // Your worker logic here
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }}
    }}

    async fn process_event(&self) -> Result<()> {{
        // TODO: Implement event processing logic
        Ok(())
    }}
}}
"#,
        pascal_name = pascal_name,
        name = template.name,
        source = template.source,
    )
}

fn to_pascal_case(s: &str) -> String {
    s.split(&['-', '_'][..])
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect()
}
