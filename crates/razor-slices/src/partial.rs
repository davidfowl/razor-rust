//! Explicit child-slice rendering without re-encoding the child's markup.

use std::io;

use crate::{Slice, WriteHtmlValue};

/// A child slice that can be interpolated with [`crate::html!`].
///
/// Construct with [`partial`]. Owns its slice (or a reference to it), rendering
/// anew on each write. It does not cache output or infer layout/context state.
pub struct Partial<S> {
    slice: S,
}

/// Marks a child [`Slice`] for rendering inside a parent's HTML output.
///
/// Writes the child directly into the parent destination using
/// [`Slice::render_to_writer`], without buffering or re-encoding its markup.
/// The child is responsible for encoding its values, just as when rendered
/// on its own. This is not a raw-HTML escape hatch for arbitrary strings.
///
/// ```
/// use razor_slices::{html, partial, HtmlString, Slice};
/// let row = html! { "<li>" {"<David>"} {HtmlString::from("<br>")} "</li>" };
/// let list = html! { "<ul>" {partial(&row)} {partial(&row)} "</ul>" };
/// assert_eq!(list.render_to_string().unwrap(),
///     "<ul><li>&lt;David&gt;<br></li><li>&lt;David&gt;<br></li></ul>");
/// ```
///
/// To own a child, bind `partial(child)` before capturing the wrapper:
///
/// ```
/// use razor_slices::{html, partial, Slice};
/// let child = partial(html! { "<b>" {"&"} "</b>" });
/// let parent = html! { {child} };
/// assert_eq!(parent.render_to_string().unwrap(), "<b>&amp;</b>");
/// assert_eq!(parent.render_to_string().unwrap(), "<b>&amp;</b>");
/// ```
///
/// Strings are not slices:
///
/// ```compile_fail
/// use razor_slices::partial;
/// let _ = partial("<b>not a slice</b>");
/// ```
///
/// Child interpolation must be explicit:
///
/// ```compile_fail
/// use razor_slices::html;
/// let child = html! { "<b>child</b>" };
/// let _ = html! { {child} };
/// ```
///
/// A reusable parent cannot consume a captured child on each render:
///
/// ```compile_fail
/// use razor_slices::{html, partial};
/// let child = html! { "<b>child</b>" };
/// let _ = html! { {partial(child)} };
/// ```
///
/// A borrowed child must outlive its parent:
///
/// ```compile_fail
/// use razor_slices::{html, partial, Slice};
/// let parent;
/// {
///     let child = html! { "<b>child</b>" };
///     let borrowed = &child;
///     parent = html! { {partial(borrowed)} };
/// }
/// let _ = parent.render_to_string();
/// ```
///
/// Ordinary typed factories reject the wrong model at compile time:
///
/// ```compile_fail
/// use razor_slices::{html, partial, Slice};
/// struct Todo { id: u32 }
/// fn row(model: &Todo) -> impl Slice + '_ { html! { {model.id} } }
/// let _ = partial(row(&"wrong model"));
/// ```
pub fn partial<S: Slice>(slice: S) -> Partial<S> {
    Partial { slice }
}

impl<S: Slice> WriteHtmlValue for Partial<S> {
    fn write_html_to(&self, writer: &mut dyn io::Write) -> io::Result<()> {
        self.slice.render_to_writer(writer)
    }
}
