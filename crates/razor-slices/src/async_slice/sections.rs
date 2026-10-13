//! Explicit lazy sections on the incremental async rendering path.

use std::io;

use super::{AsyncHtmlWriter, AsyncRenderError, AsyncSlice, AsyncSliceWriter};

/// Associates a reusable async section with a body without executing either.
///
/// Ordinary execution forwards to the body unchanged. Named execution renders
/// the section only on an exact name match; otherwise it delegates to the body's
/// sections. Chain associations for multiple names; the outermost association
/// wins when names repeat. There is no caching or implicit final section flush.
///
/// Owns both slices (or their references) and copies the name. Empty names fail
/// at construction with [`io::ErrorKind::InvalidInput`]. Unknown nonempty names
/// write nothing by default. Borrowed models must outlive their renderers.
///
/// ```
/// use razor_slices::{
///     with_async_section, AsyncHtmlWriter, AsyncRenderError, AsyncSlice, AsyncSliceWriter,
/// };
/// struct Text<'a>(&'a str);
/// impl AsyncSlice for Text<'_> {
///     async fn render_into<W: AsyncSliceWriter + Send>(
///         &self, out: &mut AsyncHtmlWriter<'_, W>,
///     ) -> Result<(), AsyncRenderError> {
///         out.write_value(self.0).await?;
///         Ok(())
///     }
/// }
/// let body = with_async_section(Text("Body"), "header", Text("<Title>"))?;
/// // Within an async template: out.write_section(&body, "header").await?;
/// # Ok::<(), std::io::Error>(())
/// ```
///
/// A synchronous slice is not an async section:
///
/// ```compile_fail
/// use razor_slices::{with_async_section, AsyncHtmlWriter, AsyncRenderError,
///                    AsyncSlice, AsyncSliceWriter, Slice};
/// struct Body;
/// impl AsyncSlice for Body {
///     async fn render_into<W: AsyncSliceWriter + Send>(
///         &self, _: &mut AsyncHtmlWriter<'_, W>,
///     ) -> Result<(), AsyncRenderError> { Ok(()) }
/// }
/// struct SyncOnly;
/// impl Slice for SyncOnly {
///     fn render(&self, _: &mut dyn std::fmt::Write) -> std::fmt::Result { Ok(()) }
/// }
/// let _ = with_async_section(Body, "header", SyncOnly);
/// ```
///
/// Borrowed section models cannot escape their lifetime:
///
/// ```compile_fail
/// use razor_slices::{with_async_section, AsyncHtmlWriter, AsyncRenderError,
///                    AsyncSlice, AsyncSliceWriter};
/// struct Text<'a>(&'a str);
/// impl AsyncSlice for Text<'_> {
///     async fn render_into<W: AsyncSliceWriter + Send>(
///         &self, out: &mut AsyncHtmlWriter<'_, W>,
///     ) -> Result<(), AsyncRenderError> {
///         out.write_value(self.0).await?;
///         Ok(())
///     }
/// }
/// let body;
/// {
///     let title = String::from("Title");
///     body = with_async_section(Text("Body"), "header", Text(&title)).unwrap();
/// }
/// drop(body);
/// ```
pub fn with_async_section<S: Sync, T: AsyncSlice>(
    body: S,
    name: &str,
    content: T,
) -> io::Result<AsyncSectioned<S, T>> {
    validate_name(name)?;
    Ok(AsyncSectioned {
        body,
        name: name.to_owned(),
        content,
    })
}

/// A lazy section association, retaining its body's slice or layout contract.
///
/// Construct with [`with_async_section`]. A layout association dispatches its
/// own sections, not its supplied body's sections, when used by an outer layout.
pub struct AsyncSectioned<S, T> {
    body: S,
    name: String,
    content: T,
}

impl<S: AsyncSlice, T: AsyncSlice> AsyncSlice for AsyncSectioned<S, T> {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        self.body.render_into(output).await
    }

    async fn render_body_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        self.body.render_body_into(output).await
    }

    async fn render_section_into<W: AsyncSliceWriter + Send>(
        &self,
        name: &str,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        validate_name(name)?;
        if name == self.name {
            self.content.render_body_into(output).await
        } else {
            self.body.render_section_into(name, output).await
        }
    }
}

impl<S: super::AsyncLayout, T: AsyncSlice> super::AsyncLayout for AsyncSectioned<S, T> {
    async fn render_layout_into<B: AsyncSlice, W: AsyncSliceWriter + Send>(
        &self,
        body: &B,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        self.body.render_layout_into(body, output).await
    }

    async fn render_layout_section_into<W: AsyncSliceWriter + Send>(
        &self,
        name: &str,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        validate_name(name)?;
        if name == self.name {
            self.content.render_body_into(output).await
        } else {
            self.body.render_layout_section_into(name, output).await
        }
    }
}

pub(super) fn validate_name(name: &str) -> io::Result<()> {
    if name.is_empty() {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "section name must not be empty",
        ))
    } else {
        Ok(())
    }
}
