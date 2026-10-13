//! Reusable synchronous layout/body composition.

use std::io;

use crate::{InlineSlice, Slice};

/// A layout that renders around an explicitly supplied body [`Slice`].
///
/// Implementations can insert the body with [`crate::partial()`], retaining
/// child markup without re-encoding it. Model values within the layout must
/// still use normal HTML encoding. The body is not rendered in advance.
pub trait Layout {
    /// Renders the layout, inserting `body` at the desired position.
    ///
    /// Write directly to `writer`, propagating failures and stopping before
    /// subsequent output on error. Do not flush the destination implicitly.
    /// Body execution/order is controlled by the layout's insertion points.
    fn render_layout(&self, body: &dyn Slice, writer: &mut dyn io::Write) -> io::Result<()>;
}

impl<L: Layout + ?Sized> Layout for &L {
    fn render_layout(&self, body: &dyn Slice, writer: &mut dyn io::Write) -> io::Result<()> {
        (**self).render_layout(body, writer)
    }
}

impl<L: Layout + ?Sized> Layout for Box<L> {
    fn render_layout(&self, body: &dyn Slice, writer: &mut dyn io::Write) -> io::Result<()> {
        (**self).render_layout(body, writer)
    }
}

/// Associates a body with a layout, returning a reusable [`Slice`].
///
/// Owns both arguments (which can themselves be references), without rendering
/// at construction. Every render calls the layout, which decides when to render
/// the body. Uses the existing byte-rendering bridge so original I/O failures
/// survive nested generated layouts/partials. Text rendering retains the
/// existing [`std::fmt::Error`]/[`crate::RenderError`] boundary.
///
/// ```
/// use razor_slices::{html, partial, with_layout, Layout, Slice};
/// use std::io;
///
/// struct Page { title: String }
/// impl Layout for Page {
///     fn render_layout(&self, body: &dyn Slice, writer: &mut dyn io::Write) -> io::Result<()> {
///         html! {
///             "<title>" {&self.title} "</title><main>" {partial(body)} "</main>"
///         }.render_to_writer(writer)
///     }
/// }
/// let body = html! { "<p>" {"<David>"} "</p>" };
/// let page = with_layout(body, Page { title: "Todos & more".into() });
/// let expected = "<title>Todos &amp; more</title><main><p>&lt;David&gt;</p></main>";
/// assert_eq!(page.render_to_string().unwrap(), expected);
/// assert_eq!(page.render_to_string().unwrap(), expected);
/// ```
///
/// Body values must be slices, not arbitrary raw strings:
///
/// ```compile_fail
/// use razor_slices::{with_layout, Layout, Slice};
/// use std::io;
/// struct Page;
/// impl Layout for Page {
///     fn render_layout(&self, _: &dyn Slice, _: &mut dyn io::Write) -> io::Result<()> {
///         Ok(())
///     }
/// }
/// let _ = with_layout("not a body slice", Page);
/// ```
///
/// A slice is not automatically a layout:
///
/// ```compile_fail
/// use razor_slices::{html, with_layout};
/// let _ = with_layout(html! {}, html! {});
/// ```
///
/// Borrowed bodies cannot escape their referents:
///
/// ```compile_fail
/// use razor_slices::{html, with_layout, Layout, Slice};
/// use std::io;
/// struct Page;
/// impl Layout for Page {
///     fn render_layout(&self, body: &dyn Slice, w: &mut dyn io::Write) -> io::Result<()> {
///         body.render_to_writer(w)
///     }
/// }
/// let page;
/// {
///     let body = html! { "<p>body</p>" };
///     page = with_layout(&body, Page);
/// }
/// let _ = page.render_to_string();
/// ```
///
/// A layout's model borrow must also remain valid:
///
/// ```compile_fail
/// use razor_slices::{html, with_layout, Layout, Slice};
/// use std::io;
/// struct Page<'a> { title: &'a str }
/// impl Layout for Page<'_> {
///     fn render_layout(&self, _: &dyn Slice, w: &mut dyn io::Write) -> io::Result<()> {
///         html! { {self.title} }.render_to_writer(w)
///     }
/// }
/// let page;
/// {
///     let title = String::from("Todos");
///     page = with_layout(html! {}, Page { title: &title });
/// }
/// let _ = page.render_to_string();
/// ```
pub fn with_layout<S: Slice, L: Layout>(body: S, layout: L) -> impl Slice {
    InlineSlice::new(move |writer: &mut dyn io::Write| layout.render_layout(&body, writer))
}
