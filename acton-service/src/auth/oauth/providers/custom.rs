//! Custom OIDC provider for any OpenID Connect compliant provider

use async_trait::async_trait;
use oauth2::{
    basic::BasicErrorResponse, AuthUrl, AuthorizationCode, Client, ClientId, ClientSecret,
    CsrfToken, EmptyExtraTokenFields, RedirectUrl, Scope, StandardRevocableToken,
    StandardTokenIntrospectionResponse, StandardTokenResponse, TokenResponse, TokenUrl,
};
use reqwest::Client as HttpClient;

use crate::error::Error;

use super::super::{OAuthProvider, OAuthTokens, OAuthUserInfo};

/// Type alias for our configured OAuth client
type ConfiguredClient = Client<
    BasicErrorResponse,
    StandardTokenResponse<EmptyExtraTokenFields, oauth2::basic::BasicTokenType>,
    StandardTokenIntrospectionResponse<EmptyExtraTokenFields, oauth2::basic::BasicTokenType>,
    StandardRevocableToken,
    BasicErrorResponse,
    oauth2::EndpointSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointSet,
>;

/// Configuration for a custom OIDC provider
#[derive(Clone, Debug)]
pub struct CustomOidcConfig {
    /// Client ID
    pub client_id: String,
    /// Client secret
    pub client_secret: String,
    /// Redirect URI for callbacks
    pub redirect_uri: String,
    /// Authorization endpoint URL
    pub auth_url: String,
    /// Token endpoint URL
    pub token_url: String,
    /// Userinfo endpoint URL (optional for OIDC)
    pub userinfo_url: Option<String>,
    /// Default scopes to request
    pub scopes: Vec<String>,
    /// Provider display name
    pub name: String,
}

/// Custom OIDC provider that works with any OAuth2/OIDC-compliant identity provider
///
/// This provider uses manual configuration instead of OIDC discovery, making it
/// suitable for any OAuth2 provider, not just OIDC-compliant ones.
#[derive(Clone)]
pub struct CustomOidcProvider {
    client: ConfiguredClient,
    http_client: HttpClient,
    default_scopes: Vec<String>,
    name: String,
    userinfo_endpoint: Option<String>,
}

impl CustomOidcProvider {
    /// Create a new custom OIDC provider with manual configuration
    pub fn new(config: CustomOidcConfig) -> Result<Self, Error> {
        let auth_url = AuthUrl::new(config.auth_url)
            .map_err(|e| Error::Internal(format!("Invalid auth URL: {}", e)))?;

        let token_url = TokenUrl::new(config.token_url)
            .map_err(|e| Error::Internal(format!("Invalid token URL: {}", e)))?;

        let client = Client::new(ClientId::new(config.client_id))
            .set_client_secret(ClientSecret::new(config.client_secret))
            .set_auth_uri(auth_url)
            .set_token_uri(token_url)
            .set_redirect_uri(
                RedirectUrl::new(config.redirect_uri)
                    .map_err(|e| Error::Internal(format!("Invalid redirect URI: {}", e)))?,
            );

        let http_client = HttpClient::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| Error::Internal(format!("Failed to create HTTP client: {}", e)))?;

        let default_scopes = if config.scopes.is_empty() {
            vec![
                "openid".to_string(),
                "email".to_string(),
                "profile".to_string(),
            ]
        } else {
            config.scopes
        };

        Ok(Self {
            client,
            http_client,
            default_scopes,
            name: config.name,
            userinfo_endpoint: config.userinfo_url,
        })
    }

    /// Create a provider from OIDC discovery endpoint
    ///
    /// This fetches the provider configuration from the .well-known/openid-configuration
    /// endpoint and configures the client automatically.
    pub async fn from_discovery(
        issuer_url: &str,
        client_id: String,
        client_secret: String,
        redirect_uri: String,
        scopes: Vec<String>,
        name: String,
    ) -> Result<Self, Error> {
        let discovery_url = format!(
            "{}/.well-known/openid-configuration",
            issuer_url.trim_end_matches('/')
        );

        let http_client = HttpClient::builder()
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|e| Error::Internal(format!("Failed to create HTTP client: {}", e)))?;

        let response = http_client
            .get(&discovery_url)
            .send()
            .await
            .map_err(|e| Error::External(format!("Failed to fetch OIDC discovery: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::External(format!(
                "OIDC discovery failed: {}",
                response.status()
            )));
        }

        let discovery: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::External(format!("Failed to parse OIDC discovery: {}", e)))?;

        let auth_url = discovery["authorization_endpoint"]
            .as_str()
            .ok_or_else(|| Error::External("Missing authorization_endpoint".to_string()))?;

        let token_url = discovery["token_endpoint"]
            .as_str()
            .ok_or_else(|| Error::External("Missing token_endpoint".to_string()))?;

        let userinfo_url = discovery["userinfo_endpoint"]
            .as_str()
            .map(|s| s.to_string());

        let config = CustomOidcConfig {
            client_id,
            client_secret,
            redirect_uri,
            auth_url: auth_url.to_string(),
            token_url: token_url.to_string(),
            userinfo_url,
            scopes,
            name,
        };

        Self::new(config)
    }
}

#[async_trait]
impl OAuthProvider for CustomOidcProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn authorization_url(&self, state: &str, additional_scopes: &[String]) -> String {
        let mut all_scopes: Vec<Scope> = self
            .default_scopes
            .iter()
            .map(|s| Scope::new(s.clone()))
            .collect();

        for scope in additional_scopes {
            if !self.default_scopes.contains(scope) {
                all_scopes.push(Scope::new(scope.clone()));
            }
        }

        let mut auth_request = self
            .client
            .authorize_url(|| CsrfToken::new(state.to_string()));

        for scope in all_scopes {
            auth_request = auth_request.add_scope(scope);
        }

        let (url, _) = auth_request.url();
        url.to_string()
    }

    async fn exchange_code(&self, code: &str) -> Result<OAuthTokens, Error> {
        let token_result = self
            .client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .request_async(&self.http_client)
            .await
            .map_err(|e| Error::External(format!("Token exchange failed: {}", e)))?;

        Ok(OAuthTokens {
            access_token: token_result.access_token().secret().clone(),
            refresh_token: token_result.refresh_token().map(|t| t.secret().clone()),
            expires_in: token_result.expires_in().map(|d| d.as_secs() as i64),
            token_type: "Bearer".to_string(),
            id_token: None, // OIDC id_token not available via basic OAuth2 client
        })
    }

    async fn get_user_info(&self, access_token: &str) -> Result<OAuthUserInfo, Error> {
        let endpoint = self.userinfo_endpoint.as_ref().ok_or_else(|| {
            Error::Internal("No userinfo endpoint configured for this provider".to_string())
        })?;

        let response = self
            .http_client
            .get(endpoint)
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| Error::External(format!("Failed to fetch user info: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::External(format!(
                "User info request failed: {} - {}",
                status, body
            )));
        }

        let user_info: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::External(format!("Failed to parse user info: {}", e)))?;

        // Standard OIDC claims
        let sub = user_info["sub"]
            .as_str()
            .ok_or_else(|| Error::External("Missing sub claim in response".to_string()))?;

        Ok(OAuthUserInfo {
            provider: self.name.clone(),
            provider_user_id: sub.to_string(),
            email: user_info["email"].as_str().map(|s| s.to_string()),
            email_verified: user_info["email_verified"].as_bool().unwrap_or(false),
            name: user_info["name"]
                .as_str()
                .or(user_info["preferred_username"].as_str())
                .map(|s| s.to_string()),
            picture: user_info["picture"].as_str().map(|s| s.to_string()),
            raw: user_info,
        })
    }

    async fn refresh_token(&self, refresh_token: &str) -> Result<OAuthTokens, Error> {
        let token_result = self
            .client
            .exchange_refresh_token(&oauth2::RefreshToken::new(refresh_token.to_string()))
            .request_async(&self.http_client)
            .await
            .map_err(|e| Error::External(format!("Token refresh failed: {}", e)))?;

        Ok(OAuthTokens {
            access_token: token_result.access_token().secret().clone(),
            refresh_token: token_result.refresh_token().map(|t| t.secret().clone()),
            expires_in: token_result.expires_in().map(|d| d.as_secs() as i64),
            token_type: "Bearer".to_string(),
            id_token: None, // OIDC id_token not available via basic OAuth2 client
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_custom_oidc_config() {
        let config = CustomOidcConfig {
            client_id: "test-client".to_string(),
            client_secret: "test-secret".to_string(),
            redirect_uri: "https://example.com/callback".to_string(),
            auth_url: "https://auth.example.com/authorize".to_string(),
            token_url: "https://auth.example.com/token".to_string(),
            userinfo_url: Some("https://auth.example.com/userinfo".to_string()),
            scopes: vec!["openid".to_string()],
            name: "example".to_string(),
        };

        assert_eq!(config.name, "example");
        assert_eq!(config.auth_url, "https://auth.example.com/authorize");
    }

    #[test]
    fn test_provider_creation() {
        let config = CustomOidcConfig {
            client_id: "client-id".to_string(),
            client_secret: "client-secret".to_string(),
            redirect_uri: "https://example.com/callback".to_string(),
            auth_url: "https://auth.example.com/authorize".to_string(),
            token_url: "https://auth.example.com/token".to_string(),
            userinfo_url: Some("https://auth.example.com/userinfo".to_string()),
            scopes: vec!["openid".to_string()],
            name: "example".to_string(),
        };

        let provider = CustomOidcProvider::new(config);
        assert!(provider.is_ok());

        let provider = provider.unwrap();
        assert_eq!(provider.name(), "example");
    }

    #[test]
    fn test_authorization_url() {
        let config = CustomOidcConfig {
            client_id: "test-client".to_string(),
            client_secret: "test-secret".to_string(),
            redirect_uri: "https://example.com/callback".to_string(),
            auth_url: "https://auth.example.com/authorize".to_string(),
            token_url: "https://auth.example.com/token".to_string(),
            userinfo_url: None,
            scopes: vec!["openid".to_string(), "profile".to_string()],
            name: "custom".to_string(),
        };

        let provider = CustomOidcProvider::new(config).unwrap();
        let url = provider.authorization_url("test-state", &[]);

        assert!(url.contains("auth.example.com"));
        assert!(url.contains("client_id=test-client"));
        assert!(url.contains("state=test-state"));
    }
}
