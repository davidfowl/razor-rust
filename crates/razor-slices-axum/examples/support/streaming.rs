use std::{io, sync::Arc};

use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Router,
};
use razor_slices::{AsyncHtmlWriter, AsyncRenderError, AsyncSlice, AsyncSliceWriter};
use razor_slices_axum::StreamingSliceResponse;
use tokio::sync::Notify;

pub struct Page {
    pub release: Arc<Notify>,
    pub title: String,
}

impl AsyncSlice for Page {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        output.write_literal("<p>Loading...</p>\n").await?;
        output.flush().await?;
        self.release.notified().await;
        output.write_literal("<h1>").await?;
        output.write_value(&self.title).await?;
        output.write_literal("</h1>\n").await?;
        Ok(())
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
                StreamingSliceResponse::new(Page {
                    release,
                    title: "<世界> & \"friends\" '雪'".into(),
                })
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
