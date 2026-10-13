use razor_slices::{partial, Slice, WriteHtmlValue};
use std::io::{self, Write};

#[path = "support/integration.rs"]
mod integration;
#[path = "support/todos.rs"]
#[allow(dead_code)]
mod todos;

fn main() -> io::Result<()> {
    let model = integration::sample_model();
    let page = integration::page(&model)?;
    let mut output = io::stdout().lock();
    page.render_to_writer(&mut output)?;
    output.write_all(b"<!-- partial -->\n")?;
    partial(&page).write_html_to(&mut output)
}
