#[path = "../../razor-slices/examples/support/automatic_flush.rs"]
mod support;

use std::sync::{atomic::AtomicUsize, Arc};

use axum::{
    routing::{get, post},
    Router,
};
use razor_slices_axum::StreamingSliceResponse;
use support::{section_page, Model, SectionPage};
use tokio::sync::Notify;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let release = Arc::new(Notify::new());
    let completed = Arc::new(AtomicUsize::new(0));
    let render_release = release.clone();
    let app = Router::new()
        .route(
            "/stream",
            get(move || {
                let page = SectionPage::new(section_page(Model::new(
                    render_release.clone(),
                    completed.clone(),
                )));
                async move { StreamingSliceResponse::new(page) }
            }),
        )
        .route(
            "/release",
            post(move || {
                let release = release.clone();
                async move {
                    release.notify_one();
                    "released\n"
                }
            }),
        );
    let address = std::env::var("BIND_ADDRESS").unwrap_or_else(|_| "127.0.0.1:3000".into());
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("http://{}", listener.local_addr()?);
    println!(
        "Completed child publishes {} prefix bytes, then holds before {}",
        support::PREFIX_BYTES,
        support::SUFFIX.trim_end()
    );
    axum::serve(listener, app).await?;
    Ok(())
}
