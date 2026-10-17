#[path = "../examples/support/async_layout.rs"]
mod sample;

use std::{
    fmt,
    future::{poll_fn, Future},
    io,
    pin::pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll, Wake, Waker},
};

use razor_slices::{
    async_template, declare_async_layout, render_incremental, with_async_layout,
    with_async_section, AsyncHtmlWriter, AsyncLayout, AsyncRenderError, AsyncSlice,
    AsyncSliceWriter, RenderError,
};
use tokio::sync::Notify;

#[cfg(feature = "tokio")]
#[tokio::test]
async fn exact_sample_observes_prefix_before_section_release_and_repeats_borrowed_render() {
    use razor_slices::TokioWriter;
    use tokio::io::AsyncReadExt;

    let release = Notify::new();
    let sections = AtomicUsize::new(0);
    let title = String::from("<Title>");
    let body = String::from("<body> & 雪");
    let model = sample::Model {
        title: &title,
        body: &body,
        release: &release,
        sections: &sections,
    };
    let page = sample::page(&model);
    for round in 0..2 {
        let (mut sender, mut receiver) = tokio::io::duplex(1);
        let render = async { render_incremental(&page, &mut TokioWriter::new(&mut sender)).await };
        let receive = async {
            let mut prefix = vec![0; sample::PREFIX.len()];
            receiver.read_exact(&mut prefix).await.unwrap();
            assert_eq!(prefix, sample::PREFIX.as_bytes());
            assert_eq!(sections.load(Ordering::SeqCst), round);
            release.notify_one();
            let mut suffix = vec![0; sample::PAGE.len() - prefix.len()];
            receiver.read_exact(&mut suffix).await.unwrap();
            prefix.extend(suffix);
            assert_eq!(prefix, sample::PAGE.as_bytes());
        };
        let (rendered, ()) = tokio::join!(render, receive);
        rendered.unwrap();
        assert_eq!(sections.load(Ordering::SeqCst), round + 1);
    }
    assert_eq!(
        bytes(&sample::Bare::new(&Box::new(&page))).await,
        sample::PARTIAL.as_bytes()
    );
    assert_eq!(sections.load(Ordering::SeqCst), 2);
}

#[derive(Default)]
struct State {
    bytes: Vec<u8>,
    writes: usize,
    flushes: usize,
    pending_write: Option<usize>,
    pending_flush: bool,
    fail_write: Option<usize>,
    fail_flush: bool,
}

struct Writer(Arc<Mutex<State>>);

#[derive(Debug)]
struct DestinationFailure;
impl fmt::Display for DestinationFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("original layout destination failure")
    }
}
impl std::error::Error for DestinationFailure {}

impl AsyncSliceWriter for Writer {
    async fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        poll_fn(|_| {
            let mut state = self.0.lock().unwrap();
            if state.fail_write == Some(state.writes) {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    DestinationFailure,
                )));
            }
            if state.pending_write == Some(state.writes) {
                return Poll::Pending;
            }
            state.bytes.extend_from_slice(bytes);
            state.writes += 1;
            Poll::Ready(Ok(()))
        })
        .await
    }

    async fn flush(&mut self) -> io::Result<()> {
        poll_fn(|_| {
            let mut state = self.0.lock().unwrap();
            if state.fail_flush {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    DestinationFailure,
                )));
            }
            if state.pending_flush {
                return Poll::Pending;
            }
            state.flushes += 1;
            Poll::Ready(Ok(()))
        })
        .await
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

async fn bytes(slice: &impl AsyncSlice) -> Vec<u8> {
    let state = Arc::new(Mutex::new(State::default()));
    render_incremental(slice, &mut Writer(state.clone()))
        .await
        .unwrap();
    let output = state.lock().unwrap().bytes.clone();
    output
}

struct Frame {
    open: &'static str,
    close: &'static str,
    section: &'static str,
    repeats: usize,
}

impl AsyncLayout for Frame {
    async fn render_layout_into<B: AsyncSlice, W: AsyncSliceWriter + Send>(
        &self,
        body: &B,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        output.write_literal(self.open).await?;
        for _ in 0..self.repeats {
            output.write_section(body, self.section).await?;
        }
        // Direct full-render calls on the supplied body also bypass declarations.
        body.render_into(output).await?;
        output.write_literal(self.close).await?;
        Ok(())
    }
}

fn frame(open: &'static str, close: &'static str) -> Frame {
    Frame {
        open,
        close,
        section: "header",
        repeats: 0,
    }
}

struct ForwardingLayout;

impl AsyncLayout for ForwardingLayout {
    async fn render_layout_into<B: AsyncSlice, W: AsyncSliceWriter + Send>(
        &self,
        body: &B,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        body.render_into(output).await
    }

    async fn render_layout_section_into<B: AsyncSlice, W: AsyncSliceWriter + Send>(
        &self,
        body: &B,
        name: &str,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        if name == "forwarded" {
            output.write_literal("[").await?;
            output.write_section(body, "header").await?;
            body.render_into(output).await?;
            output.write_literal("]").await?;
        } else {
            output.write_section(body, name).await?;
        }
        Ok(())
    }
}

struct TailLayout;

impl AsyncLayout for TailLayout {
    async fn render_layout_into<B: AsyncSlice, W: AsyncSliceWriter + Send>(
        &self,
        body: &B,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        body.render_into(output).await?;
        output.write_section(body, "forwarded").await?;
        output.write_section(body, "forwarded").await
    }
}

#[tokio::test]
async fn layout_owned_section_keeps_its_child_context_after_body_and_repeated_insertion() {
    let calls = AtomicUsize::new(0);
    let body = with_async_section(
        sample::PageBody::new("body"),
        "header",
        Count {
            text: "child header",
            calls: &calls,
        },
    )
    .unwrap();
    let body = declare_async_layout(body, frame("wrong", "wrong"));
    let layout = declare_async_layout(Box::new(&ForwardingLayout), Box::new(&TailLayout));
    let page = declare_async_layout(body, layout);
    for round in 1..=2 {
        assert_eq!(
            bytes(&Box::new(&page)).await,
            b"<p>body</p>[child header<p>body</p>][child header<p>body</p>]",
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2 * round);
    }
}

#[tokio::test]
async fn three_declared_layout_levels_keep_distinct_immediate_section_providers() {
    let body = with_async_section(
        sample::Text::new("body"),
        "header",
        sample::Text::new("page"),
    )
    .unwrap();
    let inner = with_async_section(
        Frame {
            repeats: 1,
            ..frame("<inner>", "</inner>")
        },
        "header",
        sample::Text::new("inner"),
    )
    .unwrap();
    let middle = with_async_section(
        Frame {
            repeats: 1,
            ..frame("<middle>", "</middle>")
        },
        "header",
        sample::Text::new("middle"),
    )
    .unwrap();
    let outer = Frame {
        repeats: 1,
        ..frame("<outer>", "</outer>")
    };
    let page = declare_async_layout(
        body,
        declare_async_layout(inner, declare_async_layout(middle, outer)),
    );
    assert_eq!(
        bytes(&page).await,
        b"<outer>middle<middle>inner<inner>pagebody</inner></middle></outer>",
    );
}

struct Count<'a> {
    text: &'a str,
    calls: &'a AtomicUsize,
}

impl AsyncSlice for Count<'_> {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        output.write_value(self.text).await?;
        Ok(())
    }
}

async_template! {
    struct InsertSection<'a, B: AsyncSlice> { model: (&'a B, &'a str) } => {
        @section { model.0 }, { model.1 };
    }
}

#[tokio::test]
async fn nested_context_is_immediate_content_not_recursive_name_search() {
    let page_calls = AtomicUsize::new(0);
    let inner_calls = AtomicUsize::new(0);
    let never_calls = AtomicUsize::new(0);
    let body = with_async_section(
        sample::PageBody::new("body"),
        "header",
        Count {
            text: "page header",
            calls: &page_calls,
        },
    )
    .unwrap();
    let body = with_async_section(
        body,
        "footer",
        Count {
            text: "wrong page footer",
            calls: &never_calls,
        },
    )
    .unwrap();
    let inner = with_async_section(
        Frame {
            repeats: 2,
            ..frame("<inner>", "</inner>")
        },
        "footer",
        Count {
            text: "inner & footer",
            calls: &inner_calls,
        },
    )
    .unwrap();
    let outer = Frame {
        open: "<outer>",
        close: "</outer>",
        section: "footer",
        repeats: 2,
    };
    let page = declare_async_layout(
        body,
        declare_async_layout(Box::new(&inner), Box::new(&outer)),
    );
    assert_eq!(page_calls.load(Ordering::SeqCst), 0);
    for round in 1..=2 {
        assert_eq!(
            bytes(&Box::new(&page)).await,
            b"<outer>inner &amp; footerinner &amp; footer<inner>page headerpage header<p>body</p></inner></outer>",
        );
        assert_eq!(page_calls.load(Ordering::SeqCst), 2 * round);
        assert_eq!(inner_calls.load(Ordering::SeqCst), 2 * round);
        assert_eq!(never_calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn unknown_section_on_inner_does_not_fall_back_to_page_provider() {
    let calls = AtomicUsize::new(0);
    let body = with_async_section(
        sample::PageBody::new("body"),
        "header",
        Count {
            text: "hidden",
            calls: &calls,
        },
    )
    .unwrap();
    let page = declare_async_layout(
        body,
        declare_async_layout(
            frame("<inner>", "</inner>"),
            Frame {
                repeats: 1,
                ..frame("<outer>", "</outer>")
            },
        ),
    );
    assert_eq!(
        bytes(&page).await,
        b"<outer><inner><p>body</p></inner></outer>"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn sections_before_and_after_declaration_and_child_wrappers_preserve_dispatch() {
    let calls = AtomicUsize::new(0);
    for after in [false, true] {
        let section = Count {
            text: "<header>",
            calls: &calls,
        };
        let layout = Frame {
            repeats: 1,
            ..frame("<layout>", "</layout>")
        };
        if after {
            let declared = declare_async_layout(sample::PageBody::new("body"), layout);
            let page = with_async_section(declared, "header", section).unwrap();
            // The outer association supplies sections to consumers, not to the
            // already-declared inner layout's original body.
            assert_eq!(bytes(&page).await, b"<layout><p>body</p></layout>");
            assert_eq!(
                bytes(&InsertSection::new((&page, "header"))).await,
                b"&lt;header&gt;"
            );
            assert_eq!(
                bytes(&sample::Bare::new(&Box::new(&page))).await,
                b"<p>body</p>"
            );
        } else {
            let body =
                with_async_section(sample::PageBody::new("body"), "header", section).unwrap();
            let page = declare_async_layout(body, layout);
            assert_eq!(
                bytes(&page).await,
                b"<layout>&lt;header&gt;<p>body</p></layout>"
            );
            assert_eq!(
                bytes(&InsertSection::new((&Box::new(&page), "header"))).await,
                b"&lt;header&gt;"
            );
            assert_eq!(
                bytes(&sample::Bare::new(&Box::new(&page))).await,
                b"<p>body</p>"
            );
        }
    }
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn section_content_bypasses_declarations_but_preserves_explicit_composition() {
    let declared = declare_async_layout(sample::Text::new("<section>"), frame("wrong", "wrong"));
    let body = with_async_section(sample::Text::new("body"), "header", declared).unwrap();
    assert_eq!(
        bytes(&InsertSection::new((&body, "header"))).await,
        b"&lt;section&gt;"
    );
    let explicit = with_async_layout(sample::Text::new("<section>"), frame("<s>", "</s>"));
    let body = with_async_section(sample::Text::new("body"), "header", explicit).unwrap();
    assert_eq!(
        bytes(&InsertSection::new((&body, "header"))).await,
        b"<s>&lt;section&gt;</s>"
    );
}

#[tokio::test]
async fn explicit_documents_remain_composed_as_children_but_supplied_bodies_are_bare() {
    let declared = declare_async_layout(sample::PageBody::new("body"), frame("wrong", "wrong"));
    let explicit = with_async_layout(declared, frame("<explicit>", "</explicit>"));
    let page = declare_async_layout(explicit, frame("<declared>", "</declared>"));
    assert_eq!(
        bytes(&page).await,
        b"<declared><explicit><p>body</p></explicit></declared>"
    );
    assert_eq!(
        bytes(&sample::Bare::new(&page)).await,
        b"<explicit><p>body</p></explicit>"
    );
    assert_eq!(
        bytes(&sample::Bare::new(&sample::PageBody::new("<body> & 雪"))).await,
        sample::PARTIAL.as_bytes(),
    );
}

#[tokio::test]
async fn outermost_section_association_wins_and_names_are_case_sensitive() {
    let calls = AtomicUsize::new(0);
    let inner = with_async_section(
        frame("", ""),
        "footer",
        Count {
            text: "shadowed",
            calls: &calls,
        },
    )
    .unwrap();
    let inner = with_async_section(inner, "footer", sample::Text::new("winner")).unwrap();
    let page = declare_async_layout(
        sample::Text::new("body"),
        declare_async_layout(
            inner,
            Frame {
                section: "footer",
                repeats: 1,
                ..frame("", "")
            },
        ),
    );
    assert_eq!(bytes(&page).await, b"winnerbody");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(bytes(&InsertSection::new((&page, "Footer"))).await, b"");
}

#[tokio::test]
async fn empty_names_fail_before_provider_work_through_nested_layouts() {
    let calls = AtomicUsize::new(0);
    let body = with_async_section(
        sample::Text::new("body"),
        "header",
        Count {
            text: "header",
            calls: &calls,
        },
    )
    .unwrap();
    let page = declare_async_layout(
        body,
        Frame {
            section: "",
            repeats: 1,
            ..frame("prefix", "suffix")
        },
    );
    let state = Arc::new(Mutex::new(State::default()));
    let error = render_incremental(&page, &mut Writer(state.clone()))
        .await
        .unwrap_err();
    assert!(
        matches!(error, AsyncRenderError::Write(ref e) if e.kind() == io::ErrorKind::InvalidInput)
    );
    assert_eq!(state.lock().unwrap().bytes, b"prefix");
    assert_eq!(state.lock().unwrap().flushes, 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(with_async_section(frame("", ""), "", sample::Text::new("x")).is_err());
}

#[test]
fn generated_nested_sample_flush_backpressure_precedes_section_await() {
    let release = Notify::new();
    let sections = AtomicUsize::new(0);
    let model = sample::Model {
        title: "<Title>",
        body: "<body> & 雪",
        release: &release,
        sections: &sections,
    };
    let page = sample::page(&model);
    let state = Arc::new(Mutex::new(State {
        pending_flush: true,
        ..State::default()
    }));
    let mut writer = Writer(state.clone());
    let waker = waker();
    let mut cx = Context::from_waker(&waker);
    let mut future = pin!(render_incremental(&page, &mut writer));
    assert_send(future.as_mut());
    assert!(future.as_mut().poll(&mut cx).is_pending());
    assert_eq!(state.lock().unwrap().bytes, sample::PREFIX.as_bytes());
    assert_eq!(state.lock().unwrap().flushes, 0);
    assert_eq!(sections.load(Ordering::SeqCst), 0);
    release.notify_one();
    // Even released application work cannot run before the pending flush.
    assert!(future.as_mut().poll(&mut cx).is_pending());
    assert_eq!(sections.load(Ordering::SeqCst), 0);
    state.lock().unwrap().pending_flush = false;
    assert!(matches!(future.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
    assert_eq!(state.lock().unwrap().bytes, sample::PAGE.as_bytes());
    assert_eq!(state.lock().unwrap().flushes, 2);
}

#[test]
fn every_nested_sample_write_preserves_original_failure_and_stops_suffix() {
    // Complete first to get the exact number of writes and byte boundaries.
    let release = Notify::new();
    let sections = AtomicUsize::new(0);
    let model = sample::Model {
        title: "<Title>",
        body: "<body> & 雪",
        release: &release,
        sections: &sections,
    };
    let page = sample::page(&model);
    let state = Arc::new(Mutex::new(State::default()));
    let mut writer = Writer(state.clone());
    let waker = waker();
    let mut cx = Context::from_waker(&waker);
    release.notify_one();
    assert!(matches!(
        pin!(render_incremental(&page, &mut writer))
            .as_mut()
            .poll(&mut cx),
        Poll::Ready(Ok(()))
    ));
    let total = state.lock().unwrap().writes;
    assert_eq!(
        total, 15,
        "sample boundary count must match pending-write coverage"
    );
    for fail_at in 0..total {
        let state = Arc::new(Mutex::new(State {
            fail_write: Some(fail_at),
            ..State::default()
        }));
        let mut writer = Writer(state.clone());
        release.notify_one();
        let mut future = pin!(render_incremental(&page, &mut writer));
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(Err(AsyncRenderError::Write(error))) => {
                assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
                assert!(error.get_ref().unwrap().is::<DestinationFailure>());
            }
            other => panic!("failure at write {fail_at}: {other:?}"),
        }
        let state = state.lock().unwrap();
        assert_eq!(state.writes, fail_at);
        assert!(sample::PAGE.as_bytes().starts_with(&state.bytes));
        assert!(state.bytes.len() < sample::PAGE.len());
        assert!(state.flushes <= 1, "no final flush after failure");
    }
}

#[test]
fn pending_write_at_every_nested_boundary_prevents_advance_then_resumes_in_order() {
    for pending_at in 0..15 {
        let release = Notify::new();
        let sections = AtomicUsize::new(0);
        let model = sample::Model {
            title: "<Title>",
            body: "<body> & 雪",
            release: &release,
            sections: &sections,
        };
        let page = sample::page(&model);
        let state = Arc::new(Mutex::new(State {
            pending_write: Some(pending_at),
            ..State::default()
        }));
        let mut writer = Writer(state.clone());
        let waker = waker();
        let mut cx = Context::from_waker(&waker);
        release.notify_one();
        let mut future = pin!(render_incremental(&page, &mut writer));
        assert!(future.as_mut().poll(&mut cx).is_pending());
        let prefix = state.lock().unwrap().bytes.clone();
        assert_eq!(state.lock().unwrap().writes, pending_at);
        assert!(sample::PAGE.as_bytes().starts_with(&prefix));
        assert!(future.as_mut().poll(&mut cx).is_pending());
        assert_eq!(state.lock().unwrap().bytes, prefix);
        state.lock().unwrap().pending_write = None;
        assert!(matches!(future.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
        assert_eq!(state.lock().unwrap().bytes, sample::PAGE.as_bytes());
        assert_eq!(state.lock().unwrap().flushes, 2);
        assert_eq!(sections.load(Ordering::SeqCst), 1);
    }
}

struct PendingSection<'a> {
    entered: &'a AtomicUsize,
    dropped: &'a AtomicUsize,
    finished: &'a AtomicUsize,
}

struct DropGuard<'a>(&'a AtomicUsize);
impl Drop for DropGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

impl AsyncSlice for PendingSection<'_> {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        let _guard = DropGuard(self.dropped);
        self.entered.fetch_add(1, Ordering::SeqCst);
        output.write_literal("<section>").await?;
        std::future::pending::<()>().await;
        self.finished.fetch_add(1, Ordering::SeqCst);
        output.write_literal("</section>").await?;
        Ok(())
    }
}

#[test]
fn dropping_nested_section_or_pending_write_drops_work_without_finishing_or_flushing() {
    for pending_write in [None, Some(0), Some(1), Some(2)] {
        let entered = AtomicUsize::new(0);
        let dropped = AtomicUsize::new(0);
        let finished = AtomicUsize::new(0);
        let body = with_async_section(
            sample::Text::new("body"),
            "header",
            PendingSection {
                entered: &entered,
                dropped: &dropped,
                finished: &finished,
            },
        )
        .unwrap();
        let page = declare_async_layout(
            body,
            declare_async_layout(
                Frame {
                    repeats: 1,
                    ..frame("<inner>", "</inner>")
                },
                frame("<outer>", "</outer>"),
            ),
        );
        let state = Arc::new(Mutex::new(State {
            pending_write,
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
        let expected = match pending_write {
            Some(0) => "",
            Some(1) => "<outer>",
            Some(2) => "<outer><inner>",
            None => "<outer><inner><section>",
            _ => unreachable!(),
        };
        assert_eq!(state.lock().unwrap().bytes, expected.as_bytes());
        assert_eq!(state.lock().unwrap().flushes, 0);
        assert_eq!(
            entered.load(Ordering::SeqCst),
            usize::from(pending_write.is_none() || pending_write == Some(2))
        );
        assert_eq!(
            dropped.load(Ordering::SeqCst),
            entered.load(Ordering::SeqCst)
        );
        assert_eq!(finished.load(Ordering::SeqCst), 0);
    }
}

struct Failing;
impl AsyncSlice for Failing {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        _: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        Err(AsyncRenderError::Render(RenderError(fmt::Error)))
    }
}

#[tokio::test]
async fn body_and_section_template_errors_keep_boundary_and_stop_outer_suffix() {
    for section_failure in [false, true] {
        let state = Arc::new(Mutex::new(State::default()));
        let mut writer = Writer(state.clone());
        let result = if section_failure {
            let body = with_async_section(sample::Text::new("body"), "header", Failing).unwrap();
            let page = declare_async_layout(
                body,
                declare_async_layout(
                    Frame {
                        repeats: 1,
                        ..frame("<inner>", "</inner>")
                    },
                    frame("<outer>", "</outer>"),
                ),
            );
            render_incremental(&page, &mut writer).await
        } else {
            let page = declare_async_layout(
                Failing,
                declare_async_layout(frame("<inner>", "</inner>"), frame("<outer>", "</outer>")),
            );
            render_incremental(&page, &mut writer).await
        };
        assert!(matches!(result, Err(AsyncRenderError::Render(_))));
        assert_eq!(state.lock().unwrap().bytes, b"<outer><inner>");
        assert_eq!(state.lock().unwrap().flushes, 0);
    }
}

#[test]
fn flush_failure_keeps_original_error_before_section_work() {
    let release = Notify::new();
    let sections = AtomicUsize::new(0);
    let model = sample::Model {
        title: "<Title>",
        body: "<body> & 雪",
        release: &release,
        sections: &sections,
    };
    let page = sample::page(&model);
    let state = Arc::new(Mutex::new(State {
        fail_flush: true,
        ..State::default()
    }));
    let mut writer = Writer(state.clone());
    let waker = waker();
    match pin!(render_incremental(&page, &mut writer))
        .as_mut()
        .poll(&mut Context::from_waker(&waker))
    {
        Poll::Ready(Err(AsyncRenderError::Write(error))) => {
            assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
            assert!(error.get_ref().unwrap().is::<DestinationFailure>());
        }
        other => panic!("unexpected flush failure: {other:?}"),
    }
    assert_eq!(state.lock().unwrap().bytes, sample::PREFIX.as_bytes());
    assert_eq!(state.lock().unwrap().flushes, 0);
    assert_eq!(sections.load(Ordering::SeqCst), 0);
}
