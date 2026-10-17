use std::{io, sync::Arc};

use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Router,
};
use razor_slices::{
    async_template, AsyncHtmlWriter, AsyncRenderError, AsyncSlice, AsyncSliceWriter,
};
use razor_slices_axum::StreamingSliceResponse;
use tokio::sync::Notify;

pub struct PageModel {
    pub release: Arc<Notify>,
    pub title: String,
}

async_template! {
    pub struct Page { model: PageModel } => {
        "<p>Loading...</p>\n"
        @flush;
        @await { model.release.notified() };
        "<h1>" {&model.title} "</h1>\n"
    }
}

pub struct BrokenPage;

impl AsyncSlice for BrokenPage {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        output.write_literal("<p>Partial output</p>\n").await?;
        Err(io::Error::other("deliberate streaming failure").into())
    }
}

pub fn app(release: Arc<Notify>) -> Router {
    Router::new()
        .route(
            "/stream",
            get(|State(release): State<Arc<Notify>>| async move {
                StreamingSliceResponse::new(Page::new(PageModel {
                    release,
                    title: "<世界> & \"friends\" '雪'".into(),
                }))
            }),
        )
        .route(
            "/release",
            post(|State(release): State<Arc<Notify>>| async move {
                release.notify_one();
                StatusCode::NO_CONTENT
            }),
        )
        .route(
            "/error",
            get(|| async { StreamingSliceResponse::new(BrokenPage) }),
        )
        .with_state(release)
}
