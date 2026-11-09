//! Custom-scheme helpers for the Electron-transfer protocol shim.
//!
//! The JavaScript shim (`packages/cli/src/electron-compat/runtime/protocol.ts`)
//! rewrites in-page `app:`/`file:`/custom-scheme URLs onto the host's
//! `tsgpui://localhost/` space before navigation. This module is the host-side
//! counterpart: it validates scheme names, classifies remote schemes, and maps
//! a custom-scheme URL back to a packaged asset path for `window.load`. Remote
//! schemes (`http:`, `https:`, `ws:`, `wss:`) are never mapped: HTTPS goes
//! through the permission-checked network transport instead.

/// Scheme names the host owns; user schemes must never collide with them.
const HOST_SCHEME: &str = "tsgpui";

/// Remote schemes are not served from packaged assets.
pub fn is_remote_scheme(scheme: &str) -> bool {
    matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https" | "ws" | "wss")
}

/// Whether `scheme` is a syntactically valid custom-scheme name.
pub fn is_valid_scheme(scheme: &str) -> bool {
    if scheme.is_empty() {
        return false;
    }
    let mut chars = scheme.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
}

/// Normalize a scheme for storage/comparison, rejecting invalid or reserved names.
pub fn normalize_scheme(raw: &str) -> Result<String, String> {
    let scheme = raw.trim().to_ascii_lowercase();
    if !is_valid_scheme(&scheme) {
        return Err(format!("invalid protocol scheme: {raw}"));
    }
    if scheme == HOST_SCHEME {
        return Err(format!("protocol scheme '{scheme}' is reserved for the host"));
    }
    if is_remote_scheme(&scheme) {
        return Err(format!(
            "protocol scheme '{scheme}:' is remote; use permissions.network for HTTPS instead"
        ));
    }
    Ok(scheme)
}

/// Extract the scheme from an absolute URL, e.g. `"app://localhost/x"` -> `Some("app")`.
pub fn scheme_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once(':')?;
    if rest.is_empty() || !is_valid_scheme(scheme) {
        return None;
    }
    Some(scheme.to_ascii_lowercase())
}

/// Map a custom-scheme URL to a packaged asset path (`/index.html`).
///
/// `app://localhost/index.html` and `app:///dist/index.html` both map to
/// `/index.html` / `/dist/index.html`. Returns `None` for remote schemes,
/// `tsgpui:` (already host-owned), invalid URLs, or paths that would escape
/// the asset map (`..`, `//`, backslash).
pub fn custom_url_to_path(url: &str) -> Option<String> {
    let scheme = scheme_of(url)?;
    if scheme == HOST_SCHEME || is_remote_scheme(&scheme) {
        return None;
    }
    let after = url.split_once(':')?.1;
    if !after.starts_with("//") {
        return None;
    }
    let after = &after[2..];
    // Drop the authority (`localhost`, `.`, or empty) and keep the path.
    let path = match after.find('/') {
        Some(idx) => &after[idx..],
        None => "/",
    };
    let path = path.split(['?', '#']).next().unwrap_or(path);
    let decoded = percent_decode(path)?;
    if decoded.starts_with("//") || decoded.contains("..") || decoded.contains('\\') {
        return None;
    }
    if decoded.is_empty() {
        return Some("/".to_string());
    }
    Some(if decoded.starts_with('/') { decoded } else { format!("/{decoded}") })
}

/// Whether `scheme` is in the bundle's allowlist (`protocol_schemes`).
pub fn is_allowed(scheme: &str, allowed: &[String]) -> bool {
    let normalized = scheme.to_ascii_lowercase();
    allowed.iter().any(|entry| entry.to_ascii_lowercase() == normalized)
}

fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return None;
            }
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheme_validation() {
        assert!(is_valid_scheme("app"));
        assert!(is_valid_scheme("my-app+1.2"));
        assert!(!is_valid_scheme("1bad"));
        assert!(!is_valid_scheme(""));
        assert!(!is_valid_scheme("bad scheme"));
        assert_eq!(normalize_scheme("App").unwrap(), "app");
        assert!(normalize_scheme("http").is_err());
        assert!(normalize_scheme("tsgpui").is_err());
        assert!(normalize_scheme("bad scheme").is_err());
    }

    #[test]
    fn custom_urls_map_to_packaged_paths() {
        assert_eq!(
            custom_url_to_path("app://localhost/index.html").as_deref(),
            Some("/index.html")
        );
        assert_eq!(
            custom_url_to_path("app:///dist/index.html").as_deref(),
            Some("/dist/index.html")
        );
        assert_eq!(
            custom_url_to_path("file:///./dist/index.html").as_deref(),
            Some("/./dist/index.html")
        );
        assert_eq!(custom_url_to_path("http://example.com/").as_deref(), None);
        assert_eq!(custom_url_to_path("tsgpui://localhost/index.html").as_deref(), None);
        assert_eq!(custom_url_to_path("app://localhost/../secret").as_deref(), None);
        assert_eq!(custom_url_to_path("app:foo").as_deref(), None);
    }

    #[test]
    fn allowlist_is_case_insensitive() {
        let allowed = vec!["app".to_string()];
        assert!(is_allowed("APP", &allowed));
        assert!(!is_allowed("file", &allowed));
    }
}
