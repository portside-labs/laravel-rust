//! Format checks used by the string rules: e-mail addresses, URLs, IP and
//! MAC addresses, UUIDs, timezones, and PHP-style regular expressions.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::{Arc, LazyLock, Mutex};

use illuminate_support::Result;
use illuminate_support::error::InvalidArgumentException;
use regex::Regex;

// ----------------------------------------------------------------------
// E-mail
// ----------------------------------------------------------------------

fn is_atext(c: char, allow_unicode: bool) -> bool {
    c.is_ascii_alphanumeric()
        || "!#$%&'*+/=?^_`{|}~-".contains(c)
        || (allow_unicode && !c.is_ascii() && !c.is_control() && !c.is_whitespace())
}

fn valid_dot_atom(local: &str, allow_unicode: bool) -> bool {
    !local.is_empty()
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..")
        && local
            .chars()
            .all(|c| c == '.' || is_atext(c, allow_unicode))
}

fn valid_quoted_local(local: &str) -> bool {
    if local.len() < 2 || !local.starts_with('"') || !local.ends_with('"') {
        return false;
    }
    let inner = &local[1..local.len() - 1];
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if chars.next().is_none() {
                    return false;
                }
            }
            '"' | '\r' | '\n' => return false,
            _ => {}
        }
    }
    true
}

fn valid_hostname(domain: &str, allow_unicode: bool, require_dot: bool) -> bool {
    if domain.is_empty() || domain.len() > 253 || domain.starts_with('.') || domain.ends_with('.') {
        return false;
    }
    if require_dot && !domain.contains('.') {
        return false;
    }
    domain.split('.').all(|label| {
        !label.is_empty()
            && label.chars().count() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|c| {
                c.is_ascii_alphanumeric()
                    || c == '-'
                    || (allow_unicode && !c.is_ascii() && c.is_alphanumeric())
            })
    })
}

fn valid_domain_literal(domain: &str) -> bool {
    let Some(inner) = domain.strip_prefix('[').and_then(|d| d.strip_suffix(']')) else {
        return false;
    };
    match inner.strip_prefix("IPv6:") {
        Some(v6) => v6.parse::<Ipv6Addr>().is_ok(),
        None => inner.parse::<Ipv4Addr>().is_ok(),
    }
}

/// Validate an e-mail address. `modes` are the `email:` rule parameters:
/// `rfc` (the default), `strict`, `filter`, and `filter_unicode`. The `dns`
/// and `spoof` modes need network / ICU support and are accepted but not
/// enforced.
pub(crate) fn is_email(value: &str, modes: &[String]) -> bool {
    if value.contains(['\r', '\n']) {
        return false;
    }
    let Some(at) = value.rfind('@') else {
        return false;
    };
    let (local, domain) = (&value[..at], &value[at + 1..]);
    if local.is_empty() || domain.is_empty() {
        return false;
    }

    let modes: Vec<&str> = modes
        .iter()
        .map(|m| m.trim())
        .filter(|m| !m.is_empty())
        .collect();
    let modes = if modes.is_empty() { vec!["rfc"] } else { modes };

    modes.iter().all(|mode| match *mode {
        "strict" => {
            local.chars().count() <= 64
                && valid_dot_atom(local, true)
                && valid_hostname(domain, true, false)
        }
        "filter" => {
            value.is_ascii()
                && local.len() <= 64
                && valid_dot_atom(local, false)
                && (valid_hostname(domain, false, true) || valid_domain_literal(domain))
        }
        "filter_unicode" => {
            local.len() <= 64
                && valid_dot_atom(local, true)
                && (valid_hostname(domain, false, true) || valid_domain_literal(domain))
        }
        // "rfc", "dns", "spoof" and anything else fall back to RFC validation.
        _ => {
            (valid_dot_atom(local, true) || valid_quoted_local(local))
                && (valid_hostname(domain, true, false) || valid_domain_literal(domain))
        }
    })
}

// ----------------------------------------------------------------------
// URLs
// ----------------------------------------------------------------------

const PROTOCOLS: &str = "aaa|aaas|about|acap|acct|acd|acr|adiumxtra|adt|afp|afs|aim|amss|android|appdata|apt|ark|attachment|aw|barion|beshare|bitcoin|bitcoincash|blob|bolo|browserext|calculator|callto|cap|cast|casts|chrome|chrome-extension|cid|coap|coap+tcp|coap+ws|coaps|coaps+tcp|coaps+ws|com-eventbrite-attendee|content|conti|crid|cvs|dab|data|dav|diaspora|dict|did|dis|dlna-playcontainer|dlna-playsingle|dns|dntp|dpp|drm|drop|dtn|dvb|ed2k|elsi|example|facetime|fax|feed|feedready|file|filesystem|finger|first-run-pen-experience|fish|fm|ftp|fuchsia-pkg|geo|gg|git|gizmoproject|go|gopher|graph|gtalk|h323|ham|hcap|hcp|http|https|hxxp|hxxps|hydrazone|iax|icap|icon|im|imap|info|iotdisco|ipn|ipp|ipps|irc|irc6|ircs|iris|iris.beep|iris.lwz|iris.xpc|iris.xpcs|isostore|itms|jabber|jar|jms|keyparc|lastfm|ldap|ldaps|leaptofrogans|lorawan|lvlt|magnet|mailserver|mailto|maps|market|message|mid|mms|modem|mongodb|moz|ms-access|ms-browser-extension|ms-calculator|ms-drive-to|ms-enrollment|ms-excel|ms-eyecontrolspeech|ms-gamebarservices|ms-gamingoverlay|ms-getoffice|ms-help|ms-infopath|ms-inputapp|ms-lockscreencomponent-config|ms-media-stream-id|ms-mixedrealitycapture|ms-mobileplans|ms-officeapp|ms-people|ms-project|ms-powerpoint|ms-publisher|ms-restoretabcompanion|ms-screenclip|ms-screensketch|ms-search|ms-search-repair|ms-secondary-screen-controller|ms-secondary-screen-setup|ms-settings|ms-settings-airplanemode|ms-settings-bluetooth|ms-settings-camera|ms-settings-cellular|ms-settings-cloudstorage|ms-settings-connectabledevices|ms-settings-displays-topology|ms-settings-emailandaccounts|ms-settings-language|ms-settings-location|ms-settings-lock|ms-settings-nfctransactions|ms-settings-notifications|ms-settings-power|ms-settings-privacy|ms-settings-proximity|ms-settings-screenrotation|ms-settings-wifi|ms-settings-workplace|ms-spd|ms-sttoverlay|ms-transit-to|ms-useractivityset|ms-virtualtouchpad|ms-visio|ms-walk-to|ms-whiteboard|ms-whiteboard-cmd|ms-word|msnim|msrp|msrps|mss|mtqp|mumble|mupdate|mvn|news|nfs|ni|nih|nntp|notes|ocf|oid|onenote|onenote-cmd|opaquelocktoken|openpgp4fpr|pack|palm|paparazzi|payto|pkcs11|platform|pop|pres|prospero|proxy|pwid|psyc|pttp|qb|query|redis|rediss|reload|res|resource|rmi|rsync|rtmfp|rtmp|rtsp|rtsps|rtspu|s3|secondlife|service|session|sftp|sgn|shttp|sieve|simpleledger|sip|sips|skype|smb|sms|smtp|snews|snmp|soap.beep|soap.beeps|soldat|spiffe|spotify|ssh|steam|stun|stuns|submit|svn|tag|teamspeak|tel|teliaeid|telnet|tftp|tg|things|thismessage|tip|tn3270|tool|ts3server|turn|turns|tv|udp|unreal|urn|ut2004|v-event|vemmi|ventrilo|videotex|vnc|view-source|wais|webcal|wpid|ws|wss|wtai|wyciwyg|xcon|xcon-userid|xfire|xmlrpc.beep|xmlrpc.beeps|xmpp|xri|ymsgr|z39.50|z39.50r|z39.50s";

fn url_pattern(protocols: &str) -> String {
    concat!(
        r"(?i)^(?:{PROTOCOLS})://",
        r"(?:(?:(?:[_.\pL\pN\-]|%[0-9A-Fa-f]{2})+:)?(?:(?:[_.\pL\pN\-]|%[0-9A-Fa-f]{2})+)@)?",
        r"(?:",
        r"(?:(?:(?:[\pL\pN\pS\pM\-_]+\.)+(?:(?:xn--[a-z0-9\-]+)|(?:[\pL\pN\pM]+)))|[a-z0-9\-_]+)\.?",
        r"|\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}",
        r"|\[(?P<ipv6>[0-9a-f:.]+)\]",
        r")",
        r"(?::[0-9]+)?",
        r"(?:/(?:[\pL\pN\-._~!$&'()*+,;=:@]|%[0-9A-Fa-f]{2})*)*",
        r"(?:\?(?:[\pL\pN\-._~!$&'\[\]()*+,;=:@/?]|%[0-9A-Fa-f]{2})*)?",
        r"(?:\#(?:[\pL\pN\-._~!$&'()*+,;=:@/?]|%[0-9A-Fa-f]{2})*)?$",
    )
    .replace("{PROTOCOLS}", protocols)
}

static URL_PATTERNS: LazyLock<Mutex<HashMap<String, Arc<Regex>>>> = LazyLock::new(Default::default);

fn url_regex(protocols: &[String]) -> Arc<Regex> {
    let key = protocols.join("|");
    if let Some(regex) = URL_PATTERNS.lock().unwrap().get(&key) {
        return regex.clone();
    }
    let list = if protocols.is_empty() {
        PROTOCOLS
            .split('|')
            .map(regex::escape)
            .collect::<Vec<_>>()
            .join("|")
    } else {
        protocols
            .iter()
            .map(|p| regex::escape(p.trim()))
            .collect::<Vec<_>>()
            .join("|")
    };
    let regex = Arc::new(Regex::new(&url_pattern(&list)).expect("the URL pattern is valid"));
    URL_PATTERNS.lock().unwrap().insert(key, regex.clone());
    regex
}

/// Laravel's `Str::isUrl($value, $protocols)`.
pub(crate) fn is_url(value: &str, protocols: &[String]) -> bool {
    let regex = url_regex(protocols);
    match regex.captures(value) {
        Some(captures) => match captures.name("ipv6") {
            Some(ip) => ip.as_str().parse::<Ipv6Addr>().is_ok(),
            None => true,
        },
        None => false,
    }
}

/// The host portion of a URL, as PHP's `parse_url($value, PHP_URL_HOST)`.
pub(crate) fn url_host(value: &str) -> Option<String> {
    let (_, rest) = value.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority.rsplit('@').next().unwrap_or_default();
    let host = if host.starts_with('[') {
        host.split(']')
            .next()
            .map(|h| format!("{h}]"))
            .unwrap_or_default()
    } else {
        host.split(':').next().unwrap_or_default().to_string()
    };
    (!host.is_empty()).then_some(host)
}

// ----------------------------------------------------------------------
// IP & MAC addresses, colors, identifiers
// ----------------------------------------------------------------------

pub(crate) fn is_ip(value: &str) -> bool {
    value.parse::<IpAddr>().is_ok()
}

pub(crate) fn is_ipv4(value: &str) -> bool {
    value.parse::<Ipv4Addr>().is_ok()
}

pub(crate) fn is_ipv6(value: &str) -> bool {
    value.parse::<Ipv6Addr>().is_ok()
}

/// PHP's `FILTER_VALIDATE_MAC`.
pub(crate) fn is_mac_address(value: &str) -> bool {
    static MAC: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(?:[0-9A-Fa-f]{2}(?:[:-][0-9A-Fa-f]{2}){5}|[0-9A-Fa-f]{4}\.[0-9A-Fa-f]{4}\.[0-9A-Fa-f]{4})$").unwrap()
    });
    if !MAC.is_match(value) {
        return false;
    }
    // A single separator style must be used throughout.
    !(value.contains(':') && value.contains('-'))
}

pub(crate) fn is_hex_color(value: &str) -> bool {
    static HEX: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)^#(?:(?:[0-9a-f]{3}){1,2}|(?:[0-9a-f]{4}){1,2})$").unwrap()
    });
    HEX.is_match(value)
}

/// Laravel's `Str::isUuid($value, $version)`.
pub(crate) fn is_uuid(value: &str, version: Option<&str>) -> bool {
    static UUID: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^[\da-fA-F]{8}-[\da-fA-F]{4}-[\da-fA-F]{4}-[\da-fA-F]{4}-[\da-fA-F]{12}$")
            .unwrap()
    });
    let Some(version) = version else {
        return UUID.is_match(value);
    };
    let Ok(uuid) = uuid::Uuid::parse_str(value) else {
        return false;
    };
    match version.trim() {
        "max" => uuid.is_max(),
        "nil" | "0" => uuid.is_nil(),
        v => v
            .parse::<usize>()
            .is_ok_and(|v| uuid.get_version_num() == v && !uuid.is_nil() && !uuid.is_max()),
    }
}

pub(crate) fn is_ulid(value: &str) -> bool {
    value.len() == 26
        && value.bytes().all(|b| b.is_ascii_alphanumeric())
        && value.as_bytes()[0] <= b'7'
        && ulid::Ulid::from_string(value).is_ok()
}

// IANA tzdb 2025b zone.tab, the single-country table used by PHP.
static COUNTRY_TIMEZONES: LazyLock<HashSet<(&str, &str)>> = LazyLock::new(|| {
    include_str!("../data/zone.tab")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let mut columns = line.split('\t');
            let country = columns.next()?;
            let _coordinates = columns.next()?;
            let timezone = columns.next()?;
            Some((country, timezone))
        })
        .collect()
});

/// Validate a timezone using the identifiers assigned to the requested country.
pub(crate) fn is_timezone_for_country(value: Option<&str>, country: Option<&str>) -> Result<bool> {
    let country = country
        .map(str::to_uppercase)
        .filter(|country| country.len() == 2)
        .ok_or_else(|| {
            InvalidArgumentException::new(
                "timezone_identifiers_list(): Argument #2 ($countryCode) must be a \
                 two-letter ISO 3166-1 compatible country code when argument #1 \
                 ($timezoneGroup) is DateTimeZone::PER_COUNTRY",
            )
        })?;

    Ok(value.is_some_and(|value| COUNTRY_TIMEZONES.contains(&(country.as_str(), value))))
}

/// Match a timezone identifier in a built-in region.
pub(crate) fn is_timezone(value: &str, group: Option<&str>) -> bool {
    let Ok(tz) = value.parse::<chrono_tz::Tz>() else {
        return false;
    };
    let name = tz.name();
    if name != value {
        return false;
    }
    const REGIONS: [&str; 10] = [
        "Africa/",
        "America/",
        "Antarctica/",
        "Arctic/",
        "Asia/",
        "Atlantic/",
        "Australia/",
        "Europe/",
        "Indian/",
        "Pacific/",
    ];
    let canonical = name == "UTC" || REGIONS.iter().any(|r| name.starts_with(r));
    match group.map(|g| g.trim().to_ascii_uppercase()) {
        None => canonical,
        Some(group) => match group.as_str() {
            "ALL" | "" => canonical,
            "ALL_WITH_BC" => true,
            "UTC" => name == "UTC",
            region => {
                let prefix = format!("{}/", region.to_ascii_lowercase());
                name.to_ascii_lowercase().starts_with(&prefix)
            }
        },
    }
}

// ----------------------------------------------------------------------
// PHP regular expressions
// ----------------------------------------------------------------------

static REGEX_CACHE: LazyLock<Mutex<HashMap<String, Arc<Regex>>>> = LazyLock::new(Default::default);

/// Thrown when a `regex` / `not_regex` pattern can't be used.
#[derive(Debug, Clone, thiserror::Error)]
#[error(
    "The regular expression [{pattern}] is invalid or uses features Rust's regex engine doesn't support: {reason}"
)]
pub struct InvalidPatternException {
    /// The pattern as written in the rule.
    pub pattern: String,
    /// Why it couldn't be compiled.
    pub reason: String,
}

/// Translate a PHP (PCRE) pattern with delimiters and flags — `/^[a-z]+$/i`
/// — into a Rust regular expression.
pub(crate) fn php_regex(pattern: &str) -> Result<Arc<Regex>, InvalidPatternException> {
    if let Some(regex) = REGEX_CACHE.lock().unwrap().get(pattern) {
        return Ok(regex.clone());
    }
    let fail = |reason: &str| InvalidPatternException {
        pattern: pattern.to_string(),
        reason: reason.to_string(),
    };
    let trimmed = pattern.trim_start();
    let mut chars = trimmed.chars();
    let open = chars.next().ok_or_else(|| fail("empty pattern"))?;
    if open.is_alphanumeric() || open == '\\' || open.is_whitespace() {
        return Err(fail("delimiter must not be alphanumeric or backslash"));
    }
    let close = match open {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        '<' => '>',
        other => other,
    };
    let body = &trimmed[open.len_utf8()..];
    let end = body
        .rfind(close)
        .ok_or_else(|| fail("no ending delimiter"))?;
    let (inner, flags) = (&body[..end], &body[end + close.len_utf8()..]);

    let mut prefix = String::new();
    let mut anchored = false;
    for flag in flags.trim_end().chars() {
        match flag {
            'i' => prefix.push('i'),
            'm' => prefix.push('m'),
            's' => prefix.push('s'),
            'x' => prefix.push('x'),
            'U' => prefix.push('U'),
            'A' => anchored = true,
            'u' | 'D' | 'S' | 'X' | 'J' | 'n' => {}
            other => return Err(fail(&format!("unknown modifier '{other}'"))),
        }
    }

    // Un-escape the delimiter and any punctuation Rust doesn't allow escaping.
    let mut translated = String::with_capacity(inner.len());
    let mut iter = inner.chars().peekable();
    while let Some(c) = iter.next() {
        if c == '\\' {
            match iter.next() {
                Some(next) if (next == open || next == close) && !is_regex_meta(next) => {
                    translated.push(next)
                }
                Some(next) if next.is_ascii_punctuation() && !is_regex_meta(next) => {
                    translated.push(next)
                }
                Some(next) => {
                    translated.push('\\');
                    translated.push(next);
                }
                None => translated.push('\\'),
            }
        } else {
            translated.push(c);
        }
    }

    let mut source = String::new();
    if !prefix.is_empty() {
        source.push_str(&format!("(?{prefix})"));
    }
    if anchored {
        source.push_str(r"\A(?:");
        source.push_str(&translated);
        source.push(')');
    } else {
        source.push_str(&translated);
    }
    let regex = Arc::new(Regex::new(&source).map_err(|e| fail(&e.to_string()))?);
    REGEX_CACHE
        .lock()
        .unwrap()
        .insert(pattern.to_string(), regex.clone());
    Ok(regex)
}

fn is_regex_meta(c: char) -> bool {
    matches!(
        c,
        '\\' | '.'
            | '+'
            | '*'
            | '?'
            | '('
            | ')'
            | '|'
            | '['
            | ']'
            | '{'
            | '}'
            | '^'
            | '$'
            | '#'
            | '&'
            | '-'
            | '~'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_validates_emails() {
        let none: Vec<String> = vec![];
        for ok in [
            "taylor@laravel.com",
            "user+tag@example.co.uk",
            "user@localhost",
            "\"quoted name\"@example.com",
            "user@[127.0.0.1]",
            "üñîçøðé@example.com",
        ] {
            assert!(is_email(ok, &none), "{ok}");
        }
        for bad in [
            "plainaddress",
            "@example.com",
            "user@",
            "user@@example.com",
            ".user@example.com",
            "user.@example.com",
            "us..er@example.com",
            "user@-example.com",
            "user@example..com",
            "user name@example.com",
            "user@exam ple.com",
        ] {
            assert!(!is_email(bad, &none), "{bad}");
        }
        let filter = vec!["filter".to_string()];
        assert!(is_email("taylor@laravel.com", &filter));
        assert!(!is_email("user@localhost", &filter));
        assert!(!is_email("üser@example.com", &filter));
    }

    #[test]
    fn it_validates_urls() {
        let none: Vec<String> = vec![];
        for ok in [
            "https://laravel.com",
            "http://localhost:8000/path?query=1#frag",
            "http://user:pass@example.com",
            "http://192.168.1.1",
            "http://[::1]:80/",
            "ftp://ftp.example.com/file.txt",
            "https://例子.测试",
        ] {
            assert!(is_url(ok, &none), "{ok}");
        }
        for bad in [
            "laravel.com",
            "http://",
            "https://exa mple.com",
            "javascript:alert(1)",
            "http://[zz::1]",
        ] {
            assert!(!is_url(bad, &none), "{bad}");
        }
        let only = vec!["https".to_string()];
        assert!(is_url("https://laravel.com", &only));
        assert!(!is_url("http://laravel.com", &only));
        assert_eq!(
            url_host("https://user@laravel.com:8080/docs").as_deref(),
            Some("laravel.com")
        );
    }

    #[test]
    fn it_validates_identifiers() {
        assert!(is_mac_address("01:23:45:67:89:ab"));
        assert!(is_mac_address("01-23-45-67-89-AB"));
        assert!(is_mac_address("0123.4567.89ab"));
        assert!(!is_mac_address("01:23:45:67:89"));
        assert!(is_hex_color("#fff"));
        assert!(is_hex_color("#FFFFFF80"));
        assert!(!is_hex_color("fff"));
        assert!(is_uuid("a0a2a2d2-0b87-4a18-83f2-2529882be2de", None));
        assert!(is_uuid("a0a2a2d2-0b87-4a18-83f2-2529882be2de", Some("4")));
        assert!(!is_uuid("a0a2a2d2-0b87-4a18-83f2-2529882be2de", Some("7")));
        assert!(is_uuid("ffffffff-ffff-ffff-ffff-ffffffffffff", Some("max")));
        assert!(is_ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        assert!(!is_ulid("81ARZ3NDEKTSV4RRFFQ69G5FAV"));
        assert!(is_timezone("Europe/Amsterdam", None));
        assert!(is_timezone("UTC", None));
        assert!(!is_timezone("europe/amsterdam", None));
        assert!(!is_timezone("US/Eastern", None));
        assert!(is_timezone("US/Eastern", Some("all_with_bc")));
        assert!(is_timezone("Africa/Lagos", Some("Africa")));
        assert!(!is_timezone("Europe/Paris", Some("Africa")));
    }

    #[test]
    fn it_translates_php_patterns() {
        assert!(php_regex("/^[a-z]+$/i").unwrap().is_match("ABC"));
        assert!(php_regex("#^\\#\\d+$#").unwrap().is_match("#12"));
        assert!(php_regex("/^https?:\\/\\//").unwrap().is_match("https://"));
        assert!(php_regex("{^a.c$}s").unwrap().is_match("a\nc"));
        assert!(php_regex("/^(?=a)/").is_err());
        assert!(php_regex("abc").is_err());
    }
}
