//! GitHub OAuth provider implementation

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

/// GitHub OAuth provider
#[derive(Clone)]
pub struct GitHubProvider {
    client: ConfiguredClient,
    http_client: HttpClient,
    default_scopes: Vec<String>,
}

impl GitHubProvider {
    /// Create a new GitHub OAuth provider from configuration
    pub fn new(config: &OAuthProviderConfig) -> Result<Self, Error> {
        let client = Client::new(ClientId::new(config.client_id.clone()))
            .set_client_secret(ClientSecret::new(config.client_secret.clone()))
            .set_auth_uri(
                AuthUrl::new("https://github.com/login/oauth/authorize".to_string())
                    .map_err(|e| Error::Internal(format!("Invalid GitHub auth URL: {}", e)))?,
            )
            .set_token_uri(
                TokenUrl::new("https://github.com/login/oauth/access_token".to_string())
                    .map_err(|e| Error::Internal(format!("Invalid GitHub token URL: {}", e)))?,
            )
            .set_redirect_uri(
                RedirectUrl::new(config.redirect_uri.clone())
                    .map_err(|e| Error::Internal(format!("Invalid redirect URI: {}", e)))?,
            );

        let http_client = HttpClient::builder()
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("acton-service")
            .build()
            .map_err(|e| Error::Internal(format!("Failed to create HTTP client: {}", e)))?;

        // Default scopes for GitHub
        let default_scopes = if config.scopes.is_empty() {
            vec!["read:user".to_string(), "user:email".to_string()]
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
impl OAuthProvider for GitHubProvider {
    fn name(&self) -> &str {
        "github"
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
            .map_err(|e| Error::External(format!("GitHub token exchange failed: {}", e)))?;

        Ok(OAuthTokens {
            access_token: token_result.access_token().secret().clone(),
            refresh_token: token_result.refresh_token().map(|t| t.secret().clone()),
            expires_in: token_result.expires_in().map(|d| d.as_secs() as i64),
            token_type: "Bearer".to_string(),
            id_token: None, // GitHub doesn't provide OIDC id_token
        })
    }

    async fn get_user_info(&self, access_token: &str) -> Result<OAuthUserInfo, Error> {
        // Get basic user info
        let response = self
            .http_client
            .get("https://api.github.com/user")
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| Error::External(format!("Failed to fetch GitHub user info: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::External(format!(
                "GitHub user info request failed: {} - {}",
                status, body
            )));
        }

        let user_info: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::External(format!("Failed to parse GitHub user info: {}", e)))?;

        // GitHub's user endpoint may not return email if it's private
        // Try to get primary verified email from the emails endpoint
        let email = if user_info["email"].is_null() {
            self.get_primary_email(access_token).await.ok()
        } else {
            user_info["email"].as_str().map(|s| s.to_string())
        };

        Ok(OAuthUserInfo {
            provider: "github".to_string(),
            provider_user_id: user_info["id"]
                .as_i64()
                .ok_or_else(|| Error::External("Missing id in GitHub response".to_string()))?
                .to_string(),
            email,
            email_verified: true, // GitHub only shows verified emails
            name: user_info["name"]
                .as_str()
                .or(user_info["login"].as_str())
                .map(|s| s.to_string()),
            picture: user_info["avatar_url"].as_str().map(|s| s.to_string()),
            raw: user_info,
        })
    }

    async fn refresh_token(&self, _refresh_token: &str) -> Result<OAuthTokens, Error> {
        // GitHub OAuth apps don't support refresh tokens
        // GitHub Apps do, but that requires a different flow
        Err(Error::NotSupported(
            "GitHub OAuth does not support refresh tokens".to_string(),
        ))
    }
}

impl GitHubProvider {
    /// Get the primary verified email from GitHub
    async fn get_primary_email(&self, access_token: &str) -> Result<String, Error> {
        let response = self
            .http_client
            .get("https://api.github.com/user/emails")
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| Error::External(format!("Failed to fetch GitHub emails: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::External("Failed to fetch GitHub emails".to_string()));
        }

        let emails: Vec<serde_json::Value> = response
            .json()
            .await
            .map_err(|e| Error::External(format!("Failed to parse GitHub emails: {}", e)))?;

        // Find primary verified email
        for email in &emails {
            if email["primary"].as_bool().unwrap_or(false)
                && email["verified"].as_bool().unwrap_or(false)
            {
                if let Some(addr) = email["email"].as_str() {
                    return Ok(addr.to_string());
                }
            }
        }

        // Fallback to any verified email
        for email in &emails {
            if email["verified"].as_bool().unwrap_or(false) {
                if let Some(addr) = email["email"].as_str() {
                    return Ok(addr.to_string());
                }
            }
        }

        Err(Error::External("No verified email found".to_string()))
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
            scopes: vec!["read:user".to_string()],
            authorization_endpoint: None,
            token_endpoint: None,
            userinfo_endpoint: None,
        };

        let provider = GitHubProvider::new(&config).unwrap();
        let url = provider.authorization_url("test-state", &[]);

        assert!(url.contains("github.com"));
        assert!(url.contains("client_id=test-client-id"));
        assert!(url.contains("state=test-state"));
    }

    #[test]
    fn test_provider_name() {
        let config = OAuthProviderConfig {
            client_id: "test".to_string(),
            client_secret: "test".to_string(),
            redirect_uri: "https://example.com/callback".to_string(),
            scopes: vec![],
            authorization_endpoint: None,
            token_endpoint: None,
            userinfo_endpoint: None,
        };

        let provider = GitHubProvider::new(&config).unwrap();
        assert_eq!(provider.name(), "github");
    }
}
