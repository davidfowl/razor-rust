use razor_slices::{html, partial, section, with_layout, with_section, Layout, Slice};
use std::io;

use super::todos::{todos_page, Todo};

pub struct PageModel {
    pub title: String,
    pub header: String,
    pub footer: String,
}

pub struct SectionLayout<'a> {
    pub model: &'a PageModel,
}

impl Layout for SectionLayout<'_> {
    fn render_layout(&self, body: &dyn Slice, writer: &mut dyn io::Write) -> io::Result<()> {
        html! {
            "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"utf-8\">\n"
            "  <title>" {&self.model.title} "</title>\n</head>\n<body>\n"
            {section(body, "header")}
            "<main>\n" {partial(body)} "</main>\n"
            {section(body, "footer")}
            {section(body, "optional-scripts")}
            "</body>\n</html>\n"
        }
        .render_to_writer(writer)
    }
}

pub fn todos_document<'a>(model: &'a PageModel, todos: &'a [Todo]) -> io::Result<impl Slice + 'a> {
    let body = with_section(
        todos_page(todos),
        "header",
        html! { "<header><p>" {&model.header} "</p></header>\n" },
    )?;
    let body = with_section(
        body,
        "footer",
        html! { "<footer>" {&model.footer} "</footer>\n" },
    )?;
    Ok(with_layout(body, SectionLayout { model }))
}

pub fn sample_model() -> PageModel {
    PageModel {
        title: "Todos <sections> & 世界".into(),
        header: "Lazy <header> & \"values\"".into(),
        footer: "Footer 'after' the body".into(),
    }
}
