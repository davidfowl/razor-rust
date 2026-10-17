//! Explicit typed values for incremental async interpolation.

use std::{future::Future, io};

use super::{AsyncHtmlWriter, AsyncSliceWriter};
use crate::HtmlString;

/// A value that can write its HTML representation incrementally.
///
/// Strings and characters escape the five reserved ASCII characters; primitives preserve the
/// synchronous representation, and [`HtmlString`] writes trusted HTML verbatim.
/// References forward without taking ownership. There is no blanket `Display`
/// or synchronous [`crate::WriteHtmlValue`] fallback.
/// [`Option`] writes nothing for `None` and borrows/delegates for `Some`,
/// retaining the contained value's encoding, formatting and failure behavior.
/// A `char` uses a four-byte UTF-8 stack buffer and the existing string path;
/// it is a Unicode scalar, not a C# UTF-16 code unit.
///
/// Custom implementations must await writes, preserve errors and supply a
/// `Send` future. Encoding/trust and bounded-memory behavior are their explicit
/// responsibility. The built-in implementations never allocate output buffers
/// proportional to input length and never flush implicitly.
///
/// Optional values do not introduce a `Display` fallback:
///
/// ```compile_fail
/// use razor_slices::async_template;
/// async_template! {
///     struct Page { model: Option<std::net::Ipv4Addr> } => { {model} }
/// }
/// ```
pub trait AsyncWriteHtmlValue: Sync {
    /// Writes this value without a final flush.
    fn write_html_async<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> impl Future<Output = io::Result<()>> + Send;
}

impl<V: AsyncWriteHtmlValue + ?Sized> AsyncWriteHtmlValue for &V {
    async fn write_html_async<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> io::Result<()> {
        (**self).write_html_async(output).await
    }
}

impl<V: AsyncWriteHtmlValue> AsyncWriteHtmlValue for Option<V> {
    async fn write_html_async<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> io::Result<()> {
        match self {
            Some(value) => value.write_html_async(output).await,
            None => Ok(()),
        }
    }
}

impl AsyncWriteHtmlValue for str {
    async fn write_html_async<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> io::Result<()> {
        output.write_value(self).await
    }
}

impl AsyncWriteHtmlValue for String {
    async fn write_html_async<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> io::Result<()> {
        self.as_str().write_html_async(output).await
    }
}

impl AsyncWriteHtmlValue for char {
    async fn write_html_async<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> io::Result<()> {
        let mut utf8 = [0; 4];
        output.write_value(self.encode_utf8(&mut utf8)).await
    }
}

impl AsyncWriteHtmlValue for HtmlString {
    async fn write_html_async<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> io::Result<()> {
        output.write_literal(&self.0).await
    }
}

impl AsyncWriteHtmlValue for bool {
    async fn write_html_async<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> io::Result<()> {
        output
            .write_literal(if *self { "true" } else { "false" })
            .await
    }
}
