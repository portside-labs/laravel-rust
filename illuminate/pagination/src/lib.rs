//! # Illuminate Pagination
//!
//! Painless pagination for query results, with JSON output identical to
//! Laravel's and Tailwind-styled links ready for your Blade templates.
//!
//! ```
//! use illuminate_pagination::{LengthAwarePaginator, PaginatorOptions};
//!
//! let paginator = LengthAwarePaginator::new(
//!     vec!["a", "b"],
//!     5,
//!     2,
//!     1,
//!     PaginatorOptions::path("/users"),
//! );
//!
//! assert_eq!(paginator.last_page(), 3);
//! assert_eq!(paginator.next_page_url().as_deref(), Some("/users?page=2"));
//! ```

mod length_aware;
mod links;
pub mod presets;
mod resolvers;
mod simple;

pub use length_aware::LengthAwarePaginator;
pub use resolvers::{
    current_page, current_path, query_string, resolve_current_page_using, resolve_current_path_using,
    resolve_query_string_using, translate, translate_using,
};
pub use presets::{
    default_simple_view, default_view, render_views_using, use_bootstrap, use_bootstrap_five,
    use_bootstrap_four, use_bootstrap_three, use_tailwind,
};
pub use simple::Paginator;

use illuminate_support::{Arr, Map, Value};

/// Options shared by every paginator.
#[derive(Clone, Debug)]
pub struct PaginatorOptions {
    /// The base path for generated URLs.
    pub path: String,
    /// The query string variable holding the page number.
    pub page_name: String,
    /// Extra query string parameters appended to every URL.
    pub query: Map<String, Value>,
    /// The URL fragment appended to every URL.
    pub fragment: Option<String>,
}

impl Default for PaginatorOptions {
    fn default() -> Self {
        Self {
            path: current_path(),
            page_name: "page".to_string(),
            query: Map::new(),
            fragment: None,
        }
    }
}

impl PaginatorOptions {
    /// Options for the given base path.
    pub fn path(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            page_name: "page".to_string(),
            query: Map::new(),
            fragment: None,
        }
    }

    /// Use a different page name.
    pub fn page_name(mut self, name: impl Into<String>) -> Self {
        self.page_name = name.into();
        self
    }

    fn with_trimmed_path(mut self) -> Self {
        if self.path != "/" {
            self.path.truncate(self.path.trim_end_matches('/').len());
        }
        self
    }
}

/// Encode a value as JSON indented with four spaces, like PHP's
/// `JSON_PRETTY_PRINT`.
pub(crate) fn pretty_json(value: &Value) -> String {
    use serde::Serialize;
    let mut buffer = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"    ");
    let mut serializer = serde_json::Serializer::with_formatter(&mut buffer, formatter);
    match value.serialize(&mut serializer) {
        Ok(()) => String::from_utf8(buffer).unwrap_or_default(),
        Err(_) => "null".to_string(),
    }
}

/// Behaviour shared by the length-aware and simple paginators.
pub(crate) fn build_url(options: &PaginatorOptions, page: u64) -> String {
    let page = page.max(1);
    let mut parameters = options.query.clone();
    parameters.insert(options.page_name.clone(), Value::from(page));
    let separator = if options.path.contains('?') { '&' } else { '?' };
    let fragment = options
        .fragment
        .as_ref()
        .map(|f| format!("#{f}"))
        .unwrap_or_default();
    format!(
        "{}{}{}{}",
        options.path,
        separator,
        Arr::query(&Value::Object(parameters)),
        fragment
    )
}

#[cfg(test)]
mod tests;
