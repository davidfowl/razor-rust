#![no_implicit_prelude]

extern crate slices;
extern crate std;

slices::async_template! {
    pub struct Renamed<'a> { model: &'a str } => {
        "prefix" {model}
        @await { async {} };
        @flush;
    }
}

struct Writer {
    bytes: std::vec::Vec<u8>,
    flushes: usize,
}

impl slices::AsyncSliceWriter for Writer {
    async fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.bytes.extend_from_slice(bytes);
        std::result::Result::Ok(())
    }

    async fn flush(&mut self) -> std::io::Result<()> {
        self.flushes += 1;
        std::result::Result::Ok(())
    }
}

struct Noop;
impl std::task::Wake for Noop {
    fn wake(self: std::sync::Arc<Self>) {}
}

#[test]
fn renamed_dependency_without_prelude_executes_repeatedly() {
    let text = <std::string::String as std::convert::From<&str>>::from("<&>");
    let page = Renamed::new(&text);
    for _ in 0..2 {
        let mut writer = Writer {
            bytes: std::vec::Vec::new(),
            flushes: 0,
        };
        let mut future = std::boxed::Box::pin(slices::render_incremental(&page, &mut writer));
        let waker = <std::task::Waker as std::convert::From<std::sync::Arc<Noop>>>::from(
            std::sync::Arc::new(Noop),
        );
        let mut context = std::task::Context::from_waker(&waker);
        std::assert!(std::matches!(
            std::future::Future::poll(future.as_mut(), &mut context),
            std::task::Poll::Ready(std::result::Result::Ok(()))
        ));
        std::mem::drop(future);
        std::assert_eq!(writer.bytes, b"prefix&lt;&amp;&gt;");
        std::assert_eq!(writer.flushes, 2);
    }
}
