use std::{
    future::{poll_fn, Future},
    io,
    pin::pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Wake, Waker},
};

use razor_slices::{
    encode_html_to_writer, render_async, render_incremental, AsyncHtmlWriter, AsyncRenderError,
    AsyncSlice, AsyncSliceWriter, Slice,
};

#[derive(Default)]
struct State {
    bytes: Vec<u8>,
    events: Vec<&'static str>,
    later_ready: bool,
    write_ready: bool,
    flush_ready: bool,
    write_error: bool,
    write_error_after: Option<usize>,
    writes: usize,
    flush_error: bool,
    max_write: usize,
}

type Shared = Arc<Mutex<State>>;

struct Writer(Shared);

impl AsyncSliceWriter for Writer {
    async fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        poll_fn(|_| {
            let mut state = self.0.lock().unwrap();
            if state.write_error || state.write_error_after == Some(state.writes) {
                return Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, "write")));
            }
            if !state.write_ready {
                return Poll::Pending;
            }
            state.max_write = state.max_write.max(bytes.len());
            state.bytes.extend_from_slice(bytes);
            state.events.push("write");
            state.writes += 1;
            Poll::Ready(Ok(()))
        })
        .await
    }

    async fn flush(&mut self) -> io::Result<()> {
        poll_fn(|_| {
            let mut state = self.0.lock().unwrap();
            if state.flush_error {
                return Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, "flush")));
            }
            if !state.flush_ready {
                return Poll::Pending;
            }
            state.events.push("flush");
            Poll::Ready(Ok(()))
        })
        .await
    }
}

struct Template(Shared);

impl AsyncSlice for Template {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        output.write_literal("<p>").await?;
        output.flush().await?;
        self.0.lock().unwrap().events.push("await");
        poll_fn(|_| {
            if self.0.lock().unwrap().later_ready {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
        output.write_value("<雪 & \"'>").await?;
        output.write_literal("</p>").await?;
        Ok(())
    }
}

struct Noop;
impl Wake for Noop {
    fn wake(self: Arc<Self>) {}
}

fn context(waker: &Waker) -> Context<'_> {
    Context::from_waker(waker)
}

fn ready_state() -> Shared {
    Arc::new(Mutex::new(State {
        write_ready: true,
        flush_ready: true,
        ..State::default()
    }))
}

#[test]
fn prefix_is_available_before_later_await_and_repeated_render_reexecutes() {
    let shared = ready_state();
    let template = Template(shared.clone());
    let mut writer = Writer(shared.clone());
    let waker = Waker::from(Arc::new(Noop));
    {
        let mut future = pin!(render_incremental(&template, &mut writer));
        assert!(future.as_mut().poll(&mut context(&waker)).is_pending());
        {
            let state = shared.lock().unwrap();
            assert_eq!(state.bytes, b"<p>");
            assert_eq!(state.events, ["write", "flush", "await"]);
        }
        shared.lock().unwrap().later_ready = true;
        assert!(matches!(
            future.as_mut().poll(&mut context(&waker)),
            Poll::Ready(Ok(()))
        ));
    }
    let expected = "<p>&lt;雪 &amp; &quot;&#39;&gt;</p>";
    assert_eq!(shared.lock().unwrap().bytes, expected.as_bytes());
    let first_events = shared.lock().unwrap().events.clone();
    let mut future = pin!(render_incremental(&template, &mut writer));
    assert!(matches!(
        future.as_mut().poll(&mut context(&waker)),
        Poll::Ready(Ok(()))
    ));
    let state = shared.lock().unwrap();
    assert_eq!(state.bytes, expected.repeat(2).as_bytes());
    assert_eq!(state.events, first_events.repeat(2));
}

#[test]
fn write_and_flush_backpressure_stop_later_application_work() {
    let shared = ready_state();
    shared.lock().unwrap().write_ready = false;
    shared.lock().unwrap().flush_ready = false;
    let template = Template(shared.clone());
    let mut writer = Writer(shared.clone());
    let waker = Waker::from(Arc::new(Noop));
    let mut future = pin!(render_incremental(&template, &mut writer));
    assert!(future.as_mut().poll(&mut context(&waker)).is_pending());
    assert!(shared.lock().unwrap().events.is_empty());
    shared.lock().unwrap().write_ready = true;
    assert!(future.as_mut().poll(&mut context(&waker)).is_pending());
    assert_eq!(shared.lock().unwrap().events, ["write"]);
    shared.lock().unwrap().flush_ready = true;
    assert!(future.as_mut().poll(&mut context(&waker)).is_pending());
    assert_eq!(shared.lock().unwrap().events, ["write", "flush", "await"]);
}

#[test]
fn write_and_mid_template_flush_errors_return_without_later_work_or_final_flush() {
    for fail_write in [true, false] {
        let shared = ready_state();
        {
            let mut state = shared.lock().unwrap();
            state.write_error = fail_write;
            state.flush_error = !fail_write;
        }
        let template = Template(shared.clone());
        let mut writer = Writer(shared.clone());
        let waker = Waker::from(Arc::new(Noop));
        let mut future = pin!(render_incremental(&template, &mut writer));
        match future.as_mut().poll(&mut context(&waker)) {
            Poll::Ready(Err(AsyncRenderError::Write(error))) => {
                assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
                assert_eq!(
                    error.to_string(),
                    if fail_write { "write" } else { "flush" }
                );
            }
            other => panic!("unexpected result: {other:?}"),
        }
        let state = shared.lock().unwrap();
        assert!(!state.events.contains(&"await"));
        assert!(!state.events.contains(&"flush"));
    }
}

#[test]
fn dropping_pending_render_preserves_prefix_and_does_not_finish_or_flush() {
    let shared = ready_state();
    let template = Template(shared.clone());
    let mut writer = Writer(shared.clone());
    let waker = Waker::from(Arc::new(Noop));
    {
        let mut future = pin!(render_incremental(&template, &mut writer));
        assert!(future.as_mut().poll(&mut context(&waker)).is_pending());
    }
    shared.lock().unwrap().later_ready = true;
    let state = shared.lock().unwrap();
    assert_eq!(state.bytes, b"<p>");
    assert_eq!(state.events, ["write", "flush", "await"]);
}

#[test]
fn bounded_chunks_preserve_utf8_and_match_existing_encoder() {
    let values = [
        String::new(),
        "plain雪🙂".repeat(1_024),
        "&<>\"'".repeat(2_048),
        format!("{}🙂雪&{}", "a".repeat(1_023), "\"".repeat(2_049)),
    ];
    for value in values {
        let shared = ready_state();
        let mut writer = Writer(shared.clone());
        let mut output = AsyncHtmlWriter::new(&mut writer);
        let waker = Waker::from(Arc::new(Noop));
        let mut future = pin!(output.write_value(&value));
        assert!(matches!(
            future.as_mut().poll(&mut context(&waker)),
            Poll::Ready(Ok(()))
        ));
        let mut expected = Vec::new();
        encode_html_to_writer(&value, &mut expected).unwrap();
        let state = shared.lock().unwrap();
        assert_eq!(state.bytes, expected);
        assert!(state.max_write <= 6 * 1_024);
        std::str::from_utf8(&state.bytes).unwrap();
    }
}

struct FailingTemplate;
impl AsyncSlice for FailingTemplate {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        output.write_literal("prefix").await?;
        Err(razor_slices::RenderError(std::fmt::Error).into())
    }
}

#[test]
fn template_error_preserves_output_without_final_flush() {
    let shared = ready_state();
    let mut writer = Writer(shared.clone());
    let waker = Waker::from(Arc::new(Noop));
    let mut future = pin!(render_incremental(&FailingTemplate, &mut writer));
    assert!(matches!(
        future.as_mut().poll(&mut context(&waker)),
        Poll::Ready(Err(AsyncRenderError::Render(_)))
    ));
    let state = shared.lock().unwrap();
    assert_eq!(state.bytes, b"prefix");
    assert_eq!(state.events, ["write"]);
}

struct EmptyTemplate;
impl AsyncSlice for EmptyTemplate {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        _: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        Ok(())
    }
}

#[test]
fn final_flush_is_awaited_and_its_error_propagates() {
    let shared = ready_state();
    shared.lock().unwrap().flush_ready = false;
    let mut writer = Writer(shared.clone());
    let waker = Waker::from(Arc::new(Noop));
    let mut future = pin!(render_incremental(&EmptyTemplate, &mut writer));
    assert!(future.as_mut().poll(&mut context(&waker)).is_pending());
    shared.lock().unwrap().flush_error = true;
    assert!(matches!(
        future.as_mut().poll(&mut context(&waker)),
        Poll::Ready(Err(AsyncRenderError::Write(_)))
    ));
    assert!(shared.lock().unwrap().events.is_empty());
}

struct SyncTemplate(Shared);
impl Slice for SyncTemplate {
    fn render(&self, writer: &mut dyn std::fmt::Write) -> std::fmt::Result {
        self.0.lock().unwrap().events.push("sync-render");
        writer.write_str("fully buffered")
    }
}

#[test]
fn existing_async_bridge_still_renders_entire_slice_before_first_pending_write() {
    let shared = ready_state();
    shared.lock().unwrap().write_ready = false;
    let slice = SyncTemplate(shared.clone());
    let mut writer = Writer(shared.clone());
    let waker = Waker::from(Arc::new(Noop));
    let mut future = pin!(render_async(&slice, &mut writer));
    assert!(future.as_mut().poll(&mut context(&waker)).is_pending());
    assert_eq!(shared.lock().unwrap().events, ["sync-render"]);
    assert!(shared.lock().unwrap().bytes.is_empty());
    shared.lock().unwrap().write_ready = true;
    assert!(matches!(
        future.as_mut().poll(&mut context(&waker)),
        Poll::Ready(Ok(()))
    ));
    assert_eq!(shared.lock().unwrap().bytes, b"fully buffered");
}

#[cfg(feature = "tokio")]
#[tokio::test]
async fn tokio_duplex_handles_short_writes_and_send_future() {
    use razor_slices::TokioWriter;
    use tokio::io::AsyncReadExt;

    let shared = ready_state();
    shared.lock().unwrap().later_ready = true;
    let template = Template(shared);
    let (destination, mut reader) = tokio::io::duplex(1);
    let producer = async {
        let mut writer = TokioWriter::new(destination);
        render_incremental(&template, &mut writer).await
    };
    let consumer = async {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await.unwrap();
        bytes
    };
    fn assert_send(_: &impl Send) {}
    assert_send(&producer);
    let (result, bytes) = tokio::join!(producer, consumer);
    result.unwrap();
    assert_eq!(bytes, "<p>&lt;雪 &amp; &quot;&#39;&gt;</p>".as_bytes());
}

#[test]
fn escaped_chunk_error_stops_without_writing_remaining_value() {
    let shared = ready_state();
    shared.lock().unwrap().write_error_after = Some(1);
    let mut writer = Writer(shared.clone());
    let mut output = AsyncHtmlWriter::new(&mut writer);
    let value = "\"".repeat(3_000);
    let waker = Waker::from(Arc::new(Noop));
    let mut future = pin!(output.write_value(&value));
    assert!(matches!(
        future.as_mut().poll(&mut context(&waker)),
        Poll::Ready(Err(_))
    ));
    let state = shared.lock().unwrap();
    assert_eq!(state.bytes, "&quot;".repeat(1_024).as_bytes());
    assert_eq!(state.events, ["write"]);
}

struct Outer<'a>(&'a Template);
impl AsyncSlice for Outer<'_> {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        output.write_literal("<main>").await?;
        self.0.render_into(output).await?;
        output.write_literal("</main>").await?;
        Ok(())
    }
}

#[test]
fn nested_async_execution_preserves_order_and_does_not_add_a_final_child_flush() {
    let shared = ready_state();
    let child = Template(shared.clone());
    let parent = Outer(&child);
    let mut writer = Writer(shared.clone());
    let waker = Waker::from(Arc::new(Noop));
    let mut future = pin!(render_incremental(&parent, &mut writer));
    assert!(future.as_mut().poll(&mut context(&waker)).is_pending());
    assert_eq!(shared.lock().unwrap().bytes, b"<main><p>");
    shared.lock().unwrap().later_ready = true;
    assert!(matches!(
        future.as_mut().poll(&mut context(&waker)),
        Poll::Ready(Ok(()))
    ));
    let state = shared.lock().unwrap();
    assert_eq!(
        state.bytes,
        "<main><p>&lt;雪 &amp; &quot;&#39;&gt;</p></main>".as_bytes()
    );
    assert_eq!(
        state
            .events
            .iter()
            .filter(|event| **event == "flush")
            .count(),
        2
    );
    assert_eq!(state.events.last(), Some(&"flush"));
}
