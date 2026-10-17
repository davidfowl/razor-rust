use razor_slices::{
    async_template, html, render_incremental, render_incremental_with_context, AsyncHtmlWriter,
    AsyncRenderError, AsyncSlice, AsyncSliceWriter, AsyncWriteHtmlValue, CancellationToken,
    HtmlString, RenderCancelled, RenderContext, Slice, WriteHtmlValue,
};
use std::{
    future::{poll_fn, Future},
    io,
    pin::pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Wake},
};

#[derive(Default)]
struct State {
    bytes: Vec<u8>,
    calls: usize,
    flushes: usize,
    max_write: usize,
    fail_at: Option<usize>,
    fail_flush: bool,
    hold_after: Option<usize>,
}

struct Writer(Arc<Mutex<State>>);

#[derive(Debug)]
struct Original;
impl std::fmt::Display for Original {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("original optional-value sink failure")
    }
}
impl std::error::Error for Original {}

fn failure() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, Original)
}

impl io::Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut state = self.0.lock().unwrap();
        state.calls += 1;
        state.max_write = state.max_write.max(bytes.len());
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

impl AsyncSliceWriter for Writer {
    async fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        poll_fn(|_| {
            let held = {
                let state = self.0.lock().unwrap();
                state
                    .hold_after
                    .is_some_and(|limit| state.bytes.len() >= limit)
            };
            if held {
                Poll::Pending
            } else {
                Poll::Ready(io::Write::write_all(self, bytes))
            }
        })
        .await
    }

    async fn flush(&mut self) -> io::Result<()> {
        io::Write::flush(self)
    }
}

fn writer() -> (Writer, Arc<Mutex<State>>) {
    let state = Arc::new(Mutex::new(State::default()));
    (Writer(state.clone()), state)
}

async_template! {
    struct Optional<V> where V: AsyncWriteHtmlValue { model: Option<V> } => {
        "[" {model} "]"
    }
}

const ENCODED: &str = "[A&amp;&lt;&gt;Z]";

#[tokio::test]
async fn optional_model_values_render_in_both_template_paths() {
    for value in [None, Some(""), Some("A&<>Z")] {
        let expected = if value == Some("A&<>Z") {
            ENCODED
        } else {
            "[]"
        };
        let page = html! { "[" {value} "]" };
        assert_eq!(page.render_to_string().unwrap(), expected);
        let (mut writer, state) = writer();
        render_incremental(&Optional::new(value), &mut writer)
            .await
            .unwrap();
        let state = state.lock().unwrap();
        assert_eq!(state.bytes, expected.as_bytes());
        assert_eq!(state.flushes, 1);
    }
}

async fn golden_case<V: WriteHtmlValue + AsyncWriteHtmlValue>(
    name: &str,
    value: Option<V>,
    actual: &mut String,
) {
    let borrowed = &value;
    let slice = html! { "[" {borrowed} "]" };
    let text = slice.render_to_string().unwrap();
    assert_eq!(slice.render_to_bytes().unwrap(), text.as_bytes());
    let (mut writer, state) = writer();
    render_incremental(&Optional::new(value.as_ref()), &mut writer)
        .await
        .unwrap();
    let state = state.lock().unwrap();
    assert_eq!(state.bytes, text.as_bytes());
    assert_eq!(state.flushes, 1);
    actual.push_str(&format!("{name}: {text}\n"));
}

#[tokio::test]
async fn pinned_csharp_golden_matches_optional_values_on_sync_and_async_paths() {
    let mut actual = String::new();
    golden_case("string-null", None::<&str>, &mut actual).await;
    golden_case("string-empty", Some(""), &mut actual).await;
    golden_case("string-encoded", Some("A&<>Z"), &mut actual).await;
    golden_case("integer-null", None::<i32>, &mut actual).await;
    golden_case("integer-zero", Some(0_i32), &mut actual).await;
    golden_case("integer-negative", Some(-42_i32), &mut actual).await;
    golden_case("integer-min", Some(i64::MIN), &mut actual).await;
    golden_case("integer-max", Some(i64::MAX), &mut actual).await;
    golden_case("html-null", None::<HtmlString>, &mut actual).await;
    golden_case(
        "html-trusted",
        Some(HtmlString::from("<b>&\"'\u{96ea}</b>")),
        &mut actual,
    )
    .await;
    golden_case("float-null", None::<f64>, &mut actual).await;
    golden_case("float-value", Some(1.5_f64), &mut actual).await;
    assert_eq!(actual, include_str!("fixtures/optional-values.txt"));
}

struct Custom(&'static str);
impl WriteHtmlValue for Custom {
    fn write_html_to(&self, writer: &mut dyn io::Write) -> io::Result<()> {
        self.0.write_html_to(writer)
    }
}
impl AsyncWriteHtmlValue for Custom {
    async fn write_html_async<W: AsyncSliceWriter + Send>(
        &self,
        output: &mut AsyncHtmlWriter<'_, W>,
    ) -> io::Result<()> {
        self.0.write_html_async(output).await
    }
}

#[tokio::test]
async fn owned_nested_referenced_and_nonclone_values_delegate_without_consuming() {
    let owned = Some(String::from("A&<>Z"));
    let synchronous = html! { "[" {owned} "]" };
    let asynchronous = Optional::new(Some(String::from("A&<>Z")));
    let custom = Some(Custom("A&<>Z"));
    let custom_sync = html! { "[" {custom} "]" };
    let custom_async = Optional::new(Some(Custom("A&<>Z")));
    for _ in 0..2 {
        assert_eq!(synchronous.render_to_string().unwrap(), ENCODED);
        assert_eq!(custom_sync.render_to_string().unwrap(), ENCODED);
        let (mut writer, state) = writer();
        render_incremental(&asynchronous, &mut writer)
            .await
            .unwrap();
        render_incremental(&custom_async, &mut writer)
            .await
            .unwrap();
        assert_eq!(state.lock().unwrap().bytes, ENCODED.repeat(2).as_bytes());
    }

    let nested = Some(Some("A&<>Z"));
    let borrowed = &nested;
    assert_eq!(
        html! { "[" {borrowed} "]" }.render_to_string().unwrap(),
        ENCODED
    );
    let (mut writer, state) = writer();
    render_incremental(&Optional::new(Some(borrowed)), &mut writer)
        .await
        .unwrap();
    assert_eq!(state.lock().unwrap().bytes, ENCODED.as_bytes());

    let (mut writer, state) = self::writer();
    Some(false).write_html_to(&mut writer).unwrap();
    AsyncHtmlWriter::new(&mut writer)
        .write_html_value(&Some(false))
        .await
        .unwrap();
    assert_eq!(state.lock().unwrap().bytes, b"falsefalse");
}

#[tokio::test]
async fn none_and_nested_none_do_not_touch_even_a_failing_destination() {
    let (mut writer, state) = writer();
    {
        let mut state = state.lock().unwrap();
        state.fail_at = Some(0);
        state.fail_flush = true;
    }
    None::<Custom>.write_html_to(&mut writer).unwrap();
    Some(None::<Custom>).write_html_to(&mut writer).unwrap();
    let mut output = AsyncHtmlWriter::new(&mut writer);
    output.write_html_value(&None::<Custom>).await.unwrap();
    output
        .write_html_value(&Some(None::<Custom>))
        .await
        .unwrap();
    let token = CancellationToken::new();
    token.cancel();
    let context = RenderContext::new((), token);
    let AsyncRenderError::Write(error) =
        render_incremental_with_context(&Optional::<&str>::new(None), &mut writer, &context)
            .await
            .unwrap_err()
    else {
        panic!("a cancelled scope must not render an absent value as success");
    };
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    assert!(error.get_ref().unwrap().is::<RenderCancelled>());
    let state = state.lock().unwrap();
    assert!(state.bytes.is_empty());
    assert_eq!(state.calls, 0);
    assert_eq!(state.flushes, 0);
}

fn assert_original(error: io::Error) {
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    assert!(error.get_ref().unwrap().is::<Original>());
}

#[tokio::test]
async fn optional_values_keep_every_byte_failure_and_final_flush_error() {
    for limit in 0..ENCODED.len() {
        let (mut writer, state) = writer();
        state.lock().unwrap().fail_at = Some(limit);
        let value = Some("A&<>Z");
        assert_original(html! { "[" {value} "]" }.write_to(&mut writer).unwrap_err());
        assert_eq!(state.lock().unwrap().bytes, &ENCODED.as_bytes()[..limit]);
        assert_eq!(state.lock().unwrap().flushes, 0);

        let (mut writer, state) = self::writer();
        state.lock().unwrap().fail_at = Some(limit);
        let AsyncRenderError::Write(error) = render_incremental(&Optional::new(value), &mut writer)
            .await
            .unwrap_err()
        else {
            panic!("expected original async writer error");
        };
        assert_original(error);
        assert_eq!(state.lock().unwrap().bytes, &ENCODED.as_bytes()[..limit]);
        assert_eq!(state.lock().unwrap().flushes, 0);
    }
    let (mut writer, state) = writer();
    state.lock().unwrap().fail_flush = true;
    let AsyncRenderError::Write(error) =
        render_incremental(&Optional::<&str>::new(None), &mut writer)
            .await
            .unwrap_err()
    else {
        panic!("expected original root-final flush error");
    };
    assert_original(error);
    assert_eq!(state.lock().unwrap().bytes, b"[]");
    assert_eq!(state.lock().unwrap().flushes, 1);
}

struct Noop;
impl Wake for Noop {
    fn wake(self: Arc<Self>) {}
}

#[test]
fn some_preserves_pending_write_backpressure_drop_and_scoped_cancellation() {
    for outcome in ["complete", "drop", "cancel"] {
        let (mut writer, state) = writer();
        state.lock().unwrap().hold_after = Some(1);
        let token = CancellationToken::new();
        let context = RenderContext::new((), token.clone());
        let page = Optional::new(Some("A&<>Z"));
        let waker = Arc::new(Noop).into();
        {
            let mut future = pin!(render_incremental_with_context(
                &page,
                &mut writer,
                &context
            ));
            assert!(future
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending());
            assert_eq!(state.lock().unwrap().bytes, b"[");
            assert_eq!(state.lock().unwrap().flushes, 0);
            match outcome {
                "complete" => {
                    state.lock().unwrap().hold_after = None;
                    assert!(matches!(
                        future.as_mut().poll(&mut Context::from_waker(&waker)),
                        Poll::Ready(Ok(()))
                    ));
                }
                "cancel" => {
                    token.cancel();
                    let Poll::Ready(Err(AsyncRenderError::Write(error))) =
                        future.as_mut().poll(&mut Context::from_waker(&waker))
                    else {
                        panic!("cancellation must interrupt an optional value's held write");
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
                ENCODED.as_bytes()
            } else {
                b"["
            }
        );
        assert_eq!(state.flushes, usize::from(outcome == "complete"));
    }
}

#[tokio::test]
async fn optional_encoded_output_retains_chunk_bounds_and_completion_accounting() {
    let text = "&".repeat(5_000);
    let page = Optional::new(Some(&text));
    let (mut writer, state) = writer();
    let mut output = AsyncHtmlWriter::new(&mut writer);
    page.render_into(&mut output).await.unwrap();
    assert_eq!(state.lock().unwrap().flushes, 0);
    output
        .write_slice(&Optional::<&str>::new(None))
        .await
        .unwrap();
    let state = state.lock().unwrap();
    assert_eq!(
        state.bytes,
        format!("[{}][]", "&amp;".repeat(5_000)).as_bytes()
    );
    assert_eq!(state.flushes, 1);
    assert!(state.max_write <= 6_144);
}
