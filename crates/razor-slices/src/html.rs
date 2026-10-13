//! HTML encoding support, equivalent to upstream's use of `HtmlEncoder` and
//! `IHtmlContent` (see `RazorSlice.HtmlAttributes.cs` / `PipeWriterHtmlExtensions.cs`
//! upstream).

use std::fmt;
use std::io;

use crate::{AsyncHtmlWriter, AsyncSliceWriter, AsyncWriteHtmlValue};

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

/// Returns the HTML entity for one of the 5 reserved ASCII bytes this
/// module encodes: `&`, `<`, `>`, `"`, `'`.
///
/// All of these are single-byte ASCII code points, and no UTF-8 continuation
/// or lead byte for a multi-byte character ever equals an ASCII byte value,
/// so scanning the raw UTF-8 byte representation of a `&str` for these bytes
/// (rather than decoding `char`s) is always safe and never splits a
/// multi-byte sequence. See [`escape_indices`] for how the scan itself works.
#[inline]
fn escape_entity(b: u8) -> &'static str {
    match b {
        b'&' => "&amp;",
        b'<' => "&lt;",
        b'>' => "&gt;",
        b'"' => "&quot;",
        b'\'' => "&#39;",
        _ => unreachable!("escape_entity called with a non-escaped byte"),
    }
}

/// Yields the indices of escaped bytes in ascending order.
///
/// Delegates to [`memchr`]'s SIMD-accelerated (SSE2/AVX2 on x86_64, NEON on
/// aarch64, with a scalar fallback elsewhere) byte search instead of
/// testing one byte at a time. `memchr` only supports needles of up to 3
/// distinct bytes, so the 5 escaped bytes (`& < > " '`) are split into two
/// monotonic scans, merging their matches without rescanning suffixes
/// when one group is absent or sparse. See ADR
/// [0005](../../docs/adr/0005-simd-html-encoding.md).
#[inline]
fn escape_indices(bytes: &[u8]) -> impl Iterator<Item = usize> + '_ {
    let mut angle_amp = memchr::memchr3_iter(b'&', b'<', b'>', bytes).peekable();
    let mut quotes = memchr::memchr2_iter(b'"', b'\'', bytes).peekable();
    std::iter::from_fn(move || match (angle_amp.peek(), quotes.peek()) {
        (Some(a), Some(q)) if a < q => angle_amp.next(),
        (Some(_), Some(_)) => quotes.next(),
        (Some(_), None) => angle_amp.next(),
        (None, Some(_)) => quotes.next(),
        (None, None) => None,
    })
}

/// Walks `bytes` emitting the longest possible runs of non-escaped bytes as
/// single `write_all` calls, substituting the HTML entity for each escaped
/// byte in between. Shared by both the [`fmt::Write`]- and
/// [`io::Write`]-based encoders below so there's a single place that
/// implements the scan.
#[inline]
fn write_encoded<E>(
    bytes: &[u8],
    mut write_all: impl FnMut(&[u8]) -> Result<(), E>,
) -> Result<(), E> {
    let mut start = 0;
    for i in escape_indices(bytes) {
        if start < i {
            write_all(&bytes[start..i])?;
        }
        write_all(escape_entity(bytes[i]).as_bytes())?;
        start = i + 1;
    }
    if start < bytes.len() {
        write_all(&bytes[start..])?;
    }
    Ok(())
}

/// HTML-encodes `value` and writes it to `writer`.
///
/// Escapes `&`, `<`, `>`, `"`, and `'`, matching the characters escaped by
/// ASP.NET Core's default `HtmlEncoder` for these common cases.
///
/// This scans the UTF-8 bytes of `value` directly and writes each run of
/// non-escaped bytes with a single `write_str` call (rather than decoding
/// and re-encoding one `char` at a time), avoiding redundant UTF-8 work. For
/// writing directly to a byte sink (e.g. a socket or buffer) without going
/// through `fmt::Write`'s UTF-8 validation at all, see
/// [`encode_html_to_writer`].
pub fn encode_html_to(value: &str, writer: &mut dyn fmt::Write) -> fmt::Result {
    write_encoded(value.as_bytes(), |chunk| {
        // SAFETY: `chunk` is a sub-slice of the valid UTF-8 `value`, split
        // only immediately before/after single-byte ASCII characters, so it
        // is itself valid UTF-8.
        writer.write_str(unsafe { std::str::from_utf8_unchecked(chunk) })
    })
}

/// HTML-encodes `value` and writes the resulting UTF-8 bytes directly to
/// `writer`, without an intermediate `String`/`char` buffer.
///
/// This is the byte-oriented, low-allocation counterpart to
/// [`encode_html_to`] for writing straight to a byte sink (e.g. a
/// `std::net::TcpStream`, `BufWriter<File>`, or any other [`io::Write`]
/// implementation) mirroring upstream's `PipeWriter`-based fast paths. See
/// <https://github.com/davidfowl/razor-rust/issues/10>.
pub fn encode_html_to_writer(value: &str, writer: &mut dyn io::Write) -> io::Result<()> {
    write_encoded(value.as_bytes(), |chunk| writer.write_all(chunk))
}

/// A fixed-capacity, stack-allocated buffer implementing [`fmt::Write`],
/// used to format primitives (integers, floats, bools) without a heap
/// allocation.
struct FixedBuf<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> FixedBuf<N> {
    fn new() -> Self {
        FixedBuf {
            buf: [0; N],
            len: 0,
        }
    }

    fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    fn as_str(&self) -> &str {
        std::str::from_utf8(self.as_bytes()).expect("fmt::Write accepts only UTF-8")
    }
}

impl<const N: usize> fmt::Write for FixedBuf<N> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let bytes = s.as_bytes();
        let end = self.len + bytes.len();
        if end > N {
            return Err(fmt::Error);
        }
        self.buf[self.len..end].copy_from_slice(bytes);
        self.len = end;
        Ok(())
    }
}

/// A value that can be written to HTML output as its own byte/text
/// representation, without necessarily allocating an intermediate `String`.
///
/// Mirrors upstream's non-boxing primitive fast paths in
/// `PipeWriterHtmlExtensions.cs`: numbers and bools are formatted directly
/// into a small stack buffer (no heap allocation in the common case), while
/// string-like values are HTML-encoded via [`encode_html_to_writer`]. See
/// <https://github.com/davidfowl/razor-rust/issues/10>.
pub trait WriteHtmlValue {
    /// Writes this value's HTML representation to `writer`.
    fn write_html_to(&self, writer: &mut dyn io::Write) -> io::Result<()>;
}

impl<T: WriteHtmlValue + ?Sized> WriteHtmlValue for &T {
    fn write_html_to(&self, writer: &mut dyn io::Write) -> io::Result<()> {
        (**self).write_html_to(writer)
    }
}

impl WriteHtmlValue for str {
    fn write_html_to(&self, writer: &mut dyn io::Write) -> io::Result<()> {
        encode_html_to_writer(self, writer)
    }
}

impl WriteHtmlValue for String {
    fn write_html_to(&self, writer: &mut dyn io::Write) -> io::Result<()> {
        encode_html_to_writer(self, writer)
    }
}

impl WriteHtmlValue for HtmlString {
    /// Writes the already-encoded HTML content verbatim, without
    /// re-encoding it (matching `IHtmlContent`/`@Html.Raw` semantics).
    fn write_html_to(&self, writer: &mut dyn io::Write) -> io::Result<()> {
        writer.write_all(self.0.as_bytes())
    }
}

impl WriteHtmlValue for bool {
    fn write_html_to(&self, writer: &mut dyn io::Write) -> io::Result<()> {
        writer.write_all(if *self { b"true" } else { b"false" })
    }
}

/// Max formatted length of the given integer type: one sign byte plus the
/// type's maximum decimal digit count, which always fits comfortably in 40
/// bytes (the widest case is `i128`/`u128`, at up to 39 digits plus sign).
macro_rules! impl_write_html_value_for_int {
    ($($t:ty),* $(,)?) => {
        $(
            impl WriteHtmlValue for $t {
                fn write_html_to(&self, writer: &mut dyn io::Write) -> io::Result<()> {
                    use fmt::Write as _;
                    let mut buf = FixedBuf::<40>::new();
                    write!(buf, "{self}").expect("integer formatting always fits in 40 bytes");
                    writer.write_all(buf.as_bytes())
                }
            }
            impl AsyncWriteHtmlValue for $t {
                async fn write_html_async<W: AsyncSliceWriter + Send>(
                    &self,
                    output: &mut AsyncHtmlWriter<'_, W>,
                ) -> io::Result<()> {
                    use fmt::Write as _;
                    let mut buf = FixedBuf::<40>::new();
                    write!(buf, "{self}").expect("integer formatting always fits in 40 bytes");
                    output.write_literal(buf.as_str()).await
                }
            }
        )*
    };
}

impl_write_html_value_for_int!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize);

/// Max formatted length for a float formatted without exponential notation:
/// `f64`'s Display never switches to scientific notation, so values with
/// extreme magnitude (e.g. subnormals near `f64::MIN_POSITIVE`) can expand
/// to a few hundred characters. 400 bytes comfortably covers that worst
/// case; the rare values that still don't fit fall back to `to_string()`
/// (an allocation, but still correct) rather than panicking.
macro_rules! impl_write_html_value_for_float {
    ($($t:ty),* $(,)?) => {
        $(
            impl WriteHtmlValue for $t {
                fn write_html_to(&self, writer: &mut dyn io::Write) -> io::Result<()> {
                    use fmt::Write as _;
                    let mut buf = FixedBuf::<400>::new();
                    if write!(buf, "{self}").is_ok() {
                        writer.write_all(buf.as_bytes())
                    } else {
                        writer.write_all(self.to_string().as_bytes())
                    }
                }
            }
            impl AsyncWriteHtmlValue for $t {
                async fn write_html_async<W: AsyncSliceWriter + Send>(
                    &self,
                    output: &mut AsyncHtmlWriter<'_, W>,
                ) -> io::Result<()> {
                    use fmt::Write as _;
                    let mut buf = FixedBuf::<400>::new();
                    write!(buf, "{self}").map_err(|error| {
                        io::Error::new(io::ErrorKind::InvalidData, error)
                    })?;
                    output.write_literal(buf.as_str()).await
                }
            }
        )*
    };
}

impl_write_html_value_for_float!(f32, f64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_indices_merges_both_groups_in_order() {
        for input in [
            b"ab>cd'ef".as_slice(),
            b"ab'cd>ef",
            b"<>&",
            b"\"'",
            b"<\"&'>",
            b"no escapes here",
            b"",
        ] {
            let expected: Vec<_> = input
                .iter()
                .enumerate()
                .filter_map(|(i, b)| matches!(b, b'&' | b'<' | b'>' | b'"' | b'\'').then_some(i))
                .collect();
            assert_eq!(escape_indices(input).collect::<Vec<_>>(), expected);
        }
    }

    fn scalar_encode(value: &str) -> String {
        let mut out = String::new();
        for c in value.chars() {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' => out.push_str("&quot;"),
                '\'' => out.push_str("&#39;"),
                _ => out.push(c),
            }
        }
        out
    }

    fn assert_both_encoders(value: &str) {
        let expected = scalar_encode(value);
        let mut text = String::new();
        encode_html_to(value, &mut text).unwrap();
        let mut bytes = Vec::new();
        encode_html_to_writer(value, &mut bytes).unwrap();
        assert_eq!(text, expected);
        assert_eq!(bytes, expected.as_bytes());
    }

    #[test]
    fn encoders_match_scalar_for_ascii_and_unicode_boundaries() {
        for b in 0..=127u8 {
            assert_both_encoders(&format!("before{}after", b as char));
        }
        for len in [0, 1, 15, 16, 31, 32, 63, 64, 127, 4096] {
            for escapes in ["<>&", "\"'", "<\"&'>", "'>&\"<"] {
                let safe = "é日本語🦀".repeat(len);
                assert_both_encoders(&format!("{escapes}{safe}{escapes}{safe}"));
                assert_both_encoders(&format!("{safe}{escapes}{safe}{escapes}"));
                assert_both_encoders(&escapes.repeat(len));
            }
        }
    }

    #[test]
    fn encoders_match_scalar_for_all_short_escape_combinations() {
        let alphabet = ['a', '&', '<', '>', '"', '\'', 'é', '🦀'];
        for mut value in 0..alphabet.len().pow(4) {
            let mut input = String::new();
            for _ in 0..4 {
                input.push(alphabet[value % alphabet.len()]);
                value /= alphabet.len();
            }
            assert_both_encoders(&input);
        }
    }

    #[test]
    fn fmt_encoder_preserves_safe_runs_and_stops_at_first_error() {
        struct Writer {
            chunks: Vec<String>,
            attempts: usize,
            fail_at: usize,
        }
        impl fmt::Write for Writer {
            fn write_str(&mut self, s: &str) -> fmt::Result {
                self.attempts += 1;
                if self.attempts == self.fail_at {
                    return Err(fmt::Error);
                }
                self.chunks.push(s.to_owned());
                Ok(())
            }
        }
        let chunks = ["abc", "&lt;", "日本語", "&quot;", "tail"];
        for fail_at in 1..=chunks.len() + 1 {
            let mut writer = Writer {
                chunks: Vec::new(),
                attempts: 0,
                fail_at,
            };
            let result = encode_html_to("abc<日本語\"tail", &mut writer);
            assert_eq!(result.is_err(), fail_at <= chunks.len());
            assert_eq!(writer.attempts, fail_at.min(chunks.len()));
            assert_eq!(writer.chunks, chunks[..fail_at - 1]);
        }
    }

    #[test]
    fn byte_encoder_propagates_first_sink_error() {
        struct Writer {
            output: Vec<u8>,
            attempts: usize,
            fail_at: usize,
        }
        impl io::Write for Writer {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.attempts += 1;
                if self.attempts == self.fail_at {
                    return Err(io::Error::new(io::ErrorKind::BrokenPipe, "test sink"));
                }
                self.output.extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                panic!("encoding must not flush")
            }
        }
        let chunks = ["abc", "&lt;", "日本語", "&quot;", "tail"];
        for fail_at in 1..=chunks.len() {
            let mut writer = Writer {
                output: Vec::new(),
                attempts: 0,
                fail_at,
            };
            let error = encode_html_to_writer("abc<日本語\"tail", &mut writer).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
            assert_eq!(error.to_string(), "test sink");
            assert_eq!(writer.attempts, fail_at);
            assert_eq!(writer.output, chunks[..fail_at - 1].concat().as_bytes());
        }
    }

    #[test]
    fn byte_encoder_handles_short_writes_and_interrupted_io() {
        struct Writer {
            output: Vec<u8>,
            interrupted: bool,
        }
        impl io::Write for Writer {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if !self.interrupted {
                    self.interrupted = true;
                    return Err(io::ErrorKind::Interrupted.into());
                }
                let len = bytes.len().min(2);
                self.output.extend_from_slice(&bytes[..len]);
                Ok(len)
            }
            fn flush(&mut self) -> io::Result<()> {
                panic!("encoding must not flush")
            }
        }
        let input = "é日本語<\"&'>tail";
        let mut writer = Writer {
            output: Vec::new(),
            interrupted: false,
        };
        encode_html_to_writer(input, &mut writer).unwrap();
        assert_eq!(writer.output, scalar_encode(input).as_bytes());
    }

    #[test]
    fn encode_html_to_writer_handles_consecutive_escapes() {
        let mut out = Vec::new();
        encode_html_to_writer("<<>>&&\"\"''", &mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "&lt;&lt;&gt;&gt;&amp;&amp;&quot;&quot;&#39;&#39;"
        );
    }

    #[test]
    fn encode_html_to_writer_handles_escape_at_start_and_end() {
        let mut out = Vec::new();
        encode_html_to_writer("<middle>", &mut out).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "&lt;middle&gt;");
    }

    #[test]
    fn encode_html_to_writer_handles_long_run_without_escapes() {
        // Exercises the memchr scan across a run long enough to span
        // multiple SIMD lanes on typical targets.
        let long_text = "x".repeat(4096);
        let mut out = Vec::new();
        encode_html_to_writer(&long_text, &mut out).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), long_text);
    }

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

    #[test]
    fn encode_html_to_writer_matches_fmt_write_version() {
        let input = r#"<a href="x">T & 'Co'</a> — mixed unicode 日本語"#;

        let mut via_fmt = String::new();
        encode_html_to(input, &mut via_fmt).unwrap();

        let mut via_writer = Vec::new();
        encode_html_to_writer(input, &mut via_writer).unwrap();

        assert_eq!(via_fmt.as_bytes(), via_writer.as_slice());
        assert_eq!(String::from_utf8(via_writer).unwrap(), via_fmt);
    }

    #[test]
    fn encode_html_to_writer_preserves_multibyte_utf8() {
        let mut out = Vec::new();
        encode_html_to_writer("héllo <日本語>", &mut out).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "héllo &lt;日本語&gt;");
    }

    #[test]
    fn encode_html_to_writer_handles_empty_and_no_escapes_needed() {
        let mut out = Vec::new();
        encode_html_to_writer("", &mut out).unwrap();
        assert!(out.is_empty());

        let mut out = Vec::new();
        encode_html_to_writer("plain text", &mut out).unwrap();
        assert_eq!(out, b"plain text");
    }

    #[test]
    fn write_html_value_for_bool() {
        let mut out = Vec::new();
        true.write_html_to(&mut out).unwrap();
        false.write_html_to(&mut out).unwrap();
        assert_eq!(out, b"truefalse");
    }

    #[test]
    fn write_html_value_for_integers() {
        let mut out = Vec::new();
        42i32.write_html_to(&mut out).unwrap();
        (-7i64).write_html_to(&mut out).unwrap();
        0u8.write_html_to(&mut out).unwrap();
        u64::MAX.write_html_to(&mut out).unwrap();
        i128::MIN.write_html_to(&mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            format!("42-70{}{}", u64::MAX, i128::MIN)
        );
    }

    #[test]
    fn write_html_value_for_floats() {
        let mut out = Vec::new();
        1.5f64.write_html_to(&mut out).unwrap();
        (-2.25f32).write_html_to(&mut out).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "1.5-2.25");
    }

    #[test]
    fn write_html_value_for_float_extreme_magnitude_falls_back_without_panicking() {
        // f64::MIN_POSITIVE's Display expansion (no exponential notation)
        // is ~300+ characters, exercising the FixedBuf overflow fallback.
        let mut out = Vec::new();
        f64::MIN_POSITIVE.write_html_to(&mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            f64::MIN_POSITIVE.to_string()
        );
    }

    #[test]
    fn write_html_value_for_str_encodes() {
        let mut out = Vec::new();
        "<script>".write_html_to(&mut out).unwrap();
        assert_eq!(out, b"&lt;script&gt;");
    }

    #[test]
    fn write_html_value_for_html_string_is_raw() {
        let mut out = Vec::new();
        HtmlString::from("<b>bold</b>")
            .write_html_to(&mut out)
            .unwrap();
        assert_eq!(out, b"<b>bold</b>");
    }
}
