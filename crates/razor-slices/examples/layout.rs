//! Run: cargo run -p razor-slices --example layout

#[path = "support/layout.rs"]
mod page;
#[path = "support/todos.rs"]
mod todos;

use razor_slices::Slice;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let todos = todos::sample_models();
    let model = page::LayoutModel {
        title: "Todos <today> & \"tomorrow\" '世界'".into(),
    };
    let document = page::todos_document(&model, &todos);
    assert_eq!(document.render_to_string()?, document.render_to_string()?);
    document.render_to_writer(&mut std::io::stdout().lock())?;
    Ok(())
}
