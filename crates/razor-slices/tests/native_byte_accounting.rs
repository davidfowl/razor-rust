use razor_slices::{
    async_template, declare_async_layout, html, render_async, render_incremental,
    with_async_section, AsyncHtmlWriter, AsyncLayout, AsyncRenderError, AsyncSlice,
    AsyncSliceWriter, CancellationToken, RenderCancelled,
};
use std::{
    future::{pending, poll_fn, Future},
    io,
    pin::pin,
    sync::{Arc, Mutex},
    task::Poll,
};

const THRESHOLD: usize = 16_384;

struct Literal<'a>(&'a str);

impl AsyncSlice for Literal<'_> {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        output.write_literal(self.0).await?;
        Ok(())
    }
}

#[derive(Default)]
struct State {
    pending: usize,
    flushes: Vec<usize>,
    native_reads: usize,
    drain_on_write: bool,
    fail_write_after: Option<usize>,
    pending_write_after: Option<usize>,
    fail_flush: bool,
    pending_flush: bool,
}

struct Writer {
    state: Arc<Mutex<State>>,
    native: bool,
}

fn writer(native: bool, prefilled: usize) -> (Writer, Arc<Mutex<State>>) {
    let state = Arc::new(Mutex::new(State {
        pending: prefilled,
        ..State::default()
    }));
    (
        Writer {
            state: state.clone(),
            native,
        },
        state,
    )
}

#[derive(Debug)]
struct Original;

impl std::fmt::Display for Original {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("original sink error")
    }
}

impl std::error::Error for Original {}

fn assert_original(error: io::Error) {
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    assert!(error.get_ref().unwrap().is::<Original>());
}

impl AsyncSliceWriter for Writer {
    fn unflushed_bytes(&self) -> Option<usize> {
        if self.native {
            let mut state = self.state.lock().unwrap();
            state.native_reads += 1;
            Some(state.pending)
        } else {
            None
        }
    }

    async fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        let wait = {
            let mut state = self.state.lock().unwrap();
            if let Some(accepted) = state.fail_write_after.take() {
                state.pending += accepted.min(bytes.len());
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, Original));
            }
            if let Some(accepted) = state.pending_write_after.take() {
                state.pending += accepted.min(bytes.len());
                true
            } else {
                state.pending = if state.drain_on_write {
                    0
                } else {
                    state.pending + bytes.len()
                };
                false
            }
        };
        if wait {
            pending().await
        } else {
            Ok(())
        }
    }

    async fn flush(&mut self) -> io::Result<()> {
        poll_fn(|_| {
            let mut state = self.state.lock().unwrap();
            if state.fail_flush {
                return Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, Original)));
            }
            if state.pending_flush {
                return Poll::Pending;
            }
            let pending = state.pending;
            state.flushes.push(pending);
            state.pending = 0;
            Poll::Ready(Ok(()))
        })
        .await
    }
}

#[tokio::test]
async fn native_prebuffered_bytes_trigger_completion_flush() {
    let (mut writer, state) = writer(true, THRESHOLD - 1);
    let mut output = AsyncHtmlWriter::new(&mut writer);
    output.write_slice(&Literal("x")).await.unwrap();
    assert_eq!(state.lock().unwrap().flushes, [THRESHOLD]);
}

const GOLDEN: &str = include_str!("fixtures/native-byte-accounting.txt");

#[tokio::test]
async fn pinned_csharp_golden_matches_native_and_fallback_accounting() {
    let large = "x".repeat(THRESHOLD);
    let cases = [
        (
            "native-prefilled-plus-one",
            true,
            THRESHOLD - 1,
            "x",
            false,
            false,
        ),
        ("native-prefilled-empty", true, THRESHOLD, "", false, false),
        (
            "native-below-threshold",
            true,
            THRESHOLD - 1,
            "",
            false,
            false,
        ),
        (
            "fallback-prefilled-plus-one",
            false,
            THRESHOLD - 1,
            "x",
            false,
            false,
        ),
        ("native-draining", true, 0, large.as_str(), true, false),
        ("fallback-draining", false, 0, large.as_str(), true, false),
        (
            "native-partial-failure",
            true,
            THRESHOLD - 1,
            "xy",
            false,
            true,
        ),
        (
            "fallback-partial-failure",
            false,
            THRESHOLD - 1,
            "xy",
            false,
            true,
        ),
    ];
    let mut actual = String::new();
    for (name, native, prefilled, text, drain, failure) in cases {
        let (mut writer, state) = writer(native, prefilled);
        state.lock().unwrap().drain_on_write = drain;
        if failure {
            state.lock().unwrap().fail_write_after = Some(1);
            let error = AsyncHtmlWriter::new(&mut writer)
                .write_slice(&Literal(text))
                .await
                .unwrap_err();
            match error {
                AsyncRenderError::Write(error) => assert_original(error),
                error => panic!("unexpected error: {error}"),
            }
            let state = state.lock().unwrap();
            assert_eq!(state.pending, THRESHOLD);
            assert!(state.flushes.is_empty());
            assert_eq!(state.native_reads, 0);
        }
        AsyncHtmlWriter::new(&mut writer)
            .write_slice(&Literal(if failure { "" } else { text }))
            .await
            .unwrap();
        let state = state.lock().unwrap();
        let flushes = if state.flushes.is_empty() {
            "none".into()
        } else {
            state
                .flushes
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(",")
        };
        actual.push_str(&format!("{name}: flushes={flushes}\n"));
    }
    assert_eq!(actual, GOLDEN);
}

#[tokio::test]
async fn native_count_is_authoritative_not_added_to_successful_write_count() {
    for size in [THRESHOLD - 1, THRESHOLD, THRESHOLD + 1] {
        let (mut writer, state) = writer(true, 0);
        let mut output = AsyncHtmlWriter::new(&mut writer);
        output
            .write_slice(&Literal(&"x".repeat(size)))
            .await
            .unwrap();
        assert_eq!(
            state.lock().unwrap().flushes,
            if size >= THRESHOLD {
                vec![size]
            } else {
                vec![]
            }
        );
    }
    let (mut writer, state) = writer(true, 0);
    let mut output = AsyncHtmlWriter::new(&mut writer);
    output
        .write_slice(&Literal(&"x".repeat(THRESHOLD / 2)))
        .await
        .unwrap();
    assert!(state.lock().unwrap().flushes.is_empty());
}

#[tokio::test]
async fn writes_awaits_and_plain_sections_do_not_query_or_trigger_auto_flush() {
    let (mut writer, state) = writer(true, THRESHOLD);
    let mut output = AsyncHtmlWriter::new(&mut writer);
    output.write_literal("x").await.unwrap();
    output.write_value("&").await.unwrap();
    output.write_section(&Literal(""), "missing").await.unwrap();
    std::future::ready(()).await;
    assert_eq!(state.lock().unwrap().native_reads, 0);
    assert!(state.lock().unwrap().flushes.is_empty());
    output.write_slice(&Literal("")).await.unwrap();
    assert_eq!(state.lock().unwrap().flushes, [THRESHOLD + 6]);
}

#[tokio::test]
async fn native_snapshot_survives_output_reborrows_and_explicit_flush_resets() {
    let (mut writer, state) = writer(true, THRESHOLD / 2);
    AsyncHtmlWriter::new(&mut writer)
        .write_slice(&Literal(&"x".repeat(THRESHOLD / 2 - 1)))
        .await
        .unwrap();
    assert!(state.lock().unwrap().flushes.is_empty());
    let mut output = AsyncHtmlWriter::new(&mut writer);
    output.write_slice(&Literal("x")).await.unwrap();
    output.write_literal("more").await.unwrap();
    output.flush().await.unwrap();
    output.write_slice(&Literal("")).await.unwrap();
    assert_eq!(state.lock().unwrap().flushes, [THRESHOLD, 4]);
}

#[tokio::test]
async fn canceled_partial_write_preserves_native_bytes_without_flushing_failed_execution() {
    let (mut writer, state) = writer(true, THRESHOLD - 1);
    state.lock().unwrap().pending_write_after = Some(1);
    let token = CancellationToken::new();
    {
        let mut output = AsyncHtmlWriter::with_cancellation(&mut writer, &token);
        let mut write = pin!(output.write_literal("xy"));
        poll_fn(|cx| {
            assert!(write.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        token.cancel();
        let error = write.await.unwrap_err();
        assert!(error.get_ref().unwrap().is::<RenderCancelled>());
    }
    assert!(state.lock().unwrap().flushes.is_empty());
    assert_eq!(state.lock().unwrap().native_reads, 0);
    AsyncHtmlWriter::new(&mut writer)
        .write_slice(&Literal(""))
        .await
        .unwrap();
    assert_eq!(state.lock().unwrap().flushes, [THRESHOLD]);
}

#[tokio::test]
async fn pending_and_failed_native_completion_flushes_keep_original_errors_and_order() {
    let (mut writer, state) = writer(true, THRESHOLD);
    state.lock().unwrap().pending_flush = true;
    let mut output = AsyncHtmlWriter::new(&mut writer);
    {
        let mut child = pin!(output.write_slice(&Literal("")));
        poll_fn(|cx| {
            assert!(child.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        assert_eq!(state.lock().unwrap().pending, THRESHOLD);
        assert!(state.lock().unwrap().flushes.is_empty());
        state.lock().unwrap().pending_flush = false;
        child.await.unwrap();
    }
    assert_eq!(state.lock().unwrap().flushes, [THRESHOLD]);
    state.lock().unwrap().pending = THRESHOLD;
    state.lock().unwrap().fail_flush = true;
    match output.write_slice(&Literal("")).await.unwrap_err() {
        AsyncRenderError::Write(error) => assert_original(error),
        error => panic!("unexpected error: {error}"),
    }
    assert_eq!(state.lock().unwrap().pending, THRESHOLD);
    state.lock().unwrap().fail_flush = false;
    output.write_slice(&Literal("")).await.unwrap();
    assert_eq!(state.lock().unwrap().flushes, [THRESHOLD, THRESHOLD]);
}

#[tokio::test]
async fn explicit_final_and_buffered_bridge_flushes_are_unchanged_for_native_zero() {
    let (mut writer, state) = writer(true, 0);
    state.lock().unwrap().drain_on_write = true;
    render_incremental(&Literal("x"), &mut writer)
        .await
        .unwrap();
    render_async(&html! { "x" }, &mut writer).await.unwrap();
    AsyncHtmlWriter::new(&mut writer).flush().await.unwrap();
    assert_eq!(state.lock().unwrap().flushes, [0, 0, 0]);
    assert_eq!(state.lock().unwrap().native_reads, 0);
}

#[tokio::test]
async fn dropped_native_flush_uses_current_sink_snapshot_not_reset_fallback() {
    for sink_reset in [false, true] {
        let (mut writer, state) = writer(true, THRESHOLD);
        state.lock().unwrap().pending_flush = true;
        let mut output = AsyncHtmlWriter::new(&mut writer);
        {
            let mut child = pin!(output.write_slice(&Literal("")));
            poll_fn(|cx| {
                assert!(child.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            if sink_reset {
                state.lock().unwrap().pending = 0;
            }
        }
        state.lock().unwrap().pending_flush = false;
        output.write_slice(&Literal("")).await.unwrap();
        assert_eq!(
            state.lock().unwrap().flushes,
            if sink_reset { vec![] } else { vec![THRESHOLD] }
        );
    }
}

async_template! {
    struct Generated { model: () } => { @render { Literal("x") }; }
}

struct LayoutOnly;

impl AsyncLayout for LayoutOnly {
    async fn render_layout_into<B: AsyncSlice, W: AsyncSliceWriter + Send>(
        &self,
        _: &B,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        output.write_literal("x").await?;
        Ok(())
    }
}

#[tokio::test]
async fn generated_children_layouts_and_section_owned_children_share_native_capability() {
    let (mut generated_writer, state) = writer(true, THRESHOLD - 1);
    render_incremental(&Generated::new(()), &mut generated_writer)
        .await
        .unwrap();
    assert_eq!(state.lock().unwrap().flushes, [THRESHOLD, 0]);

    let (mut layout_writer, state) = writer(true, THRESHOLD - 1);
    let page = declare_async_layout(Literal("not-rendered"), LayoutOnly);
    page.render_into(&mut AsyncHtmlWriter::new(&mut layout_writer))
        .await
        .unwrap();
    assert_eq!(state.lock().unwrap().flushes, [THRESHOLD]);

    let (mut section_writer, state) = writer(true, THRESHOLD);
    let provider = with_async_section(Literal("body"), "header", Literal("")).unwrap();
    AsyncHtmlWriter::new(&mut section_writer)
        .write_section(&provider, "header")
        .await
        .unwrap();
    assert_eq!(state.lock().unwrap().flushes, [THRESHOLD]);
}
