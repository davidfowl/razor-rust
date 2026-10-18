use razor_slices::{
    declare_async_layout, html, render_async, render_incremental, render_incremental_with_context,
    AsyncHtmlWriter, AsyncLayout, AsyncRenderError, AsyncSlice, AsyncSliceWriter,
    CancellationToken, RenderCancelled, RenderContext,
};
use std::{
    future::{pending, Future},
    io,
    pin::pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Wake, Waker},
};

const THRESHOLD: usize = 16_384;

#[derive(Debug)]
struct Original;
impl std::fmt::Display for Original {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("original sink failure")
    }
}
impl std::error::Error for Original {}

#[derive(Default)]
struct State {
    bytes: Vec<u8>,
    pending: usize,
    flushes: Vec<usize>,
    counter_reads: usize,
    fail_write: bool,
    fail_flush: bool,
    hold_write: bool,
    hold_flush: bool,
}

struct Writer {
    state: Arc<Mutex<State>>,
    auto: bool,
    native: bool,
}

impl AsyncSliceWriter for Writer {
    fn always_flush_on_root_completion(&self) -> bool {
        false
    }

    fn auto_flush_enabled(&self) -> bool {
        self.auto
    }

    fn unflushed_bytes(&self) -> Option<usize> {
        let mut state = self.state.lock().unwrap();
        state.counter_reads += 1;
        self.native.then_some(state.pending)
    }

    async fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        let hold = {
            let mut state = self.state.lock().unwrap();
            let len = if state.fail_write || state.hold_write {
                bytes.len().min(1)
            } else {
                bytes.len()
            };
            state.bytes.extend_from_slice(&bytes[..len]);
            state.pending += len;
            if state.fail_write {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, Original));
            }
            state.hold_write
        };
        if hold {
            pending::<()>().await;
        }
        Ok(())
    }

    async fn flush(&mut self) -> io::Result<()> {
        let hold = {
            let mut state = self.state.lock().unwrap();
            let count = state.pending;
            state.flushes.push(count);
            state.pending = 0;
            if state.fail_flush {
                return Err(io::Error::other(Original));
            }
            state.hold_flush
        };
        if hold {
            pending::<()>().await;
        }
        Ok(())
    }
}

fn writer(auto: bool, native: bool, prefilled: usize) -> (Writer, Arc<Mutex<State>>) {
    let state = Arc::new(Mutex::new(State {
        bytes: vec![b'p'; prefilled],
        pending: prefilled,
        ..State::default()
    }));
    (
        Writer {
            state: state.clone(),
            auto,
            native,
        },
        state,
    )
}

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

#[tokio::test]
async fn raw_text_root_does_not_add_a_final_flush() {
    let (mut writer, state) = writer(false, true, 0);
    render_incremental(&Literal("root"), &mut writer)
        .await
        .unwrap();
    let state = state.lock().unwrap();
    assert_eq!(state.bytes, b"root");
    assert!(state.flushes.is_empty());
    assert_eq!(state.counter_reads, 0);
}

struct Padding;
impl AsyncLayout for Padding {
    async fn render_layout_into<B: AsyncSlice, W: AsyncSliceWriter + Send>(
        &self,
        body: &B,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        output.write_slice(body).await?;
        output.write_literal(&"x".repeat(THRESHOLD / 2)).await?;
        Ok(())
    }
}

struct Case<'a> {
    name: &'a str,
    gate: Option<&'a tokio::sync::Notify>,
}

impl AsyncSlice for Case<'_> {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        if let Some(gate) = self.gate {
            gate.notified().await;
        }
        match self.name {
            "empty" | "prefilled-empty" => {}
            "below" => output.write_literal(&"x".repeat(THRESHOLD - 1)).await?,
            "at" => output.write_literal(&"x".repeat(THRESHOLD)).await?,
            "above" => output.write_literal(&"x".repeat(THRESHOLD + 1)).await?,
            "encoded" => {
                output.write_value(&"&".repeat(3276)).await?;
                output.write_literal("xxxx").await?;
            }
            "children" => {
                output
                    .write_slice(&Literal(&"x".repeat(THRESHOLD - 1)))
                    .await?;
                output.write_slice(&Literal("x")).await?;
                output.write_literal("s").await?;
            }
            "root-after-child" => {
                output
                    .write_slice(&Literal(&"x".repeat(THRESHOLD - 1)))
                    .await?;
                output.write_literal("x").await?;
            }
            "explicit" => {
                output.write_literal(&"x".repeat(THRESHOLD)).await?;
                output.flush().await?;
                output.write_literal("s").await?;
            }
            "layout" => {
                declare_async_layout(Literal(&"x".repeat(THRESHOLD / 2)), Padding)
                    .render_into(output)
                    .await?;
            }
            "prefilled-plus-one" => output.write_literal("x").await?,
            name => panic!("unknown case {name}"),
        }
        Ok(())
    }
}

struct Noop;
impl Wake for Noop {
    fn wake(self: Arc<Self>) {}
}

fn poll<F: Future>(future: std::pin::Pin<&mut F>) -> Poll<F::Output> {
    future.poll(&mut Context::from_waker(&Waker::from(Arc::new(Noop))))
}

#[tokio::test]
async fn pinned_csharp_golden_matches_raw_root_completion() {
    for scoped in [false, true] {
        let mut actual = String::new();
        for held in [false, true] {
            for name in [
                "empty",
                "below",
                "at",
                "above",
                "encoded",
                "children",
                "root-after-child",
                "explicit",
                "layout",
                "prefilled-empty",
                "prefilled-plus-one",
            ] {
                for (policy, auto, native) in [
                    ("pipe-native", true, true),
                    ("pipe-fallback", true, false),
                    ("text", false, true),
                ] {
                    let prefilled = match name {
                        "prefilled-empty" => THRESHOLD,
                        "prefilled-plus-one" => THRESHOLD - 1,
                        _ => 0,
                    };
                    let (mut writer, state) = writer(auto, native, prefilled);
                    let gate = tokio::sync::Notify::new();
                    let case = Case {
                        name,
                        gate: held.then_some(&gate),
                    };
                    let context = RenderContext::new((), CancellationToken::new());
                    let render = async {
                        if scoped {
                            render_incremental_with_context(&case, &mut writer, &context).await
                        } else {
                            render_incremental(&case, &mut writer).await
                        }
                    };
                    let mut render = pin!(render);
                    if held {
                        assert!(poll(render.as_mut()).is_pending());
                        let state = state.lock().unwrap();
                        assert_eq!(state.bytes, vec![b'p'; prefilled]);
                        assert!(state.flushes.is_empty());
                        assert_eq!(state.counter_reads, 0);
                        drop(state);
                        gate.notify_one();
                    }
                    render.await.unwrap();
                    let state = state.lock().unwrap();
                    let body = match name {
                        "empty" | "prefilled-empty" => String::new(),
                        "below" => "x".repeat(THRESHOLD - 1),
                        "at" | "root-after-child" | "layout" => "x".repeat(THRESHOLD),
                        "above" => "x".repeat(THRESHOLD + 1),
                        "encoded" => format!("{}xxxx", "&amp;".repeat(3276)),
                        "children" | "explicit" => format!("{}s", "x".repeat(THRESHOLD)),
                        "prefilled-plus-one" => "x".to_owned(),
                        _ => unreachable!(),
                    };
                    assert_eq!(
                        state.bytes,
                        format!("{}{body}", "p".repeat(prefilled)).as_bytes()
                    );
                    if !auto {
                        assert_eq!(state.counter_reads, 0);
                    }
                    let flushes = if state.flushes.is_empty() {
                        "-".to_owned()
                    } else {
                        state
                            .flushes
                            .iter()
                            .map(usize::to_string)
                            .collect::<Vec<_>>()
                            .join(",")
                    };
                    actual.push_str(&format!(
                        "{}/{policy}/{name}: bytes={},flushes={flushes},pending={}\n",
                        if held { "held" } else { "ready" },
                        state.bytes.len(),
                        state.pending,
                    ));
                }
            }
        }
        assert_eq!(actual, include_str!("fixtures/root-flush-policy.txt"));
    }
}

struct DefaultWriter(Writer);
impl AsyncSliceWriter for DefaultWriter {
    fn auto_flush_enabled(&self) -> bool {
        self.0.auto_flush_enabled()
    }

    fn unflushed_bytes(&self) -> Option<usize> {
        self.0.unflushed_bytes()
    }

    async fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.0.write_all(bytes).await
    }

    async fn flush(&mut self) -> io::Result<()> {
        self.0.flush().await
    }
}

#[tokio::test]
async fn default_root_flush_is_unconditional_and_does_not_query_native_count() {
    for scoped in [false, true] {
        for native in [false, true] {
            for auto in [false, true] {
                for (name, expected) in [
                    ("empty", vec![0]),
                    ("below", vec![THRESHOLD - 1]),
                    ("at", vec![THRESHOLD]),
                    (
                        "children",
                        if auto {
                            vec![THRESHOLD, 1]
                        } else {
                            vec![THRESHOLD + 1]
                        },
                    ),
                    ("explicit", vec![THRESHOLD, 1]),
                    (
                        "layout",
                        if auto {
                            vec![THRESHOLD, 0]
                        } else {
                            vec![THRESHOLD]
                        },
                    ),
                ] {
                    let (writer, state) = writer(auto, native, 0);
                    let mut writer = DefaultWriter(writer);
                    let case = Case { name, gate: None };
                    let context = RenderContext::new((), CancellationToken::new());
                    if scoped {
                        render_incremental_with_context(&case, &mut writer, &context)
                            .await
                            .unwrap();
                    } else {
                        render_incremental(&case, &mut writer).await.unwrap();
                    }
                    let state = state.lock().unwrap();
                    assert_eq!(state.flushes, expected, "{name}, auto={auto}");
                    assert_eq!(state.pending, 0);
                    if !auto || matches!(name, "empty" | "below" | "at" | "explicit") {
                        assert_eq!(state.counter_reads, 0);
                    }
                }
            }
        }
    }
}

#[tokio::test]
async fn raw_root_policy_does_not_change_explicit_or_buffered_bridge_flushes() {
    for auto in [false, true] {
        let (mut writer, state) = writer(auto, true, 0);
        AsyncHtmlWriter::new(&mut writer).flush().await.unwrap();
        render_async(&html! { "bridge" }, &mut writer)
            .await
            .unwrap();
        let state = state.lock().unwrap();
        assert_eq!(state.bytes, b"bridge");
        assert_eq!(state.flushes, [0, 6]);
        assert_eq!(state.counter_reads, 0);
    }
}

struct FailureCase(&'static str);
impl AsyncSlice for FailureCase {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        if self.0 == "child" {
            output.write_slice(&Literal(&"x".repeat(THRESHOLD))).await?;
        } else {
            output.write_literal(&"x".repeat(THRESHOLD)).await?;
        }
        match self.0 {
            "template" => return Err(razor_slices::RenderError(std::fmt::Error).into()),
            "explicit" => output.flush().await?,
            "root" => return Ok(()),
            _ => {}
        }
        output.write_literal("must-not-run").await?;
        Ok(())
    }
}

fn assert_original(error: AsyncRenderError, kind: io::ErrorKind) {
    let AsyncRenderError::Write(error) = error else {
        panic!("expected original destination error");
    };
    assert_eq!(error.kind(), kind);
    assert!(error.get_ref().unwrap().is::<Original>());
}

#[tokio::test]
async fn raw_root_policy_preserves_original_errors_without_cleanup_flush_or_suffix() {
    for scoped in [false, true] {
        for native in [false, true] {
            for stage in ["template", "write", "explicit", "child", "root"] {
                let (mut writer, state) = writer(true, native, 0);
                {
                    let mut state = state.lock().unwrap();
                    state.fail_write = stage == "write";
                    state.fail_flush = !matches!(stage, "template" | "write");
                }
                let context = RenderContext::new((), CancellationToken::new());
                let case = FailureCase(stage);
                let error = if scoped {
                    render_incremental_with_context(&case, &mut writer, &context).await
                } else {
                    render_incremental(&case, &mut writer).await
                }
                .unwrap_err();
                if stage == "template" {
                    assert!(matches!(error, AsyncRenderError::Render(_)));
                } else {
                    assert_original(
                        error,
                        if stage == "write" {
                            io::ErrorKind::BrokenPipe
                        } else {
                            io::ErrorKind::Other
                        },
                    );
                }
                let state = state.lock().unwrap();
                assert_eq!(
                    state.bytes,
                    vec![b'x'; if stage == "write" { 1 } else { THRESHOLD }]
                );
                assert_eq!(
                    state.flushes,
                    if matches!(stage, "template" | "write") {
                        vec![]
                    } else {
                        vec![THRESHOLD]
                    },
                );
            }
        }
    }
}

#[tokio::test]
async fn raw_root_skips_unrequested_flush_failures_and_reuses_native_partial_accounting() {
    for native in [false, true] {
        for (auto, size) in [(false, THRESHOLD), (true, THRESHOLD - 1)] {
            let (mut writer, state) = writer(auto, native, 0);
            state.lock().unwrap().fail_flush = true;
            render_incremental(&Literal(&"x".repeat(size)), &mut writer)
                .await
                .unwrap();
            let state = state.lock().unwrap();
            assert_eq!(state.bytes, vec![b'x'; size]);
            assert!(state.flushes.is_empty());
        }

        let (mut writer, state) = writer(true, native, THRESHOLD - 1);
        state.lock().unwrap().fail_write = true;
        assert_original(
            render_incremental(&Literal("xy"), &mut writer)
                .await
                .unwrap_err(),
            io::ErrorKind::BrokenPipe,
        );
        {
            let mut state = state.lock().unwrap();
            assert_eq!(
                state.bytes,
                format!("{}x", "p".repeat(THRESHOLD - 1)).as_bytes()
            );
            assert_eq!(state.pending, THRESHOLD);
            assert!(state.flushes.is_empty());
            state.fail_write = false;
        }
        render_incremental(&Literal(""), &mut writer).await.unwrap();
        assert_eq!(
            state.lock().unwrap().flushes,
            if native { vec![THRESHOLD] } else { vec![] }
        );
    }
}

struct Blocked(&'static str);
impl AsyncSlice for Blocked {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        if self.0 == "child" {
            output.write_slice(&Literal(&"x".repeat(THRESHOLD))).await?;
        } else {
            output.write_literal(&"x".repeat(THRESHOLD)).await?;
        }
        match self.0 {
            "application" => pending::<()>().await,
            "explicit" => output.flush().await?,
            "root" => return Ok(()),
            _ => {}
        }
        output.write_literal("must-not-run").await?;
        Ok(())
    }
}

#[tokio::test]
async fn raw_root_pending_operations_preserve_backpressure_cancellation_and_drop() {
    for native in [false, true] {
        for (scoped, cancel) in [(false, false), (true, false), (true, true)] {
            for (stage, auto) in [
                ("application", true),
                ("application", false),
                ("write", true),
                ("write", false),
                ("child", true),
                ("root", true),
                ("explicit", true),
                ("explicit", false),
            ] {
                let (mut writer, state) = writer(auto, native, 0);
                {
                    let mut state = state.lock().unwrap();
                    state.hold_write = stage == "write";
                    state.hold_flush = matches!(stage, "child" | "root" | "explicit");
                }
                let token = CancellationToken::new();
                let context = RenderContext::new((), token.clone());
                {
                    let case = Blocked(stage);
                    let mut render = pin!(async {
                        if scoped {
                            render_incremental_with_context(&case, &mut writer, &context).await
                        } else {
                            render_incremental(&case, &mut writer).await
                        }
                    });
                    assert!(poll(render.as_mut()).is_pending(), "{stage}");
                    if cancel {
                        token.cancel();
                        let Poll::Ready(Err(AsyncRenderError::Write(error))) =
                            poll(render.as_mut())
                        else {
                            panic!("cancellation must end pending {stage}");
                        };
                        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
                        assert!(error.get_ref().unwrap().is::<RenderCancelled>());
                    }
                }
                let expected_flushes = if matches!(stage, "child" | "root" | "explicit") {
                    vec![THRESHOLD]
                } else {
                    vec![]
                };
                {
                    let mut state = state.lock().unwrap();
                    assert_eq!(
                        state.bytes,
                        vec![b'x'; if stage == "write" { 1 } else { THRESHOLD }]
                    );
                    assert_eq!(state.flushes, expected_flushes, "{stage}");
                    if !auto || matches!(stage, "application" | "write" | "explicit") {
                        assert_eq!(state.counter_reads, 0);
                    }
                    state.hold_write = false;
                    state.hold_flush = false;
                }
                writer.write_all(b"caller").await.unwrap();
                writer.flush().await.unwrap();
                assert!(state.lock().unwrap().bytes.ends_with(b"caller"));
            }
        }
    }
}

#[tokio::test]
async fn already_cancelled_raw_root_does_not_write_or_check_completion() {
    for auto in [false, true] {
        let (mut writer, state) = writer(auto, true, THRESHOLD);
        let token = CancellationToken::new();
        token.cancel();
        let context = RenderContext::new((), token);
        let error =
            render_incremental_with_context(&Literal("must-not-run"), &mut writer, &context)
                .await
                .unwrap_err();
        let AsyncRenderError::Write(error) = error else {
            panic!("expected cancellation");
        };
        assert!(error.get_ref().unwrap().is::<RenderCancelled>());
        let state = state.lock().unwrap();
        assert_eq!(state.bytes, vec![b'p'; THRESHOLD]);
        assert!(state.flushes.is_empty());
        assert_eq!(state.counter_reads, 0);
    }
}

#[cfg(feature = "tokio")]
#[tokio::test]
async fn raw_root_threshold_drains_real_buffer_but_below_threshold_requires_caller_flush() {
    use razor_slices::TokioWriter;
    use tokio::io::{AsyncReadExt, BufWriter, DuplexStream};

    struct Raw(TokioWriter<BufWriter<DuplexStream>>);
    impl AsyncSliceWriter for Raw {
        fn always_flush_on_root_completion(&self) -> bool {
            false
        }
        async fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
            self.0.write_all(bytes).await
        }
        async fn flush(&mut self) -> io::Result<()> {
            self.0.flush().await
        }
    }

    for size in [THRESHOLD - 1, THRESHOLD] {
        let (sink, mut reader) = tokio::io::duplex(1);
        let mut writer = Raw(TokioWriter::new(BufWriter::with_capacity(
            THRESHOLD * 2,
            sink,
        )));
        let text = "x".repeat(size);
        let mut received = vec![0; size];
        if size < THRESHOLD {
            render_incremental(&Literal(&text), &mut writer)
                .await
                .unwrap();
            {
                let mut read = pin!(reader.read_exact(&mut received));
                assert!(poll(read.as_mut()).is_pending());
            }
            let (flush, read) = tokio::join!(writer.flush(), reader.read_exact(&mut received));
            flush.unwrap();
            read.unwrap();
        } else {
            let page = Literal(&text);
            let mut render = pin!(render_incremental(&page, &mut writer));
            assert!(poll(render.as_mut()).is_pending());
            let (render, read) = tokio::join!(render, reader.read_exact(&mut received));
            render.unwrap();
            read.unwrap();
        }
        assert_eq!(received, text.as_bytes());
    }
}
