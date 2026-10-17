use razor_slices::{async_template, RenderContext};
use tokio::sync::Notify;

pub struct Request<'a> {
    pub name: &'a str,
    pub entered: Notify,
    pub release: Notify,
}

async_template! {
    pub struct Header<'a> { model: &'a RenderContext<Request<'a>> } => {
        "<h1>" {model.request().name} "</h1>"
    }
}

async_template! {
    pub struct Child<'a> { model: &'a RenderContext<Request<'a>> } => {
        "<p>" {model.request().name} "</p>"
    }
}

async_template! {
    pub struct Page<'a> { model: &'a RenderContext<Request<'a>> } => {
        @{
            let provider = razor_slices::with_async_section(
                Child::new(model), "header", Header::new(model),
            )?;
        }
        "<article>"
        @section {provider}, {"header"};
        @flush;
        @{
            model.request().entered.notify_one();
        }
        @await { model.request().release.notified() };
        @render {Child::new(model)};
        "</article>"
    }
}
