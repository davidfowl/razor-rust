use razor_slices::{html, partial, with_layout, HtmlString, Layout, Slice};
use std::io;

use super::todos::{todos_page, Todo};

pub struct LayoutModel {
    pub title: String,
}

pub struct PageLayout<'a> {
    pub model: &'a LayoutModel,
}

impl Layout for PageLayout<'_> {
    fn render_layout(&self, body: &dyn Slice, writer: &mut dyn io::Write) -> io::Result<()> {
        html! {
            "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"utf-8\">\n  <title>"
            {&self.model.title}
            "</title>\n</head>\n<body>\n<main>\n"
            {partial(body)}
            "</main>\n"
            {HtmlString::from("<footer>Layout footer.</footer>\n")}
            "</body>\n</html>\n"
        }
        .render_to_writer(writer)
    }
}

pub fn todos_document<'a>(model: &'a LayoutModel, todos: &'a [Todo]) -> impl Slice + 'a {
    with_layout(todos_page(todos), PageLayout { model })
}
