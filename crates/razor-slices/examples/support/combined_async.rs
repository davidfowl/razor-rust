use std::{
    borrow::Borrow,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use razor_slices::{
    async_template, attribute, declare_async_layout, with_async_layout, with_async_section,
    AsyncHtmlWriter, AsyncLayout, AsyncRenderError, AsyncSlice, AsyncSliceWriter, HtmlString,
};
use tokio::sync::Notify;

// Shared acceptance fixtures are unused by the standalone HTTP server.
#[allow(dead_code)]
pub const PREFIX: &str = concat!(
    "<html><title>&lt;Combined&gt;</title><main><h1 data-count=\"2\">&lt;Combined&gt;</h1>",
    "<header checked=\"checked\" title=\"&lt;&amp;&quot;&#39;\">true<b>trusted</b>",
);
#[allow(dead_code)]
pub const PARTIAL: &str = concat!(
    "<article data-count=\"2\" empty=\"\">&lt;body&gt; &amp; 雪 -1.5 false",
    "<aside><b>trusted</b></aside></article>",
);
#[allow(dead_code)]
pub const PAGE: &str = concat!(
    "<html><title>&lt;Combined&gt;</title><main><h1 data-count=\"2\">&lt;Combined&gt;</h1>",
    "<header checked=\"checked\" title=\"&lt;&amp;&quot;&#39;\">true<b>trusted</b>",
    "&lt;Combined&gt;</header>",
    "<article data-count=\"2\" empty=\"\">&lt;body&gt; &amp; 雪 -1.5 false",
    "<aside><b>trusted</b></aside></article>",
    "</main><footer><a data-count=\"2\">&lt;Combined&gt;<b>trusted</b></a></footer></html>",
);

#[derive(Clone)]
pub struct Model {
    pub title: String,
    pub body: String,
    pub count: u32,
    pub ratio: f64,
    pub enabled: bool,
    pub note: Option<String>,
    pub trusted: HtmlString,
    pub release: Arc<Notify>,
    pub entered: Arc<AtomicUsize>,
    pub completed: Arc<AtomicUsize>,
    pub dropped: Arc<AtomicUsize>,
}

impl Model {
    pub fn demo() -> Self {
        Self {
            title: "<Combined>".into(),
            body: "<body> & 雪".into(),
            count: 2,
            ratio: -1.5,
            enabled: true,
            note: Some("<&\"'".into()),
            trusted: HtmlString::from("<b>trusted</b>"),
            release: Arc::new(Notify::new()),
            entered: Arc::new(AtomicUsize::new(0)),
            completed: Arc::new(AtomicUsize::new(0)),
            dropped: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn enter(&self) -> Work<'_> {
        self.entered.fetch_add(1, Ordering::SeqCst);
        Work(&self.dropped)
    }
}

struct Work<'a>(&'a AtomicUsize);
impl Drop for Work<'_> {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

async_template! {
    struct Badge<'a> { model: &'a Model } => { {model.trusted} }
}

struct Aside;
impl AsyncLayout for Aside {
    async fn render_layout_into<B: AsyncSlice, W: AsyncSliceWriter + Send>(
        &self,
        body: &B,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        output.write_literal("<aside>").await?;
        body.render_body_into(output).await?;
        output.write_literal("</aside>").await?;
        Ok(())
    }
}

async_template! {
    struct Body<M: Borrow<Model> + Sync> { model: M } => {
        @{ let model = model.borrow(); }
        "<article"
        {attribute("data-count", model.count).expect("static valid attribute")}
        {attribute("disabled", !model.enabled).expect("static valid attribute")}
        {attribute("none", None::<&str>).expect("static valid attribute")}
        {attribute("empty", Some("")).expect("static valid attribute")}
        ">" {model.body} " " {model.ratio} " " {false}
        @render { with_async_layout(Badge::new(model), Aside) };
        "</article>"
    }
}

async_template! {
    struct Header<M: Borrow<Model> + Sync> { model: M } => {
        @{ let model = model.borrow(); }
        "<header"
        {attribute("checked", model.enabled).expect("static valid attribute")}
        {attribute("title", &model.note).expect("static valid attribute")}
        ">" {model.enabled} {model.trusted}
        @flush;
        @{ let _work = model.enter(); }
        @await { model.release.notified() };
        @{ model.completed.fetch_add(1, Ordering::SeqCst); }
        {model.title} "</header>"
    }
}

async_template! {
    struct Links<M: Borrow<Model> + Sync> { model: M } => {
        @{ let model = model.borrow(); }
        "<a" {attribute("data-count", model.count).expect("static valid attribute")}
        ">" {model.title} {model.trusted} "</a>"
    }
}

async_template! {
    struct InnerView<'a, B: AsyncSlice> { model: (&'a Model, &'a B) } => {
        "<h1" {attribute("data-count", model.0.count).expect("static valid attribute")}
        ">" {model.0.title} "</h1>"
        @section { model.1 }, { "header" };
        @render { model.1 };
    }
}

async_template! {
    struct OuterView<'a, B: AsyncSlice> { model: (&'a Model, &'a B) } => {
        "<html><title>" {model.0.title} "</title><main>"
        @section { model.1 }, { "header" };
        @render { model.1 };
        "</main><footer>"
        @section { model.1 }, { "footer-links" };
        "</footer></html>"
    }
}

struct Inner<M>(M);
impl<M: Borrow<Model> + Sync> AsyncLayout for Inner<M> {
    async fn render_layout_into<B: AsyncSlice, W: AsyncSliceWriter + Send>(
        &self,
        body: &B,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        InnerView::new((self.0.borrow(), body))
            .render_into(output)
            .await
    }

    async fn render_layout_section_into<B: AsyncSlice, W: AsyncSliceWriter + Send>(
        &self,
        body: &B,
        name: &str,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        if name == "footer-links" {
            // The outer layout inserts this after the body; retain the original child.
            output.write_section(body, "links").await
        } else {
            output
                .write_section(&Badge::new(self.0.borrow()), name)
                .await
        }
    }
}

struct Outer<M>(M);
impl<M: Borrow<Model> + Sync> AsyncLayout for Outer<M> {
    async fn render_layout_into<B: AsyncSlice, W: AsyncSliceWriter + Send>(
        &self,
        body: &B,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        OuterView::new((self.0.borrow(), body))
            .render_into(output)
            .await
    }
}

pub fn page<M: Borrow<Model> + Clone + Sync>(model: M) -> impl AsyncSlice {
    let body = with_async_section(
        Body::new(model.clone()),
        "header",
        Header::new(model.clone()),
    )
    .expect("static nonempty section");
    let body = with_async_section(body, "links", Links::new(model.clone()))
        .expect("static nonempty section");
    declare_async_layout(
        body,
        declare_async_layout(Inner(model.clone()), Outer(model)),
    )
}

async_template! {
    pub struct Bare<'a, B: AsyncSlice> { model: &'a B } => {
        @render { model };
    }
}

async_template! {
    pub struct Partial<M: Borrow<Model> + Clone + Send + Sync> { model: M } => {
        @{ let document = page(model.clone()); }
        @render { Bare::new(&document) };
    }
}
