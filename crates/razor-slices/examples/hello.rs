//! Minimal "Hello from Razor Slices" sample, equivalent to upstream's
//! Getting Started sample. Run with:
//!
//! ```shell
//! cargo run -p razor-slices --example hello
//! ```
//!
//! Uses the first inline codegen milestone. See `INLINE.md` for the initial
//! grammar, capture behavior, and remaining gaps.

use razor_slices::{html, Slice};

fn hello(name: &str) -> impl Slice + '_ {
    html! {
        "<!DOCTYPE html>\n<html>\n<body>\n  <p>Hello, " {name} "!</p>\n</body>\n</html>\n"
    }
}

fn main() {
    let slice = hello("World");
    let html = slice.render_to_string().expect("rendering should not fail");
    print!("{html}");
}
