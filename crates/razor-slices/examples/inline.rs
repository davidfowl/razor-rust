//! Run: cargo run -p razor-slices --example inline

use razor_slices::{html, HtmlString, Slice};

fn greeting(name: &str) -> impl Slice + '_ {
    html! { "<p>Hello, " {name} "!</p>" }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let greeting = greeting("Damian & David");
    println!("{}", greeting.render_to_string()?);
    println!("{}", greeting.render_to_string()?);

    let raw = HtmlString::from("<strong>trusted HTML</strong>");
    let page = html! {
        r#"<div title=""# {"\"<&'"} r#"">"# {"Hello, 世界"} " " {42} " " {true} " " {raw} "</div>"
    };
    println!("{}", page.render_to_string()?);
    Ok(())
}
