//! Minimal "Hello from Razor Slices" sample, equivalent to upstream's
//! Getting Started sample. Run with:
//!
//! ```shell
//! cargo run -p razor-slices --example hello
//! ```
//!
//! This hand-implements the [`Slice`] trait directly, since template
//! syntax/codegen (see <https://github.com/davidfowl/razor-rust/issues/3>)
//! isn't implemented yet. Once that lands, this sample should be rewritten
//! to author the template as a `.rshtml`-equivalent file instead. Tracked in
//! <https://github.com/davidfowl/razor-rust/issues/8>.

use razor_slices::{encode_html_to, Slice};
use std::fmt;

/// A simple greeting template, taking a `name` "model" value.
struct Hello<'a> {
    name: &'a str,
}

impl Slice for Hello<'_> {
    fn render(&self, writer: &mut dyn fmt::Write) -> fmt::Result {
        writer.write_str("<!DOCTYPE html>\n<html>\n<body>\n  <p>Hello, ")?;
        // Interpolated values are HTML-encoded, matching upstream's `@expr`
        // auto-encoding behavior.
        encode_html_to(self.name, writer)?;
        writer.write_str("!</p>\n</body>\n</html>\n")
    }
}

fn main() {
    let slice = Hello { name: "World" };
    let html = slice.render_to_string().expect("rendering should not fail");
    print!("{html}");
}
