use std::{
    future::{poll_fn, Future},
    io,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll, Wake, Waker},
};

use razor_slices::{
    render_incremental_with_context, with_async_section, AsyncHtmlWriter, AsyncRenderError,
    AsyncSlice, AsyncSliceWriter, CancellationToken, RenderCancelled, RenderContext,
};
use tokio::sync::Notify;

#[path = "../examples/support/request_context.rs"]
mod sample;

#[derive(Default)]
struct Wakes(AtomicUsize);
impl Wake for Wakes {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Default)]
struct State {
    bytes: Vec<u8>,
    writes: usize,
    flushes: usize,
    pending_write: bool,
    pending_flush: bool,
    drops: usize,
}

struct Writer(Arc<Mutex<State>>);
impl AsyncSliceWriter for Writer {
    async fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        let pending = {
            let mut state = self.0.lock().unwrap();
            state.writes += 1;
            if state.pending_write {
                state.bytes.extend_from_slice(&bytes[..1]);
                true
            } else {
                state.bytes.extend_from_slice(bytes);
                false
            }
        };
        if pending {
            let _guard = Dropped(self.0.clone());
            std::future::pending().await
        } else {
            Ok(())
        }
    }

    async fn flush(&mut self) -> io::Result<()> {
        let pending = {
            let mut state = self.0.lock().unwrap();
            state.flushes += 1;
            state.pending_flush
        };
        if pending {
            let _guard = Dropped(self.0.clone());
            std::future::pending().await
        } else {
            Ok(())
        }
    }
}

struct Dropped(Arc<Mutex<State>>);
impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.lock().unwrap().drops += 1;
    }
}

fn assert_cancelled(result: Result<(), AsyncRenderError>) {
    match result {
        Err(AsyncRenderError::Write(error)) => {
            assert_eq!(error.kind(), io::ErrorKind::Interrupted);
            assert!(error.get_ref().unwrap().is::<RenderCancelled>());
        }
        other => panic!("expected observable cancellation: {other:?}"),
    }
}

struct Held(Arc<Mutex<State>>);
impl AsyncSlice for Held {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        out: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        out.write_literal("prefix").await?;
        {
            let _guard = Dropped(self.0.clone());
            std::future::pending::<()>().await;
        }
        out.write_literal("suffix").await?;
        Ok(())
    }
}

#[test]
fn cancellation_wakes_and_drops_held_await_without_suffix_or_final_flush() {
    let token = CancellationToken::new();
    let context = RenderContext::new("request", token.clone());
    let shared = Arc::new(Mutex::new(State::default()));
    let slice = Held(shared.clone());
    let mut writer = Writer(shared.clone());
    let wakes = Arc::new(Wakes::default());
    let waker = Waker::from(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    let mut rendering = Box::pin(render_incremental_with_context(
        &slice,
        &mut writer,
        &context,
    ));
    fn is_send(_: &impl Send) {}
    is_send(&rendering);
    assert!(rendering.as_mut().poll(&mut cx).is_pending());
    assert_eq!(shared.lock().unwrap().bytes, b"prefix");
    token.cancel();
    assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
    let Poll::Ready(result) = rendering.as_mut().poll(&mut cx) else {
        panic!("cancellation did not complete render");
    };
    assert_cancelled(result);
    let state = shared.lock().unwrap();
    assert_eq!(state.bytes, b"prefix");
    assert_eq!(state.drops, 1);
    assert_eq!(state.flushes, 0);
}

#[test]
fn dropping_scope_future_preserves_explicit_token_lifetime() {
    let context = RenderContext::new((), CancellationToken::new());
    let shared = Arc::new(Mutex::new(State::default()));
    let slice = Held(shared.clone());
    let mut writer = Writer(shared.clone());
    let waker = Waker::from(Arc::new(Wakes::default()));
    let mut rendering = Box::pin(render_incremental_with_context(
        &slice,
        &mut writer,
        &context,
    ));
    assert!(rendering
        .as_mut()
        .poll(&mut Context::from_waker(&waker))
        .is_pending());
    drop(rendering);
    assert!(!context.cancellation_token().is_cancelled());
    let state = shared.lock().unwrap();
    assert_eq!(state.bytes, b"prefix");
    assert_eq!(state.drops, 1);
    assert_eq!(state.flushes, 0);
}

#[test]
fn cross_thread_cancellation_wakes_all_registered_observers() {
    let token = CancellationToken::new();
    let wakes = Arc::new(Wakes::default());
    let waker = Waker::from(wakes.clone());
    let mut first = Box::pin(token.cancelled());
    let mut second = Box::pin(token.cancelled());
    assert!(first
        .as_mut()
        .poll(&mut Context::from_waker(&waker))
        .is_pending());
    assert!(second
        .as_mut()
        .poll(&mut Context::from_waker(&waker))
        .is_pending());
    let clone = token.clone();
    std::thread::spawn(move || clone.cancel()).join().unwrap();
    assert_eq!(wakes.0.load(Ordering::SeqCst), 2);
    assert!(first
        .as_mut()
        .poll(&mut Context::from_waker(&waker))
        .is_ready());
    assert!(second
        .as_mut()
        .poll(&mut Context::from_waker(&waker))
        .is_ready());
    token.cancel();
    assert_eq!(wakes.0.load(Ordering::SeqCst), 2);
}

struct Text;
impl AsyncSlice for Text {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        out: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        out.write_literal("text").await?;
        Ok(())
    }
}

#[test]
fn cancellation_interrupts_partial_pending_write_and_final_pending_flush() {
    for pending_write in [true, false] {
        let token = CancellationToken::new();
        let context = RenderContext::new((), token.clone());
        let shared = Arc::new(Mutex::new(State {
            pending_write,
            pending_flush: !pending_write,
            ..State::default()
        }));
        let mut writer = Writer(shared.clone());
        let wakes = Arc::new(Wakes::default());
        let waker = Waker::from(wakes.clone());
        let mut cx = Context::from_waker(&waker);
        let mut rendering = Box::pin(render_incremental_with_context(
            &Text,
            &mut writer,
            &context,
        ));
        assert!(rendering.as_mut().poll(&mut cx).is_pending());
        {
            let state = shared.lock().unwrap();
            assert_eq!(
                state.bytes,
                if pending_write {
                    b"t".as_slice()
                } else {
                    b"text"
                }
            );
            assert_eq!(state.flushes, usize::from(!pending_write));
            assert_eq!(state.drops, 0);
        }
        token.cancel();
        assert!(wakes.0.load(Ordering::SeqCst) >= 1);
        let Poll::Ready(result) = rendering.as_mut().poll(&mut cx) else {
            panic!("pending destination did not cancel");
        };
        assert_cancelled(result);
        let state = shared.lock().unwrap();
        assert_eq!(state.drops, 1);
        assert_eq!(state.writes, 1);
        assert_eq!(state.flushes, usize::from(!pending_write));
    }
}

#[test]
fn precancelled_scope_never_polls_slice_and_cannot_reset_on_repeated_render() {
    let token = CancellationToken::new();
    token.cancel();
    let context = RenderContext::new((), token);
    let state = Arc::new(Mutex::new(State::default()));
    let mut writer = Writer(state.clone());
    let waker = Waker::from(Arc::new(Wakes::default()));
    for _ in 0..2 {
        let mut rendering = Box::pin(render_incremental_with_context(
            &Text,
            &mut writer,
            &context,
        ));
        let Poll::Ready(result) = rendering.as_mut().poll(&mut Context::from_waker(&waker)) else {
            panic!("pre-cancelled render was pending");
        };
        assert_cancelled(result);
    }
    let state = state.lock().unwrap();
    assert_eq!(state.writes, 0);
    assert_eq!(state.flushes, 0);
}

#[test]
fn typed_borrowed_requests_are_isolated_concurrent_and_reusable() {
    let name_a = String::from("<a>");
    let name_b = String::from("&b");
    let make = |name| {
        RenderContext::new(
            sample::Request {
                name,
                entered: Notify::new(),
                release: Notify::new(),
            },
            CancellationToken::new(),
        )
    };
    let a = make(name_a.as_str());
    let b = make(name_b.as_str());
    let page_a = sample::Page::new(&a);
    let page_b = Box::new(sample::Page::new(&b));
    let borrowed_b = &page_b;
    let state_a = Arc::new(Mutex::new(State::default()));
    let state_b = Arc::new(Mutex::new(State::default()));
    let mut writer_a = Writer(state_a.clone());
    let mut writer_b = Writer(state_b.clone());
    let wakes_a = Arc::new(Wakes::default());
    let wakes_b = Arc::new(Wakes::default());
    let waker_a = Waker::from(wakes_a.clone());
    let waker_b = Waker::from(wakes_b.clone());
    {
        let mut render_a = Box::pin(render_incremental_with_context(&page_a, &mut writer_a, &a));
        let mut render_b = Box::pin(render_incremental_with_context(
            &borrowed_b,
            &mut writer_b,
            &b,
        ));
        assert!(render_a
            .as_mut()
            .poll(&mut Context::from_waker(&waker_a))
            .is_pending());
        assert!(render_b
            .as_mut()
            .poll(&mut Context::from_waker(&waker_b))
            .is_pending());
        assert_eq!(
            state_a.lock().unwrap().bytes,
            b"<article><h1>&lt;a&gt;</h1>"
        );
        assert_eq!(state_b.lock().unwrap().bytes, b"<article><h1>&amp;b</h1>");
        a.cancellation_token().cancel();
        assert!(!b.cancellation_token().is_cancelled());
        assert_eq!(wakes_b.0.load(Ordering::SeqCst), 0);
        let Poll::Ready(result) = render_a.as_mut().poll(&mut Context::from_waker(&waker_a)) else {
            panic!("request A not cancelled");
        };
        assert_cancelled(result);
        b.request().release.notify_one();
        assert!(matches!(
            render_b.as_mut().poll(&mut Context::from_waker(&waker_b)),
            Poll::Ready(Ok(()))
        ));
    }
    b.request().release.notify_one();
    let mut repeated = Box::pin(render_incremental_with_context(&page_b, &mut writer_b, &b));
    assert!(matches!(
        repeated.as_mut().poll(&mut Context::from_waker(&waker_b)),
        Poll::Ready(Ok(()))
    ));
    assert_eq!(
        state_b.lock().unwrap().bytes,
        b"<article><h1>&amp;b</h1><p>&amp;b</p></article>".repeat(2)
    );
    assert_eq!(
        state_a.lock().unwrap().bytes,
        b"<article><h1>&lt;a&gt;</h1>"
    );
}

struct Observe(Arc<Mutex<Vec<CancellationToken>>>);
impl AsyncSlice for Observe {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        out: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        self.0
            .lock()
            .unwrap()
            .push(out.cancellation_token().unwrap().clone());
        out.write_literal("observed").await?;
        Ok(())
    }
}

struct Shell<S>(S);
impl<S: AsyncSlice> AsyncSlice for Shell<S> {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        out: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        out.write_literal("<shell>").await?;
        out.write_section(&self.0, "header").await?;
        self.0.render_into(out).await?;
        out.write_literal("</shell>").await?;
        Ok(())
    }
}

#[test]
fn same_token_reaches_explicit_layout_child_and_lazy_section_through_wrappers() {
    let observed = Arc::new(Mutex::new(Vec::new()));
    let body = with_async_section(
        Box::new(Observe(observed.clone())),
        "header",
        Observe(observed.clone()),
    )
    .unwrap();
    let shell = Box::new(Shell(body));
    let borrowed_shell = &shell;
    let token = CancellationToken::new();
    let context = RenderContext::new((), token.clone());
    let state = Arc::new(Mutex::new(State::default()));
    let mut writer = Writer(state.clone());
    let waker = Waker::from(Arc::new(Wakes::default()));
    let mut rendering = Box::pin(render_incremental_with_context(
        &borrowed_shell,
        &mut writer,
        &context,
    ));
    assert!(matches!(
        rendering.as_mut().poll(&mut Context::from_waker(&waker)),
        Poll::Ready(Ok(()))
    ));
    assert_eq!(
        state.lock().unwrap().bytes,
        b"<shell>observedobserved</shell>"
    );
    assert_eq!(observed.lock().unwrap().len(), 2);
    assert!(!token.is_cancelled());
    token.cancel();
    assert!(observed
        .lock()
        .unwrap()
        .iter()
        .all(CancellationToken::is_cancelled));
}

#[derive(Debug)]
struct Original;
impl std::fmt::Display for Original {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("original destination error")
    }
}
impl std::error::Error for Original {}

#[test]
fn scoped_destination_errors_preserve_original_kind_and_source() {
    struct Failure;
    impl AsyncSliceWriter for Failure {
        async fn write_all(&mut self, _: &[u8]) -> io::Result<()> {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, Original))
        }
        async fn flush(&mut self) -> io::Result<()> {
            panic!("must not flush after error")
        }
    }
    let context = RenderContext::new((), CancellationToken::new());
    let mut writer = Failure;
    let waker = Waker::from(Arc::new(Wakes::default()));
    let mut rendering = Box::pin(render_incremental_with_context(
        &Text,
        &mut writer,
        &context,
    ));
    match rendering.as_mut().poll(&mut Context::from_waker(&waker)) {
        Poll::Ready(Err(AsyncRenderError::Write(error))) => {
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
            assert!(error.get_ref().unwrap().is::<Original>());
        }
        other => panic!("original error lost: {other:?}"),
    }
}

#[test]
fn explicit_writer_scope_interrupts_child_write_without_outer_render_scope() {
    let token = CancellationToken::new();
    let state = Arc::new(Mutex::new(State {
        pending_write: true,
        ..State::default()
    }));
    let mut writer = Writer(state.clone());
    let mut out = AsyncHtmlWriter::with_cancellation(&mut writer, &token);
    let waker = Waker::from(Arc::new(Wakes::default()));
    let mut rendering = Box::pin(Text.render_into(&mut out));
    assert!(rendering
        .as_mut()
        .poll(&mut Context::from_waker(&waker))
        .is_pending());
    token.cancel();
    let Poll::Ready(result) = rendering.as_mut().poll(&mut Context::from_waker(&waker)) else {
        panic!("scoped writer did not wake");
    };
    assert_cancelled(result);
    assert_eq!(state.lock().unwrap().drops, 1);
}

#[test]
fn cancellation_priority_and_normal_ready_error_are_explicit() {
    let token = CancellationToken::new();
    token.cancel();
    let entered = AtomicUsize::new(0);
    let work = poll_fn(|_| {
        entered.fetch_add(1, Ordering::SeqCst);
        Poll::Ready(Ok::<_, io::Error>(()))
    });
    let waker = Waker::from(Arc::new(Wakes::default()));
    let mut future = Box::pin(token.run(work));
    assert!(matches!(
        future.as_mut().poll(&mut Context::from_waker(&waker)),
        Poll::Ready(Err(_))
    ));
    assert_eq!(entered.load(Ordering::SeqCst), 0);

    let token = CancellationToken::new();
    let mut future = Box::pin(token.run(async {
        token.cancel();
        Err::<(), _>(io::Error::new(io::ErrorKind::PermissionDenied, Original))
    }));
    let Poll::Ready(Err(error)) = future.as_mut().poll(&mut Context::from_waker(&waker)) else {
        panic!("ready error replaced");
    };
    assert!(error.get_ref().unwrap().is::<Original>());
}

#[cfg(feature = "tokio")]
#[tokio::test]
async fn cancellation_interrupts_an_actual_full_tokio_duplex_write() {
    use razor_slices::TokioWriter;
    let (write, mut read) = tokio::io::duplex(1);
    let mut writer = TokioWriter::new(write);
    let context = RenderContext::new((), CancellationToken::new());
    let waker = Waker::from(Arc::new(Wakes::default()));
    {
        let mut rendering = Box::pin(render_incremental_with_context(
            &Text,
            &mut writer,
            &context,
        ));
        assert!(rendering
            .as_mut()
            .poll(&mut Context::from_waker(&waker))
            .is_pending());
        context.cancellation_token().cancel();
        let Poll::Ready(result) = rendering.as_mut().poll(&mut Context::from_waker(&waker)) else {
            panic!("full duplex write was not interrupted");
        };
        assert_cancelled(result);
    }
    drop(writer);
    let mut bytes = Vec::new();
    tokio::io::AsyncReadExt::read_to_end(&mut read, &mut bytes)
        .await
        .unwrap();
    assert_eq!(bytes, b"t");
}
