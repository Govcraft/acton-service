use anyhow::Result;

#[allow(clippy::too_many_arguments)]
pub async fn execute(
    _service_name: String,
    _package: Option<String>,
    _method: Option<String>,
    _request: Option<String>,
    _response: Option<String>,
    _health: bool,
    _reflection: bool,
    _streaming: bool,
    _handler: bool,
    _client: bool,
    _interceptor: Option<String>,
    _dry_run: bool,
) -> Result<()> {
    println!("Adding gRPC service... (not yet implemented)");
    Ok(())
}
