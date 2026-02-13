//! JSONL archival for audit events before retention purge
//!
//! Archives events as one-JSON-per-line files before they are permanently
//! deleted by the retention cleanup process.

use std::path::{Path, PathBuf};

use chrono::Utc;

use super::event::AuditEvent;
use crate::error::Error;

/// Archive a batch of audit events to a JSONL file.
///
/// Creates `archive_dir` if it doesn't exist and writes events as
/// newline-delimited JSON to `audit_archive_YYYYMMDD_HHMMSS.jsonl`.
///
/// Returns the path to the created archive file.
pub async fn archive_events(events: &[AuditEvent], archive_dir: &Path) -> Result<PathBuf, Error> {
    if events.is_empty() {
        return Err(Error::Internal("No events to archive".into()));
    }

    tokio::fs::create_dir_all(archive_dir).await.map_err(|e| {
        Error::Internal(format!(
            "Failed to create archive directory {}: {}",
            archive_dir.display(),
            e
        ))
    })?;

    let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
    let filename = format!("audit_archive_{}.jsonl", timestamp);
    let filepath = archive_dir.join(&filename);

    let mut lines = String::new();
    for event in events {
        let line = serde_json::to_string(event).map_err(|e| {
            Error::Internal(format!(
                "Failed to serialize audit event for archive: {}",
                e
            ))
        })?;
        lines.push_str(&line);
        lines.push('\n');
    }

    tokio::fs::write(&filepath, lines.as_bytes())
        .await
        .map_err(|e| {
            Error::Internal(format!(
                "Failed to write archive file {}: {}",
                filepath.display(),
                e
            ))
        })?;

    tracing::info!(
        "Archived {} audit events to {}",
        events.len(),
        filepath.display()
    );

    Ok(filepath)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::event::{AuditEventKind, AuditSeverity};

    fn make_test_event(seq: u64) -> AuditEvent {
        let mut event = AuditEvent::new(
            AuditEventKind::HttpRequest,
            AuditSeverity::Informational,
            "test-service".to_string(),
        );
        event.sequence = seq;
        event.hash = Some(format!("hash_{}", seq));
        event
    }

    #[tokio::test]
    async fn test_archive_events_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let events = vec![make_test_event(1), make_test_event(2), make_test_event(3)];

        let path = archive_events(&events, dir.path()).await.unwrap();
        assert!(path.exists());
        assert!(path.extension().unwrap() == "jsonl");

        let contents = tokio::fs::read_to_string(&path).await.unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 3);

        // Each line should be valid JSON
        for line in &lines {
            let event: AuditEvent = serde_json::from_str(line).unwrap();
            assert_eq!(event.service_name, "test-service");
        }
    }

    #[tokio::test]
    async fn test_archive_events_creates_directory() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a").join("b").join("c");
        let events = vec![make_test_event(1)];

        let path = archive_events(&events, &nested).await.unwrap();
        assert!(path.exists());
        assert!(nested.exists());
    }

    #[tokio::test]
    async fn test_archive_events_empty_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let result = archive_events(&[], dir.path()).await;
        assert!(result.is_err());
    }
}
