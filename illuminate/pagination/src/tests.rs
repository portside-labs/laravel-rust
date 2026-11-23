use illuminate_support::json;

use crate::{LengthAwarePaginator, Paginator, PaginatorOptions};

fn paginator(total: u64, per_page: u64, page: u64) -> LengthAwarePaginator<u64> {
    let start = (page - 1) * per_page + 1;
    let items: Vec<u64> = (start..=(start + per_page - 1).min(total)).collect();
    LengthAwarePaginator::new(items, total, per_page, page, PaginatorOptions::path("http://localhost/users"))
}

#[test]
fn it_serializes_like_laravel() {
    let p = paginator(5, 2, 2);
    let value = p.to_array();
    assert_eq!(value["current_page"], json!(2));
    assert_eq!(value["data"], json!([3, 4]));
    assert_eq!(value["first_page_url"], json!("http://localhost/users?page=1"));
    assert_eq!(value["from"], json!(3));
    assert_eq!(value["to"], json!(4));
    assert_eq!(value["last_page"], json!(3));
    assert_eq!(value["next_page_url"], json!("http://localhost/users?page=3"));
    assert_eq!(value["prev_page_url"], json!("http://localhost/users?page=1"));
    assert_eq!(value["total"], json!(5));
    assert_eq!(value["links"][0]["label"], json!("&laquo; Previous"));
    assert_eq!(value["links"][2]["active"], json!(true));
    assert_eq!(value["links"].as_array().unwrap().len(), 5);
}

#[test]
fn it_builds_url_windows() {
    let p = paginator(1000, 10, 50);
    let labels: Vec<String> = p
        .link_collection()
        .iter()
        .map(|l| l["label"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        labels,
        vec!["&laquo; Previous", "1", "2", "...", "47", "48", "49", "50", "51", "52", "53", "...", "99", "100", "Next &raquo;"]
    );
}

#[test]
fn it_appends_query_strings_and_fragments() {
    let p = paginator(30, 10, 1).append("sort", "votes").fragment("users");
    assert_eq!(p.url(2), "http://localhost/users?sort=votes&page=2#users");
}

#[test]
fn it_trims_trailing_slashes_from_the_initial_path() {
    for (path, expected_path, expected_url) in [
        ("/users", "/users", "/users?page=2"),
        ("/users/", "/users", "/users?page=2"),
        ("/users///", "/users", "/users?page=2"),
        ("/", "/", "/?page=2"),
    ] {
        let options = PaginatorOptions::path(path);
        let simple = Paginator::new(vec![1, 2], 1, 1, options.clone());
        let length_aware = LengthAwarePaginator::new(vec![1], 2, 1, 1, options);

        assert_eq!(simple.path(), expected_path);
        assert_eq!(length_aware.path(), expected_path);
        assert_eq!(simple.next_page_url().as_deref(), Some(expected_url));
        assert_eq!(length_aware.next_page_url().as_deref(), Some(expected_url));
    }
}

#[test]
fn it_keeps_paths_explicitly_set_after_construction() {
    let options = PaginatorOptions::path("/posts");
    let simple = Paginator::new(vec![1, 2], 1, 1, options.clone()).with_path("/users/");
    let length_aware = LengthAwarePaginator::new(vec![1], 2, 1, 1, options).with_path("/users/");

    assert_eq!(simple.next_page_url().as_deref(), Some("/users/?page=2"));
    assert_eq!(
        length_aware.next_page_url().as_deref(),
        Some("/users/?page=2")
    );
}

#[test]
fn simple_paginators_know_if_there_are_more_pages() {
    let p = Paginator::new(vec![1, 2, 3], 2, 1, PaginatorOptions::path("/posts"));
    assert!(p.has_more_pages());
    assert_eq!(p.items().all(), &[1, 2]);
    assert_eq!(p.next_page_url().as_deref(), Some("/posts?page=2"));
    assert!(p.links().to_html().contains("rel=\"next\""));
}

#[test]
fn it_renders_links() {
    let html = paginator(50, 10, 2).links().to_html().to_string();
    assert!(html.contains("aria-current=\"page\""));
    assert!(html.contains("Showing"));
    assert!(html.contains("href=\"http://localhost/users?page=3\""));
    assert_eq!(paginator(5, 10, 1).links().to_html(), "");
}
