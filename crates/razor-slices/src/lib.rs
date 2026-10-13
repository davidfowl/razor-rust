//! Core runtime for Razor Slices: lightweight, high-performance templates for Rust.
//!
//! This crate provides the [`Slice`] trait and supporting types used by generated
//! template code (see [`html!`] and [`async_template!`])
//! to render templates with low allocations.
//!
//! This is the runtime half of the port of
//! [RazorSlices](https://github.com/DamianEdwards/RazorSlices) to Rust. See the
//! tracking epic for overall project status: <https://github.com/davidfowl/razor-rust/issues/1>.

mod async_slice;
mod attribute;
mod html;
mod inline;
mod layout;
mod partial;
mod section;
mod slice;

#[cfg(feature = "tokio")]
pub use async_slice::TokioWriter;
pub use async_slice::{
    declare_async_layout, render_async, render_incremental, with_async_layout, with_async_section,
    AsyncHtmlWriter, AsyncLayout, AsyncRenderError, AsyncSectioned, AsyncSlice, AsyncSliceWriter,
    DeclaredAsyncLayout,
};
pub use attribute::{attribute, Attribute, AttributeValue, InvalidAttributeName};
pub use html::{encode_html_to, encode_html_to_writer, HtmlString, WriteHtmlValue};
pub use inline::InlineSlice;
pub use layout::{declare_layout, with_layout, DeclaredLayout, Layout};
pub use partial::{partial, Partial};
pub use section::{section, with_section, Section};
pub use slice::{RenderError, Slice};

#[doc(hidden)]
pub use razor_slices_macros::html as __html;

#[doc(hidden)]
pub use razor_slices_macros::async_template as __async_template;

/// Declares a reusable, model-typed [`AsyncSlice`] executed in template order.
///
/// Declares exactly one named `model` field and generates `new(model)` with the
/// struct's visibility. Each render borrows that field as `model`; it does not
/// consume the model or cache futures/output. Lifetime/type parameters and
/// where clauses are ordinary Rust. The model must be `Sync`, and all awaited
/// work must produce a `Send` render future.
///
/// String literals are trusted markup; `{ expression }` escapes string values
/// with the bounded [`AsyncHtmlWriter::write_value`] path. Nested `if`/`for`
/// use the same grammar as [`html!`]. Async-only directives:
///
/// - `@{ Rust statements }` introduces render-local bindings/work (including await).
/// - `@await { future };` awaits unit output; `@await { future }?;` propagates
///   errors from a unit-result future. Value-producing awaits use Rust bindings.
/// - `@flush;` awaits an explicit flush at that insertion point.
/// - `@render { child };` executes a bare async child on the same writer,
///   bypassing declared layouts but retaining explicit composition.
/// - `@section { provider }, { name };` inserts an explicit lazy async section.
///
/// No directive implicitly buffers, spawns, or flushes after a child. Only
/// [`render_incremental`] adds a final flush on success. Unlike synchronous
/// interpolation, values must coerce to `&str`; format numbers explicitly.
/// See `docs/async-templates.md` and `examples/async_template.rs`.
///
/// ```
/// use razor_slices::{async_template, AsyncSlice};
/// async_template! {
///     pub struct Greeting<'a> { model: &'a str } => {
///         "<p>" {model} "</p>"
///         @flush;
///         @await { async {} };
///     }
/// }
/// let template = Greeting::new("<World>");
/// fn is_async(_: &impl AsyncSlice) {}
/// is_async(&template);
/// ```
#[macro_export]
macro_rules! async_template {
    ($($template:tt)*) => {
        $crate::__async_template! { ($crate) $($template)* }
    };
}

/// Creates a reusable synchronous [`Slice`] from literal markup and values.
///
/// String literals (including raw strings) are written verbatim. Each
/// `{ expression }` must implement [`WriteHtmlValue`]: strings are escaped,
/// primitives formatted, and [`HtmlString`] emitted as trusted HTML.
///
/// Captures use Rust's `move` closure rules. Expressions run on each render,
/// not at construction; they must not consume or mutably borrow captures
/// (the renderer implements `Fn`, not `FnOnce` or `FnMut`). Borrowed captures
/// cannot outlive their referents. See the crate's `examples/inline.rs` and
/// `INLINE.md` for the initial grammar and limitations.
///
/// ```
/// use razor_slices::{html, HtmlString, Slice};
/// let name = String::from("<World>");
/// let greeting = html! { "<p>Hello, " {name} "!" {HtmlString::from("<br>")} "</p>" };
/// assert_eq!(greeting.render_to_string().unwrap(), "<p>Hello, &lt;World&gt;!<br></p>");
/// assert_eq!(greeting.render_to_bytes().unwrap(), b"<p>Hello, &lt;World&gt;!<br></p>");
/// ```
///
/// Unsupported grammar is rejected, rather than silently treated as markup:
///
/// ```compile_fail
/// use razor_slices::html;
/// let _ = html! { <p>Hello</p> };
/// ```
///
/// ```compile_fail
/// use razor_slices::html;
/// let _ = html! { {} };
/// ```
///
/// ```compile_fail
/// use razor_slices::html;
/// let _ = html! { {1 2} };
/// ```
///
/// Values are type checked, with no implicit `Display` or raw-HTML fallback:
///
/// ```compile_fail
/// use razor_slices::html;
/// struct Unsupported;
/// let _ = html! { {Unsupported} };
/// ```
///
/// Renderers cannot consume their captures:
///
/// ```compile_fail
/// use razor_slices::html;
/// let name = String::from("World");
/// let _ = html! { {{ drop(name); "World" }} };
/// ```
///
/// Borrowed captures cannot escape their lifetime:
///
/// ```compile_fail
/// use razor_slices::{html, Slice};
/// let greeting;
/// {
///     let name = String::from("World");
///     let borrowed = &name;
///     greeting = html! { {borrowed} };
/// }
/// let _ = greeting.render_to_string();
/// ```
#[macro_export]
macro_rules! html {
    ($($template:tt)*) => {
        $crate::__html! { ($crate) $($template)* }
    };
}
