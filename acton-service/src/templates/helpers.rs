//! Template helper functions and custom filters.
//!
//! These can be used in Askama templates via custom filters.

use std::fmt::Display;

/// Truncate text with ellipsis.
///
/// # Example
///
/// ```rust
/// use acton_service::templates::truncate;
///
/// assert_eq!(truncate("Hello, World!", 5), "He...");
/// assert_eq!(truncate("Hi", 10), "Hi");
/// ```
#[must_use]
pub fn truncate(s: impl Display, max_len: usize) -> String {
    let s = s.to_string();
    if s.len() <= max_len {
        s
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

/// Pluralize a word based on count.
///
/// # Example
///
/// ```rust
/// use acton_service::templates::pluralize;
///
/// assert_eq!(pluralize(1, "item", "items"), "item");
/// assert_eq!(pluralize(2, "item", "items"), "items");
/// assert_eq!(pluralize(0, "item", "items"), "items");
/// ```
#[must_use]
pub fn pluralize(count: i64, singular: &str, plural: &str) -> String {
    if count == 1 {
        singular.to_string()
    } else {
        plural.to_string()
    }
}

/// Generate CSS classes conditionally.
///
/// # Example in template:
///
/// ```html
/// <div class="{{ classes(is_active, "active", "") }} {{ classes(is_error, "error", "success") }}">
/// ```
///
/// # Example
///
/// ```rust
/// use acton_service::templates::classes;
///
/// assert_eq!(classes(true, "active", ""), "active");
/// assert_eq!(classes(false, "active", "inactive"), "inactive");
/// ```
#[must_use]
pub fn classes<'a>(condition: bool, if_true: &'a str, if_false: &'a str) -> &'a str {
    if condition {
        if_true
    } else {
        if_false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("Hello, World!", 5), "He...");
        assert_eq!(truncate("Hi", 10), "Hi");
        assert_eq!(truncate("", 5), "");
        assert_eq!(truncate("Test", 4), "Test");
        assert_eq!(truncate("Test", 3), "...");
    }

    #[test]
    fn test_pluralize() {
        assert_eq!(pluralize(0, "item", "items"), "items");
        assert_eq!(pluralize(1, "item", "items"), "item");
        assert_eq!(pluralize(2, "item", "items"), "items");
        assert_eq!(pluralize(-1, "item", "items"), "items");
    }

    #[test]
    fn test_classes() {
        assert_eq!(classes(true, "active", ""), "active");
        assert_eq!(classes(false, "active", ""), "");
        assert_eq!(classes(true, "yes", "no"), "yes");
        assert_eq!(classes(false, "yes", "no"), "no");
    }
}
