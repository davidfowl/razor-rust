use razor_slices::{
    render_incremental_with_context, AsyncRenderError, CancellationToken, RenderCancelled,
    RenderContext, TokioWriter,
};
use tokio::{io::AsyncReadExt, sync::Notify};

#[path = "support/request_context.rs"]
mod sample;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let name = String::from("<request> & 雪");
    let context = RenderContext::new(
        sample::Request {
            name: &name,
            entered: Notify::new(),
            release: Notify::new(),
        },
        CancellationToken::new(),
    );
    let page = sample::Page::new(&context);
    let prefix = "<article><h1>&lt;request&gt; &amp; 雪</h1>";
    let expected = format!("{prefix}<p>&lt;request&gt; &amp; 雪</p></article>");
    for render in 1..=3 {
        let (write, mut read) = tokio::io::duplex(1);
        let mut writer = TokioWriter::new(write);
        let rendering = async {
            let result = render_incremental_with_context(&page, &mut writer, &context).await;
            drop(writer);
            result
        };
        let reader = async {
            let mut bytes = vec![0; prefix.len()];
            read.read_exact(&mut bytes).await.unwrap();
            assert_eq!(bytes, prefix.as_bytes());
            context.request().entered.notified().await;
            println!("Render {render} prefix: {prefix}");
            if render == 3 {
                context.cancellation_token().cancel();
            } else {
                context.request().release.notify_one();
            }
            read.read_to_end(&mut bytes).await.unwrap();
            let html = String::from_utf8(bytes).unwrap();
            assert_eq!(html, if render == 3 { prefix } else { &expected });
            html
        };
        let (result, html) = tokio::join!(rendering, reader);
        if render == 3 {
            match result {
                Err(AsyncRenderError::Write(error)) => {
                    assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
                    assert!(error.get_ref().unwrap().is::<RenderCancelled>());
                    println!("Cancelled: {:?}", error.kind());
                }
                other => panic!("unexpected cancellation result: {other:?}"),
            }
        } else {
            result?;
            println!("Render {render} full: {html}");
        }
    }
    Ok(())
}
