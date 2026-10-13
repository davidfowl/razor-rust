use std::{
    io,
    sync::atomic::{AtomicUsize, Ordering},
};

use razor_slices::async_template;
use tokio::sync::Notify;

pub struct Todo<'a> {
    pub title: &'a str,
    pub done: bool,
    pub note: Option<&'a str>,
}

pub struct PageModel<'a> {
    pub title: &'a str,
    pub todos: &'a [Todo<'a>],
    pub release: &'a Notify,
    pub loads: &'a AtomicUsize,
}

async fn load_status(model: &PageModel<'_>) -> io::Result<String> {
    model.release.notified().await;
    model.loads.fetch_add(1, Ordering::SeqCst);
    Ok(String::from("<loaded>"))
}

async_template! {
    pub struct TodoPage<'a> { model: PageModel<'a> } => {
        "<h1>" {model.title} "</h1>\n"
        @flush;
        @{ let status = load_status(model).await?; }
        "<p>" {status} "</p><ul>"
        for todo in model.todos {
            "<li>"
            if todo.done { "<s>" {todo.title} "</s>" }
            else { {todo.title} }
            if let Some(note) = todo.note {
                "<small>" {note} "</small>"
            }
            "</li>"
        }
        "</ul>\n"
    }
}
