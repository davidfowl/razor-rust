#[path = "support/streaming.rs"]
mod sample;

use std::sync::Arc;
use tokio::sync::Notify;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_target(true)
        .init();
    let address = match std::env::var("BIND_ADDRESS") {
        Ok(address) => address,
        Err(std::env::VarError::NotPresent) => "127.0.0.1:3000".into(),
        Err(error) => return Err(error.into()),
    };
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("LISTENING http://{}", listener.local_addr()?);
    axum::serve(listener, sample::app(Arc::new(Notify::new()))).await?;
    Ok(())
}
