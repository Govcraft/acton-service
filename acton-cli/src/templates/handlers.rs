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

    let mut imports = vec![];
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

    // Add imports only if needed
    if !imports.is_empty() {
        for import in imports {
            result.push_str(import);
            result.push('\n');
        }
    }
    result.push_str("use serde::{Deserialize, Serialize};\n");
    result.push_str("use acton_service::prelude::Json;\n\n");

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
