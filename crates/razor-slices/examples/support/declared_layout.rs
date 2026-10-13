use razor_slices::{declare_layout, html, partial, section, with_section, Layout, Slice};
use std::io;

pub struct Model {
    pub title: String,
    pub name: String,
    pub footer: String,
}

struct Page<'a>(&'a Model);

impl Layout for Page<'_> {
    fn render_layout(&self, body: &dyn Slice, writer: &mut dyn io::Write) -> io::Result<()> {
        html! {
            "<!DOCTYPE html>\n<html><head><title>" {&self.0.title} "</title></head>\n"
            "<body><main>" {partial(body)} "</main>"
            {section(body, "footer")} "</body></html>\n"
        }
        .render_to_writer(writer)
    }
}

pub fn greeting(model: &Model) -> io::Result<impl Slice + '_> {
    let body = with_section(
        html! { "<p>Hello, " {&model.name} "!</p>" },
        "footer",
        html! { "<footer>" {&model.footer} "</footer>" },
    )?;
    Ok(declare_layout(body, Page(model)))
}

pub fn sample_model() -> Model {
    Model {
        title: "Greetings <today>".into(),
        name: "David & friends".into(),
        footer: "After the body".into(),
    }
}
