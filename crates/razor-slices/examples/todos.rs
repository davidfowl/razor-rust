//! Run: cargo run -p razor-slices --example todos

#[path = "support/todos.rs"]
mod templates;

use razor_slices::{html, partial, HtmlString, Slice};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let models = templates::sample_models();
    let page = templates::todos_page(&models);
    let output = html! {
        {partial(&page)}
        {HtmlString::from("<footer>Rendered with reusable partials.</footer>\n")}
    };
    let first = output.render_to_string()?;
    let second = output.render_to_string()?;
    assert_eq!(first, second);
    output.render_to_writer(&mut std::io::stdout().lock())?;
    Ok(())
}
