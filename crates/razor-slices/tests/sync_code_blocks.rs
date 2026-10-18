use razor_slices::{
    async_template, html, render_incremental, render_incremental_with_context, AsyncRenderError,
    AsyncSliceWriter, CancellationToken, RenderCancelled, RenderContext, Slice,
};
use std::{
    future::{poll_fn, Future},
    io,
    pin::pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll, Wake},
};

struct Effects {
    prefix: &'static str,
    values: Vec<i32>,
    touches: AtomicUsize,
    exits: AtomicUsize,
}

impl Effects {
    fn new(prefix: &'static str, values: &[i32]) -> Self {
        Self {
            prefix,
            values: values.to_vec(),
            touches: AtomicUsize::new(0),
            exits: AtomicUsize::new(0),
        }
    }

    fn enter(&self) -> Lease<'_> {
        Lease(self)
    }

    fn touch(&self) {
        self.touches.fetch_add(1, Ordering::SeqCst);
    }
}

struct Lease<'a>(&'a Effects);
impl Drop for Lease<'_> {
    fn drop(&mut self) {
        self.0.exits.fetch_add(1, Ordering::SeqCst);
    }
}

fn page(effects: &Effects) -> impl Slice + '_ {
    html! {
        @{
            let mut total = 0;
            let prefix = effects.prefix;
            let _lease = effects.enter();
            effects.touch()
        }
        "[" {prefix}
        for value in &effects.values {
            @{ total += *value; effects.touch() }
            "<span>" {value} "</span>"
        }
        {total} "]\n"
    }
}

async_template! {
    struct AsyncPage<'a> { model: &'a Effects } => {
        @{
            let mut total = 0;
            let prefix = model.prefix;
            let _lease = model.enter();
            model.touch()
        }
        "[" {prefix}
        for value in &model.values {
            @{ total += *value; model.touch() }
            "<span>" {value} "</span>"
        }
        {total} "]\n"
    }
}

#[derive(Default)]
struct State {
    bytes: Vec<u8>,
    flushes: usize,
    fail_at: Option<usize>,
    fail_flush: bool,
    hold_after: Option<usize>,
}

struct Writer(Arc<Mutex<State>>);

#[derive(Debug)]
struct Original;
impl std::fmt::Display for Original {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("original code-block writer failure")
    }
}
impl std::error::Error for Original {}

fn failure() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, Original)
}

impl io::Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut state = self.0.lock().unwrap();
        let accepted = state.fail_at.map_or(bytes.len(), |limit| {
            limit.saturating_sub(state.bytes.len()).min(bytes.len())
        });
        if accepted == 0 && !bytes.is_empty() {
            return Err(failure());
        }
        state.bytes.extend_from_slice(&bytes[..accepted]);
        Ok(accepted)
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut state = self.0.lock().unwrap();
        state.flushes += 1;
        if state.fail_flush {
            return Err(failure());
        }
        Ok(())
    }
}

impl std::fmt::Write for Writer {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        io::Write::write_all(self, text.as_bytes()).map_err(|_| std::fmt::Error)
    }
}

impl AsyncSliceWriter for Writer {
    async fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        poll_fn(|_| {
            let state = self.0.lock().unwrap();
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
}

fn sink() -> (Writer, Arc<Mutex<State>>) {
    let state = Arc::new(Mutex::new(State::default()));
    (Writer(state.clone()), state)
}

fn escape(text: &str) -> String {
    text.replace('\r', "\\r").replace('\n', "\\n")
}

fn assert_original(error: io::Error) {
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    assert!(error.get_ref().unwrap().is::<Original>());
}

const EXPECTED: &[u8] = b"[&amp;<span>1</span><span>2</span>3]\n";

#[tokio::test]
async fn razor_compiled_golden_matches_output_effect_order_and_local_cleanup() {
    let mut actual = String::new();
    for (name, prefix, values) in [
        ("empty", "", [].as_slice()),
        ("multiple", "&", [1, 2].as_slice()),
        ("single", "<", [7].as_slice()),
    ] {
        let text_effects = Effects::new(prefix, values);
        let byte_effects = Effects::new(prefix, values);
        let async_effects = Effects::new(prefix, values);
        let text_page = page(&text_effects);
        let byte_page = page(&byte_effects);
        let async_page = AsyncPage::new(&async_effects);
        assert_eq!(text_effects.touches.load(Ordering::SeqCst), 0);
        assert_eq!(byte_effects.touches.load(Ordering::SeqCst), 0);
        assert_eq!(async_effects.touches.load(Ordering::SeqCst), 0);
        let mut first = None;
        for attempt in 1..=2 {
            let text = text_page.render_to_string().unwrap();
            assert_eq!(byte_page.render_to_bytes().unwrap(), text.as_bytes());
            let (mut writer, state) = sink();
            render_incremental(&async_page, &mut writer).await.unwrap();
            assert_eq!(state.lock().unwrap().bytes, text.as_bytes());
            assert_eq!(state.lock().unwrap().flushes, 1);
            if let Some(first) = &first {
                assert_eq!(first, &text);
            } else {
                first = Some(text);
            }
            for effects in [&text_effects, &byte_effects, &async_effects] {
                assert_eq!(
                    effects.touches.load(Ordering::SeqCst),
                    attempt * (values.len() + 1)
                );
                assert_eq!(effects.exits.load(Ordering::SeqCst), attempt);
            }
        }
        actual.push_str(&format!("{name}: {}\n", escape(&first.unwrap())));
    }
    for limit in 0..EXPECTED.len() {
        let byte_effects = Effects::new("&", &[1, 2]);
        let (mut writer, state) = sink();
        state.lock().unwrap().fail_at = Some(limit);
        assert_original(
            page(&byte_effects)
                .render_to_writer(&mut writer)
                .unwrap_err(),
        );
        let touches = {
            let state = state.lock().unwrap();
            assert_eq!(state.bytes, EXPECTED[..limit]);
            assert_eq!(state.flushes, 0);
            assert_eq!(byte_effects.exits.load(Ordering::SeqCst), 1);
            let touches = byte_effects.touches.load(Ordering::SeqCst);
            actual.push_str(&format!(
                "fail-{limit}: {}|touches={touches},exits=1\n",
                escape(std::str::from_utf8(&state.bytes).unwrap())
            ));
            touches
        };
        let text_effects = Effects::new("&", &[1, 2]);
        let (mut writer, state) = sink();
        state.lock().unwrap().fail_at = Some(limit);
        assert!(page(&text_effects).render(&mut writer).is_err());
        assert_eq!(state.lock().unwrap().bytes, EXPECTED[..limit]);
        assert_eq!(text_effects.touches.load(Ordering::SeqCst), touches);
        assert_eq!(text_effects.exits.load(Ordering::SeqCst), 1);
        let async_effects = Effects::new("&", &[1, 2]);
        let (mut writer, state) = sink();
        state.lock().unwrap().fail_at = Some(limit);
        let AsyncRenderError::Write(error) =
            render_incremental(&AsyncPage::new(&async_effects), &mut writer)
                .await
                .unwrap_err()
        else {
            panic!("expected original writer failure");
        };
        assert_original(error);
        assert_eq!(state.lock().unwrap().bytes, EXPECTED[..limit]);
        assert_eq!(state.lock().unwrap().flushes, 0);
        assert_eq!(async_effects.touches.load(Ordering::SeqCst), touches);
        assert_eq!(async_effects.exits.load(Ordering::SeqCst), 1);
    }
    assert_eq!(actual, include_str!("fixtures/sync-code-blocks.txt"));
}

#[test]
fn nested_scopes_shadowing_and_owned_local_values_remain_reusable_and_hygienic() {
    let captured: Vec<_> = (1..=2).collect();
    let page = html! {
        @{
            let label = "root";
            let __razor_writer = "user";
            let mut total = 0;
            let local: Vec<_> = captured.iter().map(|value| value + 1).collect();
        }
        if true {
            @{ let label = "<inner>"; }
            {label}
        }
        for value in &local {
            @{ total += *value; }
            {value}
        }
        {label} {total} {__razor_writer}
    };
    for _ in 0..2 {
        assert_eq!(page.render_to_string().unwrap(), "&lt;inner&gt;23root5user");
    }
}

fn check(fail: bool) -> io::Result<()> {
    if fail {
        Err(failure())
    } else {
        Ok(())
    }
}

async_template! {
    struct Fallible<'a> { model: &'a Effects } => {
        @{ let _lease = model.enter(); check(true)?; }
        "unreachable"
    }
}

#[tokio::test]
async fn code_block_question_marks_preserve_original_errors_and_cleanup() {
    let effects = Effects::new("", &[]);
    let effects = &effects;
    let page = html! {
        @{ let _lease = effects.enter(); check(true)?; }
        "unreachable"
    };
    let (mut writer, state) = sink();
    assert_original(page.render_to_writer(&mut writer).unwrap_err());
    assert!(state.lock().unwrap().bytes.is_empty());
    assert_eq!(effects.exits.load(Ordering::SeqCst), 1);
    let AsyncRenderError::Write(error) = render_incremental(&Fallible::new(effects), &mut writer)
        .await
        .unwrap_err()
    else {
        panic!("expected original application error");
    };
    assert_original(error);
    assert!(state.lock().unwrap().bytes.is_empty());
    assert_eq!(state.lock().unwrap().flushes, 0);
    assert_eq!(effects.exits.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn unicode_failure_prefixes_and_final_flush_errors_keep_local_cleanup() {
    let expected = "[\u{1f980}&amp;0]\n".as_bytes();
    for limit in 0..expected.len() {
        let effects = Effects::new("\u{1f980}&", &[]);
        let (mut writer, state) = sink();
        state.lock().unwrap().fail_at = Some(limit);
        assert_original(page(&effects).render_to_writer(&mut writer).unwrap_err());
        assert_eq!(state.lock().unwrap().bytes, expected[..limit]);
        assert_eq!(effects.exits.load(Ordering::SeqCst), 1);
        let (mut writer, state) = sink();
        state.lock().unwrap().fail_at = Some(limit);
        let AsyncRenderError::Write(error) =
            render_incremental(&AsyncPage::new(&effects), &mut writer)
                .await
                .unwrap_err()
        else {
            panic!("expected original Unicode writer error");
        };
        assert_original(error);
        assert_eq!(state.lock().unwrap().bytes, expected[..limit]);
        assert_eq!(effects.exits.load(Ordering::SeqCst), 2);
    }
    let effects = Effects::new("&", &[1, 2]);
    let (mut writer, state) = sink();
    state.lock().unwrap().fail_flush = true;
    let AsyncRenderError::Write(error) = render_incremental(&AsyncPage::new(&effects), &mut writer)
        .await
        .unwrap_err()
    else {
        panic!("expected original final-flush error");
    };
    assert_original(error);
    assert_eq!(state.lock().unwrap().bytes, EXPECTED);
    assert_eq!(state.lock().unwrap().flushes, 1);
    assert_eq!(effects.exits.load(Ordering::SeqCst), 1);
}

struct Noop;
impl Wake for Noop {
    fn wake(self: Arc<Self>) {}
}

#[test]
fn existing_async_blocks_retain_locals_until_completion_cancellation_or_drop() {
    for outcome in ["complete", "cancel", "drop"] {
        let effects = Effects::new("&", &[1, 2]);
        let page = AsyncPage::new(&effects);
        let (mut writer, state) = sink();
        state.lock().unwrap().hold_after = Some(1);
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
            assert_eq!(state.lock().unwrap().bytes, b"[");
            assert_eq!(effects.touches.load(Ordering::SeqCst), 1);
            assert_eq!(effects.exits.load(Ordering::SeqCst), 0);
            match outcome {
                "complete" => {
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
                        panic!("held block must observe scoped cancellation");
                    };
                    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
                    assert!(error.get_ref().unwrap().is::<RenderCancelled>());
                }
                "drop" => {}
                _ => unreachable!(),
            }
        }

        assert_eq!(effects.exits.load(Ordering::SeqCst), 1);
        let state = state.lock().unwrap();
        assert_eq!(
            state.bytes,
            if outcome == "complete" {
                EXPECTED
            } else {
                b"["
            }
        );
        assert_eq!(state.flushes, usize::from(outcome == "complete"));
    }
}
