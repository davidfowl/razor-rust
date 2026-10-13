#[path = "support/async_layout.rs"]
mod support;

use std::sync::atomic::{AtomicUsize, Ordering};

use razor_slices::{render_incremental, TokioWriter};
use support::{page, Bare, Model, PAGE, PARTIAL, PREFIX};
use tokio::{io::AsyncReadExt, sync::Notify};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let release = Notify::new();
    let sections = AtomicUsize::new(0);
    let title = String::from("<Title>");
    let body = String::from("<body> & 雪");
    let model = Model {
        title: &title,
        body: &body,
        release: &release,
        sections: &sections,
    };
    let document = page(&model);

    for round in 0..2 {
        let (mut sender, mut receiver) = tokio::io::duplex(1);
        let render =
            async { render_incremental(&document, &mut TokioWriter::new(&mut sender)).await };
        let receive = async {
            let mut bytes = vec![0; PREFIX.len()];
            receiver.read_exact(&mut bytes).await?;
            assert_eq!(bytes, PREFIX.as_bytes());
            assert_eq!(sections.load(Ordering::SeqCst), round);
            println!("prefix-before-section-await: {PREFIX}");
            release.notify_one();
            let mut suffix = vec![0; PAGE.len() - PREFIX.len()];
            receiver.read_exact(&mut suffix).await?;
            bytes.extend(suffix);
            assert_eq!(bytes, PAGE.as_bytes());
            Ok::<_, std::io::Error>(())
        };
        let (rendered, received) = tokio::join!(render, receive);
        rendered?;
        received?;
        assert_eq!(sections.load(Ordering::SeqCst), round + 1);
    }

    let mut bytes = Vec::new();
    render_incremental(&Bare::new(&document), &mut TokioWriter::new(&mut bytes)).await?;
    assert_eq!(bytes, PARTIAL.as_bytes());
    assert_eq!(sections.load(Ordering::SeqCst), 2);
    println!("document: {PAGE}");
    println!("partial: {PARTIAL}");
    println!("section-renders: {}", sections.load(Ordering::SeqCst));
    Ok(())
}
