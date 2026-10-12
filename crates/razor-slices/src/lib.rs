//! Core runtime for Razor Slices: lightweight, high-performance templates for Rust.
//!
//! This crate provides the [`Slice`] trait and supporting types used by generated
//! template code (see the `razor-slices-macros`/codegen crate, tracked separately)
//! to render templates with low allocations.
//!
//! This is the runtime half of the port of
//! [RazorSlices](https://github.com/DamianEdwards/RazorSlices) to Rust. See the
//! tracking epic for overall project status: <https://github.com/davidfowl/razor-rust/issues/1>.

mod async_slice;
mod html;
mod slice;

#[cfg(feature = "tokio")]
pub use async_slice::TokioWriter;
pub use async_slice::{render_async, AsyncRenderError, AsyncSliceWriter};
pub use html::{encode_html_to, encode_html_to_writer, HtmlString, WriteHtmlValue};
pub use slice::{RenderError, Slice};
