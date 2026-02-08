use anyhow::{Context, Result};
use colored::Colorize;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct HealthResponse {
    status: String,
    #[serde(default)]
    timestamp: Option<String>,
}

pub async fn execute(verbose: bool, url: String) -> Result<()> {
    println!("{}", "Checking service health...".bold());
    println!();

    let base_url = url.trim_end_matches('/');

    // Check health endpoint
    let health_url = format!("{}/health", base_url);
    print!("Health endpoint ({})... ", health_url);

    match check_endpoint(&health_url, verbose).await {
        Ok(response) => {
            println!("{}", "✓ OK".green().bold());
            if verbose {
                println!("  Status: {}", response.status);
                if let Some(timestamp) = response.timestamp {
                    println!("  Timestamp: {}", timestamp);
                }
            }
        }
        Err(e) => {
            println!("{}", "✗ FAILED".red().bold());
            println!("  Error: {}", e);
            return Err(e);
        }
    }

    // Check readiness endpoint
    let ready_url = format!("{}/ready", base_url);
    print!("Readiness endpoint ({})... ", ready_url);

    match check_endpoint(&ready_url, verbose).await {
        Ok(response) => {
            println!("{}", "✓ OK".green().bold());
            if verbose {
                println!("  Status: {}", response.status);
                if let Some(timestamp) = response.timestamp {
                    println!("  Timestamp: {}", timestamp);
                }
            }
        }
        Err(e) => {
            println!("{}", "✗ FAILED".red().bold());
            println!("  Error: {}", e);
            return Err(e);
        }
    }

    println!();
    println!("{}", "Service is healthy and ready!".green().bold());

    Ok(())
}

async fn check_endpoint(url: &str, verbose: bool) -> Result<HealthResponse> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .context("Failed to create HTTP client")?;

    let response = client
        .get(url)
        .send()
        .await
        .context("Failed to send request")?;

    let status = response.status();

    if verbose {
        println!();
        println!("  HTTP Status: {}", status);
    }

    if !status.is_success() {
        anyhow::bail!(
            "HTTP {}: {}",
            status.as_u16(),
            status.canonical_reason().unwrap_or("Unknown")
        );
    }

    let body = response
        .text()
        .await
        .context("Failed to read response body")?;

    if verbose {
        println!("  Response: {}", body);
    }

    // Try to parse as JSON, fall back to simple status check
    match serde_json::from_str::<HealthResponse>(&body) {
        Ok(health) => Ok(health),
        Err(_) => {
            // If not JSON, just check if response contains "ok" or "healthy"
            if body.to_lowercase().contains("ok") || body.to_lowercase().contains("healthy") {
                Ok(HealthResponse {
                    status: "ok".to_string(),
                    timestamp: None,
                })
            } else {
                anyhow::bail!("Unexpected response format: {}", body)
            }
        }
    }
}
