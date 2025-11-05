pub fn generate_handlers_mod() -> String {
r#"// Add your handler modules here
// Example:
// pub mod users;
"#.to_string()
}

#[allow(dead_code)]
pub fn generate_example_handler() -> String {
r#"use acton_service::prelude::*;
use axum::{
    extract::State,
    Json,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    status: String,
    version: String,
}

/// Example health check handler
pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

// TODO: Add your handlers here
// Example:
//
// #[derive(Debug, Deserialize)]
// pub struct CreateUserRequest {
//     username: String,
//     email: String,
// }
//
// #[derive(Debug, Serialize)]
// pub struct User {
//     id: String,
//     username: String,
//     email: String,
// }
//
// pub async fn create_user(
//     State(state): State<AppState>,
//     Json(req): Json<CreateUserRequest>,
// ) -> Result<Json<User>, AppError> {
//     // TODO: Implement user creation
//     todo!("Implement create_user handler")
// }
"#.to_string()
}

pub struct HandlerTemplate {
    pub function_name: String,
    pub method: String,
    pub path: String,
    pub has_request_body: bool,
    pub has_path_params: bool,
    #[allow(dead_code)]
    pub with_auth: bool,
    pub with_state: bool,
}

/// Generate a new endpoint handler function
pub fn generate_endpoint_handler(template: &HandlerTemplate) -> String {
    let request_type = if template.has_request_body {
        format!("{}Request", to_pascal_case(&template.function_name))
    } else {
        String::new()
    };

    let response_type = format!("{}Response", to_pascal_case(&template.function_name));

    let mut imports = vec!["use axum::Json;"];
    let mut params = vec![];

    if template.with_state {
        imports.push("use axum::extract::State;");
        params.push("State(state): State<AppState>".to_string());
    }

    if template.has_path_params {
        imports.push("use axum::extract::Path;");
        params.push("Path(id): Path<String>".to_string());
    }

    if template.has_request_body {
        params.push(format!("Json(req): Json<{}>", request_type));
    }

    let params_str = if params.is_empty() {
        String::new()
    } else {
        format!("\n    {},\n", params.join(",\n    "))
    };

    let mut result = String::new();

    // Add imports
    for import in imports {
        result.push_str(import);
        result.push('\n');
    }
    result.push_str("use serde::{Deserialize, Serialize};\n\n");

    // Add request struct if needed
    if template.has_request_body {
        result.push_str(&format!(
            "#[derive(Debug, Deserialize)]\npub struct {} {{\n    // TODO: Add your fields here\n}}\n\n",
            request_type
        ));
    }

    // Add response struct
    result.push_str(&format!(
        "#[derive(Debug, Serialize)]\npub struct {} {{\n    // TODO: Add your fields here\n}}\n\n",
        response_type
    ));

    // Add handler function
    result.push_str(&format!(
        "/// {} {}\npub async fn {}({}) -> Json<{}> {{\n    // TODO: Implement handler logic\n    todo!(\"Implement {} handler\")\n}}\n",
        template.method,
        template.path,
        template.function_name,
        params_str,
        response_type,
        template.function_name
    ));

    result
}

/// Generate axum route registration
pub fn generate_route_registration(method: &str, path: &str, handler: &str, version: &str) -> String {
    let route_method = match method.to_uppercase().as_str() {
        "GET" => "get",
        "POST" => "post",
        "PUT" => "put",
        "DELETE" => "delete",
        "PATCH" => "patch",
        _ => "get",
    };

    format!(
        r#"        .route("/{}{}", routing::{}(handlers::{}))"#,
        version, path, route_method, handler
    )
}

fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect()
}
