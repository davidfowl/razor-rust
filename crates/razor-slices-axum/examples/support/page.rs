use std::{fmt, io};

use razor_slices::{html, partial, HtmlString, Slice};

pub struct Greeting<'a> {
    pub name: &'a str,
    pub trusted_body: HtmlString,
}

pub fn greeting_page(model: Greeting<'_>) -> impl Slice + '_ {
    let greeting = html! { "<p>Hello, " {model.name} "!</p>" };
    html! {
        "<!DOCTYPE html>\n<html><body>\n"
        {partial(&greeting)}
        "\n"
        {model.trusted_body}
        "\n</body></html>\n"
    }
}

pub struct BrokenPage;

impl Slice for BrokenPage {
    fn render(&self, writer: &mut dyn fmt::Write) -> fmt::Result {
        writer.write_str("<p>THIS MUST NOT ESCAPE</p>")?;
        Err(fmt::Error)
    }

    fn render_to_writer(&self, writer: &mut dyn io::Write) -> io::Result<()> {
        writer.write_all(b"<p>THIS MUST NOT ESCAPE</p>")?;
        Err(io::Error::other("deliberate example rendering failure"))
    }
}
