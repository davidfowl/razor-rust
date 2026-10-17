use std::{
    future::{poll_fn, Future},
    io,
    pin::pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Wake, Waker},
};

use razor_slices::{
    async_template, attribute, declare_async_layout, render_incremental,
    render_incremental_with_context, with_async_layout, with_async_section, AsyncHtmlWriter,
    AsyncLayout, AsyncRenderError, AsyncSlice, AsyncSliceWriter, CancellationToken, HtmlString,
    RenderCancelled, RenderContext,
};

const THRESHOLD: usize = 16_384;

#[derive(Default)]
struct State {
    bytes: Vec<u8>,
    flushes: Vec<usize>,
    pending: bool,
    pending_write: bool,
    fail_write: bool,
    fail_flush: bool,
    later: bool,
}

struct Writer(Arc<Mutex<State>>);

#[derive(Debug)]
struct Original;
impl std::fmt::Display for Original {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("original sink error")
    }
}
impl std::error::Error for Original {}

impl AsyncSliceWriter for Writer {
    async fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        poll_fn(|_| {
            let mut state = self.0.lock().unwrap();
            if state.fail_write {
                return Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, Original)));
            }
            if state.pending_write {
                return Poll::Pending;
            }
            state.bytes.extend_from_slice(bytes);
            Poll::Ready(Ok(()))
        })
        .await
    }

    async fn flush(&mut self) -> io::Result<()> {
        poll_fn(|_| {
            let mut state = self.0.lock().unwrap();
            if state.fail_flush {
                return Poll::Ready(Err(io::Error::other(Original)));
            }
            if state.pending {
                return Poll::Pending;
            }
            let len = state.bytes.len();
            state.flushes.push(len);
            Poll::Ready(Ok(()))
        })
        .await
    }
}

struct Noop;
impl Wake for Noop {
    fn wake(self: Arc<Self>) {}
}

fn ready<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(Noop));
    let mut future = pin!(future);
    match future.as_mut().poll(&mut Context::from_waker(&waker)) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("unexpected pending operation"),
    }
}

struct Literal<'a>(&'a str);
impl<'a> Literal<'a> {
    fn new(text: &'a str) -> Self {
        Self(text)
    }
}
impl AsyncSlice for Literal<'_> {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        out: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        out.write_literal(self.0).await?;
        Ok(())
    }
}

#[test]
fn inclusive_output_byte_boundaries_and_large_single_write() {
    for size in [THRESHOLD - 1, THRESHOLD, THRESHOLD + 1, THRESHOLD * 4] {
        let state = Arc::new(Mutex::new(State::default()));
        let mut writer = Writer(state.clone());
        let mut out = AsyncHtmlWriter::new(&mut writer);
        let text = "x".repeat(size);
        ready(out.write_slice(&Literal::new(&text))).unwrap();
        let state = state.lock().unwrap();
        assert_eq!(state.bytes, text.as_bytes());
        assert_eq!(
            state.flushes,
            if size >= THRESHOLD {
                vec![size]
            } else {
                vec![]
            }
        );
    }
}

#[test]
fn writes_do_not_flush_until_successful_child_boundary_and_empty_child_counts() {
    let state = Arc::new(Mutex::new(State::default()));
    let mut writer = Writer(state.clone());
    let mut out = AsyncHtmlWriter::new(&mut writer);
    ready(out.write_literal(&"x".repeat(THRESHOLD))).unwrap();
    assert!(state.lock().unwrap().flushes.is_empty());
    ready(out.write_slice(&Literal::new(""))).unwrap();
    assert_eq!(state.lock().unwrap().flushes, [THRESHOLD]);
}

#[test]
fn accumulation_and_explicit_flush_reset_are_shared_across_children() {
    let state = Arc::new(Mutex::new(State::default()));
    let mut writer = Writer(state.clone());
    let mut out = AsyncHtmlWriter::new(&mut writer);
    let half = "x".repeat(THRESHOLD / 2);
    ready(out.write_slice(&Literal::new(&half))).unwrap();
    ready(out.write_slice(&Literal::new(&half))).unwrap();
    ready(out.write_slice(&Literal::new(&half))).unwrap();
    ready(out.flush()).unwrap();
    ready(out.write_slice(&Literal::new(&half))).unwrap();
    assert_eq!(
        state.lock().unwrap().flushes,
        [THRESHOLD, THRESHOLD + THRESHOLD / 2]
    );
}

struct Mixed<'a> {
    padding: &'a str,
}
impl AsyncSlice for Mixed<'_> {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        out: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        out.write_literal(self.padding).await?;
        out.write_value("\u{96ea}&").await?;
        out.write_html_value(&HtmlString::from("<b>")).await?;
        out.write_html_value(&42_u32).await?;
        out.write_html_value(&attribute("title", "<&").unwrap())
            .await?;
        Ok(())
    }
}

#[test]
fn multibyte_escaped_trusted_numeric_attribute_output_counts_not_input_characters() {
    let suffix = "\u{96ea}&amp;<b>42 title=\"&lt;&amp;\"";
    for size in [THRESHOLD - 1, THRESHOLD, THRESHOLD + 1] {
        let state = Arc::new(Mutex::new(State::default()));
        let mut writer = Writer(state.clone());
        let mut out = AsyncHtmlWriter::new(&mut writer);
        let padding = "x".repeat(size - suffix.len());
        ready(out.write_slice(&Mixed { padding: &padding })).unwrap();
        let state = state.lock().unwrap();
        assert_eq!(state.bytes, format!("{padding}{suffix}").as_bytes());
        assert_eq!(
            state.flushes,
            if size >= THRESHOLD {
                vec![size]
            } else {
                vec![]
            }
        );
    }
}

async_template! {
    struct Escaped<'a> { model: &'a str } => { {model} }
}

#[test]
fn many_encoded_chunks_accumulate_without_per_chunk_flushes() {
    let value = "&\u{96ea}".repeat(4_096);
    let expected = "&amp;\u{96ea}".repeat(4_096);
    let state = Arc::new(Mutex::new(State::default()));
    let mut writer = Writer(state.clone());
    let mut out = AsyncHtmlWriter::new(&mut writer);
    ready(out.write_slice(&Escaped::new(&value))).unwrap();
    let state = state.lock().unwrap();
    assert_eq!(state.bytes, expected.as_bytes());
    assert_eq!(state.flushes, [expected.len()]);
}

struct Parent {
    state: Arc<Mutex<State>>,
    prefix: String,
}
async_template! {
    struct GeneratedParent<'a> { model: &'a Parent } => {
        @render { Literal::new(&model.prefix) };
        @{ model.state.lock().unwrap().later = true; }
        "tail"
    }
}

#[test]
fn pending_automatic_flush_blocks_later_code_and_drop_stops_rendering() {
    for drop_pending in [false, true] {
        let state = Arc::new(Mutex::new(State {
            pending: true,
            ..State::default()
        }));
        let parent = Parent {
            state: state.clone(),
            prefix: "x".repeat(THRESHOLD),
        };
        let page = GeneratedParent::new(&parent);
        let mut writer = Writer(state.clone());
        let waker = Waker::from(Arc::new(Noop));
        {
            let mut future = pin!(render_incremental(&page, &mut writer));
            assert!(future
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending());
            assert!(!state.lock().unwrap().later);
            state.lock().unwrap().pending = false;
            if !drop_pending {
                assert!(matches!(
                    future.as_mut().poll(&mut Context::from_waker(&waker)),
                    Poll::Ready(Ok(()))
                ));
            }
        }
        let state = state.lock().unwrap();
        assert_eq!(state.later, !drop_pending);
        assert_eq!(
            state.bytes.len(),
            THRESHOLD + if drop_pending { 0 } else { 4 }
        );
        assert_eq!(state.flushes.len(), if drop_pending { 0 } else { 2 });
    }
}

#[test]
fn cancellation_interrupts_pending_automatic_flush_without_suffix_or_final_flush() {
    let token = CancellationToken::new();
    let context = RenderContext::new((), token.clone());
    let state = Arc::new(Mutex::new(State {
        pending: true,
        ..State::default()
    }));
    let parent = Parent {
        state: state.clone(),
        prefix: "x".repeat(THRESHOLD),
    };
    let page = GeneratedParent::new(&parent);
    let mut writer = Writer(state.clone());
    let waker = Waker::from(Arc::new(Noop));
    let mut future = pin!(render_incremental_with_context(
        &page,
        &mut writer,
        &context
    ));
    assert!(future
        .as_mut()
        .poll(&mut Context::from_waker(&waker))
        .is_pending());
    assert_eq!(state.lock().unwrap().bytes, parent.prefix.as_bytes());
    assert!(!state.lock().unwrap().later);
    token.cancel();
    let Poll::Ready(Err(AsyncRenderError::Write(error))) =
        future.as_mut().poll(&mut Context::from_waker(&waker))
    else {
        panic!("cancellation must interrupt the automatic flush");
    };
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    assert!(error.get_ref().unwrap().is::<RenderCancelled>());
    let state = state.lock().unwrap();
    assert_eq!(state.bytes, parent.prefix.as_bytes());
    assert!(!state.later);
    assert!(state.flushes.is_empty());
}

#[test]
fn pending_write_does_not_count_flush_or_advance_application_work() {
    let state = Arc::new(Mutex::new(State {
        pending_write: true,
        ..State::default()
    }));
    let parent = Parent {
        state: state.clone(),
        prefix: "x".repeat(THRESHOLD),
    };
    let page = GeneratedParent::new(&parent);
    let mut writer = Writer(state.clone());
    let waker = Waker::from(Arc::new(Noop));
    {
        let mut future = pin!(render_incremental(&page, &mut writer));
        assert!(future
            .as_mut()
            .poll(&mut Context::from_waker(&waker))
            .is_pending());
        let state = state.lock().unwrap();
        assert!(state.bytes.is_empty());
        assert!(state.flushes.is_empty());
        assert!(!state.later);
    }
    state.lock().unwrap().pending_write = false;
    assert!(!state.lock().unwrap().later);
}

async_template! {
    struct HeldRoot<'a> { model: &'a str } => {
        @render { Literal::new("") };
        {HtmlString::from(*model)}
        @await { std::future::pending::<()>() };
    }
}

#[test]
fn crossing_threshold_before_root_await_does_not_eagerly_flush() {
    let state = Arc::new(Mutex::new(State::default()));
    let text = "x".repeat(THRESHOLD + 1);
    let page = HeldRoot::new(&text);
    let mut writer = Writer(state.clone());
    let waker = Waker::from(Arc::new(Noop));
    let mut future = pin!(render_incremental(&page, &mut writer));
    assert!(future
        .as_mut()
        .poll(&mut Context::from_waker(&waker))
        .is_pending());
    let state = state.lock().unwrap();
    assert_eq!(state.bytes, text.as_bytes());
    assert!(state.flushes.is_empty());
}

#[test]
fn requested_pending_flush_resets_count_even_if_its_future_is_dropped() {
    let state = Arc::new(Mutex::new(State {
        pending: true,
        ..State::default()
    }));
    let mut writer = Writer(state.clone());
    let mut out = AsyncHtmlWriter::new(&mut writer);
    let text = "x".repeat(THRESHOLD);
    let child = Literal::new(&text);
    let waker = Waker::from(Arc::new(Noop));
    {
        let mut future = pin!(out.write_slice(&child));
        assert!(future
            .as_mut()
            .poll(&mut Context::from_waker(&waker))
            .is_pending());
    }
    state.lock().unwrap().pending = false;
    ready(out.write_slice(&Literal::new("x"))).unwrap();
    assert!(state.lock().unwrap().flushes.is_empty());
}

#[test]
fn original_write_or_auto_flush_errors_stop_later_code_and_final_flush() {
    for fail_write in [true, false] {
        let state = Arc::new(Mutex::new(State {
            fail_write,
            fail_flush: !fail_write,
            ..State::default()
        }));
        let parent = Parent {
            state: state.clone(),
            prefix: "x".repeat(THRESHOLD),
        };
        let mut writer = Writer(state.clone());
        let error = ready(render_incremental(
            &GeneratedParent::new(&parent),
            &mut writer,
        ))
        .unwrap_err();
        let AsyncRenderError::Write(error) = error else {
            panic!("wrong error");
        };
        assert!(error.get_ref().unwrap().is::<Original>());
        assert!(!state.lock().unwrap().later);
        assert!(state.lock().unwrap().flushes.is_empty());
    }
}

struct Failing;
impl AsyncSlice for Failing {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        out: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        out.write_literal(&"x".repeat(THRESHOLD)).await?;
        Err(io::Error::other(Original).into())
    }
}

#[test]
fn failed_child_is_not_a_successful_flush_boundary() {
    let state = Arc::new(Mutex::new(State::default()));
    let mut writer = Writer(state.clone());
    let mut out = AsyncHtmlWriter::new(&mut writer);
    assert!(ready(out.write_slice(&Failing)).is_err());
    assert!(state.lock().unwrap().flushes.is_empty());
}

struct Section;
impl AsyncSlice for Section {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        _: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        Ok(())
    }
    async fn render_section_into<W: AsyncSliceWriter + Send>(
        &self,
        _: &str,
        out: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        out.write_literal(&"s".repeat(THRESHOLD)).await?;
        Ok(())
    }
}

#[test]
fn plain_section_dispatch_is_not_a_boundary_but_section_child_is() {
    let state = Arc::new(Mutex::new(State::default()));
    let mut writer = Writer(state.clone());
    let mut out = AsyncHtmlWriter::new(&mut writer);
    ready(out.write_section(&Section, "scripts")).unwrap();
    assert!(state.lock().unwrap().flushes.is_empty());
    let child = with_async_section(Section, "scripts", Literal::new("")).unwrap();
    ready(out.write_section(&child, "scripts")).unwrap();
    assert_eq!(state.lock().unwrap().flushes, [THRESHOLD]);
}

struct Layout;
impl AsyncLayout for Layout {
    async fn render_layout_into<B: AsyncSlice, W: AsyncSliceWriter + Send>(
        &self,
        body: &B,
        out: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        out.write_literal("[").await?;
        body.render_body_into(out).await?;
        out.write_literal("]").await?;
        Ok(())
    }
}

#[test]
fn nested_declared_layout_body_boundary_and_partial_bypass_repeat_exactly() {
    let text = "x".repeat(THRESHOLD - 2);
    let page = declare_async_layout(Literal::new(&text), declare_async_layout(Layout, Layout));
    let state = Arc::new(Mutex::new(State::default()));
    let mut writer = Writer(state.clone());
    for _ in 0..2 {
        ready(render_incremental(&page, &mut writer)).unwrap();
    }
    {
        let state = state.lock().unwrap();
        let document = format!("[[{text}]]");
        assert_eq!(state.bytes, document.repeat(2).as_bytes());
        assert_eq!(
            state.flushes,
            [
                THRESHOLD,
                THRESHOLD + 2,
                2 * THRESHOLD + 2,
                2 * THRESHOLD + 4
            ]
        );
    }
    let mut out = AsyncHtmlWriter::new(&mut writer);
    ready(out.write_slice(&page)).unwrap();
    assert!(state.lock().unwrap().bytes.ends_with(text.as_bytes()));
    assert_eq!(state.lock().unwrap().flushes.len(), 4);
}

#[test]
fn scoped_nested_layouts_keep_completion_accounting_across_repeated_renders() {
    let context = RenderContext::new("request", CancellationToken::new());
    let text = "x".repeat(THRESHOLD - 2);
    let page = declare_async_layout(Literal::new(&text), declare_async_layout(Layout, Layout));
    let state = Arc::new(Mutex::new(State::default()));
    let mut writer = Writer(state.clone());
    for _ in 0..2 {
        ready(render_incremental_with_context(
            &page,
            &mut writer,
            &context,
        ))
        .unwrap();
    }
    let state = state.lock().unwrap();
    assert_eq!(state.bytes, format!("[[{text}]]").repeat(2).as_bytes());
    assert_eq!(
        state.flushes,
        [
            THRESHOLD,
            THRESHOLD + 2,
            2 * THRESHOLD + 2,
            2 * THRESHOLD + 4
        ]
    );
    assert!(!context.cancellation_token().is_cancelled());
}

#[test]
fn completed_layout_flushes_after_its_body_remained_below_threshold() {
    let text = "x".repeat(THRESHOLD - 2);
    for explicit in [false, true] {
        let state = Arc::new(Mutex::new(State::default()));
        let mut writer = Writer(state.clone());
        let mut out = AsyncHtmlWriter::new(&mut writer);
        if explicit {
            let page = with_async_layout(Literal::new(&text), Layout);
            ready(out.write_slice(&page)).unwrap();
        } else {
            let page = declare_async_layout(Literal::new(&text), Layout);
            ready(page.render_into(&mut out)).unwrap();
        }
        let state = state.lock().unwrap();
        assert_eq!(state.bytes, format!("[{text}]").as_bytes());
        assert_eq!(state.flushes, [THRESHOLD]);
    }

    let text = "x".repeat(THRESHOLD - 3);
    let page = declare_async_layout(Literal::new(&text), declare_async_layout(Layout, Layout));
    let state = Arc::new(Mutex::new(State::default()));
    let mut writer = Writer(state.clone());
    ready(render_incremental(&page, &mut writer)).unwrap();
    let state = state.lock().unwrap();
    assert_eq!(state.bytes, format!("[[{text}]]").as_bytes());
    assert_eq!(state.flushes, [THRESHOLD, THRESHOLD + 1]);
}

#[test]
fn layout_owned_section_child_auto_flushes_on_each_repeated_insertion() {
    let text = "x".repeat(THRESHOLD);
    let layout = with_async_section(Layout, "scripts", Literal::new(&text)).unwrap();
    let state = Arc::new(Mutex::new(State::default()));
    let mut writer = Writer(state.clone());
    let mut out = AsyncHtmlWriter::new(&mut writer);
    for _ in 0..2 {
        ready(layout.render_layout_section_into(&Literal::new(""), "scripts", &mut out)).unwrap();
    }
    let state = state.lock().unwrap();
    assert_eq!(state.bytes, text.repeat(2).as_bytes());
    assert_eq!(state.flushes, [THRESHOLD, THRESHOLD * 2]);
}
