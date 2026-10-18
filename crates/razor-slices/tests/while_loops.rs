use razor_slices::{
    async_template, html, render_incremental, render_incremental_with_context, AsyncHtmlWriter,
    AsyncRenderError, AsyncSlice, AsyncSliceWriter, CancellationToken, RenderCancelled,
    RenderContext, Slice,
};
use std::{
    future::{poll_fn, Future},
    io,
    pin::pin,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll, Wake},
};

struct Model {
    values: Vec<&'static str>,
    index: AtomicUsize,
    conditions: AtomicUsize,
}

impl Model {
    fn new(values: &[&'static str]) -> Self {
        Self {
            values: values.to_vec(),
            index: AtomicUsize::new(0),
            conditions: AtomicUsize::new(0),
        }
    }

    fn pending(&self) -> bool {
        self.conditions.fetch_add(1, Ordering::SeqCst);
        self.index.load(Ordering::SeqCst) < self.values.len()
    }

    fn take(&self) -> &'static str {
        self.values[self.index.fetch_add(1, Ordering::SeqCst)]
    }

    fn next(&self) -> Option<(usize, &'static str)> {
        self.conditions.fetch_add(1, Ordering::SeqCst);
        let index = self.index.load(Ordering::SeqCst);
        self.values.get(index).map(|value| {
            self.index.fetch_add(1, Ordering::SeqCst);
            (index, *value)
        })
    }

    fn reset(&self) {
        self.index.store(0, Ordering::SeqCst);
        self.conditions.store(0, Ordering::SeqCst);
    }
}

async_template! {
    struct Boolean<'a> { model: &'a Model } => {
        "[" while model.pending() { {model.take()} } "]"
    }
}

async_template! {
    struct Binding<'a> { model: &'a Model } => {
        "[" while let Some((index, text)) = model.next() { {index} ":" {text} ";" } "]"
    }
}

async_template! {
    struct Nested<'a> { model: &'a Model } => {
        "["
        while let Some((index, text)) = model.next() {
            {index} ":" {text}
            for inner in 0..2 { if inner < 2 { {inner} } }
            ";"
        }
        "]"
    }
}

#[derive(Default)]
struct State {
    bytes: Vec<u8>,
    pending: usize,
    flushes: Vec<usize>,
    fail_at: Option<usize>,
    fail_flush: bool,
    hold_after: Option<usize>,
}

struct Writer {
    state: Arc<Mutex<State>>,
    native: bool,
}

#[derive(Debug)]
struct Original;
impl std::fmt::Display for Original {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("original loop writer failure")
    }
}
impl std::error::Error for Original {}

impl io::Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut state = self.state.lock().unwrap();
        let accepted = state.fail_at.map_or(bytes.len(), |limit| {
            limit.saturating_sub(state.bytes.len()).min(bytes.len())
        });
        if accepted == 0 && !bytes.is_empty() {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, Original));
        }
        state.bytes.extend_from_slice(&bytes[..accepted]);
        state.pending += accepted;
        Ok(accepted)
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut state = self.state.lock().unwrap();
        let pending = state.pending;
        state.flushes.push(pending);
        state.pending = 0;
        if state.fail_flush {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, Original));
        }
        Ok(())
    }
}

impl AsyncSliceWriter for Writer {
    async fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        poll_fn(|_| {
            let state = self.state.lock().unwrap();
            if state
                .hold_after
                .is_some_and(|limit| state.bytes.len() >= limit)
            {
                return Poll::Pending;
            }
            drop(state);
            Poll::Ready(io::Write::write_all(self, bytes))
        })
        .await
    }

    async fn flush(&mut self) -> io::Result<()> {
        io::Write::flush(self)
    }

    fn unflushed_bytes(&self) -> Option<usize> {
        self.native.then(|| self.state.lock().unwrap().pending)
    }
}

fn sink() -> (Writer, Arc<Mutex<State>>) {
    let state = Arc::new(Mutex::new(State::default()));
    (
        Writer {
            state: state.clone(),
            native: false,
        },
        state,
    )
}

async fn check_case(
    name: &str,
    values: &[&'static str],
    page: &impl Slice,
    asynchronous: &impl AsyncSlice,
    model: &Model,
    actual: &mut String,
) {
    assert_eq!(model.conditions.load(Ordering::SeqCst), 0);
    let expected = page.render_to_string().unwrap();
    assert_eq!(model.conditions.load(Ordering::SeqCst), values.len() + 1);
    assert_eq!(model.index.load(Ordering::SeqCst), values.len());
    model.reset();
    assert_eq!(page.render_to_bytes().unwrap(), expected.as_bytes());
    model.reset();
    let (mut writer, state) = sink();
    render_incremental(asynchronous, &mut writer).await.unwrap();
    assert_eq!(model.conditions.load(Ordering::SeqCst), values.len() + 1);
    assert_eq!(state.lock().unwrap().bytes, expected.as_bytes());
    actual.push_str(&format!("{name}: {expected}\n"));
}

#[tokio::test]
async fn pinned_csharp_golden_matches_boolean_binding_empty_and_nested_loops() {
    let mut actual = String::new();
    for (name, values) in [
        ("boolean-empty", [].as_slice()),
        ("boolean-values", ["&", "<"].as_slice()),
    ] {
        let model = Model::new(values);
        let borrowed = &model;
        let page = html! { "[" while borrowed.pending() { {borrowed.take()} } "]" };
        check_case(
            name,
            values,
            &page,
            &Boolean::new(&model),
            &model,
            &mut actual,
        )
        .await;
    }
    for (name, values) in [
        ("binding-empty", [].as_slice()),
        ("binding-values", ["&", "<"].as_slice()),
    ] {
        let model = Model::new(values);
        let borrowed = &model;
        let page = html! {
            "[" while let Some((index, text)) = borrowed.next() { {index} ":" {text} ";" } "]"
        };
        check_case(
            name,
            values,
            &page,
            &Binding::new(&model),
            &model,
            &mut actual,
        )
        .await;
    }
    let values = ["&", "<"];
    let model = Model::new(&values);
    let borrowed = &model;
    let page = html! {
        "["
        while let Some((index, text)) = borrowed.next() {
            {index} ":" {text}
            for inner in 0..2 { if inner < 2 { {inner} } }
            ";"
        }
        "]"
    };
    check_case(
        "nested",
        &values,
        &page,
        &Nested::new(&model),
        &model,
        &mut actual,
    )
    .await;
    assert_eq!(actual, include_str!("fixtures/while-loops.txt"));
}

#[test]
fn conditions_and_pattern_bindings_work_in_synchronous_bodies() {
    let model = Model::new(&["&", "<"]);
    let borrowed = &model;
    let page = html! {
        "[" while let Some((index, text)) = borrowed.next() { {index} ":" {text} ";" } "]"
    };
    assert_eq!(model.conditions.load(Ordering::SeqCst), 0);
    assert_eq!(page.render_to_string().unwrap(), "[0:&amp;;1:&lt;;]");
    assert_eq!(model.conditions.load(Ordering::SeqCst), 3);
    assert_eq!(page.render_to_string().unwrap(), "[]");
    model.reset();
    assert_eq!(page.render_to_string().unwrap(), "[0:&amp;;1:&lt;;]");
}

async_template! {
    struct LocalLoops { model: usize } => {
        @{ let mut outer = 0; }
        while outer < *model {
            @{ let mut inner = Some(0); }
            while let Some(value) = inner {
                {outer} {value}
                @{ inner = if value == 0 { Some(1) } else { None }; }
            }
            @{ outer += 1; }
        }
    }
}

#[tokio::test]
async fn render_local_nested_loop_state_is_recreated_on_each_render() {
    let page = LocalLoops::new(2);
    for _ in 0..2 {
        let (mut writer, state) = sink();
        render_incremental(&page, &mut writer).await.unwrap();
        assert_eq!(state.lock().unwrap().bytes, b"00011011");
    }
    let model = Model::new(&["&"]);
    let page = Boolean::new(&model);
    assert_eq!(model.conditions.load(Ordering::SeqCst), 0);
    for expected in [b"[&amp;]".as_slice(), b"[]"] {
        let (mut writer, state) = sink();
        render_incremental(&page, &mut writer).await.unwrap();
        assert_eq!(state.lock().unwrap().bytes, expected);
    }
}

fn assert_original(error: io::Error) {
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    assert!(error.get_ref().unwrap().is::<Original>());
}

#[tokio::test]
async fn every_ascii_writer_failure_retains_prefix_source_and_condition_count() {
    let expected = b"[0:&amp;;1:&lt;;]";
    for limit in 0..expected.len() {
        let conditions = match limit {
            0 => 0,
            1..=8 => 1,
            9..=15 => 2,
            _ => 3,
        };
        let model = Model::new(&["&", "<"]);
        let borrowed = &model;
        let page = html! {
            "[" while let Some((index, text)) = borrowed.next() { {index} ":" {text} ";" } "]"
        };
        let (mut writer, state) = sink();
        state.lock().unwrap().fail_at = Some(limit);
        assert_original(page.render_to_writer(&mut writer).unwrap_err());
        assert_eq!(state.lock().unwrap().bytes, expected[..limit]);
        assert!(state.lock().unwrap().flushes.is_empty());
        assert_eq!(model.conditions.load(Ordering::SeqCst), conditions);
        model.reset();
        let (mut writer, state) = sink();
        state.lock().unwrap().fail_at = Some(limit);
        let AsyncRenderError::Write(error) = render_incremental(&Binding::new(&model), &mut writer)
            .await
            .unwrap_err()
        else {
            panic!("expected original writer error");
        };
        assert_original(error);
        assert_eq!(state.lock().unwrap().bytes, expected[..limit]);
        assert!(state.lock().unwrap().flushes.is_empty());
        assert_eq!(model.conditions.load(Ordering::SeqCst), conditions);
    }
    let model = Model::new(&["&", "<"]);
    let (mut writer, state) = sink();
    state.lock().unwrap().fail_flush = true;
    let AsyncRenderError::Write(error) = render_incremental(&Binding::new(&model), &mut writer)
        .await
        .unwrap_err()
    else {
        panic!("expected original final-flush error");
    };
    assert_original(error);
    assert_eq!(state.lock().unwrap().bytes, expected);
    assert_eq!(state.lock().unwrap().flushes, [expected.len()]);
}

struct HeldModel {
    rows: Model,
    open: AtomicBool,
}

impl HeldModel {
    async fn wait(&self) {
        poll_fn(|_| {
            if self.open.load(Ordering::SeqCst) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await
    }
}

async_template! {
    struct Held<'a> { model: &'a HeldModel } => {
        "["
        while let Some((index, text)) = model.rows.next() {
            {index} ":"
            @await { model.wait() };
            {text} ";"
        }
        "]"
    }
}

struct Noop;
impl Wake for Noop {
    fn wake(self: Arc<Self>) {}
}

#[test]
fn pending_iteration_does_not_advance_conditions_and_honors_cancellation_or_drop() {
    for pending in ["await", "write"] {
        for outcome in ["complete", "cancel", "drop"] {
            let model = HeldModel {
                rows: Model::new(&["&", "<"]),
                open: AtomicBool::new(pending == "write"),
            };
            let page = Held::new(&model);
            let (mut writer, state) = sink();
            if pending == "write" {
                state.lock().unwrap().hold_after = Some(3);
            }
            let token = CancellationToken::new();
            let context = RenderContext::new((), token.clone());
            let waker = Arc::new(Noop).into();
            {
                let mut render = pin!(render_incremental_with_context(
                    &page,
                    &mut writer,
                    &context
                ));
                assert!(render
                    .as_mut()
                    .poll(&mut Context::from_waker(&waker))
                    .is_pending());
                assert_eq!(state.lock().unwrap().bytes, b"[0:");
                assert_eq!(model.rows.conditions.load(Ordering::SeqCst), 1);
                assert_eq!(model.rows.index.load(Ordering::SeqCst), 1);
                match outcome {
                    "complete" => {
                        model.open.store(true, Ordering::SeqCst);
                        state.lock().unwrap().hold_after = None;
                        assert!(matches!(
                            render.as_mut().poll(&mut Context::from_waker(&waker)),
                            Poll::Ready(Ok(()))
                        ));
                    }
                    "cancel" => {
                        token.cancel();
                        let Poll::Ready(Err(AsyncRenderError::Write(error))) =
                            render.as_mut().poll(&mut Context::from_waker(&waker))
                        else {
                            panic!("held loop must observe cancellation");
                        };
                        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
                        assert!(error.get_ref().unwrap().is::<RenderCancelled>());
                    }
                    "drop" => {}
                    _ => unreachable!(),
                }
            }
            let state = state.lock().unwrap();
            assert_eq!(
                state.bytes,
                if outcome == "complete" {
                    b"[0:&amp;;1:&lt;;]".as_slice()
                } else {
                    b"[0:"
                }
            );
            assert_eq!(state.flushes.len(), usize::from(outcome == "complete"));
            assert_eq!(
                model.rows.conditions.load(Ordering::SeqCst),
                if outcome == "complete" { 3 } else { 1 }
            );
        }
    }
}

async_template! {
    struct Child<'a> { model: &'a str } => { {model} }
}

struct Section;
impl AsyncSlice for Section {
    async fn render_into<W: AsyncSliceWriter + Send>(
        &self,
        _: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        panic!("section dispatch must not render the provider body")
    }

    async fn render_section_into<W: AsyncSliceWriter + Send>(
        &self,
        name: &str,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> Result<(), AsyncRenderError> {
        assert_eq!(name, "row");
        output.write_literal("S").await?;
        Ok(())
    }
}

struct DirectivesModel {
    rows: Model,
    section: Section,
}

async_template! {
    struct Directives<'a> { model: &'a DirectivesModel } => {
        "["
        while let Some((index, text)) = model.rows.next() {
            @{ let observed = index; }
            @await { async { assert_eq!(observed, index); } };
            @render { Child::new(text) };
            @section { model.section }, { "row" };
            @flush;
        }
        "]"
    }
}

#[tokio::test]
async fn nested_async_directives_keep_scope_dispatch_and_explicit_flush_order() {
    let model = DirectivesModel {
        rows: Model::new(&["&", "<"]),
        section: Section,
    };
    let (mut writer, state) = sink();
    render_incremental(&Directives::new(&model), &mut writer)
        .await
        .unwrap();
    let state = state.lock().unwrap();
    assert_eq!(state.bytes, b"[&amp;S&lt;S]");
    assert_eq!(state.flushes, [7, 5, 1]);
}

async_template! {
    struct Chunks<'a> { model: &'a [&'a str] } => {
        @{ let mut chunks = model.iter(); }
        while let Some(text) = chunks.next() { {text} }
    }
}

#[tokio::test]
async fn loop_iterations_are_not_new_automatic_flush_boundaries() {
    for native in [false, true] {
        for size in [16_383, 16_384, 16_385] {
            let first = "x".repeat(size / 2);
            let second = "x".repeat(size - first.len());
            let parts = [first.as_str(), second.as_str()];
            let page = Chunks::new(&parts);
            for completed_child in [false, true] {
                let (mut writer, state) = sink();
                writer.native = native;
                let mut output = AsyncHtmlWriter::new(&mut writer);
                if completed_child {
                    output.write_slice(&page).await.unwrap();
                } else {
                    page.render_into(&mut output).await.unwrap();
                }

                let state = state.lock().unwrap();
                assert_eq!(state.bytes, vec![b'x'; size]);
                assert_eq!(
                    state.flushes,
                    if completed_child && size >= 16_384 {
                        vec![size]
                    } else {
                        vec![]
                    }
                );
            }
        }
    }
}

#[test]
fn while_grammar_type_scope_and_capture_contracts_are_compile_checked() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new(env!("CARGO"))
        .args(["test", "--doc", "--manifest-path"])
        .arg(root.join("tests/while-loops-negative/Cargo.toml"))
        .arg("--target-dir")
        .arg(root.join("../../target/while-loops-negative"))
        .output()
        .expect("run while-loop compile-fail doctests");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains("7 passed"),
        "while-loop compile contract failed:\n{stdout}\n{stderr}"
    );
}
