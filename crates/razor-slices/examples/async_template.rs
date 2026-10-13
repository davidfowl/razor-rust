#[path = "support/async_template.rs"]
mod support;

use std::sync::atomic::{AtomicUsize, Ordering};

use razor_slices::{render_incremental, TokioWriter};
use support::{PageModel, Todo, TodoPage};
use tokio::{io::AsyncReadExt, sync::Notify};

const PREFIX: &str = "<h1>&lt;Todos&gt;</h1>\n";
const PAGE: &str = concat!(
    "<h1>&lt;Todos&gt;</h1>\n",
    "<p>&lt;loaded&gt;</p><ul><li>Read &amp; learn</li>",
    "<li><s>Ship &lt;Rust&gt;</s><small>&quot;done&quot;</small></li></ul>\n",
);

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let release = Notify::new();
    let loads = AtomicUsize::new(0);
    let todos = [
        Todo {
            title: "Read & learn",
            done: false,
            note: None,
        },
        Todo {
            title: "Ship <Rust>",
            done: true,
            note: Some("\"done\""),
        },
    ];
    let page = TodoPage::new(PageModel {
        title: "<Todos>",
        todos: &todos,
        release: &release,
        loads: &loads,
    });

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
