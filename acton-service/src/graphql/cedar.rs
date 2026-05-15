//! Cedar policy authorization for GraphQL resolvers (requires the
//! `graphql-cedar` feature).
//!
//! The same [`CedarAuthz`] instance that protects HTTP and gRPC endpoints is
//! injected into every GraphQL request by
//! [`ActonGraphQL`](super::service::ActonGraphQL). Resolvers can call
//! [`CedarResolverCheck::authorize`] to evaluate a policy at any granularity
//! (root, field, sub-field), using whatever resource type makes sense for the
//! resolver.

use async_graphql::Context;
use cedar_policy::{Context as CedarContext, Decision, EntityUid};

use super::context::GraphQLContextExt;
use crate::middleware::cedar::CedarAuthz;
use crate::middleware::token::Claims;

/// Reasons a resolver-level Cedar check can fail.
#[derive(Debug, thiserror::Error)]
pub enum CedarResolverError {
    /// Cedar is not installed on the schema. This usually means the
    /// `ServiceBuilder` was not configured with `with_cedar(...)`.
    #[error("Cedar authorization is not configured for this schema")]
    NotConfigured,

    /// No authenticated claims on the request. Without a principal Cedar
    /// cannot evaluate a policy.
    #[error("Cedar requires authenticated claims but the request was anonymous")]
    Unauthenticated,

    /// Failed to parse a principal/action/resource UID.
    #[error("Invalid Cedar entity UID: {0}")]
    InvalidUid(String),

    /// Failed to build the Cedar context.
    #[error("Failed to build Cedar context: {0}")]
    Context(String),

    /// Underlying policy evaluation failed.
    #[error("Cedar evaluation failed: {0}")]
    Evaluation(String),

    /// Policy explicitly denied the request.
    #[error("Forbidden: access denied by policy")]
    Denied,
}

/// Resolver-level Cedar authorization check.
///
/// # Example
///
/// ```rust,ignore
/// use async_graphql::{Context, Object};
/// use acton_service::graphql::{CedarResolverCheck, GraphQLContextExt};
///
/// struct Query;
///
/// #[Object]
/// impl Query {
///     async fn document(&self, ctx: &Context<'_>, id: String) -> async_graphql::Result<String> {
///         CedarResolverCheck::for_context(ctx)?
///             .with_action("readDocument")
///             .with_resource_type("Document")
///             .with_resource_id(&id)
///             .authorize()
///             .await?;
///         Ok(format!("Document {}", id))
///     }
/// }
/// ```
pub struct CedarResolverCheck<'ctx> {
    cedar: &'ctx CedarAuthz,
    claims: &'ctx Claims,
    action_type: String,
    action_id: String,
    resource_type: String,
    resource_id: String,
    context_json: serde_json::Map<String, serde_json::Value>,
}

impl<'ctx> CedarResolverCheck<'ctx> {
    /// Construct a check from a resolver `Context`. Returns
    /// [`CedarResolverError::NotConfigured`] if Cedar was not installed and
    /// [`CedarResolverError::Unauthenticated`] if no claims are present.
    pub fn for_context(ctx: &Context<'ctx>) -> Result<Self, CedarResolverError> {
        let cedar = ctx
            .data_opt::<CedarAuthz>()
            .ok_or(CedarResolverError::NotConfigured)?;
        let claims = ctx.claims().ok_or(CedarResolverError::Unauthenticated)?;
        Ok(Self {
            cedar,
            claims,
            action_type: "Action".to_string(),
            action_id: String::new(),
            resource_type: "Resource".to_string(),
            resource_id: "default".to_string(),
            context_json: serde_json::Map::new(),
        })
    }

    /// Override the action type name (defaults to `Action`).
    pub fn with_action_type(mut self, ty: impl Into<String>) -> Self {
        self.action_type = ty.into();
        self
    }

    /// Set the action id (e.g. `readDocument`).
    pub fn with_action(mut self, id: impl Into<String>) -> Self {
        self.action_id = id.into();
        self
    }

    /// Override the resource type name (defaults to `Resource`).
    pub fn with_resource_type(mut self, ty: impl Into<String>) -> Self {
        self.resource_type = ty.into();
        self
    }

    /// Override the resource id (defaults to `default`).
    pub fn with_resource_id(mut self, id: impl Into<String>) -> Self {
        self.resource_id = id.into();
        self
    }

    /// Add a key/value to the Cedar evaluation context.
    pub fn with_context_attr(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.context_json.insert(key.into(), value.into());
        self
    }

    /// Evaluate the request. Returns `Ok(())` on `Decision::Allow`, otherwise
    /// [`CedarResolverError::Denied`].
    pub async fn authorize(self) -> Result<(), CedarResolverError> {
        let principal = build_principal(self.claims)?;
        let action: EntityUid = format!(r#"{}::"{}""#, self.action_type, self.action_id)
            .parse()
            .map_err(|e: cedar_policy::ParseErrors| {
                CedarResolverError::InvalidUid(e.to_string())
            })?;
        let resource: EntityUid = format!(r#"{}::"{}""#, self.resource_type, self.resource_id)
            .parse()
            .map_err(|e: cedar_policy::ParseErrors| {
                CedarResolverError::InvalidUid(e.to_string())
            })?;
        let context = if self.context_json.is_empty() {
            CedarContext::empty()
        } else {
            CedarContext::from_json_value(
                serde_json::Value::Object(self.context_json.clone()),
                None,
            )
            .map_err(|e| CedarResolverError::Context(e.to_string()))?
        };

        let decision = self
            .cedar
            .authorize(&principal, &action, &resource, context, self.claims)
            .await
            .map_err(|e| CedarResolverError::Evaluation(e.to_string()))?;

        match decision {
            Decision::Allow => Ok(()),
            Decision::Deny => Err(CedarResolverError::Denied),
        }
    }
}

fn build_principal(claims: &Claims) -> Result<EntityUid, CedarResolverError> {
    let principal_str = if claims.is_user() {
        format!(r#"User::"{}""#, claims.sub)
    } else if claims.is_client() {
        format!(r#"Client::"{}""#, claims.sub)
    } else {
        format!(r#"Principal::"{}""#, claims.sub)
    };
    principal_str
        .parse()
        .map_err(|e: cedar_policy::ParseErrors| CedarResolverError::InvalidUid(e.to_string()))
}
