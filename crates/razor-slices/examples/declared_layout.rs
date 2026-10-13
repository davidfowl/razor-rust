#[path = "support/declared_layout.rs"]
mod page;

use razor_slices::{html, partial, Slice};
use std::io;

fn main() -> io::Result<()> {
    let model = page::sample_model();
    let document = page::greeting(&model)?;
    let mut writer = io::stdout().lock();
    document.render_to_writer(&mut writer)?;
    let bare = html! { "\nBare partial:\n" {partial(&document)} "\n" };
    bare.render_to_writer(&mut writer)
}
