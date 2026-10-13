//! Run with `cargo run -p razor-slices-axum --example server`.

#[path = "support/page.rs"]
mod page;

use axum::{routing::get, Router};
use page::{greeting_page, BrokenPage, Greeting};
use razor_slices::HtmlString;
use razor_slices_axum::SliceResponse;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_target(true)
        .init();
    let app = Router::new()
        .route(
            "/",
            get(|| async {
                SliceResponse::new(greeting_page(Greeting {
                    name: "<世界> & \"friends\" '雪'",
                    trusted_body: HtmlString::from("<strong>Trusted & ready</strong>"),
                }))
            }),
        )
        .route("/error", get(|| async { SliceResponse::new(BrokenPage) }));
    let address = match std::env::var("BIND_ADDRESS") {
        Ok(address) => address,
        Err(std::env::VarError::NotPresent) => "127.0.0.1:3000".into(),
        Err(error) => return Err(error.into()),
    };
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("LISTENING http://{}", listener.local_addr()?);
    axum::serve(listener, app).await?;
    Ok(())
}
