//! The [`Slice`] trait: the Rust equivalent of upstream's `RazorSlice` base
//! class (see `src/RazorSlices/RazorSlice.cs` upstream).
//!
//! Synchronous rendering supports text/byte writers and convenience in-memory
//! buffers (`String` / `Vec<u8>`). The buffered bridge to
//! an async writer (mirroring `RenderAsync(PipeWriter, ...)`) is provided by
//! [`crate::render_async`] / [`crate::AsyncSliceWriter`]
//! (<https://github.com/davidfowl/razor-rust/issues/11>).

use std::{fmt, io};

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
/// Implementations can be generated inline with [`crate::html!`] or written
/// by hand. Further codegen work is tracked in
/// <https://github.com/davidfowl/razor-rust/issues/3>.
pub trait Slice {
    /// Writes the rendered template body to `writer`.
    ///
    /// Implementations should call [`crate::encode_html_to`] for any
    /// interpolated values that aren't already-encoded [`crate::HtmlString`]
    /// content, matching upstream's auto-encoding behavior for `@expr`.
    fn render(&self, writer: &mut dyn fmt::Write) -> fmt::Result;

    /// Renders directly to a byte writer, without buffering the template.
    ///
    /// The default adapts [`Self::render`], retaining original destination I/O
    /// errors. A text renderer's own [`fmt::Error`] becomes an
    /// [`io::ErrorKind::Other`] error with that error as its source.
    /// Byte-native implementations should override this to preserve their
    /// own I/O errors, as [`crate::InlineSlice`] does.
    ///
    /// Does not flush the destination. Output can be partial on failure.
    fn render_to_writer(&self, writer: &mut dyn io::Write) -> io::Result<()> {
        let mut adapter = ByteWriter {
            writer,
            error: None,
        };
        let result = self.render(&mut adapter);
        match adapter.error {
            Some(error) => Err(error),
            None => result.map_err(io::Error::other),
        }
    }

    /// Renders a named section directly to a byte writer at its insertion point.
    ///
    /// Names are nonempty and case-sensitive. The default writes nothing for
    /// an unknown section, matching upstream's optional section execution.
    /// Use [`crate::with_section`] to associate reusable section slices, and
    /// [`crate::section()`] to insert them in [`crate::html!`].
    ///
    /// Implementations must reject empty names, propagate errors and avoid
    /// implicitly flushing the destination. Output may be partial on failure.
    fn render_section_to_writer(&self, name: &str, _writer: &mut dyn io::Write) -> io::Result<()> {
        crate::section::validate_name(name)
    }

    /// Renders the template to an owned [`String`].
    fn render_to_string(&self) -> Result<String, RenderError> {
        let mut out = String::new();
        self.render(&mut out)?;
        Ok(out)
    }

    /// Renders the template and returns the UTF-8 encoded bytes.
    ///
    /// This is a straightforward `String` -> `Vec<u8>` conversion for now; a
    /// direct-to-bytes fast path (avoiding the UTF-16/UTF-8 non-issue that
    /// `String` sidesteps, but still avoiding an intermediate `String`
    /// allocation) is a possible future optimization. For synchronous output
    /// without buffering first, see [`Self::render_to_writer`]. For the
    /// buffered async destination bridge, see [`crate::render_async`].
    fn render_to_bytes(&self) -> Result<Vec<u8>, RenderError> {
        Ok(self.render_to_string()?.into_bytes())
    }
}

// Blanket impls so `&dyn Slice`, `Box<dyn Slice>`, etc. are themselves
// `Slice`s: useful groundwork for composing slices (layouts rendering a
// child slice, partials embedded in a parent template — see #4/#5), where a
// slice is held/passed around as a trait object rather than a concrete type.
impl<S: Slice + ?Sized> Slice for &S {
    fn render(&self, writer: &mut dyn fmt::Write) -> fmt::Result {
        (**self).render(writer)
    }

    fn render_to_writer(&self, writer: &mut dyn io::Write) -> io::Result<()> {
        (**self).render_to_writer(writer)
    }

    fn render_section_to_writer(&self, name: &str, writer: &mut dyn io::Write) -> io::Result<()> {
        (**self).render_section_to_writer(name, writer)
    }
}

impl<S: Slice + ?Sized> Slice for Box<S> {
    fn render(&self, writer: &mut dyn fmt::Write) -> fmt::Result {
        (**self).render(writer)
    }

    fn render_to_writer(&self, writer: &mut dyn io::Write) -> io::Result<()> {
        (**self).render_to_writer(writer)
    }

    fn render_section_to_writer(&self, name: &str, writer: &mut dyn io::Write) -> io::Result<()> {
        (**self).render_section_to_writer(name, writer)
    }
}

struct ByteWriter<'a> {
    writer: &'a mut dyn io::Write,
    error: Option<io::Error>,
}

impl fmt::Write for ByteWriter<'_> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        if self.error.is_some() {
            return Err(fmt::Error);
        }
        self.writer.write_all(text.as_bytes()).map_err(|error| {
            self.error = Some(error);
            fmt::Error
        })
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

    #[test]
    fn reference_to_slice_renders_like_the_slice() {
        let slice = Hello { name: "World" };
        let by_ref: &Hello = &slice;
        assert_eq!(by_ref.render_to_string().unwrap(), "<p>Hello, World!</p>");
    }

    #[test]
    fn boxed_slice_renders_like_the_slice() {
        let boxed: Box<dyn Slice> = Box::new(Hello { name: "World" });
        assert_eq!(boxed.render_to_string().unwrap(), "<p>Hello, World!</p>");
    }

    #[test]
    fn boxed_dyn_slice_can_be_passed_where_impl_slice_is_expected() {
        fn render_any(slice: &impl Slice) -> String {
            slice.render_to_string().unwrap()
        }

        let boxed: Box<dyn Slice> = Box::new(Hello { name: "World" });
        assert_eq!(render_any(&boxed), "<p>Hello, World!</p>");
    }
}
