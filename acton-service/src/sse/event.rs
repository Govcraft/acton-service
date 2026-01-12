//! SSE event construction and serialization helpers.

use axum::response::sse::Event;
use serde::Serialize;
use std::time::Duration;

/// Extension trait for building SSE events with common patterns.
pub trait SseEventExt {
    /// Create an event with JSON-serialized data.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails.
    fn json<T: Serialize>(data: &T) -> Result<Event, serde_json::Error>;

    /// Create a named event with JSON data.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails.
    fn json_named<T: Serialize>(event_name: &str, data: &T) -> Result<Event, serde_json::Error>;
}

impl SseEventExt for Event {
    fn json<T: Serialize>(data: &T) -> Result<Event, serde_json::Error> {
        let json = serde_json::to_string(data)?;
        Ok(Event::default().data(json))
    }

    fn json_named<T: Serialize>(event_name: &str, data: &T) -> Result<Event, serde_json::Error> {
        let json = serde_json::to_string(data)?;
        Ok(Event::default().event(event_name).data(json))
    }
}

/// Helper for creating typed SSE events.
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::sse::TypedEvent;
///
/// #[derive(Serialize)]
/// struct NotificationData {
///     message: String,
///     level: String,
/// }
///
/// let event = TypedEvent::new(NotificationData {
///     message: "Hello!".to_string(),
///     level: "info".to_string(),
/// })
/// .event_type("notification")
/// .id("event-123")
/// .into_event()?;
/// ```
#[derive(Debug, Clone)]
pub struct TypedEvent<T> {
    /// Event type name (optional).
    pub event_type: Option<String>,
    /// Event ID for reconnection (optional).
    pub id: Option<String>,
    /// Retry interval hint (optional).
    pub retry: Option<Duration>,
    /// Event payload.
    pub data: T,
}

impl<T: Serialize> TypedEvent<T> {
    /// Create a new typed event.
    #[must_use]
    pub fn new(data: T) -> Self {
        Self {
            event_type: None,
            id: None,
            retry: None,
            data,
        }
    }

    /// Set the event type name.
    #[must_use]
    pub fn event_type(mut self, name: impl Into<String>) -> Self {
        self.event_type = Some(name.into());
        self
    }

    /// Set the event ID.
    #[must_use]
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Set the retry interval.
    #[must_use]
    pub fn retry(mut self, retry: Duration) -> Self {
        self.retry = Some(retry);
        self
    }

    /// Convert to axum Event.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn into_event(self) -> Result<Event, serde_json::Error> {
        let json = serde_json::to_string(&self.data)?;
        let mut event = Event::default().data(json);

        if let Some(name) = self.event_type {
            event = event.event(name);
        }
        if let Some(id) = self.id {
            event = event.id(id);
        }
        if let Some(retry) = self.retry {
            event = event.retry(retry);
        }

        Ok(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    struct TestData {
        message: String,
    }

    #[test]
    fn test_event_json() {
        let data = TestData {
            message: "Hello".to_string(),
        };
        let event = Event::json(&data);
        assert!(event.is_ok());
    }

    #[test]
    fn test_event_json_named() {
        let data = TestData {
            message: "Hello".to_string(),
        };
        let event = Event::json_named("test", &data);
        assert!(event.is_ok());
    }

    #[test]
    fn test_typed_event() {
        let event = TypedEvent::new(TestData {
            message: "Hello".to_string(),
        })
        .event_type("notification")
        .id("event-1")
        .retry(Duration::from_secs(5));

        assert_eq!(event.event_type, Some("notification".to_string()));
        assert_eq!(event.id, Some("event-1".to_string()));
        assert_eq!(event.retry, Some(Duration::from_secs(5)));

        let axum_event = event.into_event();
        assert!(axum_event.is_ok());
    }
}
