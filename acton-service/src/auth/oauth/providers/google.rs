//! Google OAuth provider implementation

use async_trait::async_trait;
use oauth2::{
    basic::BasicErrorResponse, AuthUrl, AuthorizationCode, Client, ClientId, ClientSecret,
    CsrfToken, EmptyExtraTokenFields, RedirectUrl, Scope, StandardRevocableToken,
    StandardTokenIntrospectionResponse, StandardTokenResponse, TokenResponse, TokenUrl,
};
use reqwest::Client as HttpClient;

use crate::auth::config::OAuthProviderConfig;
use crate::auth::oauth::{OAuthProvider, OAuthTokens, OAuthUserInfo};
use crate::error::Error;

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

/// Google OAuth provider
#[derive(Clone)]
pub struct GoogleProvider {
    client: ConfiguredClient,
    http_client: HttpClient,
    default_scopes: Vec<String>,
}

impl GoogleProvider {
    /// Create a new Google OAuth provider from configuration
    pub fn new(config: &OAuthProviderConfig) -> Result<Self, Error> {
        let client = Client::new(ClientId::new(config.client_id.clone()))
            .set_client_secret(ClientSecret::new(config.client_secret.clone()))
            .set_auth_uri(
                AuthUrl::new("https://accounts.google.com/o/oauth2/v2/auth".to_string())
                    .map_err(|e| Error::Internal(format!("Invalid Google auth URL: {}", e)))?,
            )
            .set_token_uri(
                TokenUrl::new("https://oauth2.googleapis.com/token".to_string())
                    .map_err(|e| Error::Internal(format!("Invalid Google token URL: {}", e)))?,
            )
            .set_redirect_uri(
                RedirectUrl::new(config.redirect_uri.clone())
                    .map_err(|e| Error::Internal(format!("Invalid redirect URI: {}", e)))?,
            );

        let http_client = HttpClient::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| Error::Internal(format!("Failed to create HTTP client: {}", e)))?;

        // Default scopes for Google
        let default_scopes = if config.scopes.is_empty() {
            vec![
                "openid".to_string(),
                "email".to_string(),
                "profile".to_string(),
            ]
        } else {
            config.scopes.clone()
        };

        Ok(Self {
            client,
            http_client,
            default_scopes,
        })
    }
}

#[async_trait]
impl OAuthProvider for GoogleProvider {
    fn name(&self) -> &str {
        "google"
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
            .map_err(|e| Error::External(format!("Google token exchange failed: {}", e)))?;

        Ok(OAuthTokens {
            access_token: token_result.access_token().secret().clone(),
            refresh_token: token_result.refresh_token().map(|t| t.secret().clone()),
            expires_in: token_result.expires_in().map(|d| d.as_secs() as i64),
            token_type: "Bearer".to_string(),
            id_token: None, // OIDC id_token not available via basic OAuth2 client
        })
    }

    async fn get_user_info(&self, access_token: &str) -> Result<OAuthUserInfo, Error> {
        let response = self
            .http_client
            .get("https://www.googleapis.com/oauth2/v3/userinfo")
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| Error::External(format!("Failed to fetch Google user info: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::External(format!(
                "Google user info request failed: {} - {}",
                status, body
            )));
        }

        let user_info: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::External(format!("Failed to parse Google user info: {}", e)))?;

        Ok(OAuthUserInfo {
            provider: "google".to_string(),
            provider_user_id: user_info["sub"]
                .as_str()
                .ok_or_else(|| Error::External("Missing sub in Google response".to_string()))?
                .to_string(),
            email: user_info["email"].as_str().map(|s| s.to_string()),
            email_verified: user_info["email_verified"].as_bool().unwrap_or(false),
            name: user_info["name"].as_str().map(|s| s.to_string()),
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
            .map_err(|e| Error::External(format!("Google token refresh failed: {}", e)))?;

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
    fn test_authorization_url_generation() {
        let config = OAuthProviderConfig {
            client_id: "test-client-id".to_string(),
            client_secret: "test-secret".to_string(),
            redirect_uri: "https://example.com/callback".to_string(),
            scopes: vec!["openid".to_string(), "email".to_string()],
            authorization_endpoint: None,
            token_endpoint: None,
            userinfo_endpoint: None,
        };

        let provider = GoogleProvider::new(&config).unwrap();
        let url = provider.authorization_url("test-state", &[]);

        assert!(url.contains("accounts.google.com"));
        assert!(url.contains("client_id=test-client-id"));
        assert!(url.contains("state=test-state"));
        assert!(url.contains("redirect_uri="));
    }

    #[test]
    fn test_additional_scopes() {
        let config = OAuthProviderConfig {
            client_id: "test-client-id".to_string(),
            client_secret: "test-secret".to_string(),
            redirect_uri: "https://example.com/callback".to_string(),
            scopes: vec!["openid".to_string()],
            authorization_endpoint: None,
            token_endpoint: None,
            userinfo_endpoint: None,
        };

        let provider = GoogleProvider::new(&config).unwrap();
        let url = provider.authorization_url("test-state", &["calendar".to_string()]);

        assert!(url.contains("openid"));
        assert!(url.contains("calendar"));
    }
}
