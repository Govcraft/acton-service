use anyhow::Result;

#[allow(clippy::too_many_arguments)]
pub async fn execute(
    _method: String,
    _path: String,
    _version: String,
    _handler: Option<String>,
    _auth: Option<String>,
    _rate_limit: Option<u32>,
    _model: Option<String>,
    _validate: bool,
    _response: String,
    _cache: bool,
    _event: Option<String>,
    _openapi: bool,
    _dry_run: bool,
) -> Result<()> {
    println!("Adding endpoint... (not yet implemented)");
    Ok(())
}
