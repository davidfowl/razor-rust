use razor_slices::{
    declare_async_layout, render_incremental, AsyncHtmlWriter, AsyncLayout, AsyncRenderError,
    AsyncSlice, AsyncSliceWriter,
};
use std::{
    fmt,
    future::{poll_fn, Future},
    io,
    pin::pin,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    task::Poll,
};
use tokio::sync::Notify;

#[derive(Default)]
struct State {
    locals: [AtomicUsize; 3],
    owners: [AtomicUsize; 3],
    writer_drops: AtomicUsize,
    held: AtomicBool,
    entered: AtomicBool,
    release: Notify,
}

fn counts(values: &[AtomicUsize; 3]) -> [usize; 3] {
    std::array::from_fn(|index| values[index].load(Ordering::SeqCst))
}

struct Guard {
    state: Arc<State>,
    node: usize,
    owned: bool,
}

impl Drop for Guard {
    fn drop(&mut self) {
        let values = if self.owned {
            &self.state.owners
        } else {
            &self.state.locals
        };
        values[self.node].fetch_add(1, Ordering::SeqCst);
    }
}

struct Node {
    owner: Guard,
}

impl Node {
    fn new(state: &Arc<State>, node: usize) -> Self {
        Self {
            owner: Guard {
                state: state.clone(),
                node,
                owned: true,
            },
        }
    }

    fn local(&self) -> Guard {
        Guard {
            state: self.owner.state.clone(),
            node: self.owner.node,
            owned: false,
        }
    }
}

struct Page(Node);

impl AsyncSlice for Page {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        let _local = self.0.local();
        let state = &self.0.owner.state;
        if state.held.load(Ordering::SeqCst) {
            state.entered.store(true, Ordering::SeqCst);
            state.release.notified().await;
        }
        output.write_literal("P").await?;
        Ok(())
    }
}

struct Frame {
    node: Node,
    open: &'static str,
    close: &'static str,
}

impl AsyncLayout for Frame {
    async fn render_layout_into<B: AsyncSlice, W: AsyncSliceWriter + Send>(
        &self,
        body: &B,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        let _local = self.node.local();
        output.write_literal(self.open).await?;
        body.render_body_into(output).await?;
        output.write_literal(self.close).await?;
        Ok(())
    }
}

fn frame(state: &Arc<State>, node: usize, open: &'static str, close: &'static str) -> Frame {
    Frame {
        node: Node::new(state, node),
        open,
        close,
    }
}

#[derive(Debug)]
struct Original(Arc<()>);

impl fmt::Display for Original {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str("original layout lifecycle failure")
    }
}

impl std::error::Error for Original {}

struct Destination {
    bytes: Vec<u8>,
    fail_at: Option<usize>,
    source: Arc<()>,
    state: Arc<State>,
    flushes: usize,
}

impl Destination {
    fn new(state: &Arc<State>, fail_at: Option<usize>) -> Self {
        Self {
            bytes: Vec::new(),
            fail_at,
            source: Arc::new(()),
            state: state.clone(),
            flushes: 0,
        }
    }

    fn assert_original(&self, error: AsyncRenderError) {
        let AsyncRenderError::Write(error) = error else {
            panic!("expected original destination error");
        };
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        let original = error.get_ref().unwrap().downcast_ref::<Original>().unwrap();
        assert!(Arc::ptr_eq(&original.0, &self.source));
    }
}

impl Drop for Destination {
    fn drop(&mut self) {
        self.state.writer_drops.fetch_add(1, Ordering::SeqCst);
    }
}

impl AsyncSliceWriter for Destination {
    async fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        if self.fail_at == Some(self.bytes.len()) {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                Original(self.source.clone()),
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    async fn flush(&mut self) -> io::Result<()> {
        self.flushes += 1;
        Ok(())
    }
}

#[tokio::test]
async fn pinned_layout_cascade_common_subset_and_owned_retries() {
    let mut actual = Vec::new();
    for held in [false, true] {
        for fail_at in [None, Some(0), Some(1), Some(2), Some(3), Some(4)] {
            let state = Arc::new(State::default());
            state.held.store(held, Ordering::SeqCst);
            let document = declare_async_layout(
                Page(Node::new(&state, 0)),
                declare_async_layout(frame(&state, 1, "I", "i"), frame(&state, 2, "O", "o")),
            );
            assert_eq!(counts(&state.locals), [0; 3]);
            let wrapper = Box::new(&document);
            let mut destination = Destination::new(&state, fail_at);
            let result = {
                let mut render = pin!(render_incremental(&wrapper, &mut destination));
                match poll_fn(|context| Poll::Ready(render.as_mut().poll(context))).await {
                    Poll::Ready(result) => {
                        assert!(!held || matches!(fail_at, Some(0 | 1)));
                        result
                    }
                    Poll::Pending => {
                        assert!(held);
                        assert!(state.entered.load(Ordering::SeqCst));
                        assert_eq!(counts(&state.locals), [0; 3]);
                        state.release.notify_one();
                        render.await
                    }
                }
            };
            let status = match result {
                Ok(()) => {
                    assert!(fail_at.is_none());
                    "ok"
                }
                Err(error) => {
                    assert!(fail_at.is_some());
                    destination.assert_original(error);
                    "same-error"
                }
            };
            let local = counts(&state.locals);
            let label =
                fail_at.map_or_else(|| "success".into(), |offset| format!("error-{offset}"));
            actual.push(format!(
                "common/{}/{label}: status={status};output={};local={},{},{};gate={}",
                if held { "held" } else { "sync" },
                std::str::from_utf8(&destination.bytes).unwrap(),
                local[0],
                local[1],
                local[2],
                usize::from(state.entered.load(Ordering::SeqCst)),
            ));
            assert_eq!(counts(&state.owners), [0; 3]);
            assert_eq!(destination.flushes, usize::from(fail_at.is_none()));
            state.held.store(false, Ordering::SeqCst);
            destination.fail_at = None;
            for _ in 0..2 {
                destination.bytes.clear();
                render_incremental(&wrapper, &mut destination)
                    .await
                    .unwrap();
                assert_eq!(destination.bytes, b"OIPio");
                assert_eq!(counts(&state.owners), [0; 3]);
            }
            assert_eq!(counts(&state.locals), local.map(|count| count + 2));
            assert_eq!(destination.flushes, usize::from(fail_at.is_none()) + 2);
            drop(wrapper);
            assert_eq!(counts(&state.owners), [0; 3]);
            drop(document);
            assert_eq!(counts(&state.owners), [1; 3]);
            assert_eq!(state.writer_drops.load(Ordering::SeqCst), 0);
            drop(destination);
            assert_eq!(state.writer_drops.load(Ordering::SeqCst), 1);
        }
    }
    let expected: Vec<_> = include_str!("../../../tests/parity/fixtures/layout-lifecycle.txt")
        .lines()
        .filter(|line| line.starts_with("common/"))
        .collect();
    assert_eq!(expected.len(), 12);
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn dropping_borrowed_declarations_does_not_dispose_their_content_or_layouts() {
    let state = Arc::new(State::default());
    let page = Page(Node::new(&state, 0));
    let inner = frame(&state, 1, "I", "i");
    let outer = frame(&state, 2, "O", "o");
    let document = Box::new(declare_async_layout(
        &page,
        declare_async_layout(&inner, &outer),
    ));
    let mut destination = Destination::new(&state, Some(2));
    let error = render_incremental(&document, &mut destination)
        .await
        .unwrap_err();
    destination.assert_original(error);
    assert_eq!(destination.bytes, b"OI");
    destination.fail_at = None;
    destination.bytes.clear();
    render_incremental(&document, &mut destination)
        .await
        .unwrap();
    assert_eq!(destination.bytes, b"OIPio");
    drop(document);
    assert_eq!(counts(&state.owners), [0; 3]);
    drop(page);
    assert_eq!(counts(&state.owners), [1, 0, 0]);
    drop(inner);
    assert_eq!(counts(&state.owners), [1, 1, 0]);
    drop(outer);
    assert_eq!(counts(&state.owners), [1; 3]);
    assert_eq!(state.writer_drops.load(Ordering::SeqCst), 0);
    drop(destination);
    assert_eq!(state.writer_drops.load(Ordering::SeqCst), 1);
}
