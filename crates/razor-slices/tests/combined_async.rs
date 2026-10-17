#[path = "../examples/support/combined_async.rs"]
mod sample;

use razor_slices::{
    render_incremental, render_incremental_with_context, AsyncRenderError, AsyncSlice,
    AsyncSliceWriter, CancellationToken, RenderCancelled, RenderContext,
};
use std::{
    fmt,
    future::{poll_fn, Future},
    io,
    pin::pin,
    sync::{atomic::Ordering, Arc, Mutex},
    task::{Context, Poll, Wake, Waker},
};

#[derive(Default)]
struct State {
    bytes: Vec<u8>,
    boundaries: Vec<usize>,
    calls: usize,
    flushes: usize,
    pending: Option<usize>,
    fail: Option<usize>,
}

struct Writer(Arc<Mutex<State>>);

#[derive(Debug)]
struct DestinationFailure;
impl fmt::Display for DestinationFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("original combined destination failure")
    }
}
impl std::error::Error for DestinationFailure {}

impl Writer {
    async fn operation(&mut self, bytes: Option<&[u8]>) -> io::Result<()> {
        poll_fn(|_| {
            let mut state = self.0.lock().unwrap();
            if state.fail == Some(state.calls) {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    DestinationFailure,
                )));
            }
            if state.pending == Some(state.calls) {
                return Poll::Pending;
            }
            let len = state.bytes.len();
            state.boundaries.push(len);
            state.calls += 1;
            if let Some(bytes) = bytes {
                state.bytes.extend_from_slice(bytes);
            } else {
                state.flushes += 1;
            }
            Poll::Ready(Ok(()))
        })
        .await
    }
}

impl AsyncSliceWriter for Writer {
    async fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.operation(Some(bytes)).await
    }
    async fn flush(&mut self) -> io::Result<()> {
        self.operation(None).await
    }
}

struct Noop;
impl Wake for Noop {
    fn wake(self: Arc<Self>) {}
}
fn waker() -> Waker {
    Waker::from(Arc::new(Noop))
}
fn assert_send(_: impl Send) {}

async fn bytes(page: &impl AsyncSlice) -> Vec<u8> {
    let state = Arc::new(Mutex::new(State::default()));
    render_incremental(page, &mut Writer(state.clone()))
        .await
        .unwrap();
    let bytes = state.lock().unwrap().bytes.clone();
    bytes
}

#[tokio::test]
async fn borrowed_owned_repeated_documents_and_wrapped_partials_keep_immediate_context() {
    let model = sample::Model::demo();
    let borrowed = sample::page(&model);
    let owned = sample::page(model.clone());
    for page in [&borrowed, &borrowed] {
        model.release.notify_one();
        assert_eq!(bytes(&Box::new(&page)).await, sample::PAGE.as_bytes());
        assert_eq!(
            bytes(&sample::Bare::new(&Box::new(&page))).await,
            sample::PARTIAL.as_bytes()
        );
    }
    for _ in 0..2 {
        model.release.notify_one();
        assert_eq!(bytes(&Box::new(&owned)).await, sample::PAGE.as_bytes());
        assert_eq!(
            bytes(&sample::Partial::new(model.clone())).await,
            sample::PARTIAL.as_bytes()
        );
    }
    assert_eq!(model.entered.load(Ordering::SeqCst), 4);
    assert_eq!(model.completed.load(Ordering::SeqCst), 4);
    assert_eq!(model.dropped.load(Ordering::SeqCst), 4);
}

#[test]
fn scoped_nested_generated_layouts_repeat_then_cancel_the_held_child_section() {
    let model = sample::Model::demo();
    let token = CancellationToken::new();
    let context = RenderContext::new(&model, token.clone());
    let page = sample::page(*context.request());
    let state = Arc::new(Mutex::new(State::default()));
    let mut writer = Writer(state.clone());
    let waker = waker();
    let mut cx = Context::from_waker(&waker);
    for round in 0..2 {
        model.release.notify_one();
        let wrapped = Box::new(&page);
        let mut future = pin!(render_incremental_with_context(
            &wrapped,
            &mut writer,
            &context
        ));
        assert_send(future.as_mut());
        assert!(matches!(future.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
        assert_eq!(
            state.lock().unwrap().bytes,
            sample::PAGE.repeat(round + 1).as_bytes()
        );
    }
    let mut future = pin!(render_incremental_with_context(
        &page,
        &mut writer,
        &context
    ));
    assert!(future.as_mut().poll(&mut cx).is_pending());
    assert_eq!(model.entered.load(Ordering::SeqCst), 3);
    let expected = format!("{}{}", sample::PAGE.repeat(2), sample::PREFIX);
    assert_eq!(state.lock().unwrap().bytes, expected.as_bytes());
    token.cancel();
    let Poll::Ready(Err(AsyncRenderError::Write(error))) = future.as_mut().poll(&mut cx) else {
        panic!("cancellation must interrupt the nested layout's held section");
    };
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    assert!(error.get_ref().unwrap().is::<RenderCancelled>());
    model.release.notify_one();
    assert_eq!(model.completed.load(Ordering::SeqCst), 2);
    assert_eq!(model.dropped.load(Ordering::SeqCst), 3);
    assert_eq!(state.lock().unwrap().bytes, expected.as_bytes());
    assert_eq!(state.lock().unwrap().flushes, 5);
}

#[cfg(feature = "tokio")]
#[tokio::test]
async fn one_byte_destination_observes_exact_prefix_before_release_on_reused_owned_model() {
    use razor_slices::TokioWriter;
    use tokio::io::AsyncReadExt;
    let model = sample::Model::demo();
    for round in 0..2 {
        let (mut sender, mut receiver) = tokio::io::duplex(1);
        let page = sample::page(model.clone());
        let render = async { render_incremental(&page, &mut TokioWriter::new(&mut sender)).await };
        let receive = async {
            let mut prefix = vec![0; sample::PREFIX.len()];
            receiver.read_exact(&mut prefix).await.unwrap();
            assert_eq!(prefix, sample::PREFIX.as_bytes());
            assert_eq!(model.completed.load(Ordering::SeqCst), round);
            model.release.notify_one();
            let mut suffix = vec![0; sample::PAGE.len() - prefix.len()];
            receiver.read_exact(&mut suffix).await.unwrap();
            prefix.extend(suffix);
            assert_eq!(prefix, sample::PAGE.as_bytes());
        };
        let (rendered, ()) = tokio::join!(render, receive);
        rendered.unwrap();
    }
}

fn boundaries() -> Vec<usize> {
    let model = sample::Model::demo();
    let page = sample::page(&model);
    model.release.notify_one();
    let state = Arc::new(Mutex::new(State::default()));
    let mut writer = Writer(state.clone());
    let waker = waker();
    assert!(matches!(
        pin!(render_incremental(&page, &mut writer))
            .as_mut()
            .poll(&mut Context::from_waker(&waker)),
        Poll::Ready(Ok(()))
    ));
    let state = state.lock().unwrap();
    assert_eq!(state.bytes, sample::PAGE.as_bytes());
    assert_eq!(state.flushes, 2);
    state.boundaries.clone()
}

#[test]
fn every_mixed_layout_write_and_flush_preserves_backpressure_errors_and_drop() {
    let boundaries = boundaries();
    assert!(
        boundaries.len() > 30,
        "must cover generated attributes and nested sections"
    );
    for (call, &boundary) in boundaries.iter().enumerate() {
        for fail in [false, true] {
            let model = sample::Model::demo();
            let page = sample::page(&model);
            model.release.notify_one();
            let state = Arc::new(Mutex::new(State {
                pending: (!fail).then_some(call),
                fail: fail.then_some(call),
                ..State::default()
            }));
            let mut writer = Writer(state.clone());
            let waker = waker();
            let mut cx = Context::from_waker(&waker);
            let wrapped = Box::new(&page);
            let mut future = pin!(render_incremental(&wrapped, &mut writer));
            assert_send(future.as_mut());
            if fail {
                match future.as_mut().poll(&mut cx) {
                    Poll::Ready(Err(AsyncRenderError::Write(error))) => {
                        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
                        assert!(error.get_ref().unwrap().is::<DestinationFailure>());
                    }
                    other => panic!("operation {call}: {other:?}"),
                }
            } else {
                assert!(future.as_mut().poll(&mut cx).is_pending());
                assert!(future.as_mut().poll(&mut cx).is_pending());
            }
            assert_eq!(state.lock().unwrap().calls, call);
            assert_eq!(
                state.lock().unwrap().bytes,
                sample::PAGE.as_bytes()[..boundary]
            );
            if !fail {
                state.lock().unwrap().pending = None;
                assert!(matches!(future.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
                assert_eq!(state.lock().unwrap().bytes, sample::PAGE.as_bytes());
                assert_eq!(state.lock().unwrap().flushes, 2);
            }
        }
        let model = sample::Model::demo();
        let page = sample::page(&model);
        model.release.notify_one();
        let state = Arc::new(Mutex::new(State {
            pending: Some(call),
            ..State::default()
        }));
        let mut writer = Writer(state.clone());
        let waker = waker();
        {
            let mut future = pin!(render_incremental(&page, &mut writer));
            assert!(future
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending());
        }
        assert_eq!(
            state.lock().unwrap().bytes,
            sample::PAGE.as_bytes()[..boundary]
        );
        assert_eq!(state.lock().unwrap().calls, call);
        assert_eq!(
            model.dropped.load(Ordering::SeqCst),
            model.entered.load(Ordering::SeqCst)
        );
    }
    println!(
        "verified all {} destination write/flush boundaries for Pending, original failure and drop",
        boundaries.len()
    );
}

#[test]
fn held_flush_precedes_await_and_drop_cancels_entered_section_without_suffix() {
    let calls = boundaries();
    let flush = calls
        .iter()
        .position(|&len| len == sample::PREFIX.len())
        .unwrap();
    let model = sample::Model::demo();
    let page = sample::page(&model);
    let state = Arc::new(Mutex::new(State {
        pending: Some(flush),
        ..State::default()
    }));
    let mut writer = Writer(state.clone());
    let waker = waker();
    let mut cx = Context::from_waker(&waker);
    {
        let mut future = pin!(render_incremental(&page, &mut writer));
        assert!(future.as_mut().poll(&mut cx).is_pending());
        assert_eq!(state.lock().unwrap().bytes, sample::PREFIX.as_bytes());
        assert_eq!(model.entered.load(Ordering::SeqCst), 0);
        model.release.notify_one();
        assert!(future.as_mut().poll(&mut cx).is_pending());
        assert_eq!(model.completed.load(Ordering::SeqCst), 0);
        state.lock().unwrap().pending = None;
        assert!(matches!(future.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
    }
    let model = sample::Model::demo();
    let page = sample::page(&model);
    let state = Arc::new(Mutex::new(State::default()));
    let mut writer = Writer(state.clone());
    {
        let mut future = pin!(render_incremental(&page, &mut writer));
        assert!(future.as_mut().poll(&mut cx).is_pending());
        assert_eq!(model.entered.load(Ordering::SeqCst), 1);
        assert_eq!(model.dropped.load(Ordering::SeqCst), 0);
    }
    model.release.notify_one();
    assert_eq!(model.dropped.load(Ordering::SeqCst), 1);
    assert_eq!(model.completed.load(Ordering::SeqCst), 0);
    assert_eq!(state.lock().unwrap().bytes, sample::PREFIX.as_bytes());
    assert_eq!(state.lock().unwrap().flushes, 1);
}
