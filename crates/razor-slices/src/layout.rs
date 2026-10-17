//! Reusable synchronous layout/body composition.

use std::{fmt, io};

use crate::{InlineSlice, Slice};

/// A layout that renders around an explicitly supplied body [`Slice`].
///
/// Implementations can insert the body with [`crate::partial()`], retaining
/// child markup without re-encoding it. Model values within the layout must
/// still use normal HTML encoding. The supplied body bypasses declared layouts
/// even when rendered directly with [`Slice::render_to_writer`]. Explicit
/// composition is retained. The body is not rendered in advance.
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
    InlineSlice::new(move |writer: &mut dyn io::Write| layout.render_layout(&Body(&body), writer))
}

/// A typed layout declaration, distinct from explicit document composition.
///
/// Construct with [`declare_layout`]. Owns its body and layout (which may be
/// references). Full rendering applies the layout; child rendering bypasses
/// it. When the body implements [`Layout`], this association also implements
/// `Layout`, allowing that layout to declare an outer layout.
pub struct DeclaredLayout<S, L> {
    body: S,
    layout: L,
}

/// Declares the layout used when a slice is rendered as a full document.
///
/// Construction is lazy. [`crate::partial()`], layout body insertion and named
/// section execution bypass declarations, not explicit [`with_layout`]
/// composition. Typed Rust factories provide model checking and preserve
/// borrowed lifetimes; no generated model/declaration grammar is introduced.
///
/// ```
/// use razor_slices::{declare_layout, html, partial, Layout, Slice};
/// use std::io;
/// struct Page<'a>(&'a str);
/// impl Layout for Page<'_> {
///     fn render_layout(&self, body: &dyn Slice, w: &mut dyn io::Write) -> io::Result<()> {
///         html! { "<title>" {self.0} "</title>" {partial(body)} }.render_to_writer(w)
///     }
/// }
/// let title = String::from("A & B");
/// let page = declare_layout(html! { "<p>Body</p>" }, Page(&title));
/// assert_eq!(page.render_to_string()?, "<title>A &amp; B</title><p>Body</p>");
/// assert_eq!(html! { {partial(&page)} }.render_to_string()?, "<p>Body</p>");
/// # Ok::<(), razor_slices::RenderError>(())
/// ```
///
/// Only typed layouts can be associated:
///
/// ```compile_fail
/// use razor_slices::{declare_layout, html};
/// let _ = declare_layout(html! {}, "not a layout");
/// ```
///
/// A non-slice body cannot be rendered:
///
/// ```compile_fail
/// use razor_slices::{declare_layout, Layout, Slice};
/// use std::io;
/// struct Page;
/// impl Layout for Page {
///     fn render_layout(&self, body: &dyn Slice, w: &mut dyn io::Write) -> io::Result<()> {
///         body.render_to_writer(w)
///     }
/// }
/// let _ = declare_layout("not a slice", Page).render_to_string();
/// ```
///
/// Typed declaration factories reject the wrong model:
///
/// ```compile_fail
/// use razor_slices::{declare_layout, html, Layout, Slice};
/// use std::io;
/// struct Model { title: String }
/// struct Page;
/// impl Layout for Page {
///     fn render_layout(&self, body: &dyn Slice, w: &mut dyn io::Write) -> io::Result<()> {
///         body.render_to_writer(w)
///     }
/// }
/// fn page(model: &Model) -> impl Slice + '_ {
///     declare_layout(html! { {&model.title} }, Page)
/// }
/// let _ = page(&"wrong model");
/// ```
///
/// Borrowed layout models cannot escape their lifetime:
///
/// ```compile_fail
/// use razor_slices::{declare_layout, html, Layout, Slice};
/// use std::io;
/// struct Page<'a>(&'a str);
/// impl Layout for Page<'_> {
///     fn render_layout(&self, _: &dyn Slice, w: &mut dyn io::Write) -> io::Result<()> {
///         html! { {self.0} }.render_to_writer(w)
///     }
/// }
/// let page;
/// {
///     let title = String::from("Title");
///     page = declare_layout(html! {}, Page(&title));
/// }
/// let _ = page.render_to_string();
/// ```
///
/// Body models must likewise outlive the declaration:
///
/// ```compile_fail
/// use razor_slices::{declare_layout, html, Layout, Slice};
/// use std::io;
/// struct Page;
/// impl Layout for Page {
///     fn render_layout(&self, body: &dyn Slice, w: &mut dyn io::Write) -> io::Result<()> {
///         body.render_to_writer(w)
///     }
/// }
/// let page;
/// {
///     let model = String::from("Body");
///     let borrowed = &model;
///     page = declare_layout(html! { {borrowed} }, Page);
/// }
/// let _ = page.render_to_string();
/// ```
pub fn declare_layout<S, L: Layout>(body: S, layout: L) -> DeclaredLayout<S, L> {
    DeclaredLayout { body, layout }
}

impl<S: Slice, L: Layout> Slice for DeclaredLayout<S, L> {
    fn render(&self, writer: &mut dyn fmt::Write) -> fmt::Result {
        InlineSlice::new(|w: &mut dyn io::Write| self.render_to_writer(w)).render(writer)
    }

    fn render_to_writer(&self, writer: &mut dyn io::Write) -> io::Result<()> {
        self.layout.render_layout(&Body(&self.body), writer)
    }

    fn render_body_to_writer(&self, writer: &mut dyn io::Write) -> io::Result<()> {
        self.body.render_body_to_writer(writer)
    }

    fn render_section_to_writer(&self, name: &str, writer: &mut dyn io::Write) -> io::Result<()> {
        self.body.render_section_to_writer(name, writer)
    }
}

impl<S: Layout, L: Layout> Layout for DeclaredLayout<S, L> {
    fn render_layout(&self, body: &dyn Slice, writer: &mut dyn io::Write) -> io::Result<()> {
        let content =
            InlineSlice::new(|w: &mut dyn io::Write| self.body.render_layout(&Body(body), w));
        self.layout.render_layout(&content, writer)
    }
}

struct Body<'a>(&'a dyn Slice);

impl Slice for Body<'_> {
    fn render(&self, writer: &mut dyn fmt::Write) -> fmt::Result {
        InlineSlice::new(|w: &mut dyn io::Write| self.0.render_body_to_writer(w)).render(writer)
    }

    fn render_to_writer(&self, writer: &mut dyn io::Write) -> io::Result<()> {
        self.0.render_body_to_writer(writer)
    }

    fn render_section_to_writer(&self, name: &str, writer: &mut dyn io::Write) -> io::Result<()> {
        self.0.render_section_to_writer(name, writer)
    }
}
