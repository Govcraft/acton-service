//! Pagination and filtering types for repository queries
//!
//! This module provides types for controlling pagination, ordering, and filtering
//! of repository query results.
//!
//! # Example
//!
//! ```rust
//! use acton_service::repository::{FilterCondition, OrderDirection, Pagination};
//!
//! // Create pagination parameters
//! let pagination = Pagination::new(0, 20);
//!
//! // Create filter conditions
//! let filters = vec![
//!     FilterCondition::eq("status", "active"),
//!     FilterCondition::gte("age", 18),
//! ];
//!
//! // Specify ordering
//! let order_by = Some(("created_at", OrderDirection::Descending));
//! ```

use std::fmt;

/// Direction for ordering results
///
/// # Example
///
/// ```rust
/// use acton_service::repository::OrderDirection;
///
/// let asc = OrderDirection::Ascending;
/// let desc = OrderDirection::Descending;
///
/// assert_eq!(format!("{}", asc), "asc");
/// assert_eq!(format!("{}", desc), "desc");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OrderDirection {
    /// Sort in ascending order (A-Z, 0-9)
    #[default]
    Ascending,
    /// Sort in descending order (Z-A, 9-0)
    Descending,
}

impl fmt::Display for OrderDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ascending => write!(f, "asc"),
            Self::Descending => write!(f, "desc"),
        }
    }
}

/// Pagination parameters for limiting query results
///
/// # Example
///
/// ```rust
/// use acton_service::repository::Pagination;
///
/// // Get the first 20 results
/// let page1 = Pagination::first_page(20);
/// assert_eq!(page1.offset, 0);
/// assert_eq!(page1.limit, 20);
///
/// // Get the second page
/// let page2 = Pagination::new(20, 20);
/// assert_eq!(page2.offset, 20);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pagination {
    /// Number of results to skip
    pub offset: u64,
    /// Maximum number of results to return
    pub limit: u64,
}

impl Pagination {
    /// Create new pagination parameters
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::repository::Pagination;
    ///
    /// let pagination = Pagination::new(40, 20); // Skip 40, take 20
    /// ```
    #[must_use]
    pub const fn new(offset: u64, limit: u64) -> Self {
        Self { offset, limit }
    }

    /// Create pagination for the first page with the given limit
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::repository::Pagination;
    ///
    /// let first_page = Pagination::first_page(25);
    /// assert_eq!(first_page.offset, 0);
    /// assert_eq!(first_page.limit, 25);
    /// ```
    #[must_use]
    pub const fn first_page(limit: u64) -> Self {
        Self { offset: 0, limit }
    }

    /// Create pagination for a specific page number (1-indexed)
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::repository::Pagination;
    ///
    /// let page3 = Pagination::page(3, 20); // Page 3 with 20 items per page
    /// assert_eq!(page3.offset, 40); // Skip first 2 pages (40 items)
    /// assert_eq!(page3.limit, 20);
    /// ```
    #[must_use]
    pub const fn page(page_number: u64, page_size: u64) -> Self {
        let offset = page_number.saturating_sub(1) * page_size;
        Self {
            offset,
            limit: page_size,
        }
    }
}

impl Default for Pagination {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: 20,
        }
    }
}

/// Comparison operators for filter conditions
///
/// These operators are used to compare field values in filter conditions.
///
/// # Example
///
/// ```rust
/// use acton_service::repository::FilterOperator;
///
/// assert_eq!(format!("{}", FilterOperator::Equal), "=");
/// assert_eq!(format!("{}", FilterOperator::Like), "LIKE");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterOperator {
    /// Equal to (=)
    Equal,
    /// Not equal to (!=)
    NotEqual,
    /// Greater than (>)
    GreaterThan,
    /// Greater than or equal to (>=)
    GreaterThanOrEqual,
    /// Less than (<)
    LessThan,
    /// Less than or equal to (<=)
    LessThanOrEqual,
    /// Pattern matching (LIKE)
    Like,
    /// Value is in a list (IN)
    In,
    /// Value is null (IS NULL)
    IsNull,
    /// Value is not null (IS NOT NULL)
    IsNotNull,
}

impl fmt::Display for FilterOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Equal => write!(f, "="),
            Self::NotEqual => write!(f, "!="),
            Self::GreaterThan => write!(f, ">"),
            Self::GreaterThanOrEqual => write!(f, ">="),
            Self::LessThan => write!(f, "<"),
            Self::LessThanOrEqual => write!(f, "<="),
            Self::Like => write!(f, "LIKE"),
            Self::In => write!(f, "IN"),
            Self::IsNull => write!(f, "IS NULL"),
            Self::IsNotNull => write!(f, "IS NOT NULL"),
        }
    }
}

/// A value that can be used in filter conditions
///
/// Supports common SQL types for use in WHERE clauses.
///
/// # Example
///
/// ```rust
/// use acton_service::repository::FilterValue;
///
/// let string_val: FilterValue = "active".into();
/// let int_val: FilterValue = 42_i64.into();
/// let bool_val: FilterValue = true.into();
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum FilterValue {
    /// String value
    String(String),
    /// 64-bit integer value
    Integer(i64),
    /// 64-bit floating point value
    Float(f64),
    /// Boolean value
    Boolean(bool),
    /// List of string values (for IN operator)
    StringList(Vec<String>),
    /// List of integer values (for IN operator)
    IntegerList(Vec<i64>),
    /// Null value (for IS NULL / IS NOT NULL)
    Null,
}

impl From<&str> for FilterValue {
    fn from(s: &str) -> Self {
        Self::String(s.to_string())
    }
}

impl From<String> for FilterValue {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}

impl From<i64> for FilterValue {
    fn from(n: i64) -> Self {
        Self::Integer(n)
    }
}

impl From<i32> for FilterValue {
    fn from(n: i32) -> Self {
        Self::Integer(i64::from(n))
    }
}

impl From<f64> for FilterValue {
    fn from(n: f64) -> Self {
        Self::Float(n)
    }
}

impl From<bool> for FilterValue {
    fn from(b: bool) -> Self {
        Self::Boolean(b)
    }
}

impl From<Vec<String>> for FilterValue {
    fn from(list: Vec<String>) -> Self {
        Self::StringList(list)
    }
}

impl From<Vec<i64>> for FilterValue {
    fn from(list: Vec<i64>) -> Self {
        Self::IntegerList(list)
    }
}

/// A single filter condition for querying entities
///
/// Filter conditions are used to build WHERE clauses for repository queries.
///
/// # Example
///
/// ```rust
/// use acton_service::repository::FilterCondition;
///
/// // Simple equality filter
/// let status_filter = FilterCondition::eq("status", "active");
///
/// // Comparison filter
/// let age_filter = FilterCondition::gte("age", 18);
///
/// // Pattern matching
/// let name_filter = FilterCondition::like("name", "%smith%");
///
/// // Null check
/// let deleted_filter = FilterCondition::is_null("deleted_at");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct FilterCondition {
    /// The field name to filter on
    pub field: String,
    /// The comparison operator
    pub operator: FilterOperator,
    /// The value to compare against
    pub value: FilterValue,
}

impl FilterCondition {
    /// Create a new filter condition
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::repository::{FilterCondition, FilterOperator, FilterValue};
    ///
    /// let filter = FilterCondition::new(
    ///     "status",
    ///     FilterOperator::Equal,
    ///     FilterValue::String("active".to_string()),
    /// );
    /// ```
    pub fn new(field: impl Into<String>, operator: FilterOperator, value: FilterValue) -> Self {
        Self {
            field: field.into(),
            operator,
            value,
        }
    }

    /// Create an equality filter (field = value)
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::repository::FilterCondition;
    ///
    /// let filter = FilterCondition::eq("status", "active");
    /// ```
    pub fn eq(field: impl Into<String>, value: impl Into<FilterValue>) -> Self {
        Self {
            field: field.into(),
            operator: FilterOperator::Equal,
            value: value.into(),
        }
    }

    /// Create a not-equal filter (field != value)
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::repository::FilterCondition;
    ///
    /// let filter = FilterCondition::ne("status", "deleted");
    /// ```
    pub fn ne(field: impl Into<String>, value: impl Into<FilterValue>) -> Self {
        Self {
            field: field.into(),
            operator: FilterOperator::NotEqual,
            value: value.into(),
        }
    }

    /// Create a greater-than filter (field > value)
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::repository::FilterCondition;
    ///
    /// let filter = FilterCondition::gt("price", 100_i64);
    /// ```
    pub fn gt(field: impl Into<String>, value: impl Into<FilterValue>) -> Self {
        Self {
            field: field.into(),
            operator: FilterOperator::GreaterThan,
            value: value.into(),
        }
    }

    /// Create a greater-than-or-equal filter (field >= value)
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::repository::FilterCondition;
    ///
    /// let filter = FilterCondition::gte("age", 18_i64);
    /// ```
    pub fn gte(field: impl Into<String>, value: impl Into<FilterValue>) -> Self {
        Self {
            field: field.into(),
            operator: FilterOperator::GreaterThanOrEqual,
            value: value.into(),
        }
    }

    /// Create a less-than filter (field < value)
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::repository::FilterCondition;
    ///
    /// let filter = FilterCondition::lt("quantity", 10_i64);
    /// ```
    pub fn lt(field: impl Into<String>, value: impl Into<FilterValue>) -> Self {
        Self {
            field: field.into(),
            operator: FilterOperator::LessThan,
            value: value.into(),
        }
    }

    /// Create a less-than-or-equal filter (field <= value)
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::repository::FilterCondition;
    ///
    /// let filter = FilterCondition::lte("rating", 5_i64);
    /// ```
    pub fn lte(field: impl Into<String>, value: impl Into<FilterValue>) -> Self {
        Self {
            field: field.into(),
            operator: FilterOperator::LessThanOrEqual,
            value: value.into(),
        }
    }

    /// Create a LIKE pattern filter
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::repository::FilterCondition;
    ///
    /// let filter = FilterCondition::like("email", "%@example.com");
    /// ```
    pub fn like(field: impl Into<String>, pattern: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            operator: FilterOperator::Like,
            value: FilterValue::String(pattern.into()),
        }
    }

    /// Create an IN list filter for strings
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::repository::FilterCondition;
    ///
    /// let filter = FilterCondition::in_strings("status", vec!["active".to_string(), "pending".to_string()]);
    /// ```
    pub fn in_strings(field: impl Into<String>, values: Vec<String>) -> Self {
        Self {
            field: field.into(),
            operator: FilterOperator::In,
            value: FilterValue::StringList(values),
        }
    }

    /// Create an IN list filter for integers
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::repository::FilterCondition;
    ///
    /// let filter = FilterCondition::in_integers("category_id", vec![1, 2, 3]);
    /// ```
    pub fn in_integers(field: impl Into<String>, values: Vec<i64>) -> Self {
        Self {
            field: field.into(),
            operator: FilterOperator::In,
            value: FilterValue::IntegerList(values),
        }
    }

    /// Create an IS NULL filter
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::repository::FilterCondition;
    ///
    /// let filter = FilterCondition::is_null("deleted_at");
    /// ```
    pub fn is_null(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            operator: FilterOperator::IsNull,
            value: FilterValue::Null,
        }
    }

    /// Create an IS NOT NULL filter
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::repository::FilterCondition;
    ///
    /// let filter = FilterCondition::is_not_null("email_verified_at");
    /// ```
    pub fn is_not_null(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            operator: FilterOperator::IsNotNull,
            value: FilterValue::Null,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_order_direction_display() {
        assert_eq!(format!("{}", OrderDirection::Ascending), "asc");
        assert_eq!(format!("{}", OrderDirection::Descending), "desc");
    }

    #[test]
    fn test_order_direction_default() {
        assert_eq!(OrderDirection::default(), OrderDirection::Ascending);
    }

    #[test]
    fn test_pagination_new() {
        let pagination = Pagination::new(10, 25);
        assert_eq!(pagination.offset, 10);
        assert_eq!(pagination.limit, 25);
    }

    #[test]
    fn test_pagination_first_page() {
        let pagination = Pagination::first_page(50);
        assert_eq!(pagination.offset, 0);
        assert_eq!(pagination.limit, 50);
    }

    #[test]
    fn test_pagination_page() {
        let page1 = Pagination::page(1, 20);
        assert_eq!(page1.offset, 0);
        assert_eq!(page1.limit, 20);

        let page3 = Pagination::page(3, 20);
        assert_eq!(page3.offset, 40);
        assert_eq!(page3.limit, 20);
    }

    #[test]
    fn test_pagination_page_zero_handling() {
        // Page 0 should be treated as page 1 (saturating_sub prevents underflow)
        let page0 = Pagination::page(0, 20);
        assert_eq!(page0.offset, 0);
    }

    #[test]
    fn test_pagination_default() {
        let pagination = Pagination::default();
        assert_eq!(pagination.offset, 0);
        assert_eq!(pagination.limit, 20);
    }

    #[test]
    fn test_filter_operator_display() {
        assert_eq!(format!("{}", FilterOperator::Equal), "=");
        assert_eq!(format!("{}", FilterOperator::NotEqual), "!=");
        assert_eq!(format!("{}", FilterOperator::GreaterThan), ">");
        assert_eq!(format!("{}", FilterOperator::GreaterThanOrEqual), ">=");
        assert_eq!(format!("{}", FilterOperator::LessThan), "<");
        assert_eq!(format!("{}", FilterOperator::LessThanOrEqual), "<=");
        assert_eq!(format!("{}", FilterOperator::Like), "LIKE");
        assert_eq!(format!("{}", FilterOperator::In), "IN");
        assert_eq!(format!("{}", FilterOperator::IsNull), "IS NULL");
        assert_eq!(format!("{}", FilterOperator::IsNotNull), "IS NOT NULL");
    }

    #[test]
    fn test_filter_value_from_str() {
        let value: FilterValue = "test".into();
        assert_eq!(value, FilterValue::String("test".to_string()));
    }

    #[test]
    fn test_filter_value_from_string() {
        let value: FilterValue = String::from("test").into();
        assert_eq!(value, FilterValue::String("test".to_string()));
    }

    #[test]
    fn test_filter_value_from_i64() {
        let value: FilterValue = 42_i64.into();
        assert_eq!(value, FilterValue::Integer(42));
    }

    #[test]
    fn test_filter_value_from_i32() {
        let value: FilterValue = 42_i32.into();
        assert_eq!(value, FilterValue::Integer(42));
    }

    #[test]
    fn test_filter_value_from_f64() {
        let value: FilterValue = 3.14_f64.into();
        assert_eq!(value, FilterValue::Float(3.14));
    }

    #[test]
    fn test_filter_value_from_bool() {
        let value: FilterValue = true.into();
        assert_eq!(value, FilterValue::Boolean(true));
    }

    #[test]
    fn test_filter_value_from_string_vec() {
        let value: FilterValue = vec!["a".to_string(), "b".to_string()].into();
        assert_eq!(
            value,
            FilterValue::StringList(vec!["a".to_string(), "b".to_string()])
        );
    }

    #[test]
    fn test_filter_value_from_i64_vec() {
        let value: FilterValue = vec![1_i64, 2_i64, 3_i64].into();
        assert_eq!(value, FilterValue::IntegerList(vec![1, 2, 3]));
    }

    #[test]
    fn test_filter_condition_new() {
        let filter = FilterCondition::new(
            "status",
            FilterOperator::Equal,
            FilterValue::String("active".to_string()),
        );
        assert_eq!(filter.field, "status");
        assert_eq!(filter.operator, FilterOperator::Equal);
        assert_eq!(filter.value, FilterValue::String("active".to_string()));
    }

    #[test]
    fn test_filter_condition_eq() {
        let filter = FilterCondition::eq("status", "active");
        assert_eq!(filter.field, "status");
        assert_eq!(filter.operator, FilterOperator::Equal);
        assert_eq!(filter.value, FilterValue::String("active".to_string()));
    }

    #[test]
    fn test_filter_condition_ne() {
        let filter = FilterCondition::ne("status", "deleted");
        assert_eq!(filter.operator, FilterOperator::NotEqual);
    }

    #[test]
    fn test_filter_condition_gt() {
        let filter = FilterCondition::gt("price", 100_i64);
        assert_eq!(filter.operator, FilterOperator::GreaterThan);
        assert_eq!(filter.value, FilterValue::Integer(100));
    }

    #[test]
    fn test_filter_condition_gte() {
        let filter = FilterCondition::gte("age", 18_i64);
        assert_eq!(filter.operator, FilterOperator::GreaterThanOrEqual);
    }

    #[test]
    fn test_filter_condition_lt() {
        let filter = FilterCondition::lt("quantity", 10_i64);
        assert_eq!(filter.operator, FilterOperator::LessThan);
    }

    #[test]
    fn test_filter_condition_lte() {
        let filter = FilterCondition::lte("rating", 5_i64);
        assert_eq!(filter.operator, FilterOperator::LessThanOrEqual);
    }

    #[test]
    fn test_filter_condition_like() {
        let filter = FilterCondition::like("email", "%@example.com");
        assert_eq!(filter.operator, FilterOperator::Like);
        assert_eq!(
            filter.value,
            FilterValue::String("%@example.com".to_string())
        );
    }

    #[test]
    fn test_filter_condition_in_strings() {
        let filter = FilterCondition::in_strings(
            "status",
            vec!["active".to_string(), "pending".to_string()],
        );
        assert_eq!(filter.operator, FilterOperator::In);
        assert_eq!(
            filter.value,
            FilterValue::StringList(vec!["active".to_string(), "pending".to_string()])
        );
    }

    #[test]
    fn test_filter_condition_in_integers() {
        let filter = FilterCondition::in_integers("category_id", vec![1, 2, 3]);
        assert_eq!(filter.operator, FilterOperator::In);
        assert_eq!(filter.value, FilterValue::IntegerList(vec![1, 2, 3]));
    }

    #[test]
    fn test_filter_condition_is_null() {
        let filter = FilterCondition::is_null("deleted_at");
        assert_eq!(filter.field, "deleted_at");
        assert_eq!(filter.operator, FilterOperator::IsNull);
        assert_eq!(filter.value, FilterValue::Null);
    }

    #[test]
    fn test_filter_condition_is_not_null() {
        let filter = FilterCondition::is_not_null("email_verified_at");
        assert_eq!(filter.operator, FilterOperator::IsNotNull);
        assert_eq!(filter.value, FilterValue::Null);
    }

    #[test]
    fn test_filter_condition_clone() {
        let filter = FilterCondition::eq("status", "active");
        let cloned = filter.clone();
        assert_eq!(filter, cloned);
    }

    #[test]
    fn test_pagination_clone() {
        let pagination = Pagination::new(10, 20);
        let cloned = pagination.clone();
        assert_eq!(pagination, cloned);
    }
}
