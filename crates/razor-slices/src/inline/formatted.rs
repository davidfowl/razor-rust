//! Explicit synchronous formatting, streamed through the existing HTML encoder.

use std::{fmt, io};

use crate::{encode_html_to_writer, WriteHtmlValue};

/// Explicit Rust formatting arguments are encoded as text, even when they
/// contain a value whose direct interpolation would emit trusted HTML.
///
/// Formatting runs again on each render and does not collect a formatted
/// `String`. This uses Rust's format syntax/traits, not .NET format strings,
/// culture providers or `ICustomFormatter`. It does not add a blanket `Display`
/// implementation or an incremental async formatting bridge.
///
/// ```
/// use razor_slices::{html, Slice};
/// let value = 42;
/// let page = html! { "<p>" {format_args!("{value:04}")} "</p>" };
/// assert_eq!(page.render_to_string().unwrap(), "<p>0042</p>");
/// ```
///
/// Direct custom values still require an explicit HTML-writing contract:
///
/// ```compile_fail
/// use razor_slices::html;
/// struct Custom;
/// impl std::fmt::Display for Custom {
///     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
///         f.write_str("<custom>")
///     }
/// }
/// let _ = html! { {Custom} };
/// ```
///
/// `fmt::Arguments` is not an async value (`AsyncWriteHtmlValue` requires
/// `Sync`); no synchronous formatter is held across an incremental write:
///
/// ```compile_fail
/// use razor_slices::async_template;
/// async_template! {
///     struct Page { model: u32 } => { {format_args!("{model:04}")} }
/// }
/// ```
impl WriteHtmlValue for fmt::Arguments<'_> {
    fn write_html_to(&self, writer: &mut dyn io::Write) -> io::Result<()> {
        let mut adapter = EncodedFormatter {
            writer,
            error: None,
        };
        let result = fmt::write(&mut adapter, *self);
        match adapter.error {
            Some(error) => Err(error),
            None => result.map_err(io::Error::other),
        }
    }
}

struct EncodedFormatter<'a> {
    writer: &'a mut dyn io::Write,
    error: Option<io::Error>,
}

impl fmt::Write for EncodedFormatter<'_> {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        if self.error.is_some() {
            return Err(fmt::Error);
        }
        encode_html_to_writer(value, self.writer).map_err(|error| {
            self.error = Some(error);
            fmt::Error
        })
    }
}
