//! Construct OAuth providers once from application configuration.

use std::collections::HashMap;

use crate::auth::config::{OAuthConfig, OAuthProviderConfig};
use crate::error::Error;

use super::{CustomOidcConfig, CustomOidcProvider, GitHubProvider, GoogleProvider, OAuthProvider};

/// Configured providers keyed by their login route name.
///
/// The exact keys `google` and `github` select the built-in providers. Every
/// other key selects a custom OIDC provider and requires valid HTTP(S)
/// `authorization_endpoint`, `token_endpoint`, and `userinfo_endpoint` URLs.
/// Invalid configuration fails during construction, before any login starts.
///
/// Keep one registry for the process and share it through `Arc` as needed.
/// The registry constructs providers; applications still own routes and account
/// creation or linking policy. `OAuthConfig::enabled` remains the application's
/// decision about whether to expose those routes.
#[derive(Default)]
pub struct OAuthProviderRegistry {
    providers: HashMap<String, Box<dyn OAuthProvider>>,
}

impl OAuthProviderRegistry {
    /// Construct all providers, rejecting invalid entries at startup.
    pub fn from_config(config: &OAuthConfig) -> Result<Self, Error> {
        let providers = config
            .providers
            .iter()
            .map(|(name, config)| {
                build_provider(name, config).map(|provider| (name.clone(), provider))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { providers })
    }

    /// Construct providers that emit `AuthOAuthCallback` audit events on code
    /// exchange, including failed callbacks.
    ///
    /// Uses the same startup validation and selection rules as [`Self::from_config`].
    #[cfg(feature = "audit")]
    pub fn from_config_audited(
        config: &OAuthConfig,
        logger: crate::audit::AuditLogger,
    ) -> Result<Self, Error> {
        let registry = Self::from_config(config)?;
        let providers = registry
            .providers
            .into_iter()
            .map(|(name, provider)| {
                let audited = crate::audit::AuditedOAuthProvider::new(provider, logger.clone());
                (name, Box::new(audited) as Box<dyn OAuthProvider>)
            })
            .collect();
        Ok(Self { providers })
    }

    /// Look up a configured provider by its exact name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&dyn OAuthProvider> {
        self.providers.get(name).map(Box::as_ref)
    }

    /// Iterate the configured names, in unspecified order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.providers.keys().map(String::as_str)
    }
}

fn config_error(name: &str, detail: impl std::fmt::Display) -> Error {
    Error::Config(Box::new(figment::Error::from(format!(
        "OAuth provider '{name}': {detail}"
    ))))
}

fn required_endpoint(name: &str, key: &str, value: &Option<String>) -> Result<String, Error> {
    let endpoint = value
        .as_ref()
        .ok_or_else(|| config_error(name, format!("missing {key}")))?;
    let url = reqwest::Url::parse(endpoint)
        .map_err(|_| config_error(name, format!("{key} must be an absolute HTTP(S) URL")))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(config_error(
            name,
            format!("{key} must be an absolute HTTP(S) URL"),
        ));
    }
    Ok(endpoint.clone())
}

fn build_provider(
    name: &str,
    config: &OAuthProviderConfig,
) -> Result<Box<dyn OAuthProvider>, Error> {
    match name {
        "google" => GoogleProvider::new(config)
            .map(|provider| Box::new(provider) as Box<dyn OAuthProvider>)
            .map_err(|error| config_error(name, error)),
        "github" => GitHubProvider::new(config)
            .map(|provider| Box::new(provider) as Box<dyn OAuthProvider>)
            .map_err(|error| config_error(name, error)),
        _ => {
            let config = CustomOidcConfig {
                client_id: config.client_id.clone(),
                client_secret: config.client_secret.clone(),
                redirect_uri: config.redirect_uri.clone(),
                auth_url: required_endpoint(
                    name,
                    "authorization_endpoint",
                    &config.authorization_endpoint,
                )?,
                token_url: required_endpoint(name, "token_endpoint", &config.token_endpoint)?,
                userinfo_url: Some(required_endpoint(
                    name,
                    "userinfo_endpoint",
                    &config.userinfo_endpoint,
                )?),
                scopes: config.scopes.clone(),
                name: name.to_string(),
            };
            CustomOidcProvider::new(config)
                .map(|provider| Box::new(provider) as Box<dyn OAuthProvider>)
                .map_err(|error| config_error(name, error))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider_config() -> OAuthProviderConfig {
        OAuthProviderConfig {
            client_id: "test-client".to_string(),
            client_secret: "test-secret".to_string(),
            redirect_uri: "https://example.com/callback".to_string(),
            scopes: vec![],
            authorization_endpoint: None,
            token_endpoint: None,
            userinfo_endpoint: None,
        }
    }

    fn custom_config() -> OAuthProviderConfig {
        OAuthProviderConfig {
            authorization_endpoint: Some("https://idp.example.com/authorize".to_string()),
            token_endpoint: Some("https://idp.example.com/token".to_string()),
            userinfo_endpoint: Some("https://idp.example.com/userinfo".to_string()),
            ..provider_config()
        }
    }

    fn config(name: &str, provider: OAuthProviderConfig) -> OAuthConfig {
        OAuthConfig {
            enabled: true,
            state_ttl_secs: 600,
            providers: HashMap::from([(name.to_string(), provider)]),
        }
    }

    #[test]
    fn built_in_keys_select_the_matching_provider() {
        let mut config = config("google", provider_config());
        config
            .providers
            .insert("github".to_string(), provider_config());
        let registry = OAuthProviderRegistry::from_config(&config).expect("valid built-ins");
        let google = registry.get("google").expect("Google configured");
        let github = registry.get("github").expect("GitHub configured");
        assert_eq!(google.name(), "google");
        assert_eq!(github.name(), "github");
        assert!(google
            .authorization_url("nonce", &[])
            .starts_with("https://accounts.google.com/"));
        assert!(github
            .authorization_url("nonce", &[])
            .starts_with("https://github.com/"));
        let mut names: Vec<_> = registry.names().collect();
        names.sort_unstable();
        assert_eq!(names, ["github", "google"]);
        assert!(registry.get("missing").is_none());
    }

    #[test]
    fn custom_provider_requires_every_endpoint_at_startup() {
        for key in [
            "authorization_endpoint",
            "token_endpoint",
            "userinfo_endpoint",
        ] {
            let mut provider = custom_config();
            match key {
                "authorization_endpoint" => provider.authorization_endpoint = None,
                "token_endpoint" => provider.token_endpoint = None,
                _ => provider.userinfo_endpoint = None,
            }
            let error = OAuthProviderRegistry::from_config(&config("enterprise", provider))
                .err()
                .expect("missing endpoint rejected");
            assert!(matches!(error, Error::Config(_)));
            assert!(error.to_string().contains(key));
            assert!(error.to_string().contains("enterprise"));
        }
    }

    #[test]
    fn custom_provider_uses_configured_name_endpoints_and_scopes() {
        let mut provider = custom_config();
        provider.scopes = vec!["openid".to_string(), "groups".to_string()];
        let registry = OAuthProviderRegistry::from_config(&config("enterprise", provider))
            .expect("complete custom config");
        let provider = registry.get("enterprise").expect("custom configured");
        assert_eq!(provider.name(), "enterprise");
        let url = provider.authorization_url("nonce", &[]);
        assert!(url.starts_with("https://idp.example.com/authorize?"));
        assert!(url.contains("groups"));
        assert!(url.contains("state=nonce"));
    }

    #[test]
    fn invalid_custom_urls_are_rejected_before_login() {
        for endpoint in ["not a url", "file:///tmp/token", "ftp://example.com/token"] {
            for key in [
                "authorization_endpoint",
                "token_endpoint",
                "userinfo_endpoint",
            ] {
                let mut provider = custom_config();
                match key {
                    "authorization_endpoint" => {
                        provider.authorization_endpoint = Some(endpoint.to_string())
                    }
                    "token_endpoint" => provider.token_endpoint = Some(endpoint.to_string()),
                    _ => provider.userinfo_endpoint = Some(endpoint.to_string()),
                }
                let error = OAuthProviderRegistry::from_config(&config("enterprise", provider))
                    .err()
                    .expect("invalid endpoint rejected");
                assert!(matches!(error, Error::Config(_)));
                assert!(error.to_string().contains(key));
            }
        }
    }

    #[test]
    fn invalid_builtin_redirect_is_a_startup_config_error() {
        let mut provider = provider_config();
        provider.redirect_uri = "not a url".to_string();
        assert!(matches!(
            OAuthProviderRegistry::from_config(&config("google", provider)),
            Err(Error::Config(_))
        ));
    }

    #[cfg(feature = "audit")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn audited_registry_emits_callbacks_and_preserves_exchange_results() {
        use crate::audit::{AuditConfig, AuditEvent, AuditEventKind, AuditLogger, AuditSeverity};
        use acton_reactive::prelude::*;

        #[acton_actor]
        struct CaptureEvents {
            sender: Option<tokio::sync::mpsc::UnboundedSender<AuditEvent>>,
        }

        let mut runtime = ActonApp::launch_async().await;
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let mut capture = runtime.new_actor::<CaptureEvents>();
        capture.model.sender = Some(sender);
        capture.act_on::<AuditEvent>(|actor, context| {
            if let Some(sender) = &actor.model.sender {
                sender
                    .send(context.message().clone())
                    .expect("event receiver open");
            }
            Reply::ready()
        });
        let logger = AuditLogger::new(
            capture.start().await,
            "oauth-registry-test".to_string(),
            AuditConfig::default(),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind token endpoint");
        let address = listener.local_addr().expect("bound address");
        let app = axum::Router::new()
            .route(
                "/token",
                axum::routing::post(|| async {
                    axum::Json(
                        serde_json::json!({"access_token": "test-token", "token_type": "bearer"}),
                    )
                }),
            )
            .route(
                "/reject",
                axum::routing::post(|| async {
                    (
                        axum::http::StatusCode::BAD_REQUEST,
                        axum::Json(serde_json::json!({"error": "invalid_grant"})),
                    )
                }),
            );
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("token server runs");
        });
        for (path, succeeds, severity) in [
            ("token", true, AuditSeverity::Notice),
            ("reject", false, AuditSeverity::Warning),
        ] {
            let mut provider = custom_config();
            provider.token_endpoint = Some(format!("http://{address}/{path}"));
            let registry = OAuthProviderRegistry::from_config_audited(
                &config("enterprise", provider),
                logger.clone(),
            )
            .expect("audited registry valid");
            let provider = registry.get("enterprise").expect("provider configured");
            assert_eq!(provider.name(), "enterprise");
            assert!(provider
                .authorization_url("nonce", &[])
                .contains("state=nonce"));
            let result = provider.exchange_code("callback-code").await;
            assert_eq!(result.is_ok(), succeeds);
            if let Ok(tokens) = result {
                assert_eq!(tokens.access_token, "test-token");
            }
            let event = tokio::time::timeout(std::time::Duration::from_secs(5), events.recv())
                .await
                .expect("callback emitted")
                .expect("event channel open");
            assert_eq!(event.kind, AuditEventKind::AuthOAuthCallback);
            assert_eq!(event.severity, severity);
            let metadata = event.metadata.expect("callback metadata");
            assert_eq!(metadata["provider"], "enterprise");
            assert_eq!(
                metadata["outcome"],
                if succeeds { "success" } else { "failure" }
            );
            assert!(!metadata.to_string().contains("callback-code"));
            assert!(!metadata.to_string().contains("test-token"));
        }
        server.abort();
        runtime.shutdown_all().await.expect("capture actor stopped");
    }

    #[test]
    fn empty_configuration_has_no_providers() {
        let registry = OAuthProviderRegistry::from_config(&OAuthConfig::default())
            .expect("empty registry valid");
        assert_eq!(registry.names().count(), 0);
        assert!(registry.get("google").is_none());
    }
}
