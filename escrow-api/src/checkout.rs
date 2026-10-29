//! What a business may attach to an offer so its customer can be sent to checkout and back: its own
//! order number, and a link back. Both are optional, and both are checked here because the link is
//! shown to a stranger (the buyer) as coming from the seller.

use crate::error::ApiError;

pub const MAX_REFERENCE_CHARS: usize = 100;
pub const MAX_RETURN_URL_CHARS: usize = 500;

/// A business's order number: 1 to 100 characters, no control characters. Empty means none.
pub fn parse_reference(raw: Option<&str>) -> Result<Option<String>, ApiError> {
    let Some(raw) = raw.map(str::trim).filter(|r| !r.is_empty()) else { return Ok(None) };
    if raw.chars().count() > MAX_REFERENCE_CHARS || raw.chars().any(char::is_control) {
        return Err(ApiError::bad_request(
            "invalid_reference",
            format!("reference must be at most {MAX_REFERENCE_CHARS} characters, with no control characters"),
        ));
    }
    Ok(Some(raw.to_string()))
}

/// A link back to the business: `https://`, with a plain host and no login in it. `http://` is
/// accepted only for a machine's own address, so a business can try it out locally. Empty means none.
pub fn parse_return_url(raw: Option<&str>) -> Result<Option<String>, ApiError> {
    let Some(raw) = raw.map(str::trim).filter(|r| !r.is_empty()) else { return Ok(None) };
    let bad = |why: &str| ApiError::bad_request("invalid_return_url", format!("return_url {why}"));
    if raw.chars().count() > MAX_RETURN_URL_CHARS {
        return Err(bad(&format!("must be at most {MAX_RETURN_URL_CHARS} characters")));
    }
    if raw.chars().any(|c| c.is_control() || c.is_whitespace() || c == '\\') {
        return Err(bad("must not contain spaces, control characters or backslashes"));
    }
    let (secure, rest) = match (raw.strip_prefix("https://"), raw.strip_prefix("http://")) {
        (Some(rest), _) => (true, rest),
        (None, Some(rest)) => (false, rest),
        _ => return Err(bad("must start with https://")),
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() {
        return Err(bad("must name a website"));
    }
    if authority.contains('@') {
        return Err(bad("must not contain a login (user@)"));
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) if !h.ends_with(']') || h.starts_with('[') => (h, Some(p)),
        _ => (authority, None),
    };
    if let Some(p) = port {
        if p.is_empty() || p.len() > 5 || !p.bytes().all(|b| b.is_ascii_digit()) {
            return Err(bad("has an invalid port"));
        }
    }
    let local = matches!(host, "localhost" | "127.0.0.1" | "[::1]");
    if !secure && !local {
        return Err(bad("must start with https:// (http:// is allowed only for localhost)"));
    }
    let plain_host = host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    if !local && (!plain_host || host.starts_with(['.', '-']) || host.ends_with(['.', '-']) || host.contains("..")) {
        return Err(bad("has an invalid website name"));
    }
    Ok(Some(raw.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_are_trimmed_and_limited() {
        assert_eq!(parse_reference(None).unwrap(), None);
        assert_eq!(parse_reference(Some("   ")).unwrap(), None);
        assert_eq!(parse_reference(Some("  order #1001 ")).unwrap().as_deref(), Some("order #1001"));
        assert!(parse_reference(Some(&"x".repeat(100))).is_ok());
        assert!(parse_reference(Some(&"x".repeat(101))).is_err());
        assert!(parse_reference(Some("a\nb")).is_err());
    }

    #[test]
    fn ordinary_https_links_are_accepted() {
        for ok in [
            "https://shop.example.com",
            "https://shop.example.com/orders/1001?done=1#top",
            "https://shop.example.com:8443/thanks",
            "http://localhost:3000/thanks",
            "http://127.0.0.1/thanks",
            "http://[::1]:3000/thanks",
        ] {
            assert_eq!(parse_return_url(Some(ok)).unwrap().as_deref(), Some(ok), "{ok}");
        }
        assert_eq!(parse_return_url(Some("  ")).unwrap(), None);
    }

    #[test]
    fn anything_that_could_disguise_where_it_goes_is_refused() {
        for bad in [
            "http://shop.example.com",
            "ftp://shop.example.com",
            "javascript:alert(1)",
            "data:text/html,hi",
            "//shop.example.com",
            "shop.example.com",
            "https://",
            "https:///path",
            "https://trusted.com@evil.com/",
            "https://trusted.com:pass@evil.com/",
            "https://shop.example.com\\@evil.com",
            "https://shop .example.com",
            "https://shop.example.com:99999x/",
            "https://exa_mple.com",
            "https://-bad.example.com",
            "https://bad..example.com",
            "https://ex\u{e4}mple.com",
        ] {
            assert!(parse_return_url(Some(bad)).is_err(), "{bad} should be refused");
        }
        assert!(parse_return_url(Some(&format!("https://a.com/{}", "x".repeat(500)))).is_err());
    }
}
