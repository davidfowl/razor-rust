#[path = "support/combined_async.rs"]
mod sample;

use std::sync::atomic::Ordering;

use razor_slices::{render_incremental, AsyncSlice, TokioWriter};
use tokio::io::AsyncReadExt;

async fn observe(
    page: &impl AsyncSlice,
    model: &sample::Model,
) -> Result<(), Box<dyn std::error::Error>> {
    for round in 0..2 {
        let (mut sender, mut receiver) = tokio::io::duplex(1);
        let render = async { render_incremental(page, &mut TokioWriter::new(&mut sender)).await };
        let receive = async {
            let mut bytes = vec![0; sample::PREFIX.len()];
            receiver.read_exact(&mut bytes).await?;
            assert_eq!(bytes, sample::PREFIX.as_bytes());
            assert_eq!(model.completed.load(Ordering::SeqCst), round);
            println!("prefix-before-release: {}", sample::PREFIX);
            model.release.notify_one();
            let mut suffix = vec![0; sample::PAGE.len() - bytes.len()];
            receiver.read_exact(&mut suffix).await?;
            bytes.extend(suffix);
            assert_eq!(bytes, sample::PAGE.as_bytes());
            Ok::<_, std::io::Error>(())
        };
        let (rendered, received) = tokio::join!(render, receive);
        rendered?;
        received?;
        assert_eq!(model.completed.load(Ordering::SeqCst), round + 1);
    }
    let mut bytes = Vec::new();
    render_incremental(
        &sample::Bare::new(&Box::new(page)),
        &mut TokioWriter::new(&mut bytes),
    )
    .await?;
    assert_eq!(bytes, sample::PARTIAL.as_bytes());
    bytes.clear();
    render_incremental(
        &sample::Partial::new(model),
        &mut TokioWriter::new(&mut bytes),
    )
    .await?;
    assert_eq!(bytes, sample::PARTIAL.as_bytes());
    assert_eq!(model.entered.load(Ordering::SeqCst), 2);
    assert_eq!(model.completed.load(Ordering::SeqCst), 2);
    assert_eq!(model.dropped.load(Ordering::SeqCst), 2);
    println!("document: {}", sample::PAGE);
    println!("partial: {}", sample::PARTIAL);
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let borrowed = sample::Model::demo();
    println!("borrowed-model:");
    observe(&sample::page(&borrowed), &borrowed).await?;
    let owned = sample::Model::demo();
    println!("owned-model:");
    observe(&sample::page(owned.clone()), &owned).await?;
    Ok(())
}
