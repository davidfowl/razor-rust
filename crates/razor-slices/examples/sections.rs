#[path = "support/sections.rs"]
mod page;
#[path = "support/todos.rs"]
mod todos;

use razor_slices::Slice;
use std::io;

fn main() -> io::Result<()> {
    let model = page::sample_model();
    let todos = todos::sample_models();
    let document = page::todos_document(&model, &todos)?;
    document.render_to_writer(&mut io::stdout().lock())
}
