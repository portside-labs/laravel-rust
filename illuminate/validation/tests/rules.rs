//! Every built-in rule, with passing and failing cases and Laravel's exact
//! default messages.

mod common;

use common::{assert_fails, assert_passes, bad, container, ok};
use illuminate_support::json;

#[tokio::test]
async fn accepted() {
    let _c = container();
    for value in [
        json!("yes"),
        json!("on"),
        json!("1"),
        json!(1),
        json!(true),
        json!("true"),
    ] {
        ok(value, "accepted").await;
    }
    for value in [json!("no"), json!(false), json!(0), json!("2"), json!(1.0)] {
        bad(value, "accepted").await;
    }
    assert_fails(json!({}), "accepted", "The field field must be accepted.").await;
}

#[tokio::test]
async fn accepted_if() {
    let _c = container();
    assert_fails(
        json!({"other": "yes", "field": "no"}),
        "accepted_if:other,yes",
        "The field field must be accepted when other is yes.",
    )
    .await;
    assert_passes(
        json!({"other": "no", "field": "no"}),
        "accepted_if:other,yes",
    )
    .await;
    assert_passes(
        json!({"other": "yes", "field": "on"}),
        "accepted_if:other,yes",
    )
    .await;
}

#[tokio::test]
async fn active_url() {
    let _c = container();
    ok(json!("https://laravel.com"), "active_url").await;
    bad(json!("laravel.com"), "active_url").await;
    assert_fails(
        json!({"field": "not a url"}),
        "active_url",
        "The field field must be a valid URL.",
    )
    .await;
}

#[tokio::test]
async fn after_and_before() {
    let _c = container();
    ok(json!("2024-01-02"), "after:2024-01-01").await;
    assert_fails(
        json!({"field": "2023-12-31"}),
        "after:2024-01-01",
        "The field field must be a date after 2024-01-01.",
    )
    .await;
    assert_fails(
        json!({"field": "2000-01-01"}),
        "after:tomorrow",
        "The field field must be a date after tomorrow.",
    )
    .await;
    bad(json!("2000-01-01"), "after:+1 week").await;
    ok(json!("2999-01-01"), "after:+1 week").await;

    ok(json!("2024-01-01"), "after_or_equal:2024-01-01").await;
    assert_fails(
        json!({"field": "2023-12-31"}),
        "after_or_equal:2024-01-01",
        "The field field must be a date after or equal to 2024-01-01.",
    )
    .await;

    ok(json!("2023-12-31"), "before:2024-01-01").await;
    assert_fails(
        json!({"field": "2024-01-01"}),
        "before:2024-01-01",
        "The field field must be a date before 2024-01-01.",
    )
    .await;
    ok(json!("2024-01-01"), "before_or_equal:2024-01-01").await;
    assert_fails(
        json!({"field": "2024-01-02"}),
        "before_or_equal:2024-01-01",
        "The field field must be a date before or equal to 2024-01-01.",
    )
    .await;

    ok(json!("2024-01-01"), "date_equals:2024-01-01").await;
    assert_fails(
        json!({"field": "2024-01-02"}),
        "date_equals:2024-01-01",
        "The field field must be a date equal to 2024-01-01.",
    )
    .await;
}

#[tokio::test]
async fn date_rules_compare_with_other_fields() {
    let _c = container();
    assert_passes(
        json!({"start_date": "2024-01-01", "field": "2024-01-05"}),
        "after:start_date",
    )
    .await;
    assert_fails(
        json!({"start_date": "2024-01-01", "field": "2023-01-05"}),
        "after:start_date",
        "The field field must be a date after start date.",
    )
    .await;
    assert_passes(
        json!({"field": "02/01/2024"}),
        "date_format:d/m/Y|after:01/01/2024",
    )
    .await;
    bad(json!("31/12/2023"), "date_format:d/m/Y|after:01/01/2024").await;
    assert_passes(
        json!({"start": "01/01/2024", "field": "05/01/2024"}),
        "date_format:d/m/Y|after:start",
    )
    .await;
}

#[tokio::test]
async fn alpha_rules() {
    let _c = container();
    ok(json!("abc"), "alpha").await;
    ok(json!("Ünïcödé"), "alpha").await;
    bad(json!("é"), "alpha:ascii").await;
    bad(json!(123), "alpha").await;
    assert_fails(
        json!({"field": "abc1"}),
        "alpha",
        "The field field must only contain letters.",
    )
    .await;

    ok(json!("a-b_c1"), "alpha_dash").await;
    ok(json!(123), "alpha_dash").await;
    assert_fails(
        json!({"field": "a b"}),
        "alpha_dash",
        "The field field must only contain letters, numbers, dashes, and underscores.",
    )
    .await;

    ok(json!("abc123"), "alpha_num").await;
    bad(json!("ñ1"), "alpha_num:ascii").await;
    assert_fails(
        json!({"field": "abc-1"}),
        "alpha_num",
        "The field field must only contain letters and numbers.",
    )
    .await;
}

#[tokio::test]
async fn array_rules() {
    let _c = container();
    ok(json!(["a"]), "array").await;
    ok(json!({"a": 1}), "array").await;
    assert_fails(
        json!({"field": "x"}),
        "array",
        "The field field must be an array.",
    )
    .await;
    ok(
        json!({"name": "Taylor", "username": "taylor"}),
        "array:name,username",
    )
    .await;
    assert_fails(
        json!({"field": {"name": "Taylor", "admin": true}}),
        "array:name,username",
        "The field field must be an array.",
    )
    .await;

    ok(json!({"name": 1}), "array_keys:name,username").await;
    assert_fails(
        json!({"field": {"name": 1, "x": 2}}),
        "array_keys:name",
        "The field field must only contain the following keys: name.",
    )
    .await;

    ok(json!([1, 2]), "list").await;
    assert_fails(
        json!({"field": {"a": 1}}),
        "list",
        "The field field must be a list.",
    )
    .await;

    ok(json!({"a": 1, "b": 2}), "required_array_keys:a,b").await;
    assert_fails(
        json!({"field": {"a": 1}}),
        "required_array_keys:a,b",
        "The field field must contain entries for: a, b.",
    )
    .await;

    ok(json!({"timezone": "UTC"}), "in_array_keys:timezone,locale").await;
    assert_fails(
        json!({"field": {"a": 1}}),
        "in_array_keys:timezone",
        "The field field must contain at least one of the following keys: timezone.",
    )
    .await;

    ok(json!(["admin", "editor"]), "contains:admin").await;
    assert_fails(
        json!({"field": ["editor"]}),
        "contains:admin",
        "The field field is missing a required value.",
    )
    .await;
    bad(json!("admin"), "contains:admin").await;

    ok(json!(["editor"]), "doesnt_contain:admin").await;
    assert_fails(
        json!({"field": ["admin"]}),
        "doesnt_contain:admin,owner",
        "The field field must not contain any of the following: admin, owner.",
    )
    .await;
}

#[tokio::test]
async fn ascii_and_base64() {
    let _c = container();
    ok(json!("abc!"), "ascii").await;
    assert_fails(
        json!({"field": "é"}),
        "ascii",
        "The field field must only contain single-byte alphanumeric characters and symbols.",
    )
    .await;

    ok(json!("aGVsbG8="), "base64").await;
    assert_fails(
        json!({"field": "not base64!"}),
        "base64",
        "The field field must be a valid Base64 string.",
    )
    .await;
}

#[tokio::test]
async fn between() {
    let _c = container();
    ok(json!("abc"), "between:1,5").await;
    assert_fails(
        json!({"field": "abcdef"}),
        "between:1,5",
        "The field field must be between 1 and 5 characters.",
    )
    .await;
    assert_fails(
        json!({"field": 10}),
        "numeric|between:1,5",
        "The field field must be between 1 and 5.",
    )
    .await;
    ok(json!(3), "integer|between:1,5").await;
    assert_fails(
        json!({"field": [1, 2, 3, 4]}),
        "array|between:1,3",
        "The field field must have between 1 and 3 items.",
    )
    .await;
}

#[tokio::test]
async fn boolean() {
    let _c = container();
    for value in [
        json!(true),
        json!(false),
        json!(0),
        json!(1),
        json!("0"),
        json!("1"),
    ] {
        ok(value, "boolean").await;
    }
    bad(json!("yes"), "boolean").await;
    bad(json!(1), "boolean:strict").await;
    ok(json!(false), "boolean:strict").await;
    assert_fails(
        json!({"field": "true"}),
        "boolean",
        "The field field must be true or false.",
    )
    .await;
}

#[tokio::test]
async fn confirmed() {
    let _c = container();
    assert_passes(
        json!({"field": "secret", "field_confirmation": "secret"}),
        "confirmed",
    )
    .await;
    assert_fails(
        json!({"field": "secret", "field_confirmation": "other"}),
        "confirmed",
        "The field field confirmation does not match.",
    )
    .await;
    assert_passes(
        json!({"field": "secret", "repeat": "secret"}),
        "confirmed:repeat",
    )
    .await;
    assert_fails(
        json!({"field": "secret"}),
        "confirmed",
        "The field field confirmation does not match.",
    )
    .await;
}

#[tokio::test]
async fn date_and_date_format() {
    let _c = container();
    ok(json!("2024-01-01"), "date").await;
    ok(json!("2024-01-01 12:30:00"), "date").await;
    bad(json!("2024-02-30"), "date").await;
    bad(json!("tomorrow"), "date").await;
    assert_fails(
        json!({"field": "not-a-date"}),
        "date",
        "The field field must be a valid date.",
    )
    .await;

    ok(json!("2024-01-01"), "date_format:Y-m-d").await;
    ok(json!("12:30"), "date_format:Y-m-d,H:i").await;
    assert_fails(
        json!({"field": "01/01/2024"}),
        "date_format:Y-m-d",
        "The field field must match the format Y-m-d.",
    )
    .await;
}

#[tokio::test]
async fn decimal() {
    let _c = container();
    ok(json!("9.99"), "decimal:2").await;
    ok(json!(9.99), "decimal:2").await;
    assert_fails(
        json!({"field": "9.9"}),
        "decimal:2",
        "The field field must have 2 decimal places.",
    )
    .await;
    ok(json!("9.123"), "decimal:2,4").await;
    assert_fails(
        json!({"field": "9.1"}),
        "decimal:2,4",
        "The field field must have 2-4 decimal places.",
    )
    .await;
    bad(json!("abc"), "decimal:0").await;
}

#[tokio::test]
async fn declined() {
    let _c = container();
    for value in [
        json!("no"),
        json!("off"),
        json!(0),
        json!(false),
        json!("false"),
        json!("0"),
    ] {
        ok(value, "declined").await;
    }
    assert_fails(
        json!({"field": "yes"}),
        "declined",
        "The field field must be declined.",
    )
    .await;
    assert_fails(
        json!({"other": "x", "field": "yes"}),
        "declined_if:other,x",
        "The field field must be declined when other is x.",
    )
    .await;
    assert_passes(json!({"other": "y", "field": "yes"}), "declined_if:other,x").await;
}

#[tokio::test]
async fn different_and_same() {
    let _c = container();
    assert_passes(json!({"field": "a", "other": "b"}), "different:other").await;
    assert_passes(json!({"field": "a"}), "different:other").await;
    assert_fails(
        json!({"field": "a", "other": "a"}),
        "different:other",
        "The field field and other must be different.",
    )
    .await;
    assert_passes(json!({"field": "a", "other": "a"}), "same:other").await;
    assert_fails(
        json!({"field": "a", "other": "b"}),
        "same:other",
        "The field field must match other.",
    )
    .await;
    assert_fails(
        json!({"field": 1, "other": "1"}),
        "same:other",
        "The field field must match other.",
    )
    .await;
}

#[tokio::test]
async fn digits() {
    let _c = container();
    ok(json!("123"), "digits:3").await;
    ok(json!(123), "digits:3").await;
    bad(json!("12a"), "digits:3").await;
    bad(json!(-12), "digits:3").await;
    assert_fails(
        json!({"field": "12"}),
        "digits:3",
        "The field field must be 3 digits.",
    )
    .await;
    ok(json!("123"), "digits_between:2,4").await;
    assert_fails(
        json!({"field": "1"}),
        "digits_between:2,4",
        "The field field must be between 2 and 4 digits.",
    )
    .await;
    ok(json!(123), "max_digits:3").await;
    assert_fails(
        json!({"field": 1234}),
        "max_digits:3",
        "The field field must not have more than 3 digits.",
    )
    .await;
    ok(json!(123), "min_digits:3").await;
    assert_fails(
        json!({"field": 12}),
        "min_digits:3",
        "The field field must have at least 3 digits.",
    )
    .await;
}

#[tokio::test]
async fn email() {
    let _c = container();
    ok(json!("taylor@laravel.com"), "email").await;
    ok(json!("taylor@laravel.com"), "email:rfc,dns").await;
    bad(json!("user@localhost"), "email:filter").await;
    bad(json!(123), "email").await;
    assert_fails(
        json!({"field": "nope"}),
        "email",
        "The field field must be a valid email address.",
    )
    .await;
}

#[tokio::test]
async fn starts_and_ends_with() {
    let _c = container();
    ok(json!("foobar"), "starts_with:foo,baz").await;
    assert_fails(
        json!({"field": "bar"}),
        "starts_with:foo",
        "The field field must start with one of the following: foo.",
    )
    .await;
    ok(json!("xbar"), "ends_with:foo,bar").await;
    assert_fails(
        json!({"field": "x"}),
        "ends_with:foo,bar",
        "The field field must end with one of the following: foo, bar.",
    )
    .await;
    ok(json!("barfoo"), "doesnt_start_with:foo").await;
    assert_fails(
        json!({"field": "foobar"}),
        "doesnt_start_with:foo",
        "The field field must not start with one of the following: foo.",
    )
    .await;
    ok(json!("foobar"), "doesnt_end_with:foo").await;
    assert_fails(
        json!({"field": "barfoo"}),
        "doesnt_end_with:foo",
        "The field field must not end with one of the following: foo.",
    )
    .await;
}

#[tokio::test]
async fn filled() {
    let _c = container();
    assert_passes(json!({}), "filled").await;
    ok(json!("x"), "filled").await;
    assert_fails(
        json!({"field": ""}),
        "filled",
        "The field field must have a value.",
    )
    .await;
    assert_fails(
        json!({"field": null}),
        "filled",
        "The field field must have a value.",
    )
    .await;
}

#[tokio::test]
async fn comparisons() {
    let _c = container();
    assert_passes(json!({"field": 5, "other": 3}), "gt:other").await;
    assert_fails(
        json!({"field": 2, "other": 3}),
        "gt:other",
        "The field field must be greater than 3.",
    )
    .await;
    assert_fails(
        json!({"field": "ab", "other": "abc"}),
        "gt:other",
        "The field field must be greater than 3 characters.",
    )
    .await;
    assert_passes(json!({"field": 10}), "gt:5").await;
    assert_fails(
        json!({"field": 4}),
        "gt:5",
        "The field field must be greater than 5.",
    )
    .await;
    assert_passes(json!({"field": "10", "other": "9"}), "gt:other").await;
    assert_passes(json!({"field": [1, 2], "other": [1]}), "array|gt:other").await;
    assert_fails(
        json!({"field": [1], "other": [1, 2]}),
        "array|gt:other",
        "The field field must have more than 2 items.",
    )
    .await;
    bad(json!({"a": 1}), "gt:5").await;

    assert_passes(json!({"field": 5}), "gte:5").await;
    assert_fails(
        json!({"field": 4}),
        "gte:5",
        "The field field must be greater than or equal to 5.",
    )
    .await;
    assert_passes(json!({"field": 4}), "lt:5").await;
    assert_fails(
        json!({"field": 5}),
        "lt:5",
        "The field field must be less than 5.",
    )
    .await;
    assert_passes(json!({"field": 5}), "lte:5").await;
    assert_fails(
        json!({"field": 6}),
        "lte:5",
        "The field field must be less than or equal to 5.",
    )
    .await;
    assert_fails(
        json!({"field": "abcdef", "other": "abc"}),
        "lte:other",
        "The field field must be less than or equal to 3 characters.",
    )
    .await;
}

#[tokio::test]
async fn hex_color() {
    let _c = container();
    ok(json!("#fff"), "hex_color").await;
    ok(json!("#ffffff"), "hex_color").await;
    assert_fails(
        json!({"field": "fff"}),
        "hex_color",
        "The field field must be a valid hexadecimal color.",
    )
    .await;
}

#[tokio::test]
async fn in_and_not_in() {
    let _c = container();
    ok(json!("a"), "in:a,b").await;
    ok(json!(1), "in:1,2").await;
    assert_fails(
        json!({"field": "c"}),
        "in:a,b",
        "The selected field is invalid.",
    )
    .await;
    bad(json!(["a"]), "in:a,b").await;
    ok(json!(["a", "b"]), "array|in:a,b").await;
    bad(json!(["a", "c"]), "array|in:a,b").await;
    ok(json!("with,comma"), "in:\"with,comma\",other").await;

    ok(json!("c"), "not_in:a,b").await;
    assert_fails(
        json!({"field": "a"}),
        "not_in:a,b",
        "The selected field is invalid.",
    )
    .await;
}

#[tokio::test]
async fn in_array() {
    let _c = container();
    assert_passes(
        json!({"others": ["a", "b"], "field": "a"}),
        "in_array:others.*",
    )
    .await;
    assert_fails(
        json!({"others": ["a", "b"], "field": "c"}),
        "in_array:others.*",
        "The field field must exist in others.*.",
    )
    .await;
}

#[tokio::test]
async fn integer_and_numeric() {
    let _c = container();
    for value in [json!(5), json!("5"), json!("-5"), json!(5.0)] {
        ok(value, "integer").await;
    }
    for value in [json!("5.5"), json!(5.5), json!("abc"), json!("05")] {
        bad(value, "integer").await;
    }
    bad(json!("5"), "integer:strict").await;
    ok(json!(5), "integer:strict").await;
    assert_fails(
        json!({"field": "abc"}),
        "integer",
        "The field field must be an integer.",
    )
    .await;
    ok(json!("5"), "int").await;

    for value in [json!(1), json!("1.5"), json!("-1e3"), json!(" 2 ")] {
        ok(value, "numeric").await;
    }
    bad(json!("1"), "numeric:strict").await;
    ok(json!(1.5), "numeric:strict").await;
    assert_fails(
        json!({"field": "abc"}),
        "numeric",
        "The field field must be a number.",
    )
    .await;
}

#[tokio::test]
async fn ip_and_mac_addresses() {
    let _c = container();
    ok(json!("127.0.0.1"), "ip").await;
    ok(json!("::1"), "ip").await;
    assert_fails(
        json!({"field": "999.0.0.1"}),
        "ip",
        "The field field must be a valid IP address.",
    )
    .await;
    ok(json!("127.0.0.1"), "ipv4").await;
    assert_fails(
        json!({"field": "::1"}),
        "ipv4",
        "The field field must be a valid IPv4 address.",
    )
    .await;
    ok(json!("::1"), "ipv6").await;
    assert_fails(
        json!({"field": "127.0.0.1"}),
        "ipv6",
        "The field field must be a valid IPv6 address.",
    )
    .await;
    ok(json!("01:23:45:67:89:ab"), "mac_address").await;
    assert_fails(
        json!({"field": "nope"}),
        "mac_address",
        "The field field must be a valid MAC address.",
    )
    .await;
}

#[tokio::test]
async fn json_rule() {
    let _c = container();
    ok(json!("{\"a\": 1}"), "json").await;
    ok(json!("[]"), "json").await;
    ok(json!(5), "json").await;
    bad(json!(["a"]), "json").await;
    assert_fails(
        json!({"field": "{a"}),
        "json",
        "The field field must be a valid JSON string.",
    )
    .await;
}

#[tokio::test]
async fn case_rules() {
    let _c = container();
    ok(json!("abc"), "lowercase").await;
    assert_fails(
        json!({"field": "Abc"}),
        "lowercase",
        "The field field must be lowercase.",
    )
    .await;
    ok(json!("ABC"), "uppercase").await;
    assert_fails(
        json!({"field": "AbC"}),
        "uppercase",
        "The field field must be uppercase.",
    )
    .await;
}

#[tokio::test]
async fn max_min_and_size() {
    let _c = container();
    ok(json!("abc"), "max:3").await;
    ok(json!("ééé"), "max:3").await;
    assert_fails(
        json!({"field": "abcd"}),
        "max:3",
        "The field field must not be greater than 3 characters.",
    )
    .await;
    assert_fails(
        json!({"field": 4}),
        "integer|max:3",
        "The field field must not be greater than 3.",
    )
    .await;
    assert_fails(
        json!({"field": 1234}),
        "max:3",
        "The field field must not be greater than 3 characters.",
    )
    .await;
    assert_fails(
        json!({"field": [1, 2]}),
        "array|max:1",
        "The field field must not have more than 1 items.",
    )
    .await;

    assert_fails(
        json!({"field": "ab"}),
        "min:3",
        "The field field must be at least 3 characters.",
    )
    .await;
    assert_fails(
        json!({"field": 2}),
        "numeric|min:3",
        "The field field must be at least 3.",
    )
    .await;
    assert_fails(
        json!({"field": [1]}),
        "array|min:2",
        "The field field must have at least 2 items.",
    )
    .await;
    ok(json!("2.5"), "numeric|min:2.5").await;

    ok(json!("abc"), "size:3").await;
    assert_fails(
        json!({"field": "ab"}),
        "size:3",
        "The field field must be 3 characters.",
    )
    .await;
    assert_fails(
        json!({"field": 9}),
        "integer|size:10",
        "The field field must be 10.",
    )
    .await;
    assert_fails(
        json!({"field": [1]}),
        "array|size:2",
        "The field field must contain 2 items.",
    )
    .await;
    ok(json!("10.0"), "decimal:1|size:10").await;
}

#[tokio::test]
async fn missing_rules() {
    let _c = container();
    assert_passes(json!({}), "missing").await;
    assert_fails(
        json!({"field": null}),
        "missing",
        "The field field must be missing.",
    )
    .await;
    assert_fails(
        json!({"other": "x", "field": 1}),
        "missing_if:other,x",
        "The field field must be missing when other is x.",
    )
    .await;
    assert_passes(json!({"other": "y", "field": 1}), "missing_if:other,x").await;
    assert_fails(
        json!({"other": "y", "field": 1}),
        "missing_unless:other,x",
        "The field field must be missing unless other is x.",
    )
    .await;
    assert_fails(
        json!({"other": 1, "field": 1}),
        "missing_with:other",
        "The field field must be missing when other is present.",
    )
    .await;
    assert_passes(json!({"field": 1}), "missing_with:other").await;
    assert_fails(
        json!({"a": 1, "b": 1, "field": 1}),
        "missing_with_all:a,b",
        "The field field must be missing when a / b are present.",
    )
    .await;
    assert_passes(json!({"a": 1, "field": 1}), "missing_with_all:a,b").await;
}

#[tokio::test]
async fn multiple_of() {
    let _c = container();
    ok(json!(10), "multiple_of:5").await;
    ok(json!(1.5), "multiple_of:0.5").await;
    ok(json!(0), "multiple_of:5").await;
    bad(json!("abc"), "multiple_of:5").await;
    assert_fails(
        json!({"field": 7}),
        "multiple_of:5",
        "The field field must be a multiple of 5.",
    )
    .await;
}

#[tokio::test]
async fn present_rules() {
    let _c = container();
    assert_fails(json!({}), "present", "The field field must be present.").await;
    assert_passes(json!({"field": null}), "present").await;
    assert_fails(
        json!({"other": "x"}),
        "present_if:other,x",
        "The field field must be present when other is x.",
    )
    .await;
    assert_fails(
        json!({"other": "y"}),
        "present_unless:other,x",
        "The field field must be present unless other is x.",
    )
    .await;
    assert_passes(json!({"other": "x"}), "present_unless:other,x").await;
    assert_fails(
        json!({"other": 1}),
        "present_with:other",
        "The field field must be present when other is present.",
    )
    .await;
    assert_fails(
        json!({"a": 1, "b": 1}),
        "present_with_all:a,b",
        "The field field must be present when a / b are present.",
    )
    .await;
    assert_passes(json!({"a": 1}), "present_with_all:a,b").await;
}

#[tokio::test]
async fn prohibited_rules() {
    let _c = container();
    assert_passes(json!({}), "prohibited").await;
    assert_passes(json!({"field": ""}), "prohibited").await;
    assert_fails(
        json!({"field": "x"}),
        "prohibited",
        "The field field is prohibited.",
    )
    .await;
    assert_fails(
        json!({"other": "x", "field": "y"}),
        "prohibited_if:other,x",
        "The field field is prohibited when other is x.",
    )
    .await;
    assert_fails(
        json!({"other": "yes", "field": "y"}),
        "prohibited_if_accepted:other",
        "The field field is prohibited when other is accepted.",
    )
    .await;
    assert_fails(
        json!({"other": "no", "field": "y"}),
        "prohibited_if_declined:other",
        "The field field is prohibited when other is declined.",
    )
    .await;
    assert_fails(
        json!({"other": "z", "field": "v"}),
        "prohibited_unless:other,x,y",
        "The field field is prohibited unless other is in x, y.",
    )
    .await;
    assert_passes(
        json!({"other": "x", "field": "v"}),
        "prohibited_unless:other,x,y",
    )
    .await;
    assert_fails(
        json!({"field": "a", "other": "b"}),
        "prohibits:other",
        "The field field prohibits other from being present.",
    )
    .await;
    assert_passes(json!({"field": "a"}), "prohibits:other").await;
}

#[tokio::test]
async fn regex_rules() {
    let _c = container();
    ok(json!("ABC"), "regex:/^[a-z]+$/i").await;
    ok(json!(123), "regex:/^\\d+$/").await;
    assert_fails(
        json!({"field": "abc1"}),
        "regex:/^[a-z]+$/i",
        "The field field format is invalid.",
    )
    .await;
    ok(json!("b"), "not_regex:/^a/").await;
    assert_fails(
        json!({"field": "abc"}),
        "not_regex:/^a/",
        "The field field format is invalid.",
    )
    .await;
    ok(json!("a,b"), "regex:/^a,b$/").await;
}

#[tokio::test]
async fn required() {
    let _c = container();
    for data in [
        json!({}),
        json!({"field": ""}),
        json!({"field": "   "}),
        json!({"field": []}),
        json!({"field": null}),
    ] {
        assert_fails(data, "required", "The field field is required.").await;
    }
    ok(json!(0), "required").await;
    ok(json!(false), "required").await;
    ok(json!("0"), "required").await;
}

#[tokio::test]
async fn required_conditionally() {
    let _c = container();
    assert_fails(
        json!({"other": "x"}),
        "required_if:other,x",
        "The field field is required when other is x.",
    )
    .await;
    assert_passes(json!({"other": "y"}), "required_if:other,x").await;
    assert_passes(json!({}), "required_if:other,x").await;
    assert_fails(
        json!({"other": true}),
        "required_if:other,true",
        "The field field is required when other is true.",
    )
    .await;
    assert_fails(
        json!({"other": "on"}),
        "required_if_accepted:other",
        "The field field is required when other is accepted.",
    )
    .await;
    assert_passes(json!({"other": "off"}), "required_if_accepted:other").await;
    assert_fails(
        json!({"other": "off"}),
        "required_if_declined:other",
        "The field field is required when other is declined.",
    )
    .await;
    assert_fails(
        json!({"other": "z"}),
        "required_unless:other,x,y",
        "The field field is required unless other is in x, y.",
    )
    .await;
    assert_passes(json!({"other": "x"}), "required_unless:other,x,y").await;
    assert_passes(json!({}), "required_unless:other,null").await;
    assert_fails(
        json!({"a": 1}),
        "required_with:a,b",
        "The field field is required when a / b is present.",
    )
    .await;
    assert_passes(json!({}), "required_with:a,b").await;
    assert_fails(
        json!({"a": 1, "b": 1}),
        "required_with_all:a,b",
        "The field field is required when a / b are present.",
    )
    .await;
    assert_passes(json!({"a": 1}), "required_with_all:a,b").await;
    assert_fails(
        json!({"a": 1}),
        "required_without:a,b",
        "The field field is required when a / b is not present.",
    )
    .await;
    assert_passes(json!({"a": 1, "b": 2}), "required_without:a,b").await;
    assert_fails(
        json!({}),
        "required_without_all:a,b",
        "The field field is required when none of a / b are present.",
    )
    .await;
    assert_passes(json!({"a": 1}), "required_without_all:a,b").await;
}

#[tokio::test]
async fn required_if_converts_booleans_for_boolean_fields() {
    let _c = container();
    let rules = illuminate_validation::Rules::from([
        ("has_appointment", "required|boolean"),
        ("date", "required_if:has_appointment,true"),
    ]);
    let errors = common::errors(json!({"has_appointment": "1"}), rules).await;
    assert_eq!(
        errors.first("date"),
        Some("The date field is required when has appointment is 1.")
    );
}

#[tokio::test]
async fn string_rule() {
    let _c = container();
    ok(json!("a"), "string").await;
    assert_fails(
        json!({"field": 1}),
        "string",
        "The field field must be a string.",
    )
    .await;
}

#[tokio::test]
async fn timezone() {
    let _c = container();
    ok(json!("America/New_York"), "timezone").await;
    ok(json!("UTC"), "timezone").await;
    ok(json!("Africa/Lagos"), "timezone:Africa").await;
    bad(json!("Europe/Paris"), "timezone:Africa").await;
    assert_fails(
        json!({"field": "Mars/Phobos"}),
        "timezone",
        "The field field must be a valid timezone.",
    )
    .await;
}

#[tokio::test]
async fn timezone_per_country() {
    let _c = container();

    ok(json!("Asia/Riyadh"), "timezone:per_country,sa").await;
    assert_fails(
        json!({"field": "Asia/Tokyo"}),
        "timezone:per_country,SA",
        "The field field must be a valid timezone.",
    )
    .await;
    ok(json!("Asia/Tokyo"), "timezone:PER_COUNTRY,JP").await;
    ok(json!("America/New_York"), "timezone:per_country,US").await;
    ok(json!("Pacific/Honolulu"), "timezone:per_country,US").await;
    bad(json!("America/Toronto"), "timezone:per_country,US").await;
    bad(json!("US/Eastern"), "timezone:per_country,US").await;
    bad(json!("UTC"), "timezone:per_country,SA").await;
    bad(json!("Asia/Riyadh"), "timezone:per_country,ZZ").await;
}

#[tokio::test]
async fn timezone_per_country_requires_a_two_letter_code() {
    let _c = container();

    for rule in ["timezone:per_country", "timezone:per_country,SAU"] {
        let mut validator = illuminate_validation::Validator::make(
            json!({"field": "Asia/Riyadh"}),
            [("field", rule)],
        );
        let error = validator.try_passes().await.unwrap_err();
        assert_eq!(
            error.to_string(),
            "timezone_identifiers_list(): Argument #2 ($countryCode) must be a two-letter ISO 3166-1 compatible country code when argument #1 ($timezoneGroup) is DateTimeZone::PER_COUNTRY"
        );
    }
}

#[tokio::test]
async fn url() {
    let _c = container();
    ok(json!("https://laravel.com"), "url").await;
    ok(json!("https://laravel.com"), "url:http,https").await;
    bad(json!("http://laravel.com"), "url:https").await;
    ok(json!("minecraft://server"), "url:minecraft,steam").await;
    assert_fails(
        json!({"field": "nope"}),
        "url",
        "The field field must be a valid URL.",
    )
    .await;
}

#[tokio::test]
async fn ulid_and_uuid() {
    let _c = container();
    ok(json!("01ARZ3NDEKTSV4RRFFQ69G5FAV"), "ulid").await;
    assert_fails(
        json!({"field": "nope"}),
        "ulid",
        "The field field must be a valid ULID.",
    )
    .await;
    ok(json!("a0a2a2d2-0b87-4a18-83f2-2529882be2de"), "uuid").await;
    ok(json!("a0a2a2d2-0b87-4a18-83f2-2529882be2de"), "uuid:4").await;
    bad(json!("a0a2a2d2-0b87-4a18-83f2-2529882be2de"), "uuid:7").await;
    assert_fails(
        json!({"field": "nope"}),
        "uuid",
        "The field field must be a valid UUID.",
    )
    .await;
}

#[tokio::test]
async fn encoding() {
    let _c = container();
    ok(json!("héllo"), "encoding:UTF-8").await;
    assert_fails(
        json!({"field": "héllo"}),
        "encoding:ascii",
        "The field field must be encoded in ascii.",
    )
    .await;
}

#[tokio::test]
async fn non_implicit_rules_skip_missing_and_empty_values() {
    let _c = container();
    assert_passes(json!({}), "email|max:3|integer").await;
    assert_passes(json!({"field": ""}), "email|integer").await;
    assert_passes(json!({"field": "  "}), "email").await;
    // Present `null` values still run non-implicit rules (that's what `nullable` is for).
    assert_fails(
        json!({"field": null}),
        "string",
        "The field field must be a string.",
    )
    .await;
}

#[tokio::test]
async fn attribute_names_are_humanized() {
    let _c = container();
    let errors = common::errors(
        json!({}),
        [("first_name", "required"), ("lastName", "required")],
    )
    .await;
    assert_eq!(
        errors.first("first_name"),
        Some("The first name field is required.")
    );
    assert_eq!(
        errors.first("lastName"),
        Some("The last name field is required.")
    );
}

#[tokio::test]
async fn unknown_rules_are_reported() {
    let _c = container();
    let mut validator =
        illuminate_validation::Validator::make(json!({"field": "x"}), [("field", "nonsense_rule")]);
    let error = validator.try_passes().await.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Validation rule [nonsense_rule] does not exist")
    );

    let mut validator =
        illuminate_validation::Validator::make(json!({"field": "x"}), [("field", "max")]);
    let error = validator.try_passes().await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "Validation rule max requires at least 1 parameters."
    );
}
