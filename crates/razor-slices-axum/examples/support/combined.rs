#[path = "../../../razor-slices/examples/support/combined_async.rs"]
pub mod sample;

use axum::{
    http::StatusCode,
    routing::{get, post},
    Router,
};
use razor_slices_axum::StreamingSliceResponse;

pub fn app(model: sample::Model) -> Router {
    let document = model.clone();
    let partial = model.clone();
    Router::new()
        .route(
            "/combined",
            get(move || {
                let page = sample::page(document.clone());
                async move { StreamingSliceResponse::new(page) }
            }),
        )
        .route(
            "/partial",
            get(move || {
                let page = sample::Partial::new(partial.clone());
                async move { StreamingSliceResponse::new(page) }
            }),
        )
        .route(
            "/release",
            post(move || {
                let release = model.release.clone();
                async move {
                    release.notify_one();
                    StatusCode::NO_CONTENT
                }
            }),
        )
}
