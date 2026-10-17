#[path = "support/automatic_flush.rs"]
mod support;

use std::sync::{atomic::Ordering, Arc};

use razor_slices::{render_incremental, TokioWriter};
use support::{section_page, Model, Page, SectionPage, PREFIX_BYTES, SUFFIX};
use tokio::{io::AsyncReadExt, sync::Notify};

async fn receive(
    receiver: &mut (impl tokio::io::AsyncRead + Unpin),
    release: &Notify,
) -> std::io::Result<()> {
    let mut prefix = vec![0; PREFIX_BYTES];
    receiver.read_exact(&mut prefix).await?;
    assert_eq!(prefix, vec![b'x'; PREFIX_BYTES]);
    println!("automatic-prefix-before-release: {} bytes", prefix.len());
    release.notify_one();
    let mut suffix = vec![0; SUFFIX.len()];
    receiver.read_exact(&mut suffix).await?;
    prefix.extend(suffix);
    assert_eq!(
        prefix,
        format!("{}{SUFFIX}", "x".repeat(PREFIX_BYTES)).as_bytes()
    );
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let release = Arc::new(Notify::new());
    let completed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let page = Page::new(Model::new(release.clone(), completed.clone()));
    for _ in 0..2 {
        let (sender, mut receiver) = tokio::io::duplex(32);
        // The sink buffers beyond the threshold, so read_exact requires the
        // completed child's automatic flush, not merely write acceptance.
        let mut writer = TokioWriter::new(tokio::io::BufWriter::with_capacity(65_536, sender));
        let (rendered, received) = tokio::join!(
            render_incremental(&page, &mut writer),
            receive(&mut receiver, &release),
        );
        rendered?;
        received?;
    }
    let section = SectionPage::new(section_page(Model::new(release.clone(), completed.clone())));
    let (sender, mut receiver) = tokio::io::duplex(32);
    let mut writer = TokioWriter::new(tokio::io::BufWriter::with_capacity(65_536, sender));
    let (rendered, received) = tokio::join!(
        render_incremental(&section, &mut writer),
        receive(&mut receiver, &release),
    );
    rendered?;
    received?;
    assert_eq!(completed.load(Ordering::SeqCst), 3);
    println!("exact-suffix: {}", SUFFIX.trim_end());
    println!("completed-renders: {}", completed.load(Ordering::SeqCst));
    Ok(())
}
