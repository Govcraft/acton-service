//! HTTP session management for server-rendered applications.
//!
//! This module provides cookie-based session management with pluggable storage backends,
//! designed for HTMX and server-rendered use cases.
//!
//! # Features
//!
//! - **Cookie-based sessions**: Automatic session ID management via secure cookies
//! - **Pluggable storage**: In-memory (development) or Redis (production)
//! - **Type-safe session data**: `TypedSession<T>` for automatic serialization
//! - **Flash messages**: One-time messages for post-redirect-get patterns
//! - **CSRF protection**: Token-based protection for form submissions
//!
//! # Feature Flags
//!
//! - `session`: Base session support (required)
//! - `session-memory`: In-memory session store (for development)
//! - `session-redis`: Redis session store (for production)
//!
//! # Quick Start
//!
//! ```toml
//! [dependencies]
//! acton-service = { version = "0.9", features = ["session-memory"] }
//! ```
//!
//! ```toml
//! # config.toml
//! [session]
//! cookie_name = "session_id"
//! expiry_secs = 86400
//! secure = false  # true in production
//! storage = "memory"
//! ```
//!
//! ```rust,ignore
//! use acton_service::prelude::*;
//! use acton_service::session::{Session, FlashMessage, FlashMessages};
//!
//! async fn login(session: Session, Form(creds): Form<LoginForm>) -> impl IntoResponse {
//!     // Validate credentials...
//!     session.insert("user_id", &user.id).await?;
//!     session.cycle_id().await?;  // Regenerate session ID after login
//!
//!     FlashMessages::push(&session, FlashMessage::success("Logged in!")).await?;
//!     Redirect::to("/dashboard")
//! }
//!
//! async fn dashboard(flash: FlashMessages) -> impl IntoResponse {
//!     // Flash messages are automatically consumed
//!     Html(render_page(flash.messages()))
//! }
//! ```

mod config;
mod csrf;
mod extractors;
mod flash;

pub use config::{CsrfConfig, SessionConfig, SessionStorage};
pub use csrf::{csrf_middleware, CsrfLayer, CsrfMiddleware, CsrfToken};
pub use extractors::{AuthSession, SessionAuth, SessionData, TypedSession};
pub use flash::{FlashKind, FlashMessage, FlashMessages};

// Re-export tower-sessions types for convenience
pub use tower_sessions::{Expiry, Session, SessionManagerLayer};

#[cfg(feature = "session-memory")]
pub use tower_sessions_memory_store::MemoryStore;

#[cfg(feature = "session-redis")]
pub use tower_sessions_redis_store::RedisStore;

#[cfg(feature = "session-redis")]
pub use tower_sessions_redis_store::fred;

#[cfg(feature = "session-memory")]
use time::Duration;

#[cfg(feature = "session-redis")]
use crate::error::Result;

/// Create a `SessionManagerLayer` from configuration.
///
/// This is the primary way to create session middleware from your configuration.
/// The layer should be applied to your router in `ServiceBuilder`.
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::session::{create_session_layer, SessionConfig};
///
/// let config = SessionConfig::default();
/// let layer = create_session_layer(&config)?;
/// ```
#[cfg(feature = "session-memory")]
pub fn create_memory_session_layer(config: &SessionConfig) -> SessionManagerLayer<MemoryStore> {
    use tower_sessions::cookie::SameSite;

    let store = MemoryStore::default();

    let expiry = if config.expiry_secs == 0 {
        Expiry::OnSessionEnd
    } else if let Some(inactivity) = config.inactivity_timeout_secs {
        Expiry::OnInactivity(Duration::seconds(inactivity as i64))
    } else {
        Expiry::OnInactivity(Duration::seconds(config.expiry_secs as i64))
    };

    let same_site = match config.same_site.to_lowercase().as_str() {
        "strict" => SameSite::Strict,
        "none" => SameSite::None,
        _ => SameSite::Lax,
    };

    // Clone strings to satisfy 'static lifetime requirements
    let cookie_name = config.cookie_name.clone();
    let cookie_path = config.cookie_path.clone();
    let cookie_domain = config.cookie_domain.clone();

    let mut layer = SessionManagerLayer::new(store)
        .with_name(cookie_name)
        .with_expiry(expiry)
        .with_secure(config.secure)
        .with_http_only(config.http_only)
        .with_same_site(same_site)
        .with_path(cookie_path);

    if let Some(domain) = cookie_domain {
        layer = layer.with_domain(domain);
    }

    layer
}

/// Create a Redis-backed session layer.
///
/// This function creates a new Redis connection pool using the `fred` client
/// and returns a `SessionManagerLayer` configured with the Redis store.
///
/// # Errors
///
/// Returns an error if the Redis URL is not configured or if the connection fails.
#[cfg(feature = "session-redis")]
pub async fn create_redis_session_layer(
    config: &SessionConfig,
    redis_url: &str,
) -> Result<SessionManagerLayer<RedisStore<tower_sessions_redis_store::fred::clients::Pool>>> {
    use crate::error::Error;
    use tower_sessions::cookie::SameSite;
    use tower_sessions_redis_store::fred::prelude::*;

    let redis_config = Config::from_url(redis_url)
        .map_err(|e| Error::Internal(format!("Invalid Redis URL for sessions: {e}")))?;

    let pool = Builder::from_config(redis_config)
        .build_pool(6)
        .map_err(|e| Error::Internal(format!("Failed to create session Redis pool: {e}")))?;

    pool.init()
        .await
        .map_err(|e| Error::Internal(format!("Failed to connect to Redis for sessions: {e}")))?;

    let store = RedisStore::new(pool);

    let expiry = if config.expiry_secs == 0 {
        Expiry::OnSessionEnd
    } else if let Some(inactivity) = config.inactivity_timeout_secs {
        Expiry::OnInactivity(Duration::seconds(inactivity as i64))
    } else {
        Expiry::OnInactivity(Duration::seconds(config.expiry_secs as i64))
    };

    let same_site = match config.same_site.to_lowercase().as_str() {
        "strict" => SameSite::Strict,
        "none" => SameSite::None,
        _ => SameSite::Lax,
    };

    // Clone strings to satisfy 'static lifetime requirements
    let cookie_name = config.cookie_name.clone();
    let cookie_path = config.cookie_path.clone();
    let cookie_domain = config.cookie_domain.clone();

    let mut layer = SessionManagerLayer::new(store)
        .with_name(cookie_name)
        .with_expiry(expiry)
        .with_secure(config.secure)
        .with_http_only(config.http_only)
        .with_same_site(same_site)
        .with_path(cookie_path);

    if let Some(domain) = cookie_domain {
        layer = layer.with_domain(domain);
    }

    Ok(layer)
}
