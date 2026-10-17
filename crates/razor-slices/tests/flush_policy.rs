use razor_slices::{
    async_template, declare_async_layout, html, render_async, render_incremental,
    render_incremental_with_context, with_async_layout, with_async_section, AsyncHtmlWriter,
    AsyncLayout, AsyncRenderError, AsyncSlice, AsyncSliceWriter, CancellationToken,
    RenderCancelled, RenderContext,
};
use std::{
    future::{pending, Future},
    io,
    pin::pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Wake, Waker},
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
    bytes: Vec<u8>,
    pending: usize,
    flushes: Vec<usize>,
    counter_reads: usize,
    fail_write: bool,
    fail_flush: bool,
    hold_flush: bool,
}

struct Writer {
    state: Arc<Mutex<State>>,
    auto: bool,
    native: bool,
}

impl AsyncSliceWriter for Writer {
    fn auto_flush_enabled(&self) -> bool {
        self.auto
    }

    fn unflushed_bytes(&self) -> Option<usize> {
        let mut state = self.state.lock().unwrap();
        state.counter_reads += 1;
        self.native.then_some(state.pending)
    }

    async fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        let mut state = self.state.lock().unwrap();
        if state.fail_write {
            let accepted = bytes.len().min(1);
            state.bytes.extend_from_slice(&bytes[..accepted]);
            state.pending += accepted;
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, Original));
        }
        state.bytes.extend_from_slice(bytes);
        state.pending += bytes.len();
        Ok(())
    }

    async fn flush(&mut self) -> io::Result<()> {
        let (fail, hold) = {
            let mut state = self.state.lock().unwrap();
            let count = state.pending;
            state.flushes.push(count);
            state.pending = 0;
            (state.fail_flush, state.hold_flush)
        };
        if fail {
            return Err(io::Error::other(Original));
        }
        if hold {
            pending::<()>().await;
        }
        Ok(())
    }
}

fn writer(auto: bool, native: bool) -> (Writer, Arc<Mutex<State>>) {
    let state = Arc::new(Mutex::new(State::default()));
    (
        Writer {
            state: state.clone(),
            auto,
            native,
        },
        state,
    )
}

#[tokio::test]
async fn text_like_sink_does_not_auto_flush_at_slice_completion() {
    let (mut writer, state) = writer(false, true);
    AsyncHtmlWriter::new(&mut writer)
        .write_slice(&Literal(&"x".repeat(THRESHOLD)))
        .await
        .unwrap();
    let state = state.lock().unwrap();
    assert_eq!(state.bytes, vec![b'x'; THRESHOLD]);
    assert!(state.flushes.is_empty());
    assert_eq!(state.counter_reads, 0);
}

enum Case {
    Literal(usize),
    Children,
    Explicit,
    Layout,
}

impl AsyncSlice for Case {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        match self {
            Self::Literal(size) => output.write_literal(&"x".repeat(*size)).await?,
            Self::Children => {
                let half = "x".repeat(THRESHOLD / 2);
                output.write_slice(&Literal(&half)).await?;
                output.write_slice(&Literal(&half)).await?;
            }
            Self::Explicit => {
                output.write_literal(&"x".repeat(THRESHOLD)).await?;
                output.flush().await?;
            }
            Self::Layout => {
                let half = "x".repeat(THRESHOLD / 2);
                declare_async_layout(Literal(&half), Padding(&half))
                    .render_into(output)
                    .await?;
            }
        }
        Ok(())
    }
}

struct Padding<'a>(&'a str);
impl AsyncLayout for Padding<'_> {
    async fn render_layout_into<B: AsyncSlice, W: AsyncSliceWriter + Send>(
        &self,
        body: &B,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        body.render_body_into(output).await?;
        output.write_literal(self.0).await?;
        Ok(())
    }
}

#[tokio::test]
async fn pinned_csharp_golden_matches_completion_policy_not_root_final_flush() {
    for native in [false, true] {
        let mut actual = String::new();
        for (name, case, size) in [
            ("16383", Case::Literal(THRESHOLD - 1), THRESHOLD - 1),
            ("16384", Case::Literal(THRESHOLD), THRESHOLD),
            ("16385", Case::Literal(THRESHOLD + 1), THRESHOLD + 1),
            ("children", Case::Children, THRESHOLD),
            ("explicit", Case::Explicit, THRESHOLD),
            ("layout", Case::Layout, THRESHOLD),
        ] {
            for (policy, auto) in [("pipe", true), ("text", false)] {
                let (mut writer, state) = writer(auto, native);
                AsyncHtmlWriter::new(&mut writer)
                    .write_slice(&case)
                    .await
                    .unwrap();
                let state = state.lock().unwrap();
                assert_eq!(state.bytes, vec![b'x'; size]);
                if !auto {
                    assert_eq!(state.counter_reads, 0);
                }
                actual.push_str(&format!(
                    "{policy}-{name}: flushes={}\n",
                    state.flushes.len()
                ));
            }
        }
        assert_eq!(actual, include_str!("fixtures/flush-policy.txt"));
    }
}

async_template! {
    struct Generated<'a> { model: &'a str } => {
        @render { Literal(model) };
    }
}

#[tokio::test]
async fn disabled_policy_covers_generated_nested_layout_and_section_boundaries() {
    for native in [false, true] {
        let text = "x".repeat(THRESHOLD);
        let (mut writer, state) = writer(false, native);
        let mut output = AsyncHtmlWriter::new(&mut writer);
        Generated::new(&text)
            .render_into(&mut output)
            .await
            .unwrap();
        let page = declare_async_layout(
            Literal(&text),
            declare_async_layout(Padding("a"), Padding("b")),
        );
        page.render_into(&mut output).await.unwrap();
        output.write_slice(&page).await.unwrap();
        output
            .write_slice(&with_async_layout(Literal(&text), Padding("c")))
            .await
            .unwrap();
        let section = with_async_section(Padding(""), "scripts", Literal(&text)).unwrap();
        for _ in 0..2 {
            section
                .render_layout_section_into(&Literal(""), "scripts", &mut output)
                .await
                .unwrap();
        }
        let state = state.lock().unwrap();
        assert_eq!(
            state.bytes,
            format!("{text}{text}ab{text}{text}c{text}{text}").as_bytes()
        );
        assert!(state.flushes.is_empty());
        assert_eq!(state.counter_reads, 0);
    }
}

#[tokio::test]
async fn disabled_policy_preserves_explicit_root_final_and_buffered_bridge_flushes() {
    let (mut writer, state) = writer(false, true);
    let mut output = AsyncHtmlWriter::new(&mut writer);
    output.write_literal("explicit").await.unwrap();
    output.flush().await.unwrap();
    render_incremental(&Generated::new(&"x".repeat(THRESHOLD)), &mut writer)
        .await
        .unwrap();
    render_async(&html! { "bridge" }, &mut writer)
        .await
        .unwrap();
    let state = state.lock().unwrap();
    assert_eq!(state.flushes, [8, THRESHOLD, 6]);
    assert_eq!(state.counter_reads, 0);
    assert_eq!(
        state.bytes,
        format!("explicit{}bridge", "x".repeat(THRESHOLD)).as_bytes()
    );
}

#[derive(Debug)]
struct Original;
impl std::fmt::Display for Original {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("original sink failure")
    }
}
impl std::error::Error for Original {}

fn assert_original(error: AsyncRenderError, kind: io::ErrorKind) {
    let AsyncRenderError::Write(error) = error else {
        panic!("expected original writer error");
    };
    assert_eq!(error.kind(), kind);
    assert!(error.get_ref().unwrap().is::<Original>());
}

async_template! {
    struct Explicit<'a> { model: &'a str } => {
        @render { Literal(model) };
        @flush;
        "must-not-run"
    }
}

#[tokio::test]
async fn disabled_policy_preserves_original_write_explicit_and_final_flush_errors() {
    for native in [false, true] {
        for failure in ["write", "explicit", "final", "bridge"] {
            let (mut writer, state) = writer(false, native);
            {
                let mut state = state.lock().unwrap();
                state.fail_write = failure == "write";
                state.fail_flush = !state.fail_write;
            }
            let text = "x".repeat(THRESHOLD);
            let error = match failure {
                "write" | "explicit" => {
                    render_incremental(&Explicit::new(&text), &mut writer).await
                }
                "final" => render_incremental(&Generated::new(&text), &mut writer).await,
                "bridge" => render_async(&html! { "bridge" }, &mut writer).await,
                _ => unreachable!(),
            }
            .unwrap_err();
            assert_original(
                error,
                if failure == "write" {
                    io::ErrorKind::BrokenPipe
                } else {
                    io::ErrorKind::Other
                },
            );
            let state = state.lock().unwrap();
            assert_eq!(
                state.bytes,
                match failure {
                    "write" => b"x".to_vec(),
                    "bridge" => b"bridge".to_vec(),
                    _ => text.as_bytes().to_vec(),
                }
            );
            assert_eq!(state.flushes.len(), usize::from(failure != "write"));
            assert_eq!(state.counter_reads, 0);
        }
    }
}

struct Noop;
impl Wake for Noop {
    fn wake(self: Arc<Self>) {}
}

#[test]
fn disabled_policy_still_awaits_explicit_and_final_flush_with_cancellation_and_drop() {
    for (explicit, cancel) in [(false, false), (false, true), (true, false), (true, true)] {
        let (mut writer, state) = writer(false, true);
        state.lock().unwrap().hold_flush = true;
        let token = CancellationToken::new();
        let context = RenderContext::new((), token.clone());
        let text = "x".repeat(THRESHOLD);
        let explicit_page = Explicit::new(&text);
        let final_page = Generated::new(&text);
        let waker = Waker::from(Arc::new(Noop));
        {
            let mut future = pin!(async {
                if explicit {
                    render_incremental_with_context(&explicit_page, &mut writer, &context).await
                } else {
                    render_incremental_with_context(&final_page, &mut writer, &context).await
                }
            });
            assert!(future
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending());
            assert_eq!(state.lock().unwrap().bytes, text.as_bytes());
            if cancel {
                token.cancel();
                let Poll::Ready(Err(AsyncRenderError::Write(error))) =
                    future.as_mut().poll(&mut Context::from_waker(&waker))
                else {
                    panic!("cancellation must stop the pending explicit/final flush");
                };
                assert_eq!(error.kind(), io::ErrorKind::Interrupted);
                assert!(error.get_ref().unwrap().is::<RenderCancelled>());
            }
        }
        let state = state.lock().unwrap();
        assert_eq!(state.bytes, text.as_bytes());
        assert_eq!(state.flushes, [THRESHOLD]);
        assert_eq!(state.counter_reads, 0);
    }
}
