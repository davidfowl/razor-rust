//! Explicit lazy named sections for synchronous layouts.

use std::{fmt, io};

use crate::{RenderError, Slice, WriteHtmlValue};

/// A lazy named-section insertion for [`crate::html!`].
///
/// Construct with [`section`]. Owns its slice (or a reference) and borrows the
/// name. Each write dispatches the section anew without buffering or encoding
/// the section's markup a second time.
pub struct Section<'a, S> {
    slice: S,
    name: &'a str,
}

/// Inserts a slice's named section into HTML output.
///
/// The name is validated when rendered, including for unknown sections.
/// Empty names fail with [`io::ErrorKind::InvalidInput`]; unknown nonempty names
/// write nothing. Section templates remain responsible for encoding values.
///
/// ```
/// use razor_slices::{html, section, with_section, Slice};
/// let body = with_section(
///     html! { "<main>Body</main>" },
///     "footer",
///     html! { "<footer>" {"<David>"} "</footer>" },
/// )?;
/// let page = html! { {section(&body, "footer")} {section(&body, "absent")} };
/// assert_eq!(page.render_to_string()?, "<footer>&lt;David&gt;</footer>");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// Arbitrary strings are not section providers:
///
/// ```compile_fail
/// use razor_slices::section;
/// let _ = section("not a slice", "footer");
/// ```
pub fn section<S: Slice>(slice: S, name: &str) -> Section<'_, S> {
    Section { slice, name }
}

impl<S: Slice> WriteHtmlValue for Section<'_, S> {
    fn write_html_to(&self, writer: &mut dyn io::Write) -> io::Result<()> {
        validate_name(self.name)?;
        self.slice.render_section_to_writer(self.name, writer)
    }
}

/// Associates a named section with a body, without executing either template.
///
/// Ordinary rendering is forwarded unchanged to the body. Section rendering
/// writes the associated slice only when the requested name matches exactly,
/// otherwise delegating to the body's sections. Associations can be chained;
/// the outermost association wins when names repeat. Repeated insertion renders
/// the section again, with no caching or exactly-once restriction.
///
/// Owns both slices (or their references) and the name. Empty names fail at
/// construction with [`io::ErrorKind::InvalidInput`]. No `@section` syntax,
/// implicit layout discovery or automatic nested-layout context is introduced.
///
/// ```
/// use razor_slices::{html, with_section, Slice};
/// let body = with_section(html! { "<p>Body</p>" }, "header", html! { "<h1>Title</h1>" })?;
/// assert_eq!(body.render_to_string()?, "<p>Body</p>");
/// let mut bytes = Vec::new();
/// body.render_section_to_writer("header", &mut bytes)?;
/// assert_eq!(bytes, b"<h1>Title</h1>");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// A section must be a slice, not arbitrary trusted text:
///
/// ```compile_fail
/// use razor_slices::{html, with_section};
/// let _ = with_section(html! {}, "footer", "<b>not a slice</b>");
/// ```
///
/// Borrowed section models cannot outlive their referents:
///
/// ```compile_fail
/// use razor_slices::{html, with_section, Slice};
/// let body;
/// {
///     let title = String::from("Title");
///     let borrowed = &title;
///     body = with_section(html! {}, "header", html! { {borrowed} }).unwrap();
/// }
/// let _ = body.render_to_string();
/// ```
pub fn with_section<S: Slice, T: Slice>(
    body: S,
    name: impl Into<String>,
    content: T,
) -> io::Result<impl Slice> {
    let name = name.into();
    validate_name(&name)?;
    Ok(Sectioned {
        body,
        name,
        content,
    })
}

struct Sectioned<S, T> {
    body: S,
    name: String,
    content: T,
}

impl<S: Slice, T: Slice> Slice for Sectioned<S, T> {
    fn render(&self, writer: &mut dyn fmt::Write) -> fmt::Result {
        self.body.render(writer)
    }

    fn render_to_writer(&self, writer: &mut dyn io::Write) -> io::Result<()> {
        self.body.render_to_writer(writer)
    }

    fn render_section_to_writer(&self, name: &str, writer: &mut dyn io::Write) -> io::Result<()> {
        validate_name(name)?;
        if name == self.name {
            self.content.render_to_writer(writer)
        } else {
            self.body.render_section_to_writer(name, writer)
        }
    }

    fn render_to_string(&self) -> Result<String, RenderError> {
        self.body.render_to_string()
    }

    fn render_to_bytes(&self) -> Result<Vec<u8>, RenderError> {
        self.body.render_to_bytes()
    }
}

pub(crate) fn validate_name(name: &str) -> io::Result<()> {
    if name.is_empty() {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "section name must not be empty",
        ))
    } else {
        Ok(())
    }
}
