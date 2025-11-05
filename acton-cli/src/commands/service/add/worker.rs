use anyhow::Result;

pub async fn execute(
    _name: String,
    _source: String,
    _stream: String,
    _subject: Option<String>,
    _dry_run: bool,
) -> Result<()> {
    println!("Adding worker... (not yet implemented)");
    Ok(())
}
