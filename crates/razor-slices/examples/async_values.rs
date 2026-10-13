#[path = "support/async_values.rs"]
mod support;

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use razor_slices::{render_incremental, TokioWriter};
use support::{ValueModel, ValuePage, PAGE, PREFIX};
use tokio::{io::AsyncReadExt, sync::Notify};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let release = Arc::new(Notify::new());
    let loads = Arc::new(AtomicUsize::new(0));
    let page = ValuePage::new(ValueModel::demo(release.clone(), loads.clone()));
    for round in 0..2 {
        let (mut sender, mut receiver) = tokio::io::duplex(8);
        let render = async { render_incremental(&page, &mut TokioWriter::new(&mut sender)).await };
        let receive = async {
            let mut prefix = vec![0; PREFIX.len()];
            receiver.read_exact(&mut prefix).await?;
            assert_eq!(prefix, PREFIX.as_bytes());
            assert_eq!(loads.load(Ordering::SeqCst), round);
            println!("prefix-before-await: {}", PREFIX.trim_end());
            release.notify_one();
            let mut suffix = vec![0; PAGE.len() - PREFIX.len()];
            receiver.read_exact(&mut suffix).await?;
            prefix.extend(suffix);
            assert_eq!(prefix, PAGE.as_bytes());
            Ok::<_, std::io::Error>(())
        };
        let (rendered, received) = tokio::join!(render, receive);
        rendered?;
        received?;
        assert_eq!(loads.load(Ordering::SeqCst), round + 1);
    }
    print!("page: {PAGE}");
    println!("renders: {}", loads.load(Ordering::SeqCst));
    Ok(())
}
