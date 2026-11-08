/// Sanitize a directory or file name component so it is safe to use in a
/// filesystem path.
///
/// The returned string:
/// - contains no path separators (`/`, `\\`)
/// - contains no traversal sequences (`..`)
/// - is never empty (fallback is `_`)
/// - is deterministic for any given input
///
/// Safe names (alphanumeric, spaces, hyphens, underscores, dots) are
/// preserved exactly.  Path separators become `_`.  Traversal `..` becomes
/// `__` and any immediately following separator is skipped to avoid
/// producing `___`.
pub fn sanitize_name(raw: &str) -> String {
    if raw.is_empty() {
        return "_".to_string();
    }

    // Strip leading separators that would turn into absolute paths.
    let trimmed = raw.trim_start_matches('/').trim_start_matches('\\');

    if trimmed.is_empty() {
        return "_".to_string();
    }

    let chars: Vec<char> = trimmed.chars().collect();
    let mut result = String::with_capacity(trimmed.len());
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            '/' | '\\' => {
                result.push('_');
                i += 1;
            }
            '.' if i + 1 < chars.len() && chars[i + 1] == '.' => {
                // Traversal sequence `..` -> `__`.
                result.push('_');
                result.push('_');
                // Skip the following separator if present to avoid `___`.
                if i + 2 < chars.len() && (chars[i + 2] == '/' || chars[i + 2] == '\\') {
                    i += 3; // skip `..` and the following separator
                } else {
                    i += 2;
                }
            }
            c => {
                result.push(c);
                i += 1;
            }
        }
    }

    if result.is_empty() {
        "_".to_string()
    } else {
        result
    }
}

/// Extract a safe filename from an attachment URL.
///
/// Takes the last path segment of the URL, decodes URL-encoded characters,
/// and sanitizes the result.  A `#name` fragment (added for rCTF, whose
/// file URLs don't end in the file name) names the file instead.
pub fn attachment_filename(url: &str) -> String {
    if let Some((_, name)) = url.split_once('#')
        && !name.is_empty()
        && !name.contains('/')
    {
        let decoded = urlencoding::decode(name).unwrap_or_else(|_| name.into());
        return sanitize_name(&decoded);
    }
    // Strip query string and fragment.
    let path_part = url.split('?').next().unwrap_or(url);
    let path_part = path_part.split('#').next().unwrap_or(path_part);

    // Extract the last path segment.
    let filename = path_part
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("attachment");

    // URL-decode if needed (simple percent-decoding).
    let decoded = urlencoding::decode(filename).unwrap_or_else(|_| filename.into());

    sanitize_name(&decoded)
}

/// Build a safe relative output path from category and challenge names.
///
/// Returns `"safe_category/safe_challenge"` — both components are
/// sanitized and non-empty.
pub fn safe_challenge_path(category: &str, name: &str) -> String {
    let safe_cat = sanitize_name(category);
    let safe_name = sanitize_name(name);
    format!("{safe_cat}/{safe_name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- sanitize_name ---

    #[test]
    fn preserves_safe_name() {
        assert_eq!(sanitize_name("Web"), "Web");
        assert_eq!(sanitize_name("Baby Login"), "Baby Login");
        assert_eq!(sanitize_name("crypto-101"), "crypto-101");
        assert_eq!(sanitize_name("hello_world.txt"), "hello_world.txt");
    }

    #[test]
    fn replaces_slash_with_underscore() {
        assert_eq!(sanitize_name("web/login"), "web_login");
        assert_eq!(sanitize_name("a/b/c"), "a_b_c");
    }

    #[test]
    fn replaces_backslash_with_underscore() {
        assert_eq!(sanitize_name("web\\login"), "web_login");
    }

    #[test]
    fn replaces_traversal_dots() {
        assert_eq!(sanitize_name(".."), "__");
        assert_eq!(sanitize_name("../pwn"), "__pwn");
        // web/../../escape: web + _ + ..(skip /) + ..(skip /) + escape
        // = "web" + "_" + "__" + "__" + "escape" = "web_____escape"
        assert_eq!(sanitize_name("web/../../escape"), "web_____escape");
        assert_eq!(sanitize_name("../pwn/.."), "__pwn___");
    }

    #[test]
    fn strips_leading_separators() {
        assert_eq!(sanitize_name("/tmp/x"), "tmp_x");
        assert_eq!(sanitize_name("///absolute"), "absolute");
        assert_eq!(sanitize_name("\\windows\\path"), "windows_path");
    }

    #[test]
    fn empty_becomes_underscore() {
        assert_eq!(sanitize_name(""), "_");
        assert_eq!(sanitize_name("///"), "_"); // after trim, empty
    }

    #[test]
    fn deterministic_output() {
        // Same input should always produce same output.
        let a1 = sanitize_name("../pwn");
        let a2 = sanitize_name("../pwn");
        assert_eq!(a1, a2);
    }

    #[test]
    fn non_empty_for_any_input() {
        // Never returns an empty string.
        assert!(!sanitize_name("").is_empty());
        assert!(!sanitize_name("///").is_empty());
        assert!(!sanitize_name("..").is_empty());
        assert!(!sanitize_name("...").is_empty());
    }

    // --- attachment_filename ---

    #[test]
    fn extracts_filename_from_relative_url() {
        assert_eq!(
            attachment_filename("/files/abc123/handout.zip"),
            "handout.zip"
        );
    }

    #[test]
    fn extracts_filename_from_absolute_url() {
        assert_eq!(
            attachment_filename("https://cdn.example.com/path/to/file.pdf"),
            "file.pdf"
        );
    }

    #[test]
    fn sanitizes_traversal_in_attachment_name() {
        assert_eq!(
            attachment_filename("/files/abc/../secret.txt"),
            "secret.txt" // after sanitize: "abc___secret.txt"? wait, let me re-check.
                         // rsplit('/').next() gives ".." because ".." is after "/"
                         // Actually: rsplit('/'): ["..", "secret.txt", "abc", "files", ""]
                         // Wait, the URL is "/files/abc/../secret.txt"
                         // Split by '/': ["", "files", "abc", "..", "secret.txt"]
                         // rsplit by '/': ["secret.txt", "..", "abc", "files", ""]
                         // .next() = "secret.txt" — correct!
        );
    }

    #[test]
    fn handles_url_with_query_string() {
        assert_eq!(
            attachment_filename("/files/hash/doc.pdf?token=123"),
            "doc.pdf"
        );
    }

    #[test]
    fn handles_empty_url() {
        assert_eq!(attachment_filename(""), "attachment");
    }

    #[test]
    fn handles_url_ending_with_slash() {
        assert_eq!(attachment_filename("/files/abc123/"), "attachment");
    }

    #[test]
    fn handles_url_encoded_filename() {
        assert_eq!(
            attachment_filename("/files/abc/my%20file.txt"),
            "my file.txt"
        );
    }

    // --- safe_challenge_path ---

    #[test]
    fn builds_safe_path_from_safe_names() {
        assert_eq!(safe_challenge_path("Web", "Baby Login"), "Web/Baby Login");
    }

    #[test]
    fn sanitizes_unsafe_category() {
        assert_eq!(safe_challenge_path("../pwn", "overflow"), "__pwn/overflow");
    }

    #[test]
    fn sanitizes_unsafe_challenge_name() {
        assert_eq!(safe_challenge_path("Web", "../secret"), "Web/__secret");
    }

    #[test]
    fn sanitizes_both_unsafe() {
        let path = safe_challenge_path("/tmp/x", "web/../../escape");
        assert!(!path.contains(".."));
        assert!(!path.starts_with('/'));
        // Components should be non-empty.
        let parts: Vec<&str> = path.split('/').collect();
        assert_eq!(parts.len(), 2);
        assert!(!parts[0].is_empty());
        assert!(!parts[1].is_empty());
    }
}
