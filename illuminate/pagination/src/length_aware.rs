use indexmap::IndexMap;
use serde::{Serialize, Serializer};

use illuminate_support::{Collection, HtmlString, Map, Value, json, to_value};

use crate::links;
use crate::resolvers::{query_string, translate};
use crate::{PaginatorOptions, build_url};

/// A paginator that knows the total number of items.
///
/// This is what `paginate()` returns. Serializing it produces exactly the
/// JSON structure Laravel returns from a paginated route.
#[derive(Clone, Debug)]
pub struct LengthAwarePaginator<T> {
    items: Collection<T>,
    total: u64,
    per_page: u64,
    current_page: u64,
    last_page: u64,
    on_each_side: u64,
    options: PaginatorOptions,
}

impl<T> LengthAwarePaginator<T> {
    /// Create a new paginator.
    pub fn new(
        items: impl Into<Collection<T>>,
        total: u64,
        per_page: u64,
        current_page: u64,
        options: PaginatorOptions,
    ) -> Self {
        let per_page = per_page.max(1);
        let last_page = total.div_ceil(per_page).max(1);
        Self {
            items: items.into(),
            total,
            per_page,
            current_page: current_page.max(1),
            last_page,
            on_each_side: 3,
            options: options.with_trimmed_path(),
        }
    }

    /// The items for the current page.
    pub fn items(&self) -> &Collection<T> {
        &self.items
    }

    /// Take the items out of the paginator.
    pub fn into_items(self) -> Collection<T> {
        self.items
    }

    /// Transform each item, keeping the pagination information.
    pub fn through<U>(self, callback: impl FnMut(T) -> U) -> LengthAwarePaginator<U> {
        LengthAwarePaginator {
            items: self.items.map(callback),
            total: self.total,
            per_page: self.per_page,
            current_page: self.current_page,
            last_page: self.last_page,
            on_each_side: self.on_each_side,
            options: self.options,
        }
    }

    /// The total number of items being paginated.
    pub fn total(&self) -> u64 {
        self.total
    }

    /// The number of items shown per page.
    pub fn per_page(&self) -> u64 {
        self.per_page
    }

    /// The current page.
    pub fn current_page(&self) -> u64 {
        self.current_page
    }

    /// The last page.
    pub fn last_page(&self) -> u64 {
        self.last_page
    }

    /// The number of items on the current page.
    pub fn count(&self) -> usize {
        self.items.count()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn is_not_empty(&self) -> bool {
        !self.items.is_empty()
    }

    /// The "index" of the first item on this page (1-based).
    pub fn first_item(&self) -> Option<u64> {
        if self.items.is_empty() {
            None
        } else {
            Some((self.current_page - 1) * self.per_page + 1)
        }
    }

    /// The "index" of the last item on this page.
    pub fn last_item(&self) -> Option<u64> {
        self.first_item()
            .map(|first| first + self.items.count() as u64 - 1)
    }

    /// Determine if there are enough items to split into multiple pages.
    pub fn has_pages(&self) -> bool {
        self.current_page != 1 || self.has_more_pages()
    }

    /// Determine if there are more items after this page.
    pub fn has_more_pages(&self) -> bool {
        self.current_page < self.last_page
    }

    pub fn on_first_page(&self) -> bool {
        self.current_page <= 1
    }

    pub fn on_last_page(&self) -> bool {
        !self.has_more_pages()
    }

    /// The URL for a given page.
    pub fn url(&self, page: u64) -> String {
        build_url(&self.options, page)
    }

    /// The URL for the previous page.
    pub fn previous_page_url(&self) -> Option<String> {
        (self.current_page > 1).then(|| self.url(self.current_page - 1))
    }

    /// The URL for the next page.
    pub fn next_page_url(&self) -> Option<String> {
        self.has_more_pages().then(|| self.url(self.current_page + 1))
    }

    /// A range of page URLs, keyed by page number.
    pub fn get_url_range(&self, start: u64, end: u64) -> IndexMap<u64, String> {
        (start.max(1)..=end.min(self.last_page))
            .map(|page| (page, self.url(page)))
            .collect()
    }

    /// The base path for generated URLs.
    pub fn path(&self) -> &str {
        &self.options.path
    }

    /// Set the base path for generated URLs.
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.options.path = path.into();
        self
    }

    /// Append a query string value to every page URL.
    pub fn append(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.options.query.insert(key.into(), value.into());
        self
    }

    /// Append many query string values to every page URL.
    pub fn appends(mut self, values: Map<String, Value>) -> Self {
        for (key, value) in values {
            self.options.query.insert(key, value);
        }
        self
    }

    /// Append the current request's query string to every page URL.
    pub fn with_query_string(self) -> Self {
        let mut query = query_string();
        query.shift_remove(&self.options.page_name);
        self.appends(query)
    }

    /// Set the URL fragment.
    pub fn fragment(mut self, fragment: impl Into<String>) -> Self {
        self.options.fragment = Some(fragment.into());
        self
    }

    /// Set the number of links to display on each side of the current page.
    pub fn on_each_side(mut self, count: u64) -> Self {
        self.on_each_side = count;
        self
    }

    /// The page "elements": groups of page links separated by "..." markers.
    pub fn elements(&self) -> Vec<links::Element> {
        links::elements(self.current_page, self.last_page, self.on_each_side, |p| self.url(p))
    }

    /// The links as the structured list Laravel includes in its JSON.
    pub fn link_collection(&self) -> Vec<Value> {
        let mut out = vec![json!({
            "url": self.previous_page_url(),
            "label": translate("pagination.previous"),
            "page": if self.current_page > 1 { Some(self.current_page - 1) } else { None },
            "active": false,
        })];
        for element in self.elements() {
            match element {
                links::Element::Dots => out.push(json!({"url": null, "label": "...", "active": false})),
                links::Element::Pages(pages) => {
                    for (page, url) in pages {
                        out.push(json!({
                            "url": url,
                            "label": page.to_string(),
                            "page": page,
                            "active": page == self.current_page,
                        }));
                    }
                }
            }
        }
        out.push(json!({
            "url": self.next_page_url(),
            "label": translate("pagination.next"),
            "page": if self.has_more_pages() { Some(self.current_page + 1) } else { None },
            "active": false,
        }));
        out
    }

    /// Render the pagination links with the default view (Tailwind CSS,
    /// unless you chose another with [`use_bootstrap_five`](crate::use_bootstrap_five)
    /// and friends).
    pub fn links(&self) -> HtmlString {
        self.links_with(&crate::presets::get_default_view())
    }

    /// Render the pagination links with the given view: a built-in one
    /// (`pagination::tailwind`, `pagination::bootstrap-5`, ...) or one of
    /// your own, rendered by the [registered renderer](crate::render_views_using).
    pub fn links_with(&self, view: &str) -> HtmlString {
        let data = self.link_data();
        let html = match links::builtin_view(view) {
            Some((None, false)) => links::render_tailwind(self),
            Some((None, true)) => links::render_simple_tailwind(
                self.has_pages(),
                self.previous_page_url(),
                self.next_page_url(),
            ),
            Some((Some(flavour), false)) => links::render_bootstrap(flavour, &data),
            Some((Some(flavour), true)) => links::render_simple_bootstrap(flavour, &data),
            None => crate::presets::render_custom(view, data.to_view_data())
                .unwrap_or_else(|| links::render_tailwind(self)),
        };
        HtmlString::new(html)
    }

    fn link_data(&self) -> links::LinkData {
        links::LinkData {
            has_pages: self.has_pages(),
            current_page: self.current_page,
            previous: self.previous_page_url(),
            next: self.next_page_url(),
            elements: self.elements(),
            summary: Some((self.first_item(), self.last_item(), self.total)),
        }
    }

    /// Alias of `links`.
    pub fn render(&self) -> HtmlString {
        self.links()
    }
}

impl<T: Serialize> LengthAwarePaginator<T> {
    /// The paginator as a value, in Laravel's JSON shape.
    pub fn to_array(&self) -> Value {
        json!({
            "current_page": self.current_page,
            "data": to_value(&self.items),
            "first_page_url": self.url(1),
            "from": self.first_item(),
            "last_page": self.last_page,
            "last_page_url": self.url(self.last_page),
            "links": self.link_collection(),
            "next_page_url": self.next_page_url(),
            "path": self.path(),
            "per_page": self.per_page,
            "prev_page_url": self.previous_page_url(),
            "to": self.last_item(),
            "total": self.total,
        })
    }

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

impl<T: Serialize> Serialize for LengthAwarePaginator<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_array().serialize(serializer)
    }
}

impl<T> IntoIterator for LengthAwarePaginator<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a LengthAwarePaginator<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

impl<T: Serialize> From<LengthAwarePaginator<T>> for Value {
    fn from(paginator: LengthAwarePaginator<T>) -> Self {
        paginator.to_array()
    }
}
