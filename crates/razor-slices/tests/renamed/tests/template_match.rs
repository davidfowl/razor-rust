use slices::{async_template, html, render_incremental, Slice};

#[test]
fn match_arms_use_hygienic_renamed_runtime_paths() {
    let page = html! {
        match Some("<&>") {
            Some(__razor_writer) => { {__razor_writer} }
            None => {}
        }
    };
    assert_eq!(page.render_to_string().unwrap(), "&lt;&amp;&gt;");
}

async_template! {
    struct Page<__RazorWriter: slices::AsyncWriteHtmlValue + Sync> { model: Option<__RazorWriter> } => {
        match model {
            Some(value) => { "<b>" {value} "</b>" }
            None => {}
        }
    }
}

struct Writer(Vec<u8>);
impl slices::AsyncSliceWriter for Writer {
    async fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.0.extend_from_slice(bytes);
        Ok(())
    }
    async fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
struct Noop;
impl std::task::Wake for Noop {
    fn wake(self: std::sync::Arc<Self>) {}
}

#[test]
fn async_match_bindings_avoid_writer_type_and_dependency_names() {
    use std::future::Future;
    let mut writer = Writer(Vec::new());
    let page = Page::new(Some("<&>"));
    let mut render = Box::pin(render_incremental(&page, &mut writer));
    let waker = std::sync::Arc::new(Noop).into();
    assert!(matches!(
        render
            .as_mut()
            .poll(&mut std::task::Context::from_waker(&waker)),
        std::task::Poll::Ready(Ok(()))
    ));
    drop(render);
    assert_eq!(writer.0, b"<b>&lt;&amp;&gt;</b>");
}
