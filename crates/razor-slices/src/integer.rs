//! Bounded explicit unsigned integer formatting for synchronous and async values.

use std::io;

use crate::{AsyncHtmlWriter, AsyncSliceWriter, AsyncWriteHtmlValue, WriteHtmlValue};

/// The supported unsigned integer representations.
///
/// These correspond to invariant C# UInt64 `D`, `x` and `X` formatting, not
/// arbitrary .NET format strings or format providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegerFormat {
    /// Decimal digits without grouping or a sign.
    Decimal,
    /// Hexadecimal digits `0`-`9` and `a`-`f`, without a prefix.
    LowerHex,
    /// Hexadecimal digits `0`-`9` and `A`-`F`, without a prefix.
    UpperHex,
}

/// A reusable unsigned integer with an explicit representation and zero padding.
///
/// Construct with [`format_integer`]. Both value-writing traits use the same
/// fixed 255-byte stack buffer and preserve destination errors without flushing.
/// Async writes await the destination and retain normal cancellation semantics.
/// The value contains no borrowed formatter, allocated output or cached future.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormattedInteger {
    value: u64,
    format: IntegerFormat,
    min_digits: u8,
}

/// Formats an unsigned 64-bit integer with at least `min_digits` digits.
///
/// All `u8` precision values (0-255) are supported. A precision of zero still
/// writes at least one digit, including for zero. A smaller precision never
/// truncates significant digits. There is no sign, radix prefix or grouping;
/// all generated characters are safe ASCII digits/hex letters.
///
/// This explicit adapter does not change primitive interpolation defaults.
/// It supports neither signed/128-bit inputs nor arbitrary Rust/.NET formats,
/// cultures or providers. Smaller unsigned inputs can use `u64::from(value)`.
///
/// ```
/// use razor_slices::{async_template, format_integer, html, IntegerFormat, Slice};
/// let value = format_integer(255, IntegerFormat::UpperHex, 4);
/// let page = html! { "<p>" {value} "</p>" };
/// assert_eq!(page.render_to_string().unwrap(), "<p>00FF</p>");
///
/// async_template! {
///     struct Page { model: u64 } => {
///         {format_integer(*model, IntegerFormat::Decimal, 4)}
///     }
/// }
/// ```
///
/// Signed values cannot be silently reinterpreted:
///
/// ```compile_fail
/// use razor_slices::{format_integer, IntegerFormat};
/// let _ = format_integer(-1_i64, IntegerFormat::UpperHex, 0);
/// ```
pub fn format_integer(value: u64, format: IntegerFormat, min_digits: u8) -> FormattedInteger {
    FormattedInteger {
        value,
        format,
        min_digits,
    }
}

struct Buffer {
    bytes: [u8; 255],
    start: usize,
}

impl FormattedInteger {
    fn formatted(&self) -> Buffer {
        let (radix, digits): (u64, &[u8; 16]) = match self.format {
            IntegerFormat::Decimal => (10, b"0123456789abcdef"),
            IntegerFormat::LowerHex => (16, b"0123456789abcdef"),
            IntegerFormat::UpperHex => (16, b"0123456789ABCDEF"),
        };
        let mut buffer = Buffer {
            bytes: [b'0'; 255],
            start: 255,
        };
        let mut value = self.value;
        loop {
            buffer.start -= 1;
            buffer.bytes[buffer.start] = digits[(value % radix) as usize];
            value /= radix;
            if value == 0 {
                break;
            }
        }
        buffer.start = buffer.start.min(255 - usize::from(self.min_digits));
        buffer
    }
}

impl WriteHtmlValue for FormattedInteger {
    fn write_html_to(&self, writer: &mut dyn io::Write) -> io::Result<()> {
        let buffer = self.formatted();
        writer.write_all(&buffer.bytes[buffer.start..])
    }
}

impl AsyncWriteHtmlValue for FormattedInteger {
    async fn write_html_async<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> io::Result<()> {
        let buffer = self.formatted();
        let text =
            std::str::from_utf8(&buffer.bytes[buffer.start..]).expect("integer digits are ASCII");
        output.write_literal(text).await
    }
}
