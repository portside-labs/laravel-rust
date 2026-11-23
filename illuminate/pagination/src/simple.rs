use serde::{Serialize, Serializer};

use illuminate_support::{Collection, HtmlString, Map, Value, json, to_value};

use crate::links;
use crate::resolvers::query_string;
use crate::{PaginatorOptions, build_url};

/// A "simple" paginator: it only knows whether there is a next page, which
/// saves counting every row. This is what `simple_paginate()` returns.
#[derive(Clone, Debug)]
pub struct Paginator<T> {
    items: Collection<T>,
    per_page: u64,
    current_page: u64,
    has_more: bool,
    options: PaginatorOptions,
}

impl<T> Paginator<T> {
    /// Create a new simple paginator. Pass up to `per_page + 1` items; the
    /// extra item (if present) signals that there is another page.
    pub fn new(
        items: impl Into<Collection<T>>,
        per_page: u64,
        current_page: u64,
        options: PaginatorOptions,
    ) -> Self {
        let per_page = per_page.max(1);
        let mut items: Vec<T> = items.into().into_vec();
        let has_more = items.len() as u64 > per_page;
        items.truncate(per_page as usize);
        Self {
            items: items.into(),
            per_page,
            current_page: current_page.max(1),
            has_more,
            options: options.with_trimmed_path(),
        }
    }

    pub fn items(&self) -> &Collection<T> {
        &self.items
    }

    pub fn into_items(self) -> Collection<T> {
        self.items
    }

    /// Transform each item, keeping the pagination information.
    pub fn through<U>(self, callback: impl FnMut(T) -> U) -> Paginator<U> {
        Paginator {
            items: self.items.map(callback),
            per_page: self.per_page,
            current_page: self.current_page,
            has_more: self.has_more,
            options: self.options,
        }
    }

    pub fn per_page(&self) -> u64 {
        self.per_page
    }

    pub fn current_page(&self) -> u64 {
        self.current_page
    }

    pub fn count(&self) -> usize {
        self.items.count()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn first_item(&self) -> Option<u64> {
        if self.items.is_empty() {
            None
        } else {
            Some((self.current_page - 1) * self.per_page + 1)
        }
    }

    pub fn last_item(&self) -> Option<u64> {
        self.first_item()
            .map(|first| first + self.items.count() as u64 - 1)
    }

    pub fn has_pages(&self) -> bool {
        self.current_page != 1 || self.has_more
    }

    pub fn has_more_pages(&self) -> bool {
        self.has_more
    }

    pub fn on_first_page(&self) -> bool {
        self.current_page <= 1
    }

    pub fn url(&self, page: u64) -> String {
        build_url(&self.options, page)
    }

    pub fn previous_page_url(&self) -> Option<String> {
        (self.current_page > 1).then(|| self.url(self.current_page - 1))
    }

    pub fn next_page_url(&self) -> Option<String> {
        self.has_more.then(|| self.url(self.current_page + 1))
    }

    pub fn path(&self) -> &str {
        &self.options.path
    }

    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.options.path = path.into();
        self
    }

    pub fn append(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.options.query.insert(key.into(), value.into());
        self
    }

    pub fn appends(mut self, values: Map<String, Value>) -> Self {
        for (key, value) in values {
            self.options.query.insert(key, value);
        }
        self
    }

    pub fn with_query_string(self) -> Self {
        let mut query = query_string();
        query.shift_remove(&self.options.page_name);
        self.appends(query)
    }

    pub fn fragment(mut self, fragment: impl Into<String>) -> Self {
        self.options.fragment = Some(fragment.into());
        self
    }

    /// Render "Previous" / "Next" links with the default simple view
    /// (Tailwind CSS unless you chose another).
    pub fn links(&self) -> HtmlString {
        self.links_with(&crate::presets::get_default_simple_view())
    }

    /// Render "Previous" / "Next" links with the given view. Simple
    /// paginators always use the simple flavour of the built-in views.
    pub fn links_with(&self, view: &str) -> HtmlString {
        let data = links::LinkData {
            has_pages: self.has_pages(),
            current_page: self.current_page,
            previous: self.previous_page_url(),
            next: self.next_page_url(),
            elements: Vec::new(),
            summary: None,
        };
        let html = match links::builtin_view(view) {
            Some((Some(flavour), _)) => links::render_simple_bootstrap(flavour, &data),
            Some((None, _)) => links::render_simple_tailwind(data.has_pages, data.previous, data.next),
            None => crate::presets::render_custom(view, data.to_view_data()).unwrap_or_else(|| {
                links::render_simple_tailwind(data.has_pages, data.previous.clone(), data.next.clone())
            }),
        };
        HtmlString::new(html)
    }

    /// Alias of `links`.
    pub fn render(&self) -> HtmlString {
        self.links()
    }
}

impl<T: Serialize> Paginator<T> {
    /// The paginator as a value, in Laravel's JSON shape.
    pub fn to_array(&self) -> Value {
        json!({
            "current_page": self.current_page,
            "current_page_url": self.url(self.current_page),
            "data": to_value(&self.items),
            "first_page_url": self.url(1),
            "from": self.first_item(),
            "next_page_url": self.next_page_url(),
            "path": self.path(),
            "per_page": self.per_page,
            "prev_page_url": self.previous_page_url(),
            "to": self.last_item(),
        })
    }
}

/// Laravel's static pagination view presets, as `Paginator::use_bootstrap_five()`.
///
/// ```
/// use illuminate_pagination::Paginator;
///
/// Paginator::use_bootstrap_five();
/// assert_eq!(Paginator::get_default_view(), "pagination::bootstrap-5");
/// Paginator::use_tailwind();
/// assert_eq!(Paginator::get_default_simple_view(), "pagination::simple-tailwind");
/// ```
impl Paginator<()> {
    /// Set the default pagination view.
    pub fn default_view(view: &str) {
        crate::presets::default_view(view);
    }

    /// Set the default "simple" pagination view.
    pub fn default_simple_view(view: &str) {
        crate::presets::default_simple_view(view);
    }

    /// The default pagination view.
    pub fn get_default_view() -> String {
        crate::presets::get_default_view()
    }

    /// The default "simple" pagination view.
    pub fn get_default_simple_view() -> String {
        crate::presets::get_default_simple_view()
    }

    /// Render pagination links with Tailwind CSS (the default).
    pub fn use_tailwind() {
        crate::presets::use_tailwind();
    }

    /// Render pagination links with Bootstrap (version 4).
    pub fn use_bootstrap() {
        crate::presets::use_bootstrap();
    }

    /// Render pagination links with Bootstrap 3.
    pub fn use_bootstrap_three() {
        crate::presets::use_bootstrap_three();
    }

    /// Render pagination links with Bootstrap 4.
    pub fn use_bootstrap_four() {
        crate::presets::use_bootstrap_four();
    }

    /// Render pagination links with Bootstrap 5.
    pub fn use_bootstrap_five() {
        crate::presets::use_bootstrap_five();
    }
}

impl<T: Serialize> Paginator<T> {
    /// The paginator as JSON.
    pub fn to_json(&self) -> String {
        self.to_array().to_string()
    }

    /// The paginator as pretty-printed JSON (indented like PHP's
    /// `JSON_PRETTY_PRINT`).
    pub fn to_pretty_json(&self) -> String {
        crate::pretty_json(&self.to_array())
    }
}

impl<T: Serialize> Serialize for Paginator<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_array().serialize(serializer)
    }
}

impl<T> IntoIterator for Paginator<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<T: Serialize> From<Paginator<T>> for Value {
    fn from(paginator: Paginator<T>) -> Self {
        paginator.to_array()
    }
}
