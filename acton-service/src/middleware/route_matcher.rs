//! Per-route rate limit matching
//!
//! Provides route pattern matching for per-route rate limiting configuration.
//! Supports exact paths, method prefixes, wildcards, and automatic ID normalization.

use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

use crate::config::RouteRateLimitConfig;

/// Regex for matching UUIDs in paths
static UUID_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}")
        .expect("UUID regex is valid")
});

/// Compiled route patterns for efficient matching
///
/// Routes are matched in priority order:
/// 1. Method-prefixed exact matches (e.g., `POST /api/v1/uploads`)
/// 2. Exact path matches (e.g., `/api/v1/users`)
/// 3. Wildcard patterns sorted by specificity (most specific first)
#[derive(Debug, Clone)]
pub struct CompiledRoutePatterns {
    /// Method-prefixed exact matches (e.g., "POST /api/v1/uploads" -> config)
    method_exact: HashMap<String, RouteRateLimitConfig>,

    /// Exact path matches (e.g., "/api/v1/users" -> config)
    exact: HashMap<String, RouteRateLimitConfig>,

    /// Wildcard patterns sorted by specificity (most specific first)
    patterns: Vec<CompiledPattern>,
}

/// A compiled wildcard pattern
#[derive(Debug, Clone)]
struct CompiledPattern {
    /// Original pattern string for debugging
    #[allow(dead_code)]
    original: String,

    /// HTTP method filter (None = any method)
    method: Option<String>,

    /// Compiled regex for path matching
    regex: Regex,

    /// Rate limit configuration
    config: RouteRateLimitConfig,

    /// Specificity score (higher = more specific, takes priority)
    specificity: usize,
}

impl CompiledRoutePatterns {
    /// Compile route patterns from configuration
    ///
    /// Parses route pattern strings and compiles them into efficient matchers.
    pub fn compile(routes: &HashMap<String, RouteRateLimitConfig>) -> Self {
        let mut method_exact = HashMap::new();
        let mut exact = HashMap::new();
        let mut patterns = Vec::new();

        for (pattern, config) in routes {
            let (method, path) = Self::parse_method_prefix(pattern);

            if Self::has_wildcards(&path) || path.contains("{id}") {
                // Wildcard pattern - compile to regex
                let regex = Self::compile_pattern_to_regex(&path);
                let specificity = Self::calculate_specificity(&path);

                patterns.push(CompiledPattern {
                    original: pattern.clone(),
                    method,
                    regex,
                    config: config.clone(),
                    specificity,
                });
            } else if let Some(m) = method {
                // Method-prefixed exact match
                let key = format!("{} {}", m, path);
                method_exact.insert(key, config.clone());
            } else {
                // Exact path match
                exact.insert(path, config.clone());
            }
        }

        // Sort patterns by specificity (highest first)
        patterns.sort_by(|a, b| b.specificity.cmp(&a.specificity));

        Self {
            method_exact,
            exact,
            patterns,
        }
    }

    /// Match a request path and method against configured patterns
    ///
    /// Returns the rate limit config for the most specific matching pattern,
    /// or None if no patterns match.
    ///
    /// # Arguments
    /// * `method` - HTTP method (e.g., "GET", "POST")
    /// * `path` - Request path (e.g., "/api/v1/users/123")
    ///
    /// # Returns
    /// The `RouteRateLimitConfig` for the matching pattern, or None.
    pub fn match_route(&self, method: &str, path: &str) -> Option<&RouteRateLimitConfig> {
        // Normalize the path (replace IDs with {id})
        let normalized = normalize_path(path);

        // 1. Check method-prefixed exact match (highest priority)
        let method_key = format!("{} {}", method, normalized);
        if let Some(config) = self.method_exact.get(&method_key) {
            return Some(config);
        }

        // 2. Check exact path match
        if let Some(config) = self.exact.get(&normalized) {
            return Some(config);
        }

        // 3. Check wildcard patterns (sorted by specificity)
        for pattern in &self.patterns {
            // Check method filter
            if let Some(ref m) = pattern.method {
                if m != method {
                    continue;
                }
            }

            // Check regex match
            if pattern.regex.is_match(&normalized) {
                return Some(&pattern.config);
            }
        }

        None
    }

    /// Check if there are any route patterns configured
    pub fn is_empty(&self) -> bool {
        self.method_exact.is_empty() && self.exact.is_empty() && self.patterns.is_empty()
    }

    /// Parse method prefix from pattern (e.g., "POST /api/users" -> (Some("POST"), "/api/users"))
    fn parse_method_prefix(pattern: &str) -> (Option<String>, String) {
        let trimmed = pattern.trim();

        // Check if starts with HTTP method
        let methods = ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"];
        for method in methods {
            if let Some(rest) = trimmed.strip_prefix(method) {
                let rest = rest.trim_start();
                if rest.starts_with('/') {
                    return (Some(method.to_string()), rest.to_string());
                }
            }
        }

        (None, trimmed.to_string())
    }

    /// Check if a pattern contains wildcards
    fn has_wildcards(path: &str) -> bool {
        path.contains('*')
    }

    /// Compile a pattern with wildcards to a regex
    fn compile_pattern_to_regex(pattern: &str) -> Regex {
        let mut regex_str = String::from("^");

        let mut chars = pattern.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '*' => {
                    if chars.peek() == Some(&'*') {
                        // ** matches any number of path segments
                        chars.next();
                        regex_str.push_str(".*");
                    } else {
                        // * matches a single path segment (no slashes)
                        regex_str.push_str("[^/]+");
                    }
                }
                '{' => {
                    // {id} or similar placeholder - match a single segment
                    // Skip until closing brace
                    for c in chars.by_ref() {
                        if c == '}' {
                            break;
                        }
                    }
                    regex_str.push_str("[^/]+");
                }
                '.' | '+' | '?' | '(' | ')' | '[' | ']' | '^' | '$' | '|' | '\\' => {
                    // Escape regex special characters
                    regex_str.push('\\');
                    regex_str.push(c);
                }
                _ => {
                    regex_str.push(c);
                }
            }
        }

        regex_str.push('$');

        Regex::new(&regex_str).expect("Generated regex should be valid")
    }

    /// Calculate specificity score for pattern ordering
    ///
    /// Higher scores = more specific patterns (take priority)
    fn calculate_specificity(pattern: &str) -> usize {
        let mut score = 0;

        // Count literal segments (more = more specific)
        for segment in pattern.split('/') {
            if !segment.is_empty() && !segment.contains('*') && !segment.contains('{') {
                score += 10;
            } else if segment == "*" {
                score += 5; // Single wildcard is somewhat specific
            } else if segment == "**" {
                score += 1; // Double wildcard is least specific
            } else if segment.contains('{') {
                score += 7; // Placeholder is fairly specific
            }
        }

        // Longer patterns are generally more specific
        score += pattern.len();

        score
    }
}

/// Normalize a request path by replacing dynamic segments with `{id}`
///
/// This allows route patterns to match paths with different IDs.
///
/// # Examples
/// ```ignore
/// normalize_path("/api/v1/users/123") // -> "/api/v1/users/{id}"
/// normalize_path("/api/v1/docs/550e8400-e29b-41d4-a716-446655440000") // -> "/api/v1/docs/{id}"
/// ```
pub fn normalize_path(path: &str) -> String {
    // Replace UUIDs with {id}
    let normalized = UUID_REGEX.replace_all(path, "{id}");

    // Replace numeric IDs in path segments
    // We need to be careful to only replace standalone numbers, not numbers that are part of version strings like "v1"
    let segments: Vec<&str> = normalized.split('/').collect();
    let normalized_segments: Vec<String> = segments
        .iter()
        .map(|segment| {
            if !segment.is_empty() && segment.chars().all(|c| c.is_ascii_digit()) {
                "{id}".to_string()
            } else {
                segment.to_string()
            }
        })
        .collect();

    normalized_segments.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path_uuids() {
        assert_eq!(
            normalize_path("/api/v1/users/550e8400-e29b-41d4-a716-446655440000"),
            "/api/v1/users/{id}"
        );
    }

    #[test]
    fn test_normalize_path_numeric_ids() {
        assert_eq!(normalize_path("/api/v1/users/123"), "/api/v1/users/{id}");
        assert_eq!(
            normalize_path("/api/v1/users/123/posts/456"),
            "/api/v1/users/{id}/posts/{id}"
        );
    }

    #[test]
    fn test_normalize_path_preserves_version() {
        assert_eq!(normalize_path("/api/v1/users"), "/api/v1/users");
        assert_eq!(normalize_path("/api/v2/users/123"), "/api/v2/users/{id}");
    }

    #[test]
    fn test_normalize_path_no_ids() {
        assert_eq!(normalize_path("/api/v1/users"), "/api/v1/users");
        assert_eq!(normalize_path("/health"), "/health");
    }

    #[test]
    fn test_parse_method_prefix() {
        let (method, path) = CompiledRoutePatterns::parse_method_prefix("POST /api/v1/users");
        assert_eq!(method, Some("POST".to_string()));
        assert_eq!(path, "/api/v1/users");

        let (method, path) = CompiledRoutePatterns::parse_method_prefix("/api/v1/users");
        assert_eq!(method, None);
        assert_eq!(path, "/api/v1/users");

        let (method, path) = CompiledRoutePatterns::parse_method_prefix("GET  /api/v1/users");
        assert_eq!(method, Some("GET".to_string()));
        assert_eq!(path, "/api/v1/users");
    }

    #[test]
    fn test_compile_pattern_to_regex() {
        let regex = CompiledRoutePatterns::compile_pattern_to_regex("/api/v1/users/*");
        assert!(regex.is_match("/api/v1/users/123"));
        assert!(regex.is_match("/api/v1/users/abc"));
        assert!(!regex.is_match("/api/v1/users/123/posts"));

        let regex = CompiledRoutePatterns::compile_pattern_to_regex("/api/*/admin");
        assert!(regex.is_match("/api/v1/admin"));
        assert!(regex.is_match("/api/v2/admin"));
        assert!(!regex.is_match("/api/v1/v2/admin"));

        let regex = CompiledRoutePatterns::compile_pattern_to_regex("/api/**/admin");
        assert!(regex.is_match("/api/v1/admin"));
        assert!(regex.is_match("/api/v1/v2/admin"));
        assert!(regex.is_match("/api/foo/bar/baz/admin"));
    }

    #[test]
    fn test_compile_pattern_with_placeholder() {
        let regex = CompiledRoutePatterns::compile_pattern_to_regex("/api/v1/users/{id}");
        assert!(regex.is_match("/api/v1/users/123"));
        assert!(regex.is_match("/api/v1/users/abc"));
        assert!(!regex.is_match("/api/v1/users/123/posts"));
    }

    #[test]
    fn test_match_route_exact() {
        let mut routes = HashMap::new();
        routes.insert(
            "/api/v1/users".to_string(),
            RouteRateLimitConfig {
                requests_per_minute: 100,
                burst_size: 10,
                per_user: true,
            },
        );

        let patterns = CompiledRoutePatterns::compile(&routes);
        let config = patterns.match_route("GET", "/api/v1/users");
        assert!(config.is_some());
        assert_eq!(config.unwrap().requests_per_minute, 100);

        // No match for different path
        assert!(patterns.match_route("GET", "/api/v1/posts").is_none());
    }

    #[test]
    fn test_match_route_method_prefix() {
        let mut routes = HashMap::new();
        routes.insert(
            "POST /api/v1/uploads".to_string(),
            RouteRateLimitConfig {
                requests_per_minute: 10,
                burst_size: 2,
                per_user: true,
            },
        );
        routes.insert(
            "/api/v1/uploads".to_string(),
            RouteRateLimitConfig {
                requests_per_minute: 100,
                burst_size: 10,
                per_user: true,
            },
        );

        let patterns = CompiledRoutePatterns::compile(&routes);

        // POST should match method-specific config
        let config = patterns.match_route("POST", "/api/v1/uploads");
        assert!(config.is_some());
        assert_eq!(config.unwrap().requests_per_minute, 10);

        // GET should match generic config
        let config = patterns.match_route("GET", "/api/v1/uploads");
        assert!(config.is_some());
        assert_eq!(config.unwrap().requests_per_minute, 100);
    }

    #[test]
    fn test_match_route_with_id_normalization() {
        let mut routes = HashMap::new();
        routes.insert(
            "/api/v1/users/{id}".to_string(),
            RouteRateLimitConfig {
                requests_per_minute: 50,
                burst_size: 5,
                per_user: true,
            },
        );

        let patterns = CompiledRoutePatterns::compile(&routes);

        // Should match numeric ID
        let config = patterns.match_route("GET", "/api/v1/users/123");
        assert!(config.is_some());
        assert_eq!(config.unwrap().requests_per_minute, 50);

        // Should match UUID
        let config =
            patterns.match_route("GET", "/api/v1/users/550e8400-e29b-41d4-a716-446655440000");
        assert!(config.is_some());
        assert_eq!(config.unwrap().requests_per_minute, 50);
    }

    #[test]
    fn test_match_route_wildcard() {
        let mut routes = HashMap::new();
        routes.insert(
            "/api/*/admin/*".to_string(),
            RouteRateLimitConfig {
                requests_per_minute: 20,
                burst_size: 2,
                per_user: true,
            },
        );

        let patterns = CompiledRoutePatterns::compile(&routes);

        let config = patterns.match_route("GET", "/api/v1/admin/users");
        assert!(config.is_some());
        assert_eq!(config.unwrap().requests_per_minute, 20);

        let config = patterns.match_route("GET", "/api/v2/admin/settings");
        assert!(config.is_some());
        assert_eq!(config.unwrap().requests_per_minute, 20);
    }

    #[test]
    fn test_specificity_ordering() {
        let mut routes = HashMap::new();
        // Less specific pattern
        routes.insert(
            "/api/v1/*".to_string(),
            RouteRateLimitConfig {
                requests_per_minute: 100,
                burst_size: 10,
                per_user: true,
            },
        );
        // More specific pattern
        routes.insert(
            "/api/v1/users".to_string(),
            RouteRateLimitConfig {
                requests_per_minute: 50,
                burst_size: 5,
                per_user: true,
            },
        );

        let patterns = CompiledRoutePatterns::compile(&routes);

        // Should match the more specific exact pattern, not the wildcard
        let config = patterns.match_route("GET", "/api/v1/users");
        assert!(config.is_some());
        assert_eq!(config.unwrap().requests_per_minute, 50);

        // Should match the wildcard for other paths
        let config = patterns.match_route("GET", "/api/v1/posts");
        assert!(config.is_some());
        assert_eq!(config.unwrap().requests_per_minute, 100);
    }
}
