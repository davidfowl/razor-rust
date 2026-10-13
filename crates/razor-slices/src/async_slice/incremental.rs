//! Incremental async template execution, independent of synchronous `Slice`.

use std::{future::Future, io};

use super::{
    AsyncRenderError, AsyncSliceWriter, AsyncWriteHtmlValue, CancellationToken, RenderContext,
};
use crate::encode_html_to_writer;

/// A reusable template that writes output while awaiting work in template order.
///
/// Unlike [`super::render_async`], this contract never buffers the whole template.
/// Implementations await each write/flush and propagate errors with `?`. Call
/// [`render_incremental`] for a final flush after successful execution; nested
/// templates can call [`AsyncSlice::render_into`] on the same output directly.
///
/// This statically dispatched trait is not object-safe. Its futures are `Send`;
/// it does not alter the synchronous [`crate::Slice`] contract or `html!` grammar.
/// [`crate::async_template!`] generates reusable implementations of this trait.
/// Repeated renders execute the implementation again. Implementations own any
/// state or side effects; the runtime does not cache output or reset model state.
pub trait AsyncSlice: Sync {
    /// Executes this template without implicitly flushing on completion.
    ///
    /// Dropping the returned future stops polling it. Output already accepted
    /// by the writer is not rolled back, and application work spawned separately
    /// is not cancelled. See [`render_incremental`] for cancellation limitations.
    fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> impl Future<Output = Result<(), AsyncRenderError>> + Send;

    /// Executes a named section at its insertion point, without a final flush.
    ///
    /// Names are nonempty and case-sensitive; unknown sections write nothing by
    /// default, matching upstream's optional section execution. Use
    /// [`super::with_async_section`] for explicit associations or override this
    /// method for handwritten providers. Implementations must reject empty
    /// names and propagate errors. [`AsyncHtmlWriter::write_section`] validates
    /// names before dispatching, including to custom providers.
    fn render_section_into<W: AsyncSliceWriter + Send>(
        &self,
        name: &str,
        _output: &mut AsyncHtmlWriter<'_, W>,
    ) -> impl Future<Output = Result<(), AsyncRenderError>> + Send {
        async move { super::sections::validate_name(name).map_err(Into::into) }
    }
}

impl<S: AsyncSlice + ?Sized> AsyncSlice for &S {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        (**self).render_into(output).await
    }

    async fn render_section_into<W: AsyncSliceWriter + Send>(
        &self,
        name: &str,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        (**self).render_section_into(name, output).await
    }
}

impl<S: AsyncSlice + ?Sized> AsyncSlice for Box<S> {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        (**self).render_into(output).await
    }

    async fn render_section_into<W: AsyncSliceWriter + Send>(
        &self,
        name: &str,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        (**self).render_section_into(name, output).await
    }
}

/// HTML-aware output for incremental async templates.
///
/// Literal writes borrow their input directly. Escaped values reuse the existing
/// HTML encoder in chunks of at most 1,024 input bytes, with a fixed 6,144-byte
/// scratch buffer (the largest entity is six bytes). No full-value or full-page
/// allocation, channel, background task, or blocking executor is used.
///
/// Every write awaits the destination before advancing. `write_all` means the
/// sink accepted all bytes, not necessarily that a client received them; explicit
/// [`Self::flush`] requests visibility according to the sink's own contract.
pub struct AsyncHtmlWriter<'a, W> {
    writer: &'a mut W,
    cancellation: Option<&'a CancellationToken>,
}

impl<'a, W: AsyncSliceWriter + Send> AsyncHtmlWriter<'a, W> {
    /// Borrows the destination without taking ownership or closing it.
    pub fn new(writer: &'a mut W) -> Self {
        Self {
            writer,
            cancellation: None,
        }
    }

    /// Borrows the destination and a shared cancellation scope.
    ///
    /// Writes/flushes observe cancellation, including while pending. For
    /// cancellation during application awaits use
    /// [`render_incremental_with_context`] or [`CancellationToken::run`] around
    /// the entire execution, not this constructor alone.
    ///
    /// The token must outlive its output borrow:
    ///
    /// ```compile_fail
    /// use razor_slices::{AsyncHtmlWriter, AsyncSliceWriter, CancellationToken};
    /// struct Writer;
    /// impl AsyncSliceWriter for Writer {
    ///     async fn write_all(&mut self, _: &[u8]) -> std::io::Result<()> { Ok(()) }
    ///     async fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    /// }
    /// let mut writer = Writer;
    /// let output;
    /// {
    ///     let token = CancellationToken::new();
    ///     output = AsyncHtmlWriter::with_cancellation(&mut writer, &token);
    /// }
    /// drop(output);
    /// ```
    pub fn with_cancellation(writer: &'a mut W, cancellation: &'a CancellationToken) -> Self {
        Self {
            writer,
            cancellation: Some(cancellation),
        }
    }

    /// Returns the same scope token in pages, children and sections.
    ///
    /// Legacy unscoped writers return `None`. Application operations may clone
    /// this token and explicitly participate without borrowing the writer.
    pub fn cancellation_token(&self) -> Option<&CancellationToken> {
        self.cancellation
    }

    /// Writes trusted literal markup without escaping.
    ///
    /// Do not use this for untrusted model values; use [`Self::write_value`].
    pub async fn write_literal(&mut self, literal: &str) -> io::Result<()> {
        let write = self.writer.write_all(literal.as_bytes());
        match self.cancellation {
            Some(token) => token.run(write).await,
            None => write.await,
        }
    }

    /// Writes a string value, escaping `&`, `<`, `>`, `"`, and `'`.
    ///
    /// Preserves UTF-8, including characters crossing the chunk boundary.
    /// Like the synchronous encoder, this does not implement the broader
    /// Unicode allow-list policy of ASP.NET Core's default `HtmlEncoder`.
    pub async fn write_value(&mut self, mut value: &str) -> io::Result<()> {
        let mut scratch = [0; 6 * 1_024];
        while !value.is_empty() {
            let mut end = value.len().min(1_024);
            while !value.is_char_boundary(end) {
                end -= 1;
            }
            let (chunk, rest) = value.split_at(end);
            let mut buffer = io::Cursor::new(scratch.as_mut_slice());
            encode_html_to_writer(chunk, &mut buffer)?;
            let len = buffer.position() as usize;
            let write = self.writer.write_all(&scratch[..len]);
            match self.cancellation {
                Some(token) => token.run(write).await?,
                None => write.await?,
            }
            value = rest;
        }
        Ok(())
    }

    /// Writes an explicitly supported typed value without buffering or flushing.
    ///
    /// This is the generated interpolation path. [`Self::write_value`] remains
    /// the string-only encoding API, including its existing coercions.
    pub async fn write_html_value(
        &mut self,
        value: &(impl AsyncWriteHtmlValue + ?Sized),
    ) -> io::Result<()> {
        value.write_html_async(self).await
    }

    /// Executes a named section on the same output without buffering or flushing.
    ///
    /// Section templates encode their own values; this insertion does not
    /// re-encode their markup. Empty names fail with an `InvalidInput` I/O error
    /// inside [`AsyncRenderError::Write`] before calling the provider. Unknown
    /// nonempty names are optional and write nothing by default.
    pub async fn write_section(
        &mut self,
        slice: &(impl AsyncSlice + ?Sized),
        name: &str,
    ) -> Result<(), AsyncRenderError> {
        super::sections::validate_name(name)?;
        slice.render_section_into(name, self).await
    }

    /// Awaits a mid-template flush, propagating backpressure and failure.
    pub async fn flush(&mut self) -> io::Result<()> {
        let flush = self.writer.flush();
        match self.cancellation {
            Some(token) => token.run(flush).await,
            None => flush.await,
        }
    }
}

/// Executes an [`AsyncSlice`] incrementally and flushes after success.
///
/// Earlier output can be observed before later awaited work completes, provided
/// the destination exposes it (use a mid-template flush for buffered sinks).
/// Writer or template errors return immediately: there is no success fallback,
/// retry, rollback, or final flush after an error.
///
/// This legacy entry point cancels by dropping its future. For an observable
/// cooperative token, use [`render_incremental_with_context`].
/// A pending write may already have accepted a prefix, depending on the sink;
/// do not restart a cancelled render on the same destination expecting atomic
/// output. Cancellation of application awaits is their responsibility. The
/// destination remains caller-owned and is not shut down. No automatic
/// threshold flush or C# `PipeWriter` completion/cancellation flags are provided.
pub async fn render_incremental(
    slice: &(impl AsyncSlice + ?Sized),
    writer: &mut (impl AsyncSliceWriter + Send),
) -> Result<(), AsyncRenderError> {
    let mut output = AsyncHtmlWriter::new(writer);
    slice.render_into(&mut output).await?;
    output.flush().await?;
    Ok(())
}

/// Executes with immutable typed request data and an observable cancellation scope.
///
/// Bind the context through the slice's typed model when it needs request data.
/// The shared output automatically propagates the token to nested slices and
/// sections without adding a generic parameter to existing template contracts.
/// The scope races the entire execution (including held application awaits)
/// and each destination write/flush against cancellation.
///
/// Returns [`AsyncRenderError::Write`] containing an `Interrupted` I/O error
/// with [`super::RenderCancelled`] source. Original destination/template errors
/// are otherwise unchanged. No suffix or final flush runs after cancellation.
/// Partial writes are not rolled back; the caller still owns the destination.
/// A cancelled token stays cancelled across repeated renders.
///
/// ```compile_fail
/// use razor_slices::{
///     async_template, render_incremental_with_context, AsyncSliceWriter,
///     CancellationToken, RenderContext,
/// };
/// async_template! { struct Page { model: () } => { "page" } }
/// struct Writer;
/// impl AsyncSliceWriter for Writer {
///     async fn write_all(&mut self, _: &[u8]) -> std::io::Result<()> { Ok(()) }
///     async fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
/// }
/// let page = Page::new(());
/// let mut writer = Writer;
/// let render;
/// {
///     let context = RenderContext::new((), CancellationToken::new());
///     render = render_incremental_with_context(&page, &mut writer, &context);
/// }
/// drop(render);
/// ```
pub async fn render_incremental_with_context<T: Sync>(
    slice: &(impl AsyncSlice + ?Sized),
    writer: &mut (impl AsyncSliceWriter + Send),
    context: &RenderContext<T>,
) -> Result<(), AsyncRenderError> {
    let token = context.cancellation_token();
    token
        .run(async {
            let mut output = AsyncHtmlWriter::with_cancellation(writer, token);
            slice.render_into(&mut output).await?;
            output.flush().await?;
            Ok(())
        })
        .await
}
