use razor_slices::{html, partial, InlineSlice, Slice, WriteHtmlValue};
use std::io;

pub struct Todo {
    pub id: u32,
    pub title: String,
    pub due_by: Option<String>,
    pub is_complete: bool,
}

pub fn todo_row(model: &Todo) -> impl Slice + '_ {
    html! {
        "<tr>\n  <td>" {model.id} "</td>\n  <td>" {&model.title} "</td>\n  <td>"
        {model.due_by.as_deref().unwrap_or("")} "</td>\n  <td>"
        {if model.is_complete { "Completed" } else { "Incomplete" }}
        "</td>\n</tr>\n"
    }
}

pub fn todos_page(models: &[Todo]) -> impl Slice + '_ {
    InlineSlice::new(move |writer: &mut dyn io::Write| {
        html! {
            "<h1>Todos (" {models.len()} ")</h1>\n<table>\n"
            "<tr><th>Id</th><th>Title</th><th>Due by</th><th>Status</th></tr>\n"
        }
        .render_to_writer(writer)?;
        for model in models {
            partial(todo_row(model)).write_html_to(writer)?;
        }
        html! { "</table>\n" }.render_to_writer(writer)
    })
}

pub fn sample_models() -> Vec<Todo> {
    vec![
        Todo {
            id: 1,
            title: "Wash <dishes> & \"cups\"".into(),
            due_by: None,
            is_complete: true,
        },
        Todo {
            id: 2,
            title: "Call 'Grandma' 世界".into(),
            due_by: Some("2026-10-14".into()),
            is_complete: false,
        },
    ]
}
