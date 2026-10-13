use std::sync::atomic::{AtomicUsize, Ordering};

use razor_slices::{
    async_template, declare_async_layout, with_async_section, AsyncHtmlWriter, AsyncLayout,
    AsyncRenderError, AsyncSlice, AsyncSliceWriter,
};
use tokio::sync::Notify;

pub const PREFIX: &str = "<html><title>&lt;Title&gt;</title><main><h1>&lt;Title&gt;</h1><header>";
pub const PAGE: &str = concat!(
    "<html><title>&lt;Title&gt;</title><main><h1>&lt;Title&gt;</h1><header>",
    "&lt;Title&gt;</header><p>&lt;body&gt; &amp; 雪</p></main>",
    "<footer>&lt;inner links&gt;</footer></html>",
);
pub const PARTIAL: &str = "<p>&lt;body&gt; &amp; 雪</p>";

pub struct Model<'a> {
    pub title: &'a str,
    pub body: &'a str,
    pub release: &'a Notify,
    pub sections: &'a AtomicUsize,
}

async_template! {
    pub struct Text<'a> { model: &'a str } => { {model} }
}

async_template! {
    pub struct PageBody<'a> { model: &'a str } => { "<p>" {model} "</p>" }
}

async_template! {
    pub struct Header<'a> { model: &'a Model<'a> } => {
        "<header>"
        @flush;
        @await { model.release.notified() };
        @{ model.sections.fetch_add(1, Ordering::SeqCst); }
        {model.title} "</header>"
    }
}

async_template! {
    struct InnerView<'a, B: AsyncSlice> { model: (&'a str, &'a B) } => {
        "<h1>" {model.0} "</h1>"
        @section { model.1 }, { "header" };
        @render { model.1 };
        @section { model.1 }, { "absent" };
    }
}

async_template! {
    struct OuterView<'a, B: AsyncSlice> { model: (&'a str, &'a B) } => {
        "<html><title>" {model.0} "</title><main>"
        @render { model.1 };
        "</main><footer>"
        @section { model.1 }, { "footer-links" };
        "</footer></html>"
    }
}

pub struct Inner<'a>(pub &'a str);

impl AsyncLayout for Inner<'_> {
    async fn render_layout_into<B: AsyncSlice, W: AsyncSliceWriter + Send>(
        &self,
        body: &B,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        InnerView::new((self.0, body)).render_into(output).await
    }
}

pub struct Outer<'a>(pub &'a str);

impl AsyncLayout for Outer<'_> {
    async fn render_layout_into<B: AsyncSlice, W: AsyncSliceWriter + Send>(
        &self,
        body: &B,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        OuterView::new((self.0, body)).render_into(output).await
    }
}

pub fn page<'a>(model: &'a Model<'a>) -> impl AsyncSlice + 'a {
    let body = with_async_section(PageBody::new(model.body), "header", Header::new(model))
        .expect("static nonempty header section name");
    let inner = with_async_section(
        Inner(model.title),
        "footer-links",
        Text::new("<inner links>"),
    )
    .expect("static nonempty footer section name");
    declare_async_layout(body, declare_async_layout(inner, Outer(model.title)))
}

async_template! {
    pub struct Bare<'a, B: AsyncSlice> { model: &'a B } => {
        @render { model };
    }
}
