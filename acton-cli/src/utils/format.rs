/// Convert service name to PascalCase for types
pub fn to_pascal_case(s: &str) -> String {
    s.split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect()
}

/// Convert service name to snake_case for functions
pub fn to_snake_case(s: &str) -> String {
    s.replace('-', "_")
}

/// Convert HTTP method to lowercase for function names
pub fn method_to_function_name(method: &str, path: &str) -> String {
    let method_lower = method.to_lowercase();
    let path_clean = path
        .trim_start_matches('/')
        .replace('/', "_")
        .replace(':', "")
        .replace('-', "_");

    if path_clean.is_empty() {
        method_lower
    } else {
        format!("{}_{}", method_lower, path_clean)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_pascal_case() {
        assert_eq!(to_pascal_case("user-service"), "UserService");
        assert_eq!(to_pascal_case("auth-api"), "AuthApi");
        assert_eq!(to_pascal_case("simple"), "Simple");
    }

    #[test]
    fn test_to_snake_case() {
        assert_eq!(to_snake_case("user-service"), "user_service");
        assert_eq!(to_snake_case("auth-api"), "auth_api");
    }

    #[test]
    fn test_method_to_function_name() {
        assert_eq!(method_to_function_name("GET", "/users"), "get_users");
        assert_eq!(method_to_function_name("POST", "/users"), "post_users");
        assert_eq!(method_to_function_name("GET", "/users/:id"), "get_users_id");
    }
}
