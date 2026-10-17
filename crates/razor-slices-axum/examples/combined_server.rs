#[path = "support/combined.rs"]
mod combined;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .init();
    let address = match std::env::var("BIND_ADDRESS") {
        Ok(address) => address,
        Err(std::env::VarError::NotPresent) => "127.0.0.1:3000".into(),
        Err(error) => return Err(error.into()),
    };
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("LISTENING http://{}", listener.local_addr()?);
    axum::serve(listener, combined::app(combined::sample::Model::demo())).await?;
    Ok(())
}
