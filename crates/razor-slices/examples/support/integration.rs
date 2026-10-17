use razor_slices::{
    attribute, declare_layout, html, partial, section, with_section, Layout, Slice,
};
use std::io;

use crate::todos::{sample_models, Todo};

pub struct Model {
    pub title: String,
    pub footer: String,
    pub todos: Vec<Todo>,
}

struct Page<'a>(&'a str);

impl Layout for Page<'_> {
    fn render_layout(&self, body: &dyn Slice, writer: &mut dyn io::Write) -> io::Result<()> {
        html! {
            "<!doctype html>\n<html><head><title>" {self.0}
            "</title></head><body>\n"
            {partial(body)}
            {section(body, "footer")}
            {section(body, "optional")}
            "</body></html>\n"
        }
        .render_to_writer(writer)
    }
}

pub fn page(model: &Model) -> io::Result<impl Slice + '_> {
    let body = html! {
        "<ul>\n"
        for todo in &model.todos {
            "<li"
            {attribute("data-title", &todo.title).map_err(io::Error::other)?}
            {attribute("data-due", todo.due_by.as_deref()).map_err(io::Error::other)?}
            "><input type=\"checkbox\""
            {attribute("checked", todo.is_complete).map_err(io::Error::other)?}
            ">"
            if todo.is_complete { "<s>" {&todo.title} "</s>" }
            else { {&todo.title} }
            "</li>\n"
        }
        "</ul>\n"
    };
    let body = with_section(
        body,
        "footer",
        html! { "<footer>" {&model.footer} "</footer>\n" },
    )?;
    Ok(declare_layout(body, Page(&model.title)))
}

pub fn sample_model() -> Model {
    Model {
        title: "Tasks & sections".into(),
        footer: "<done> & reusable".into(),
        todos: sample_models(),
    }
}
