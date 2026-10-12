//! HTML encoding support, equivalent to upstream's use of `HtmlEncoder` and
//! `IHtmlContent` (see `RazorSlice.HtmlAttributes.cs` / `PipeWriterHtmlExtensions.cs`
//! upstream).

use std::fmt;

/// A string that is already known to be safe HTML and should **not** be
/// re-encoded when written to a template's output.
///
/// This mirrors upstream's `Microsoft.AspNetCore.Html.HtmlString`: wrap raw
/// markup (e.g. from a trusted partial render) in `HtmlString` to emit it
/// verbatim via `@Html.Raw`-style usage.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HtmlString(pub String);

impl HtmlString {
    /// An empty, already-encoded HTML string.
    pub fn empty() -> Self {
        HtmlString(String::new())
    }
}

impl fmt::Display for HtmlString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for HtmlString {
    fn from(value: String) -> Self {
        HtmlString(value)
    }
}

impl From<&str> for HtmlString {
    fn from(value: &str) -> Self {
        HtmlString(value.to_owned())
    }
}

/// HTML-encodes `value` and writes it to `writer`.
///
/// Escapes `&`, `<`, `>`, `"`, and `'`, matching the characters escaped by
/// ASP.NET Core's default `HtmlEncoder` for these common cases. This is a
/// straightforward, correctness-first implementation; a lower-allocation
/// byte-oriented encoder (writing UTF-8 directly, matching upstream's
/// `PipeWriter`-based fast paths) is tracked as a follow-up.
///
/// See <https://github.com/davidfowl/razor-rust/issues/10> for the
/// zero-allocation / `PipeWriter`-equivalent encoding follow-up.
pub fn encode_html_to(value: &str, writer: &mut dyn fmt::Write) -> fmt::Result {
    for ch in value.chars() {
        match ch {
            '&' => writer.write_str("&amp;")?,
            '<' => writer.write_str("&lt;")?,
            '>' => writer.write_str("&gt;")?,
            '"' => writer.write_str("&quot;")?,
            '\'' => writer.write_str("&#39;")?,
            c => writer.write_char(c)?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_reserved_characters() {
        let mut out = String::new();
        encode_html_to(r#"<a href="x">T & 'Co'</a>"#, &mut out).unwrap();
        assert_eq!(
            out,
            "&lt;a href=&quot;x&quot;&gt;T &amp; &#39;Co&#39;&lt;/a&gt;"
        );
    }

    #[test]
    fn html_string_is_not_encoded_by_consumers() {
        // HtmlString itself doesn't encode; it's a marker type consumers
        // check for (via Display) to skip encoding, mirroring IHtmlContent.
        let raw = HtmlString::from("<b>bold</b>");
        assert_eq!(raw.to_string(), "<b>bold</b>");
    }
}
