use tonic::Request;

// Include the generated proto code
pub mod user {
    tonic::include_proto!("user");
}

use user::user_service_client::UserServiceClient;
use user::{CreateUserRequest, GetUserRequest, ListUsersRequest};

pub type UserClient = UserServiceClient<tonic::transport::Channel>;

/// Connect to the backend gRPC service
pub async fn connect_to_backend() -> Result<UserClient, Box<dyn std::error::Error>> {
    let backend_url = "http://localhost:8081";
    tracing::info!("Connecting to backend gRPC service at {}", backend_url);

    let client = UserServiceClient::connect(backend_url).await?;

    tracing::info!("Connected to backend gRPC service");
    Ok(client)
}

/// Example: Create a user via gRPC
pub async fn create_user_grpc(
    client: &mut UserClient,
    name: String,
    email: String,
) -> Result<user::User, Box<dyn std::error::Error>> {
    tracing::info!("gRPC Client: Creating user {} <{}>", name, email);

    let request = Request::new(CreateUserRequest { name, email });

    let response = client.create_user(request).await?;
    let user = response.into_inner().user.ok_or("No user in response")?;

    tracing::info!("gRPC Client: User created with ID {}", user.id);
    Ok(user)
}

/// Example: Get a user by ID via gRPC
pub async fn get_user_grpc(
    client: &mut UserClient,
    user_id: String,
) -> Result<user::User, Box<dyn std::error::Error>> {
    tracing::info!("gRPC Client: Getting user {}", user_id);

    let request = Request::new(GetUserRequest { user_id });

    let response = client.get_user(request).await?;
    let user = response.into_inner().user.ok_or("No user in response")?;

    tracing::info!("gRPC Client: User retrieved: {}", user.id);
    Ok(user)
}

/// Example: List users via gRPC
pub async fn list_users_grpc(
    client: &mut UserClient,
    limit: i32,
    offset: i32,
) -> Result<Vec<user::User>, Box<dyn std::error::Error>> {
    tracing::info!("gRPC Client: Listing users (limit: {}, offset: {})", limit, offset);

    let request = Request::new(ListUsersRequest { limit, offset });

    let response = client.list_users(request).await?;
    let list_response = response.into_inner();

    tracing::info!("gRPC Client: Retrieved {} users out of {} total",
                   list_response.users.len(), list_response.total);

    Ok(list_response.users)
}
