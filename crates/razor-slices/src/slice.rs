//! The [`Slice`] trait: the Rust equivalent of upstream's `RazorSlice` base
//! class (see `src/RazorSlices/RazorSlice.cs` upstream).
//!
//! This initial version renders synchronously to an in-memory buffer
//! (`String` / `Vec<u8>`), matching the subset of upstream's supported
//! rendering targets that doesn't require async I/O. Async/streaming
//! rendering to a writer (mirroring `RenderAsync(PipeWriter, ...)`) is
//! tracked as a follow-up: <https://github.com/davidfowl/razor-rust/issues/11>.

use std::fmt;

/// An error that occurred while rendering a [`Slice`].
#[derive(Debug)]
pub struct RenderError(pub fmt::Error);

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error rendering slice: {}", self.0)
    }
}

impl std::error::Error for RenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

impl From<fmt::Error> for RenderError {
    fn from(value: fmt::Error) -> Self {
        RenderError(value)
    }
}

/// A renderable template, equivalent to upstream's `RazorSlice` /
/// `RazorSlice<TModel>`.
///
/// Implementations are normally generated from a template file by the
/// codegen crate (tracked in <https://github.com/davidfowl/razor-rust/issues/3>),
/// but the trait is hand-implementable, which is useful for tests and for
/// this initial runtime-only milestone.
pub trait Slice {
    /// Writes the rendered template body to `writer`.
    ///
    /// Implementations should call [`crate::encode_html_to`] for any
    /// interpolated values that aren't already-encoded [`crate::HtmlString`]
    /// content, matching upstream's auto-encoding behavior for `@expr`.
    fn render(&self, writer: &mut dyn fmt::Write) -> fmt::Result;

    /// Renders the template to an owned [`String`].
    fn render_to_string(&self) -> Result<String, RenderError> {
        let mut out = String::new();
        self.render(&mut out)?;
        Ok(out)
    }

    /// Renders the template and returns the UTF-8 encoded bytes.
    ///
    /// This is a straightforward `String` -> `Vec<u8>` conversion for now;
    /// a direct-to-bytes fast path (avoiding the UTF-16/UTF-8 non-issue that
    /// `String` sidesteps, but still avoiding an intermediate `String`
    /// allocation) is tracked as a follow-up alongside
    /// <https://github.com/davidfowl/razor-rust/issues/11>.
    fn render_to_bytes(&self) -> Result<Vec<u8>, RenderError> {
        Ok(self.render_to_string()?.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode_html_to;

    struct Hello<'a> {
        name: &'a str,
    }

    impl Slice for Hello<'_> {
        fn render(&self, writer: &mut dyn fmt::Write) -> fmt::Result {
            writer.write_str("<p>Hello, ")?;
            encode_html_to(self.name, writer)?;
            writer.write_str("!</p>")
        }
    }

    #[test]
    fn renders_to_string() {
        let slice = Hello { name: "World" };
        assert_eq!(slice.render_to_string().unwrap(), "<p>Hello, World!</p>");
    }

    #[test]
    fn encodes_interpolated_values() {
        let slice = Hello { name: "<script>" };
        assert_eq!(
            slice.render_to_string().unwrap(),
            "<p>Hello, &lt;script&gt;!</p>"
        );
    }

    #[test]
    fn renders_to_bytes() {
        let slice = Hello { name: "World" };
        assert_eq!(slice.render_to_bytes().unwrap(), b"<p>Hello, World!</p>");
    }
}
