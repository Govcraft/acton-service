use tonic::{Request, Response, Status};
use uuid::Uuid;
use chrono::Utc;

// Include the generated proto code
pub mod user {
    tonic::include_proto!("user");
}

use user::user_service_server::{UserService, UserServiceServer};
use user::{
    CreateUserRequest, CreateUserResponse, GetUserRequest, GetUserResponse,
    ListUsersRequest, ListUsersResponse, User,
};

// Import the shared user store from the REST handlers
use crate::handlers::users::USER_STORE;

#[derive(Debug, Default)]
pub struct UserServiceImpl {}

impl UserServiceImpl {
    pub fn new() -> Self {
        Self {}
    }
}

#[tonic::async_trait]
impl UserService for UserServiceImpl {
    async fn create_user(
        &self,
        request: Request<CreateUserRequest>,
    ) -> Result<Response<CreateUserResponse>, Status> {
        let req = request.into_inner();

        tracing::info!("gRPC: Creating user {} <{}>", req.name, req.email);

        if req.name.is_empty() {
            return Err(Status::invalid_argument("Name cannot be empty"));
        }
        if req.email.is_empty() {
            return Err(Status::invalid_argument("Email cannot be empty"));
        }

        let user = User {
            id: Uuid::new_v4().to_string(),
            name: req.name,
            email: req.email,
            created_at: Utc::now().to_rfc3339(),
        };

        // Convert to REST model for storage
        let rest_user = crate::models::User {
            id: user.id.clone(),
            name: user.name.clone(),
            email: user.email.clone(),
            created_at: user.created_at.clone(),
        };

        USER_STORE.insert(rest_user).await;

        tracing::info!("gRPC: User created with ID: {}", user.id);

        Ok(Response::new(CreateUserResponse { user: Some(user) }))
    }

    async fn get_user(
        &self,
        request: Request<GetUserRequest>,
    ) -> Result<Response<GetUserResponse>, Status> {
        let req = request.into_inner();

        tracing::info!("gRPC: Getting user {}", req.user_id);

        match USER_STORE.get(&req.user_id).await {
            Some(rest_user) => {
                let user = User {
                    id: rest_user.id,
                    name: rest_user.name,
                    email: rest_user.email,
                    created_at: rest_user.created_at,
                };

                tracing::info!("gRPC: User found: {}", user.id);
                Ok(Response::new(GetUserResponse { user: Some(user) }))
            }
            None => {
                tracing::warn!("gRPC: User not found: {}", req.user_id);
                Err(Status::not_found(format!("User {} not found", req.user_id)))
            }
        }
    }

    async fn list_users(
        &self,
        request: Request<ListUsersRequest>,
    ) -> Result<Response<ListUsersResponse>, Status> {
        let req = request.into_inner();
        let limit = if req.limit <= 0 { 10 } else { req.limit as usize };
        let offset = if req.offset < 0 { 0 } else { req.offset as usize };

        tracing::info!("gRPC: Listing users (limit: {}, offset: {})", limit, offset);

        let rest_users = USER_STORE.list().await;
        let total = rest_users.len() as i32;

        let users: Vec<User> = rest_users
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|u| User {
                id: u.id,
                name: u.name,
                email: u.email,
                created_at: u.created_at,
            })
            .collect();

        tracing::info!("gRPC: Returning {} users out of {} total", users.len(), total);

        Ok(Response::new(ListUsersResponse { users, total }))
    }
}

pub fn create_grpc_service() -> UserServiceServer<UserServiceImpl> {
    UserServiceServer::new(UserServiceImpl::new())
}
