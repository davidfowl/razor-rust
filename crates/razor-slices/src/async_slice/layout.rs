//! Typed incremental layouts with render-local immediate-content context.

use std::future::Future;

use super::{AsyncHtmlWriter, AsyncRenderError, AsyncSlice, AsyncSliceWriter};

/// A reusable async layout around a statically typed content slice.
///
/// Insert `body` with [`AsyncSlice::render_body_into`] and its optional sections
/// with [`AsyncHtmlWriter::write_section`]. Neither operation buffers or adds a
/// flush. Declared outer layouts see this layout's sections, not a recursive
/// search through the original body's sections, matching upstream ContentSlice.
/// Implementations must await writes and propagate failures.
pub trait AsyncLayout: Sync {
    /// Executes this layout with the supplied bare body on the same destination.
    fn render_layout_into<B: AsyncSlice, W: AsyncSliceWriter + Send>(
        &self,
        body: &B,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> impl Future<Output = Result<(), AsyncRenderError>> + Send;

    /// Executes this layout's own named section for an outer layout.
    ///
    /// Unknown nonempty names are optional; empty names are invalid. Explicit
    /// associations via [`super::with_async_section`] retain this contract.
    fn render_layout_section_into<W: AsyncSliceWriter + Send>(
        &self,
        name: &str,
        _output: &mut AsyncHtmlWriter<'_, W>,
    ) -> impl Future<Output = Result<(), AsyncRenderError>> + Send {
        async move { super::sections::validate_name(name).map_err(Into::into) }
    }
}

impl<L: AsyncLayout + ?Sized> AsyncLayout for &L {
    async fn render_layout_into<B: AsyncSlice, W: AsyncSliceWriter + Send>(
        &self,
        body: &B,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        (**self).render_layout_into(body, output).await
    }

    async fn render_layout_section_into<W: AsyncSliceWriter + Send>(
        &self,
        name: &str,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        (**self).render_layout_section_into(name, output).await
    }
}

impl<L: AsyncLayout + ?Sized> AsyncLayout for Box<L> {
    async fn render_layout_into<B: AsyncSlice, W: AsyncSliceWriter + Send>(
        &self,
        body: &B,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        (**self).render_layout_into(body, output).await
    }

    async fn render_layout_section_into<W: AsyncSliceWriter + Send>(
        &self,
        name: &str,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        (**self).render_layout_section_into(name, output).await
    }
}

/// A typed declared layout for a slice or a layout declaring an outer layout.
///
/// Construct with [`declare_async_layout`]. Full renders apply declarations;
/// body/partial and section execution bypass them. State is borrowed per render,
/// never cached or stored on the reusable layout.
pub struct DeclaredAsyncLayout<S, L> {
    body: S,
    layout: L,
}

/// Declares an async layout without rendering, allocating or spawning work.
///
/// Typed factories can associate generated [`crate::async_template!`] models
/// without new template syntax. Body and layout models may be borrowed; they
/// must remain valid for every render. Only [`super::render_incremental`] adds
/// a final flush on successful full rendering.
///
/// Non-layout values cannot be declared:
///
/// ```compile_fail
/// use razor_slices::{async_template, declare_async_layout};
/// async_template! { struct Page { model: () } => { "body" } }
/// let _ = declare_async_layout(Page::new(()), "not a layout");
/// ```
///
/// A synchronous body cannot be executed as an async declaration:
///
/// ```compile_fail
/// use razor_slices::{declare_async_layout, html, AsyncLayout, AsyncSlice,
///     AsyncSliceWriter, AsyncHtmlWriter, AsyncRenderError};
/// struct Layout;
/// impl AsyncLayout for Layout {
///     async fn render_layout_into<B: AsyncSlice, W: AsyncSliceWriter + Send>(
///         &self, body: &B, out: &mut AsyncHtmlWriter<'_, W>,
///     ) -> Result<(), AsyncRenderError> { body.render_body_into(out).await }
/// }
/// fn accepts(_: &impl AsyncSlice) {}
/// accepts(&declare_async_layout(html! { "sync" }, Layout));
/// ```
pub fn declare_async_layout<S, L: AsyncLayout>(body: S, layout: L) -> DeclaredAsyncLayout<S, L> {
    DeclaredAsyncLayout { body, layout }
}

impl<S: AsyncSlice, L: AsyncLayout> AsyncSlice for DeclaredAsyncLayout<S, L> {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        self.layout
            .render_layout_into(&Body(&self.body), output)
            .await
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
        self.body.render_section_into(name, output).await
    }
}

impl<S: AsyncLayout, L: AsyncLayout> AsyncLayout for DeclaredAsyncLayout<S, L> {
    async fn render_layout_into<B: AsyncSlice, W: AsyncSliceWriter + Send>(
        &self,
        body: &B,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        let content = LayoutContent {
            layout: &self.body,
            body,
        };
        self.layout.render_layout_into(&content, output).await
    }

    async fn render_layout_section_into<W: AsyncSliceWriter + Send>(
        &self,
        name: &str,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        self.body.render_layout_section_into(name, output).await
    }
}

/// Explicitly composes an async document, retaining its layout in child renders.
///
/// Construction and execution are lazy. Unlike [`declare_async_layout`], this
/// composed document remains composed when inserted as a partial or section.
pub fn with_async_layout<S: AsyncSlice, L: AsyncLayout>(body: S, layout: L) -> impl AsyncSlice {
    ExplicitLayout { body, layout }
}

struct ExplicitLayout<S, L> {
    body: S,
    layout: L,
}

impl<S: AsyncSlice, L: AsyncLayout> AsyncSlice for ExplicitLayout<S, L> {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        self.layout
            .render_layout_into(&Body(&self.body), output)
            .await
    }
}

struct LayoutContent<'a, L, B> {
    layout: &'a L,
    body: &'a B,
}

impl<L: AsyncLayout, B: AsyncSlice> AsyncSlice for LayoutContent<'_, L, B> {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        self.layout
            .render_layout_into(&Body(self.body), output)
            .await
    }

    async fn render_section_into<W: AsyncSliceWriter + Send>(
        &self,
        name: &str,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        self.layout.render_layout_section_into(name, output).await
    }
}

struct Body<'a, S: ?Sized>(&'a S);

impl<S: AsyncSlice + ?Sized> AsyncSlice for Body<'_, S> {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        self.0.render_body_into(output).await
    }

    async fn render_section_into<W: AsyncSliceWriter + Send>(
        &self,
        name: &str,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        self.0.render_section_into(name, output).await
    }
}
