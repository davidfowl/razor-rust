use razor_slices::{
    async_template, html, partial, render_incremental, AsyncRenderError, AsyncSliceWriter,
    HtmlString, Slice,
};
use std::{
    future::{poll_fn, Future},
    io,
    pin::pin,
    sync::{
        atomic::{AtomicI32, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll, Wake},
};

struct Effects {
    value: AtomicI32,
    trace: Mutex<Vec<&'static str>>,
    exits: AtomicUsize,
}

impl Effects {
    fn new(value: i32) -> Self {
        Self {
            value: AtomicI32::new(value),
            trace: Mutex::new(Vec::new()),
            exits: AtomicUsize::new(0),
        }
    }
    fn select(&self) -> i32 {
        self.touch("select");
        self.value.load(Ordering::SeqCst)
    }
    fn guard(&self, candidate: i32) -> bool {
        self.touch("guard");
        candidate == 3
    }
    fn touch(&self, label: &'static str) -> &'static str {
        self.trace.lock().unwrap().push(label);
        label
    }
}

fn page(effects: &Effects) -> impl Slice + '_ {
    html! {
        "A"
        match effects.select() {
            0 => { "<b>" {effects.touch("zero")} "</b>" }
            1 | 2 => { "<i>" {effects.touch("pair")} ":" {effects.value.load(Ordering::SeqCst)} "</i>" },
            value if effects.guard(value) => { "<u>" {effects.touch("guard")} ":" {value} "</u>" }
            _ => { "<em>" {effects.touch("fallback")} "</em>" }
        }
        "Z\n"
    }
}

async_template! {
    struct Page<'a> { model: &'a Effects } => {
        "A"
        match model.select() {
            0 => { "<b>" {model.touch("zero")} "</b>" }
            1 | 2 => { "<i>" {model.touch("pair")} ":" {model.value.load(Ordering::SeqCst)} "</i>" },
            value if model.guard(value) => { "<u>" {model.touch("guard")} ":" {value} "</u>" }
            _ => { "<em>" {model.touch("fallback")} "</em>" }
        }
        "Z\n"
    }
}

#[derive(Default)]
struct State {
    bytes: Vec<u8>,
    fail_at: Option<usize>,
    hold_after: Option<usize>,
    flushes: usize,
}
struct Writer(Arc<Mutex<State>>);

#[derive(Debug)]
struct Original;
impl std::fmt::Display for Original {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("original match failure")
    }
}
impl std::error::Error for Original {}
fn failure() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, Original)
}
fn assert_original(error: io::Error) {
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    assert!(error.get_ref().unwrap().is::<Original>());
}
impl io::Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut state = self.0.lock().unwrap();
        let count = state.fail_at.map_or(bytes.len(), |limit| {
            limit.saturating_sub(state.bytes.len()).min(bytes.len())
        });
        if count == 0 && !bytes.is_empty() {
            return Err(failure());
        }
        state.bytes.extend_from_slice(&bytes[..count]);
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.lock().unwrap().flushes += 1;
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

const EXPECTED: &[u8] = b"A<u>guard:3</u>Z\n";

#[tokio::test]
async fn actual_razor_switch_goldens_match_streamed_output_and_ordered_effects() {
    let mut actual = String::new();
    for value in [-1, 0, 1, 2, 3, 9] {
        let text = Effects::new(value);
        let bytes = Effects::new(value);
        let asynchronous = Effects::new(value);
        let text_page = page(&text);
        let byte_page = page(&bytes);
        let async_page = Page::new(&asynchronous);
        assert!(text.trace.lock().unwrap().is_empty());
        assert!(bytes.trace.lock().unwrap().is_empty());
        assert!(asynchronous.trace.lock().unwrap().is_empty());
        let mut first = None;
        for _ in 0..2 {
            for effects in [&text, &bytes, &asynchronous] {
                effects.trace.lock().unwrap().clear();
            }
            let output = text_page.render_to_string().unwrap();
            assert_eq!(byte_page.render_to_bytes().unwrap(), output.as_bytes());
            let (mut writer, state) = sink();
            render_incremental(&async_page, &mut writer).await.unwrap();
            assert_eq!(state.lock().unwrap().bytes, output.as_bytes());
            let trace = text.trace.lock().unwrap().clone();
            assert_eq!(*bytes.trace.lock().unwrap(), trace);
            assert_eq!(*asynchronous.trace.lock().unwrap(), trace);
            if let Some(first) = &first {
                assert_eq!(first, &output);
            } else {
                first = Some(output);
            }
        }
        actual.push_str(&format!(
            "{value}: {}|{}\n",
            escape(&first.unwrap()),
            text.trace.lock().unwrap().join(",")
        ));
    }
    for limit in 0..EXPECTED.len() {
        let effects = Effects::new(3);
        let (mut writer, state) = sink();
        state.lock().unwrap().fail_at = Some(limit);
        assert_original(page(&effects).render_to_writer(&mut writer).unwrap_err());
        assert_eq!(state.lock().unwrap().bytes, EXPECTED[..limit]);
        assert_eq!(state.lock().unwrap().flushes, 0);
        let trace = effects.trace.lock().unwrap().clone();
        actual.push_str(&format!(
            "fail-{limit}: {}|{}\n",
            escape(std::str::from_utf8(&EXPECTED[..limit]).unwrap()),
            trace.join(",")
        ));
        let effects = Effects::new(3);
        let (mut writer, state) = sink();
        state.lock().unwrap().fail_at = Some(limit);
        assert!(page(&effects).render(&mut writer).is_err());
        assert_eq!(state.lock().unwrap().bytes, EXPECTED[..limit]);
        assert_eq!(*effects.trace.lock().unwrap(), trace);
        let effects = Effects::new(3);
        let (mut writer, state) = sink();
        state.lock().unwrap().fail_at = Some(limit);
        let AsyncRenderError::Write(error) = render_incremental(&Page::new(&effects), &mut writer)
            .await
            .unwrap_err()
        else {
            panic!("expected original selected-arm writer failure");
        };
        assert_original(error);
        assert_eq!(state.lock().unwrap().bytes, EXPECTED[..limit]);
        assert_eq!(state.lock().unwrap().flushes, 0);
        assert_eq!(*effects.trace.lock().unwrap(), trace);
    }
    assert_eq!(actual, include_str!("fixtures/template-match.txt"));
}

#[test]
fn expression_matches_already_select_single_values() {
    let value = 2;
    let page = html! { {match value { 1 => "one", _ => "&" }} };
    assert_eq!(page.render_to_string().unwrap(), "&amp;");
}

#[test]
fn patterns_guards_scopes_hygiene_and_heterogeneous_parts_remain_reusable() {
    let owned = Some(String::from("<&>"));
    let child = html! { "<hr>" };
    let nested_page = html! {
        match (&owned, 1u8) {
            (Some(__razor_writer), 0..=2) if !__razor_writer.is_empty() => {
                for value in [Some(1), None] {
                    match value {
                        Some(number) => { {number} {__razor_writer} }
                        None => { {partial(&child)} {HtmlString::from("<br>")} }
                    }
                }
            },
            _ => { "empty" }
        }
    };
    for _ in 0..2 {
        assert_eq!(
            nested_page.render_to_string().unwrap(),
            "1&lt;&amp;&gt;<hr><br>"
        );
    }
    let effects = Effects::new(0);
    let page = page(&effects);
    for (value, expected) in [
        (0, "A<b>zero</b>Z\n"),
        (3, "A<u>guard:3</u>Z\n"),
        (2, "A<i>pair:2</i>Z\n"),
        (-1, "A<em>fallback</em>Z\n"),
    ] {
        effects.value.store(value, Ordering::SeqCst);
        assert_eq!(page.render_to_string().unwrap(), expected);
        assert_eq!(
            effects
                .trace
                .lock()
                .unwrap()
                .iter()
                .filter(|v| **v == "select")
                .count(),
            1
        );
        effects.trace.lock().unwrap().clear();
    }
    assert_eq!(
        html! { match () { () => {} } }.render_to_string().unwrap(),
        ""
    );
}

struct Lease<'a>(&'a Effects);
impl Drop for Lease<'_> {
    fn drop(&mut self) {
        self.0.exits.fetch_add(1, Ordering::SeqCst);
    }
}
fn unexpected() -> &'static str {
    panic!("unselected arm must not execute")
}
async_template! {
    struct Held<'a> { model: &'a Effects } => {
        "A"
        match model.select() {
            value @ 3 => {
                @{ let _lease = Lease(model); }
                "B" {value}
                @flush;
                @await { async { model.touch("await"); } };
                {model.touch("body")}
            }
            _ => { {unexpected()} }
        }
        "Z"
    }
}

struct Noop;
impl Wake for Noop {
    fn wake(self: Arc<Self>) {}
}

#[test]
fn pending_selected_arm_keeps_bindings_and_drops_locals_without_reselecting() {
    for complete in [false, true] {
        let effects = Effects::new(3);
        let page = Held::new(&effects);
        let (mut writer, state) = sink();
        state.lock().unwrap().hold_after = Some(1);
        let waker = Arc::new(Noop).into();
        {
            let mut render = pin!(render_incremental(&page, &mut writer));
            assert!(render
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending());
            assert_eq!(state.lock().unwrap().bytes, b"A");
            assert_eq!(*effects.trace.lock().unwrap(), ["select"]);
            assert_eq!(effects.exits.load(Ordering::SeqCst), 0);
            if complete {
                state.lock().unwrap().hold_after = None;
                assert!(matches!(
                    render.as_mut().poll(&mut Context::from_waker(&waker)),
                    Poll::Ready(Ok(()))
                ));
            }
        }
        assert_eq!(effects.exits.load(Ordering::SeqCst), 1);
        assert_eq!(
            *effects.trace.lock().unwrap(),
            if complete {
                vec!["select", "await", "body"]
            } else {
                vec!["select"]
            }
        );
        assert_eq!(
            state.lock().unwrap().bytes,
            if complete {
                b"AB3bodyZ".as_slice()
            } else {
                b"A"
            }
        );
        assert_eq!(state.lock().unwrap().flushes, if complete { 2 } else { 0 });
    }
}

fn fallible() -> io::Result<i32> {
    Err(failure())
}
async_template! {
    struct Fallible<'a> { model: &'a Effects } => {
        match model.select() {
            3 => {
                @{ let _lease = Lease(model); }
                @await { async { Err::<(), _>(failure()) } }?;
                {model.touch("unreachable")}
            }
            _ => {}
        }
    }
}

#[tokio::test]
async fn scrutinee_and_selected_await_errors_preserve_identity_and_stop_output() {
    let page = html! { match fallible()? { _ => { "unreachable" } } };
    let (mut writer, state) = sink();
    assert_original(page.render_to_writer(&mut writer).unwrap_err());
    assert!(state.lock().unwrap().bytes.is_empty());
    let effects = Effects::new(3);
    let AsyncRenderError::Write(error) = render_incremental(&Fallible::new(&effects), &mut writer)
        .await
        .unwrap_err()
    else {
        panic!("expected original selected-arm await failure");
    };
    assert_original(error);
    assert!(state.lock().unwrap().bytes.is_empty());
    assert_eq!(*effects.trace.lock().unwrap(), ["select"]);
    assert_eq!(effects.exits.load(Ordering::SeqCst), 1);
}
