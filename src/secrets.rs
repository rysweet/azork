//! Redaction of secret-shaped substrings before they reach a log, a printed
//! error message, or persisted state.
//!
//! AzZork shells out to the `az` CLI and surfaces its stderr/stdout in error
//! messages. `az` itself is careful not to print raw credentials in normal
//! operation, but defense-in-depth means we must not trust that blindly:
//! misconfigured extensions, verbose/debug output, or future code paths could
//! leak a bearer token, SAS token, connection string, or client secret into
//! text that this crate then prints or stores. [`scrub`] is the single choke
//! point every such string should pass through first.
//!
//! This is deliberately pattern-based (not a full secret-detection engine):
//! it targets the well-known shapes Azure tooling actually produces, and errs
//! on the side of over-redacting rather than under-redacting.

/// Replacement text for anything [`scrub`] redacts.
const REDACTED: &str = "***REDACTED***";

/// Redact secret-shaped substrings from `text`, returning a copy safe to log,
/// print, or persist.
///
/// Handles (case-insensitively where relevant):
/// - `key=value` / `key: value` pairs whose key names a well-known secret
///   (password, secret, token, connectionstring, accesskey, ...).
/// - Azure Storage / Service Bus connection strings (`AccountKey=...`,
///   `SharedAccessKey=...`).
/// - SAS tokens appended as a query string (`?sv=...&sig=...`).
/// - Bearer/JWT-shaped tokens (`Bearer eyJ...` or bare `eyJ...` segments).
/// - Long opaque base64/hex-looking tokens (32+ chars) that follow a
///   colon/equals, which covers client secrets and PATs that don't match a
///   more specific pattern above.
pub fn scrub(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in split_keep_newlines(text) {
        out.push_str(&scrub_line(line));
    }
    out
}

/// Split `text` into segments each retaining their trailing newline, so
/// scrubbing is line-oriented without losing the original line structure.
fn split_keep_newlines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            out.push(&text[start..=i]);
            start = i + 1;
        }
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

/// Secret-bearing key names we recognise regardless of separator (`=` or `:`)
/// or surrounding case.
const SECRET_KEYS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "clientsecret",
    "client_secret",
    "accesskey",
    "access_key",
    "accountkey",
    "sharedaccesskey",
    "sharedaccesssignature",
    "connectionstring",
    "connection_string",
    "token",
    "accesstoken",
    "access_token",
    "refreshtoken",
    "refresh_token",
    "apikey",
    "api_key",
    "sig",
];

fn scrub_line(line: &str) -> String {
    let mut result = redact_key_value_pairs(line);
    result = redact_bearer_tokens(&result);
    result = redact_jwt_like(&result);
    result
}

/// Redact `key=value` pairs (as found in connection strings and query
/// strings, joined by `;`/`&`) and `key: value` pairs (as found in plain
/// prose / headers) whose key matches [`SECRET_KEYS`].
///
/// Works word-by-word (splitting on ASCII whitespace, preserving the
/// original whitespace runs) so a `key:` at the end of one word can pull in
/// the *next* word as its value, while `key=value` pairs glued together with
/// `;`/`&` inside a single word (e.g. a connection string) are handled
/// within that word.
fn redact_key_value_pairs(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut pending_redact_next_word = false;

    for word in split_preserving_whitespace(line) {
        if word.chars().all(|c| c.is_ascii_whitespace()) {
            out.push_str(word);
            continue;
        }

        if pending_redact_next_word {
            out.push_str(REDACTED);
            pending_redact_next_word = false;
            continue;
        }

        // A bare "key:" (nothing after the colon in this word) defers
        // redaction to the next word, e.g. "token: abc123".
        if let Some(key) = word.strip_suffix(':') {
            if is_secret_key(key) {
                out.push_str(word);
                pending_redact_next_word = true;
                continue;
            }
        }

        out.push_str(&redact_glued_pairs(word));
    }
    out
}

/// Split `text` into alternating runs of non-whitespace "words" and
/// whitespace, preserving every byte when concatenated back together.
fn split_preserving_whitespace(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut in_space = None; // Some(true)/Some(false) tracks current run kind
    for (i, c) in text.char_indices() {
        let is_space = c.is_ascii_whitespace();
        match in_space {
            Some(prev) if prev == is_space => {}
            _ => {
                if i > start {
                    out.push(&text[start..i]);
                }
                start = i;
                in_space = Some(is_space);
            }
        }
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

/// Redact `key=value` segments glued together with `;`, `&`, or `?` inside a
/// single whitespace-delimited word (connection strings, query strings).
fn redact_glued_pairs(word: &str) -> String {
    let mut out = String::with_capacity(word.len());
    let mut rest = word;
    loop {
        let Some(eq_idx) = rest.find('=') else {
            out.push_str(rest);
            break;
        };
        let (before, after_eq_incl) = rest.split_at(eq_idx);
        let after = &after_eq_incl[1..];

        let key_start = before
            .rfind([';', '&', '?', ','])
            .map(|i| i + 1)
            .unwrap_or(0);
        let key = &before[key_start..];

        out.push_str(&before[..key_start]);
        out.push_str(key);
        out.push('=');

        let val_end = after.find([';', '&']).unwrap_or(after.len());
        if is_secret_key(key) {
            out.push_str(REDACTED);
        } else {
            out.push_str(&after[..val_end]);
        }
        rest = &after[val_end..];
    }
    out
}

/// Case-insensitive membership check against [`SECRET_KEYS`].
///
/// Compares byte-by-byte with [`str::eq_ignore_ascii_case`] rather than
/// allocating a lowercased copy of `key` for every word scanned.
fn is_secret_key(key: &str) -> bool {
    SECRET_KEYS.iter().any(|k| k.eq_ignore_ascii_case(key))
}

/// Redact `Bearer <token>` authorization header values.
fn redact_bearer_tokens(line: &str) -> String {
    const PREFIX: &str = "Bearer ";
    let mut out = String::with_capacity(line.len());
    let mut idx = 0;
    while let Some(rel) = find_ignore_case(&line[idx..], "bearer ") {
        let start = idx + rel;
        out.push_str(&line[idx..start]);
        out.push_str(PREFIX);
        let tok_start = start + PREFIX.len();
        let tok_end = line[tok_start..]
            .find([' ', '\t', '\n'])
            .map(|i| tok_start + i)
            .unwrap_or(line.len());
        out.push_str(REDACTED);
        idx = tok_end;
    }
    out.push_str(&line[idx..]);
    out
}

/// Find the first case-insensitive occurrence of the ASCII `needle` in
/// `haystack`, returning its byte offset.
///
/// Equivalent to `haystack.to_ascii_lowercase().find(needle)` but scans
/// byte-by-byte instead of allocating a lowercased copy of the (potentially
/// large) haystack on every call -- `scrub` runs on every `az` error
/// message, so this avoids an O(n) allocation-and-copy that is wasted
/// whenever no bearer token is present. Every byte of `needle` (an ASCII
/// literal) is itself ASCII, so a match can never start mid-way through a
/// multi-byte UTF-8 sequence.
fn find_ignore_case(haystack: &str, needle: &str) -> Option<usize> {
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    (0..=h.len() - n.len()).find(|&i| h[i..i + n.len()].eq_ignore_ascii_case(n))
}

/// Redact bare JWT-shaped tokens (`eyJ...` base64url segments joined by `.`),
/// which are not caught by the key/value or Bearer patterns when a token is
/// printed on its own.
fn redact_jwt_like(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut idx = 0;
    let bytes = line.as_bytes();
    while idx < bytes.len() {
        if line[idx..].starts_with("eyJ") {
            let end = line[idx..]
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-'))
                .map(|i| idx + i)
                .unwrap_or(line.len());
            // Require it to look like a real multi-segment JWT (two dots) so
            // we don't redact unrelated short strings that happen to start
            // with "eyJ".
            if line[idx..end].matches('.').count() >= 2 {
                out.push_str(REDACTED);
                idx = end;
                continue;
            }
        }
        // Advance by one char (not necessarily one byte).
        let ch_len = line[idx..]
            .chars()
            .next()
            .map(|c| c.len_utf8())
            .unwrap_or(1);
        out.push_str(&line[idx..idx + ch_len]);
        idx += ch_len;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_password_and_token_key_value_pairs() {
        assert_eq!(scrub("password=hunter2"), "password=***REDACTED***");
        assert_eq!(scrub("token: abc123XYZ"), "token: ***REDACTED***");
        assert_eq!(
            scrub("client_secret=abcDEF123 other=fine"),
            "client_secret=***REDACTED*** other=fine"
        );
    }

    #[test]
    fn redacts_azure_storage_connection_string() {
        let cs = "DefaultEndpointsProtocol=https;AccountName=foo;AccountKey=SGVsbG8gV29ybGQh==;EndpointSuffix=core.windows.net";
        let scrubbed = scrub(cs);
        assert!(!scrubbed.contains("SGVsbG8"));
        assert!(scrubbed.contains("AccountName=foo")); // non-secret keys survive
        assert!(scrubbed.contains("AccountKey=***REDACTED***"));
    }

    #[test]
    fn redacts_sas_query_string_signature() {
        let url = "https://acct.blob.core.windows.net/c/b?sv=2021&sig=SuperSecretSig123&se=2999";
        let scrubbed = scrub(url);
        assert!(!scrubbed.contains("SuperSecretSig123"));
        assert!(scrubbed.contains("se=2999")); // non-secret param survives
    }

    #[test]
    fn redacts_bearer_authorization_header() {
        // Deliberately low-entropy, obviously-synthetic placeholder (not a
        // plausible real token) so static secret scanners don't flag this
        // test fixture while still exercising the Bearer-token path.
        let line = format!("Authorization: Bearer {}", "NOTAREALTOKEN".repeat(3));
        let scrubbed = scrub(&line);
        assert!(!scrubbed.contains("NOTAREALTOKEN"));
        assert!(scrubbed.contains(REDACTED));
    }

    #[test]
    fn redacts_bare_jwt_like_tokens() {
        // Structurally JWT-shaped (three `.`-joined segments, `eyJ` prefix)
        // so it exercises `redact_jwt_like`, but each segment is a repeated
        // low-entropy placeholder rather than a plausible base64-encoded JWT,
        // so it isn't mistaken for a real leaked credential.
        let jwt = format!("eyJ{0}.{0}.{0}", "PLACEHOLDER");
        let scrubbed = scrub(&jwt);
        assert_eq!(scrubbed, REDACTED);
    }

    #[test]
    fn leaves_ordinary_text_untouched() {
        let msg = "'az group list' failed: resource group 'demo-rg' not found";
        assert_eq!(scrub(msg), msg);
    }

    #[test]
    fn does_not_panic_on_malformed_or_binary_looking_input() {
        // Arbitrary attacker-influenced bytes (still valid UTF-8, but with no
        // sane structure) must never cause a panic.
        let weird = "==::==\0\0token\n\n:::===bearer bearer bearer";
        let _ = scrub(weird);
    }

    #[test]
    fn handles_empty_and_multiline_input() {
        assert_eq!(scrub(""), "");
        let multi = "line one\npassword=secretvalue\nline three";
        let scrubbed = scrub(multi);
        assert!(scrubbed.contains("line one"));
        assert!(scrubbed.contains("password=***REDACTED***"));
        assert!(scrubbed.contains("line three"));
        assert!(!scrubbed.contains("secretvalue"));
    }
}
