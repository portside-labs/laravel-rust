//! The built-in validation rules (Laravel's `ValidatesAttributes`).

use std::sync::LazyLock;

use base64::Engine;
use regex::Regex;

use illuminate_container::try_app;
use illuminate_support::error::InvalidArgumentException;
use illuminate_support::{Result, Str, Value};

use crate::data;
use crate::date;
use crate::files;
use crate::formats;
use crate::parser::Compiled;
use crate::php::{
    self, Dec, filter_int, gettype, in_array, is_int, is_numeric, is_numeric_str, strict_eq,
    to_php_string,
};
use crate::rule::{CurrentPasswordVerifier, ValidationContext};
use crate::rules::{DatabaseRule, DatabaseRuleKind};
use crate::validator::{NUMERIC_RULES, Validator};

static ALPHA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[\pL\pM]+\z").unwrap());
static ALPHA_ASCII: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[a-zA-Z]+\z").unwrap());
static ALPHA_DASH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[\pL\pM\pN_-]+\z").unwrap());
static ALPHA_DASH_ASCII: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A[a-zA-Z0-9_-]+\z").unwrap());
static ALPHA_NUM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[\pL\pM\pN]+\z").unwrap());
static ALPHA_NUM_ASCII: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A[a-zA-Z0-9]+\z").unwrap());
static DECIMAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[+-]?\d*\.?(\d*)$").unwrap());

const IMAGE_EXTENSIONS: [&str; 9] = [
    "jpg", "jpeg", "png", "gif", "bmp", "webp", "avif", "heic", "heif",
];
const PHP_EXTENSIONS: [&str; 8] = [
    "php", "php3", "php4", "php5", "php7", "php8", "phtml", "phar",
];

fn require_parameters(count: usize, params: &[String], rule: &str) -> Result<()> {
    if params.len() < count {
        return Err(InvalidArgumentException::new(format!(
            "Validation rule {rule} requires at least {count} parameters."
        ))
        .into());
    }
    Ok(())
}

fn accepted_value(value: &Value) -> bool {
    match value {
        Value::String(s) => matches!(s.as_str(), "yes" | "on" | "1" | "true"),
        Value::Number(n) => n.as_i64() == Some(1) && !n.is_f64(),
        Value::Bool(b) => *b,
        _ => false,
    }
}

fn declined_value(value: &Value) -> bool {
    match value {
        Value::String(s) => matches!(s.as_str(), "no" | "off" | "0" | "false"),
        Value::Number(n) => n.as_i64() == Some(0) && !n.is_f64(),
        Value::Bool(b) => !*b,
        _ => false,
    }
}

fn is_string_or_numeric(value: &Value) -> bool {
    value.is_string() || is_numeric(value)
}

fn is_scalar(value: &Value) -> bool {
    matches!(value, Value::String(_) | Value::Number(_) | Value::Bool(_))
}

fn compare(first: &Dec, second: &Dec, operator: &str) -> bool {
    match operator {
        ">" => first > second,
        ">=" => first >= second,
        "<" => first < second,
        "<=" => first <= second,
        _ => first == second,
    }
}

/// Parse a dimension ratio (`3/2` or `1.5`) like `sscanf('%f/%d')`.
fn parse_ratio(ratio: &str) -> f64 {
    let (numerator, denominator) = match ratio.split_once('/') {
        Some((n, d)) => (
            n.trim().parse::<f64>().unwrap_or(0.0),
            d.trim().parse::<f64>().unwrap_or(0.0),
        ),
        None => (ratio.trim().parse::<f64>().unwrap_or(0.0), 1.0),
    };
    let numerator = if numerator == 0.0 { 1.0 } else { numerator };
    let denominator = if denominator == 0.0 { 1.0 } else { denominator };
    numerator / denominator
}

impl Validator {
    /// PHP's notion of "required": not null, not a blank string, not an
    /// empty array, and (for files) successfully uploaded.
    pub(crate) fn required(&self, value: &Value) -> bool {
        match value {
            Value::Null => false,
            Value::String(s) => !php::trim(s).is_empty(),
            Value::Array(items) => !items.is_empty(),
            Value::Object(map) => match self.file_for(value) {
                Some(file) => file.is_valid(),
                None => !map.is_empty(),
            },
            _ => true,
        }
    }

    fn other(&self, attribute: &str) -> &Value {
        data::get(&self.data, attribute).unwrap_or(&Value::Null)
    }

    fn accepted(&self, value: &Value) -> bool {
        self.required(value) && accepted_value(value)
    }

    fn declined(&self, value: &Value) -> bool {
        self.required(value) && declined_value(value)
    }

    /// `parseDependentRuleParameters`: the other field's value, and the
    /// parameter values converted to booleans / null when appropriate.
    fn dependent(&self, params: &[String]) -> (Vec<Value>, Value) {
        let other = self.other(&params[0]).clone();
        let mut values: Vec<Value> = params[1..]
            .iter()
            .map(|v| Value::String(v.clone()))
            .collect();
        let converts_to_bool = self
            .rules
            .get(&params[0])
            .is_some_and(|rules| rules.iter().any(|r| r.raw() == Some("boolean")));
        if converts_to_bool || other.is_boolean() {
            values = values
                .into_iter()
                .map(|v| match v.as_str() {
                    Some("true") => Value::Bool(true),
                    Some("false") => Value::Bool(false),
                    _ => v,
                })
                .collect();
        }
        if other.is_null() {
            values = values
                .into_iter()
                .map(|v| match v.as_str() {
                    Some(s) if s.eq_ignore_ascii_case("null") => Value::Null,
                    _ => v,
                })
                .collect();
        }
        (values, other)
    }

    fn dependent_matches(&self, params: &[String]) -> bool {
        let (values, other) = self.dependent(params);
        in_array(&other, &values, other.is_boolean() || other.is_null())
    }

    fn any_failing_required(&self, attributes: &[String]) -> bool {
        attributes.iter().any(|key| !self.required(self.other(key)))
    }

    fn all_failing_required(&self, attributes: &[String]) -> bool {
        attributes.iter().all(|key| !self.required(self.other(key)))
    }

    fn has_any(&self, attributes: &[String]) -> bool {
        attributes.iter().any(|key| self.has(key))
    }

    fn has_all(&self, attributes: &[String]) -> bool {
        !attributes.is_empty() && attributes.iter().all(|key| self.has(key))
    }

    /// The "size" of a value (`getSize`): the number itself for numeric
    /// attributes, the item count for arrays, kilobytes for files, and the
    /// character count for everything else.
    pub(crate) fn size_of(
        &self,
        attribute: &str,
        value: &Value,
        extra_numeric: bool,
    ) -> Option<Dec> {
        let has_numeric = extra_numeric || self.has_rule(attribute, NUMERIC_RULES);
        if is_numeric(value) && has_numeric {
            return Dec::parse(&to_php_string(value));
        }
        if let Some(file) = self.file_for(value) {
            return Dec::from_f64(file.size() as f64 / 1024.0);
        }
        if data::is_array(value) {
            return Some(Dec::from_usize(data::keys(value).len()));
        }
        Some(Dec::from_usize(to_php_string(value).chars().count()))
    }

    fn size_compare(
        &self,
        attribute: &str,
        value: &Value,
        parameter: &str,
        operator: &str,
        numeric: bool,
    ) -> bool {
        match (
            self.size_of(attribute, value, numeric),
            Dec::parse(parameter),
        ) {
            (Some(size), Some(limit)) => compare(&size, &limit, operator),
            _ => false,
        }
    }

    /// `gt`, `gte`, `lt` and `lte`.
    fn compare_with_field(
        &self,
        attribute: &str,
        value: &Value,
        params: &[String],
        operator: &str,
        extra: bool,
    ) -> bool {
        let parameter = &params[0];
        let compared = data::get(&self.data, parameter);
        let compared_is_null = compared.is_none_or(Value::is_null);

        if compared_is_null && is_numeric(value) && is_numeric_str(parameter) {
            return self.size_compare(attribute, value, parameter, operator, extra);
        }
        if is_numeric_str(parameter) {
            return false;
        }
        let has_numeric = extra || self.has_rule(attribute, NUMERIC_RULES);
        if let Some(compared) = compared
            && has_numeric
            && is_numeric(value)
            && is_numeric(compared)
        {
            return match (
                Dec::parse(&to_php_string(value)),
                Dec::parse(&to_php_string(compared)),
            ) {
                (Some(a), Some(b)) => compare(&a, &b, operator),
                _ => false,
            };
        }
        if gettype(Some(value)) != gettype(compared) {
            return false;
        }
        let compared = compared.unwrap_or(&Value::Null);
        match (
            self.size_of(attribute, value, extra),
            self.size_of(attribute, compared, extra),
        ) {
            (Some(a), Some(b)) => compare(&a, &b, operator),
            _ => false,
        }
    }

    fn date_format_of(&self, attribute: &str) -> Option<String> {
        self.rule_params(attribute, &["DateFormat"])
            .and_then(|p| p.first().cloned())
    }

    /// `after`, `before`, `after_or_equal`, `before_or_equal`, `date_equals`.
    fn compare_dates(
        &self,
        attribute: &str,
        value: &Value,
        params: &[String],
        operator: &str,
    ) -> bool {
        if !is_string_or_numeric(value) {
            return false;
        }
        let value = to_php_string(value);
        if let Some(format) = self.date_format_of(attribute) {
            return self.check_date_time_order(&format, &value, &params[0], operator);
        }
        let seconds = |instant: date::Instant| instant.div_euclid(1_000_000);
        let other = date::parse(&params[0]).or_else(|| match self.value(&params[0]) {
            None | Some(Value::Null) => None,
            Some(other) => date::parse(&to_php_string(other)),
        });
        php::compare_optional(
            date::parse(&value).map(seconds),
            other.map(seconds),
            operator,
        )
    }

    fn check_date_time_order(
        &self,
        format: &str,
        first: &str,
        second: &str,
        operator: &str,
    ) -> bool {
        let first_date = date::parse_with_optional_format(format, first);
        let second_format = self
            .date_format_of(second)
            .unwrap_or_else(|| format.to_string());
        let second_date = match date::parse_with_optional_format(&second_format, second) {
            Some(date) => Some(date),
            None => match self.value(second) {
                None | Some(Value::Null) => return true,
                Some(value) => {
                    date::parse_with_optional_format(&second_format, &to_php_string(value))
                }
            },
        };
        match (first_date, second_date) {
            (Some(a), Some(b)) => php::compare_optional(Some(a), Some(b), operator),
            _ => false,
        }
    }

    fn validate_in(&self, attribute: &str, value: &Value, params: &[String]) -> bool {
        if data::is_array(value) && self.has_rule(attribute, &["Array"]) {
            let items: Vec<&Value> = match value {
                Value::Array(items) => items.iter().collect(),
                Value::Object(map) => map.values().collect(),
                _ => Vec::new(),
            };
            return items
                .iter()
                .all(|item| !data::is_array(item) && params.contains(&to_php_string(item)));
        }
        !data::is_array(value) && !data::is_file(value) && params.contains(&to_php_string(value))
    }

    fn array_values(value: &Value) -> Vec<Value> {
        match value {
            Value::Array(items) => items.clone(),
            Value::Object(map) => map.values().cloned().collect(),
            _ => Vec::new(),
        }
    }

    fn valid_file(&self, value: &Value) -> Option<&illuminate_http::UploadedFile> {
        self.file_for(value).filter(|f| f.is_valid())
    }

    fn should_block_php_upload(file: &illuminate_http::UploadedFile, params: &[String]) -> bool {
        if params.iter().any(|p| p == "php") {
            return false;
        }
        let extension = file.client_original_extension();
        PHP_EXTENSIONS.contains(&extension.trim().to_lowercase().as_str())
    }

    fn validate_mimes(&self, value: &Value, params: &[String]) -> bool {
        let Some(file) = self.valid_file(value) else {
            return false;
        };
        if Self::should_block_php_upload(file, params) {
            return false;
        }
        let mut allowed: Vec<String> = params.iter().map(|p| p.trim().to_lowercase()).collect();
        if allowed.iter().any(|p| p == "jpg" || p == "jpeg") {
            allowed.push("jpg".into());
            allowed.push("jpeg".into());
        }
        let mime = files::mime_type(file);
        files::extensions_for_mime(&mime)
            .iter()
            .any(|ext| allowed.contains(ext))
    }

    fn validate_mimetypes(&self, value: &Value, params: &[String]) -> bool {
        let Some(file) = self.valid_file(value) else {
            return false;
        };
        if Self::should_block_php_upload(file, params) {
            return false;
        }
        let mime = files::mime_type(file);
        let wildcard = format!("{}/*", mime.split('/').next().unwrap_or_default());
        params.iter().any(|p| *p == mime || *p == wildcard)
    }

    fn validate_dimensions(&self, value: &Value, params: &[String]) -> Result<bool> {
        let Some(file) = self.valid_file(value) else {
            return Ok(false);
        };
        if matches!(
            files::mime_type(file).as_str(),
            "image/svg+xml" | "image/svg"
        ) {
            return Ok(true);
        }
        let Some((width, height)) = files::dimensions(file.bytes()) else {
            return Ok(false);
        };
        require_parameters(1, params, "dimensions")?;
        let (width, height) = (width as f64, height as f64);
        let named: Vec<(&str, &str)> = params
            .iter()
            .map(|p| p.split_once('=').unwrap_or((p.as_str(), "")))
            .collect();
        let get = |key: &str| named.iter().find(|(k, _)| *k == key).map(|(_, v)| *v);
        let number = |key: &str| get(key).and_then(|v| v.trim().parse::<f64>().ok());

        let fails_basic = number("width").is_some_and(|w| w != width)
            || number("min_width").is_some_and(|w| w > width)
            || number("max_width").is_some_and(|w| w < width)
            || number("height").is_some_and(|h| h != height)
            || number("min_height").is_some_and(|h| h > height)
            || number("max_height").is_some_and(|h| h < height);
        if fails_basic || height == 0.0 {
            return Ok(false);
        }
        let precision = 1.0 / (((width + height) / 2.0).max(height) + 1.0);
        let actual = width / height;
        if let Some(ratio) = get("ratio")
            && (parse_ratio(ratio) - actual).abs() > precision
        {
            return Ok(false);
        }
        if let Some(ratio) = get("min_ratio")
            && parse_ratio(ratio) - actual > precision
        {
            return Ok(false);
        }
        if let Some(ratio) = get("max_ratio")
            && actual - parse_ratio(ratio) > precision
        {
            return Ok(false);
        }
        Ok(true)
    }

    fn distinct_values(&self, primary: &str) -> Vec<(String, Value)> {
        if let Some(cached) = self.distinct_cache.lock().unwrap().get(primary) {
            return cached.clone();
        }
        let mut flattened = Vec::new();
        match data::leading_explicit_path(primary) {
            Some(lead) => {
                if let Some(value) = data::get(&self.data, &lead) {
                    data::flatten(value, &lead, &mut flattened);
                }
            }
            None => data::flatten(&self.data, "", &mut flattened),
        }
        let values: Vec<(String, Value)> = flattened
            .into_iter()
            .filter(|(key, _)| data::matches_pattern(primary, key, false))
            .map(|(key, value)| (key, value.clone()))
            .collect();
        self.distinct_cache
            .lock()
            .unwrap()
            .insert(primary.to_string(), values.clone());
        values
    }

    fn validate_distinct(&self, attribute: &str, value: &Value, params: &[String]) -> bool {
        let primary = self.primary_attribute(attribute);
        let attribute_raw = data::segments_raw(attribute).join(".");
        let others: Vec<Value> = self
            .distinct_values(&primary)
            .into_iter()
            .filter(|(key, _)| *key != attribute_raw)
            .map(|(_, value)| value)
            .collect();
        if params.iter().any(|p| p == "ignore_case")
            && let Some(needle) = value.as_str()
        {
            let needle = needle.to_lowercase();
            return !others
                .iter()
                .any(|other| is_scalar(other) && to_php_string(other).to_lowercase() == needle);
        }
        !in_array(value, &others, params.iter().any(|p| p == "strict"))
    }

    fn guess_column(&self, attribute: &str) -> String {
        let is_implicit = self
            .implicit_attributes
            .values()
            .any(|keys| keys.iter().any(|k| k == attribute));
        let segments = data::segments(attribute);
        match segments.last() {
            Some(last) if is_implicit && last.parse::<f64>().is_err() => last.clone(),
            _ => data::unescape(attribute),
        }
    }

    /// `prepareUniqueId`: `[field]` reads another field, `null` means none.
    fn prepare_unique_id(&self, id: &Value) -> Option<Value> {
        let Value::String(raw) = id else {
            return Some(id.clone());
        };
        let raw = raw
            .replace("\\\"", "\"")
            .replace("\\'", "'")
            .replace("\\\\", "\\");
        let resolved = match (raw.find('['), raw.rfind(']')) {
            (Some(open), Some(close)) if open < close => self.other(&raw[open + 1..close]).clone(),
            _ => Value::String(raw),
        };
        match &resolved {
            Value::Null => None,
            Value::String(s) if s.eq_ignore_ascii_case("null") => None,
            Value::String(s) if filter_int(&resolved) => {
                s.trim().parse::<i64>().ok().map(Value::from)
            }
            _ => Some(resolved),
        }
    }

    async fn validate_database(
        &self,
        attribute: &str,
        value: &Value,
        params: &[String],
        rule: &Compiled,
    ) -> Result<bool> {
        require_parameters(
            1,
            params,
            if rule.name() == Some("Unique") {
                "unique"
            } else {
                "exists"
            },
        )?;
        let rule = match rule {
            Compiled::Database(rule) => rule.as_ref().clone(),
            _ => {
                let kind = if rule.name() == Some("Unique") {
                    DatabaseRuleKind::Unique
                } else {
                    DatabaseRuleKind::Exists
                };
                DatabaseRule::from_params(kind, params)
            }
        };
        let verifier = self
            .verifier
            .clone()
            .ok_or_else(|| Validator::runtime_error("Presence verifier has not been set."))?;
        let column = rule
            .column
            .clone()
            .unwrap_or_else(|| self.guess_column(attribute));

        match rule.kind {
            DatabaseRuleKind::Unique => {
                let id = rule
                    .ignore
                    .as_ref()
                    .and_then(|id| self.prepare_unique_id(id));
                let id_column = rule.id_column.clone().unwrap_or_else(|| "id".to_string());
                let count = verifier
                    .count(
                        &rule.table,
                        &column,
                        value,
                        id.as_ref(),
                        Some(&id_column),
                        &rule.wheres,
                    )
                    .await?;
                Ok(count == 0)
            }
            DatabaseRuleKind::Exists => {
                if data::is_array(value) {
                    let mut values: Vec<Value> = Vec::new();
                    for item in Self::array_values(value) {
                        if !values
                            .iter()
                            .any(|v| to_php_string(v) == to_php_string(&item))
                        {
                            values.push(item);
                        }
                    }
                    if values.is_empty() {
                        return Ok(true);
                    }
                    let count = verifier
                        .multi_count(&rule.table, &column, &values, &rule.wheres)
                        .await?;
                    Ok(count >= values.len())
                } else {
                    let count = verifier
                        .count(&rule.table, &column, value, None, None, &rule.wheres)
                        .await?;
                    Ok(count >= 1)
                }
            }
        }
    }

    /// Run a built-in (or extension) rule.
    pub(crate) async fn validate_rule(
        &self,
        name: &str,
        attribute: &str,
        value: &Value,
        params: &[String],
        rule: &Compiled,
        extra_numeric: bool,
    ) -> Result<bool> {
        match name {
            "Unique" | "Exists" => self.validate_database(attribute, value, params, rule).await,
            "CurrentPassword" => {
                let verifier = try_app::<dyn CurrentPasswordVerifier>().ok_or_else(|| {
                    Validator::runtime_error(
                        "The current_password rule needs a `dyn CurrentPasswordVerifier` in the container (the auth component binds one).",
                    )
                })?;
                Ok(verifier
                    .check(params.first().map(String::as_str), &to_php_string(value))
                    .await)
            }
            _ => self.validate_sync(name, attribute, value, params, extra_numeric),
        }
    }

    fn validate_sync(
        &self,
        name: &str,
        attribute: &str,
        v: &Value,
        p: &[String],
        extra: bool,
    ) -> Result<bool> {
        let rule = Str::snake(name);
        let req = |count: usize| require_parameters(count, p, &rule);

        Ok(match name {
            "Accepted" => self.accepted(v),
            "AcceptedIf" => {
                req(2)?;
                !self.dependent_matches(p) || self.accepted(v)
            }
            "Declined" => self.declined(v),
            "DeclinedIf" => {
                req(2)?;
                !self.dependent_matches(p) || self.declined(v)
            }
            "ActiveUrl" => v.as_str().and_then(formats::url_host).is_some(),
            "After" => {
                req(1)?;
                self.compare_dates(attribute, v, p, ">")
            }
            "AfterOrEqual" => {
                req(1)?;
                self.compare_dates(attribute, v, p, ">=")
            }
            "Before" => {
                req(1)?;
                self.compare_dates(attribute, v, p, "<")
            }
            "BeforeOrEqual" => {
                req(1)?;
                self.compare_dates(attribute, v, p, "<=")
            }
            "DateEquals" => {
                req(1)?;
                self.compare_dates(attribute, v, p, "=")
            }
            "Alpha" => {
                let ascii = p.first().is_some_and(|p| p == "ascii");
                v.as_str().is_some_and(|s| {
                    if ascii {
                        ALPHA_ASCII.is_match(s)
                    } else {
                        ALPHA.is_match(s)
                    }
                })
            }
            "AlphaDash" => {
                let ascii = p.first().is_some_and(|p| p == "ascii");
                is_string_or_numeric(v) && {
                    let s = to_php_string(v);
                    if ascii {
                        ALPHA_DASH_ASCII.is_match(&s)
                    } else {
                        ALPHA_DASH.is_match(&s)
                    }
                }
            }
            "AlphaNum" => {
                let ascii = p.first().is_some_and(|p| p == "ascii");
                is_string_or_numeric(v) && {
                    let s = to_php_string(v);
                    if ascii {
                        ALPHA_NUM_ASCII.is_match(&s)
                    } else {
                        ALPHA_NUM.is_match(&s)
                    }
                }
            }
            "Array" => {
                data::is_array(v)
                    && (p.is_empty()
                        || data::keys(v).iter().all(|k| p.contains(&data::unescape(k))))
            }
            "ArrayKeys" => {
                req(1)?;
                data::is_array(v) && data::keys(v).iter().all(|k| p.contains(&data::unescape(k)))
            }
            "Ascii" => v.as_str().is_some_and(|s| s.is_ascii()),
            "Base64" => match v.as_str() {
                Some(s) if !s.is_empty() => base64::engine::general_purpose::STANDARD
                    .decode(s)
                    .is_ok_and(|decoded| {
                        base64::engine::general_purpose::STANDARD.encode(decoded) == s
                    }),
                _ => false,
            },
            "Bail" | "Nullable" | "Sometimes" => true,
            "Between" => {
                req(2)?;
                self.size_compare(attribute, v, &p[0], ">=", false)
                    && self.size_compare(attribute, v, &p[1], "<=", false)
            }
            "Boolean" => {
                if p.first().is_some_and(|p| p == "strict") {
                    v.is_boolean()
                } else {
                    match v {
                        Value::Bool(_) => true,
                        Value::Number(n) => !n.is_f64() && matches!(n.as_i64(), Some(0) | Some(1)),
                        Value::String(s) => s == "0" || s == "1",
                        _ => false,
                    }
                }
            }
            "Confirmed" => {
                let other = p
                    .first()
                    .cloned()
                    .unwrap_or_else(|| format!("{attribute}_confirmation"));
                strict_eq(v, self.other(&other))
            }
            "Contains" => {
                data::is_array(v) && {
                    let values = Self::array_values(v);
                    p.iter()
                        .all(|param| in_array(&Value::String(param.clone()), &values, false))
                }
            }
            "DoesntContain" => {
                data::is_array(v) && {
                    let values = Self::array_values(v);
                    p.iter()
                        .all(|param| !in_array(&Value::String(param.clone()), &values, true))
                }
            }
            "Date" => is_string_or_numeric(v) && date::is_valid_date(&to_php_string(v)),
            "DateFormat" => {
                req(1)?;
                is_string_or_numeric(v) && {
                    let s = to_php_string(v);
                    p.iter().any(|format| date::matches_format(format, &s))
                }
            }
            "Decimal" => {
                req(1)?;
                is_numeric(v)
                    && match DECIMAL.captures(&to_php_string(v)) {
                        Some(captures) => {
                            let decimals = captures.get(1).map(|m| m.as_str().len()).unwrap_or(0);
                            let min = p[0].trim().parse::<usize>().unwrap_or(0);
                            match p.get(1) {
                                None => decimals == min,
                                Some(max) => {
                                    decimals >= min
                                        && decimals <= max.trim().parse::<usize>().unwrap_or(0)
                                }
                            }
                        }
                        None => false,
                    }
            }
            "Different" => {
                req(1)?;
                p.iter()
                    .all(|other| !self.has(other) || !strict_eq(v, self.other(other)))
            }
            "Digits" => {
                req(1)?;
                is_string_or_numeric(v) && {
                    let s = to_php_string(v);
                    s.bytes().all(|b| b.is_ascii_digit())
                        && p[0].trim().parse::<usize>().is_ok_and(|n| s.len() == n)
                }
            }
            "DigitsBetween" => {
                req(2)?;
                is_string_or_numeric(v) && {
                    let s = to_php_string(v);
                    let min = p[0].trim().parse::<usize>().unwrap_or(0);
                    let max = p[1].trim().parse::<usize>().unwrap_or(0);
                    s.bytes().all(|b| b.is_ascii_digit()) && s.len() >= min && s.len() <= max
                }
            }
            "Dimensions" => self.validate_dimensions(v, p)?,
            "Distinct" => self.validate_distinct(attribute, v, p),
            "StartsWith" | "DoesntStartWith" | "EndsWith" | "DoesntEndWith" => {
                if !is_string_or_numeric(v) {
                    false
                } else {
                    let s = to_php_string(v);
                    let starts = p
                        .iter()
                        .any(|needle| !needle.is_empty() && s.starts_with(needle.as_str()));
                    let ends = p
                        .iter()
                        .any(|needle| !needle.is_empty() && s.ends_with(needle.as_str()));
                    match name {
                        "StartsWith" => starts,
                        "DoesntStartWith" => !starts,
                        "EndsWith" => ends,
                        _ => !ends,
                    }
                }
            }
            "Email" => v.as_str().is_some_and(|s| formats::is_email(s, p)),
            "Encoding" => {
                req(1)?;
                let encoding = p[0].trim().to_ascii_lowercase();
                let bytes: Vec<u8> = match self.file_for(v) {
                    Some(file) => file.bytes().to_vec(),
                    None => to_php_string(v).into_bytes(),
                };
                match encoding.as_str() {
                    "utf-8" | "utf8" => std::str::from_utf8(&bytes).is_ok(),
                    "ascii" | "us-ascii" => bytes.is_ascii(),
                    "iso-8859-1" | "latin1" | "iso-8859-15" | "windows-1252" | "cp1252"
                    | "8bit" => true,
                    _ => {
                        return Err(InvalidArgumentException::new(format!(
                            "Validation rule encoding parameter [{}] is not a valid encoding.",
                            p[0]
                        ))
                        .into());
                    }
                }
            }
            "Enum" => is_scalar(v) && !v.is_boolean() && p.contains(&to_php_string(v)),
            "Extensions" => match self.valid_file(v) {
                Some(file) => {
                    !Self::should_block_php_upload(file, p)
                        && p.contains(&file.client_original_extension().to_lowercase())
                }
                None => false,
            },
            "File" => self.valid_file(v).is_some(),
            "Filled" => !self.has(attribute) || self.required(v),
            "Gt" => {
                req(1)?;
                self.compare_with_field(attribute, v, p, ">", extra)
            }
            "Gte" => {
                req(1)?;
                self.compare_with_field(attribute, v, p, ">=", extra)
            }
            "Lt" => {
                req(1)?;
                self.compare_with_field(attribute, v, p, "<", extra)
            }
            "Lte" => {
                req(1)?;
                self.compare_with_field(attribute, v, p, "<=", extra)
            }
            "HexColor" => v.as_str().is_some_and(formats::is_hex_color),
            "Image" => {
                let mut allowed: Vec<String> =
                    IMAGE_EXTENSIONS.iter().map(|e| e.to_string()).collect();
                if p.iter().any(|p| p == "allow_svg") {
                    allowed.push("svg".into());
                }
                self.validate_mimes(v, &allowed)
            }
            "In" => self.validate_in(attribute, v, p),
            "NotIn" => !self.validate_in(attribute, v, p),
            "InArray" => {
                req(1)?;
                let mut flattened = Vec::new();
                match data::leading_explicit_path(&p[0]) {
                    Some(lead) => {
                        if let Some(value) = data::get(&self.data, &lead) {
                            data::flatten(value, &lead, &mut flattened);
                        }
                    }
                    None => data::flatten(&self.data, "", &mut flattened),
                }
                let others: Vec<Value> = flattened
                    .into_iter()
                    .filter(|(key, _)| Str::is(&p[0], &data::unescape(key)))
                    .map(|(_, value)| value.clone())
                    .collect();
                in_array(v, &others, true)
            }
            "InArrayKeys" => {
                data::is_array(v)
                    && !p.is_empty()
                    && p.iter().any(|key| data::array_has_key(v, key))
            }
            "Integer" => {
                if p.first().is_some_and(|p| p == "strict") {
                    is_int(v)
                } else {
                    filter_int(v)
                }
            }
            "Ip" => v.as_str().is_some_and(formats::is_ip),
            "Ipv4" => v.as_str().is_some_and(formats::is_ipv4),
            "Ipv6" => v.as_str().is_some_and(formats::is_ipv6),
            "MacAddress" => v.as_str().is_some_and(formats::is_mac_address),
            "Json" => match v {
                Value::Null | Value::Array(_) | Value::Object(_) => false,
                other => serde_json::from_str::<Value>(&to_php_string(other)).is_ok(),
            },
            "List" => match v {
                Value::Array(_) => true,
                Value::Object(map) => {
                    !data::is_file(v) && map.keys().enumerate().all(|(i, k)| *k == i.to_string())
                }
                _ => false,
            },
            "Lowercase" => v.as_str().is_some_and(|s| s.to_lowercase() == s),
            "Uppercase" => v.as_str().is_some_and(|s| s.to_uppercase() == s),
            "Max" => {
                req(1)?;
                if self.file_for(v).is_some_and(|f| !f.is_valid()) {
                    false
                } else {
                    self.size_compare(attribute, v, &p[0], "<=", false)
                }
            }
            "Min" => {
                req(1)?;
                self.size_compare(attribute, v, &p[0], ">=", false)
            }
            "Size" => {
                req(1)?;
                self.size_compare(attribute, v, &p[0], "=", false)
            }
            "MaxDigits" | "MinDigits" => {
                req(1)?;
                is_string_or_numeric(v) && {
                    let s = to_php_string(v);
                    let limit = p[0].trim().parse::<usize>().unwrap_or(0);
                    s.bytes().all(|b| b.is_ascii_digit())
                        && if name == "MaxDigits" {
                            s.len() <= limit
                        } else {
                            s.len() >= limit
                        }
                }
            }
            "Mimes" => self.validate_mimes(v, p),
            "Mimetypes" => self.validate_mimetypes(v, p),
            "Missing" => !self.has(attribute),
            "MissingIf" => {
                req(2)?;
                !self.dependent_matches(p) || !self.has(attribute)
            }
            "MissingUnless" => {
                req(2)?;
                self.dependent_matches(p) || !self.has(attribute)
            }
            "MissingWith" => {
                req(1)?;
                !self.has_any(p) || !self.has(attribute)
            }
            "MissingWithAll" => {
                req(1)?;
                !self.has_all(p) || !self.has(attribute)
            }
            "MultipleOf" => {
                req(1)?;
                if !is_numeric(v) || !is_numeric_str(&p[0]) {
                    false
                } else {
                    match (Dec::parse(&to_php_string(v)), Dec::parse(&p[0])) {
                        (Some(numerator), Some(denominator)) => {
                            php::is_multiple_of(&numerator, &denominator)
                        }
                        _ => false,
                    }
                }
            }
            "Numeric" => {
                if p.first().is_some_and(|p| p == "strict") && v.is_string() {
                    false
                } else {
                    is_numeric(v)
                }
            }
            "Present" => self.has(attribute),
            "PresentIf" => {
                req(2)?;
                !self.dependent_matches(p) || self.has(attribute)
            }
            "PresentUnless" => {
                req(2)?;
                self.dependent_matches(p) || self.has(attribute)
            }
            "PresentWith" => {
                req(1)?;
                !self.has_any(p) || self.has(attribute)
            }
            "PresentWithAll" => {
                req(1)?;
                !self.has_all(p) || self.has(attribute)
            }
            "Prohibited" => !self.required(v),
            "ProhibitedIf" => {
                req(2)?;
                !self.dependent_matches(p) || !self.required(v)
            }
            "ProhibitedIfAccepted" => {
                req(1)?;
                !self.accepted(self.other(&p[0])) || !self.required(v)
            }
            "ProhibitedIfDeclined" => {
                req(1)?;
                !self.declined(self.other(&p[0])) || !self.required(v)
            }
            "ProhibitedUnless" => {
                req(2)?;
                self.dependent_matches(p) || !self.required(v)
            }
            "Prohibits" => {
                !self.required(v) || p.iter().all(|other| !self.required(self.other(other)))
            }
            "Regex" | "NotRegex" => {
                if !is_string_or_numeric(v) {
                    false
                } else {
                    req(1)?;
                    let matched = formats::php_regex(&p[0])?.is_match(&to_php_string(v));
                    if name == "Regex" { matched } else { !matched }
                }
            }
            "Required" => self.required(v),
            "RequiredIf" => {
                req(2)?;
                !self.has(&p[0]) || !self.dependent_matches(p) || self.required(v)
            }
            "RequiredIfAccepted" => {
                req(1)?;
                !self.accepted(self.other(&p[0])) || self.required(v)
            }
            "RequiredIfDeclined" => {
                req(1)?;
                !self.declined(self.other(&p[0])) || self.required(v)
            }
            "RequiredUnless" => {
                req(2)?;
                self.dependent_matches(p) || self.required(v)
            }
            "RequiredWith" => self.all_failing_required(p) || self.required(v),
            "RequiredWithAll" => self.any_failing_required(p) || self.required(v),
            "RequiredWithout" => !self.any_failing_required(p) || self.required(v),
            "RequiredWithoutAll" => !self.all_failing_required(p) || self.required(v),
            "RequiredArrayKeys" => {
                data::is_array(v) && p.iter().all(|key| data::array_has_key(v, key))
            }
            "Same" => {
                req(1)?;
                strict_eq(v, self.other(&p[0]))
            }
            "String" => v.is_string(),
            "Timezone" => {
                let group = p.first().map(String::as_str);
                if group.is_some_and(|group| group.trim().eq_ignore_ascii_case("per_country")) {
                    formats::is_timezone_for_country(v.as_str(), p.get(1).map(String::as_str))?
                } else {
                    v.as_str().is_some_and(|s| formats::is_timezone(s, group))
                }
            }
            "Url" => v.as_str().is_some_and(|s| formats::is_url(s, p)),
            "Ulid" => v.as_str().is_some_and(formats::is_ulid),
            "Uuid" => v.as_str().is_some_and(|s| {
                let version = if p.len() == 1 {
                    Some(p[0].as_str())
                } else {
                    None
                };
                formats::is_uuid(s, version)
            }),
            "Exclude" => false,
            "ExcludeIf" => {
                req(2)?;
                !self.has(&p[0]) || !self.dependent_matches(p)
            }
            "ExcludeUnless" => {
                req(2)?;
                self.dependent_matches(p)
            }
            "ExcludeWith" => {
                req(1)?;
                !self.has(&p[0])
            }
            "ExcludeWithout" => {
                req(1)?;
                !self.any_failing_required(p)
            }
            _ => {
                let Some(extension) = self.extensions.rules.get(&rule).cloned() else {
                    return Err(Validator::runtime_error(format!(
                        "Validation rule [{rule}] does not exist. Register it with `Validator::extend(\"{rule}\", ...)`."
                    )));
                };
                let context = ValidationContext::new(self, attribute);
                let passed = extension(&data::unescape(attribute), v, p, &context);
                if let Some(error) = context.take_error() {
                    return Err(error);
                }
                passed
            }
        })
    }
}
