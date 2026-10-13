//! Explicit single-value conditional HTML attributes.
//!
//! This mirrors the empty-prefix, single-value path in upstream
//! `RazorSlice.HtmlAttributes.cs`, not its compound attribute state machine.

use std::{fmt, io};

use crate::{HtmlString, WriteHtmlValue};

/// An invalid attribute name, rejected before any output is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidAttributeName {
    byte_index: Option<usize>,
}

impl InvalidAttributeName {
    /// The first invalid UTF-8 byte offset, or `None` for an empty name.
    pub fn byte_index(&self) -> Option<usize> {
        self.byte_index
    }
}

impl fmt::Display for InvalidAttributeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.byte_index {
            None => f.write_str("HTML attribute name must not be empty"),
            Some(index) => write!(f, "invalid HTML attribute name byte at offset {index}"),
        }
    }
}

impl std::error::Error for InvalidAttributeName {}

/// A value with conditional attribute semantics.
///
/// Strings, numbers and [`HtmlString`] use their existing [`WriteHtmlValue`]
/// implementation. Booleans repeat the attribute name for true and omit for
/// false; [`Option`] omits `None` and delegates `Some`. References delegate.
///
/// Custom implementations must write a value suitable for a double-quoted HTML
/// attribute, using existing encoded value helpers for untrusted text. Like
/// `WriteHtmlValue`, this is a trust boundary, not a sanitizer.
pub trait AttributeValue {
    /// Whether to omit the entire attribute, including its leading space.
    fn is_omitted(&self) -> bool {
        false
    }

    /// Writes the value without delimiters; `name` is already validated by
    /// [`attribute`]. Errors must be returned to the caller.
    fn write_attribute_value(&self, name: &str, writer: &mut dyn io::Write) -> io::Result<()>;
}

/// A validated, reusable attribute implementing [`WriteHtmlValue`].
///
/// Construct with [`attribute`]. Writing emits ` name="value"` (including one
/// leading space), or nothing for an omitted value. No buffering or flushing
/// occurs, and I/O errors are preserved. A failed write may leave partial output.
#[derive(Debug, Clone)]
pub struct Attribute<'a, V> {
    name: &'a str,
    value: V,
}

/// Validates `name` and adapts `value` to conditional attribute output.
///
/// Names use `[A-Za-z_:][A-Za-z0-9_:.-]*`. This conservative ASCII subset
/// supports `data-*`, `aria-*` and namespace-style names while rejecting HTML
/// delimiters, whitespace, controls and non-ASCII names. Invalid names are
/// rejected even when the value would be omitted. Validation is syntactic:
/// names such as `onclick` and URL schemes in values are not sanitized.
///
/// `true` repeats the name; `false` and `None` omit; an empty string is retained.
/// Text escapes exactly once through the existing encoder. [`HtmlString`]
/// bypasses encoding, so it must already be safe for a double-quoted attribute
/// (trusted body markup is not necessarily safe in this context).
///
/// ```
/// use razor_slices::{attribute, WriteHtmlValue};
/// let mut output = Vec::new();
/// attribute("checked", true)?.write_html_to(&mut output)?;
/// attribute("disabled", false)?.write_html_to(&mut output)?;
/// attribute("class", Some(""))?.write_html_to(&mut output)?;
/// attribute("title", "<&\"")?.write_html_to(&mut output)?;
/// assert_eq!(output, br#" checked="checked" class="" title="&lt;&amp;&quot;""#);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// Unsupported types do not silently fall back to unescaped `Display`:
///
/// ```compile_fail
/// use razor_slices::attribute;
/// attribute("value", vec!["untrusted"]).unwrap();
/// ```
///
/// Borrowed values cannot outlive their model:
///
/// ```compile_fail
/// use razor_slices::{attribute, WriteHtmlValue};
/// let value = {
///     let model = String::from("borrowed");
///     attribute("title", model.as_str()).unwrap()
/// };
/// value.write_html_to(&mut Vec::new()).unwrap();
/// ```
///
/// Borrowed names cannot outlive their storage either:
///
/// ```compile_fail
/// use razor_slices::{attribute, WriteHtmlValue};
/// let value = {
///     let name = String::from("title");
///     attribute(&name, "text").unwrap()
/// };
/// value.write_html_to(&mut Vec::new()).unwrap();
/// ```
pub fn attribute<V: AttributeValue>(
    name: &str,
    value: V,
) -> Result<Attribute<'_, V>, InvalidAttributeName> {
    if name.is_empty() {
        return Err(InvalidAttributeName { byte_index: None });
    }
    for (index, byte) in name.bytes().enumerate() {
        let valid = byte.is_ascii_alphabetic()
            || matches!(byte, b'_' | b':')
            || (index > 0 && (byte.is_ascii_digit() || matches!(byte, b'-' | b'.')));
        if !valid {
            return Err(InvalidAttributeName {
                byte_index: Some(index),
            });
        }
    }
    Ok(Attribute { name, value })
}

impl<V: AttributeValue> WriteHtmlValue for Attribute<'_, V> {
    fn write_html_to(&self, writer: &mut dyn io::Write) -> io::Result<()> {
        if self.value.is_omitted() {
            return Ok(());
        }
        writer.write_all(b" ")?;
        writer.write_all(self.name.as_bytes())?;
        writer.write_all(b"=\"")?;
        self.value.write_attribute_value(self.name, writer)?;
        writer.write_all(b"\"")
    }
}

impl AttributeValue for bool {
    fn is_omitted(&self) -> bool {
        !self
    }

    fn write_attribute_value(&self, name: &str, writer: &mut dyn io::Write) -> io::Result<()> {
        if *self {
            name.write_html_to(writer)?;
        }
        Ok(())
    }
}

impl<V: AttributeValue + ?Sized> AttributeValue for &V {
    fn is_omitted(&self) -> bool {
        (**self).is_omitted()
    }

    fn write_attribute_value(&self, name: &str, writer: &mut dyn io::Write) -> io::Result<()> {
        (**self).write_attribute_value(name, writer)
    }
}

impl<V: AttributeValue> AttributeValue for Option<V> {
    fn is_omitted(&self) -> bool {
        match self {
            None => true,
            Some(value) => value.is_omitted(),
        }
    }

    fn write_attribute_value(&self, name: &str, writer: &mut dyn io::Write) -> io::Result<()> {
        match self {
            None => Ok(()),
            Some(value) => value.write_attribute_value(name, writer),
        }
    }
}

macro_rules! impl_attribute_value {
    ($($value:ty),* $(,)?) => {
        $(
            impl AttributeValue for $value {
                fn write_attribute_value(
                    &self,
                    _name: &str,
                    writer: &mut dyn io::Write,
                ) -> io::Result<()> {
                    self.write_html_to(writer)
                }
            }
        )*
    };
}

impl_attribute_value!(
    str, String, HtmlString, i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize, f32,
    f64,
);
