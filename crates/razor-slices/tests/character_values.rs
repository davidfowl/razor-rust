use razor_slices::{
    async_template, attribute, encode_html_to_writer, html, render_incremental,
    render_incremental_with_context, AsyncHtmlWriter, AsyncRenderError, AsyncSliceWriter,
    AsyncWriteHtmlValue, CancellationToken, RenderCancelled, RenderContext, Slice, WriteHtmlValue,
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
    pending: usize,
    flushes: Vec<usize>,
    max_write: usize,
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
        f.write_str("original character sink failure")
    }
}
impl std::error::Error for Original {}

fn failure() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, Original)
}

impl io::Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut state = self.state.lock().unwrap();
        state.max_write = state.max_write.max(bytes.len());
        let accepted = state.fail_at.map_or(bytes.len(), |limit| {
            limit.saturating_sub(state.bytes.len()).min(bytes.len())
        });
        if accepted == 0 && !bytes.is_empty() {
            return Err(failure());
        }
        state.bytes.extend_from_slice(&bytes[..accepted]);
        state.pending += accepted;
        Ok(accepted)
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut state = self.state.lock().unwrap();
        let count = state.pending;
        state.flushes.push(count);
        state.pending = 0;
        if state.fail_flush {
            return Err(failure());
        }
        Ok(())
    }
}

impl AsyncSliceWriter for Writer {
    fn unflushed_bytes(&self) -> Option<usize> {
        if self.native {
            Some(self.state.lock().unwrap().pending)
        } else {
            None
        }
    }

    async fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        poll_fn(|_| {
            let held = {
                let state = self.state.lock().unwrap();
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

async_template! {
    struct Value<V> where V: AsyncWriteHtmlValue { model: V } => { "[" {model} "]" }
}

#[tokio::test]
async fn character_values_work_in_generated_bodies_and_attributes() {
    assert_eq!(
        html! { "[" {'&'} "]" }.render_to_string().unwrap(),
        "[&amp;]"
    );
    assert_eq!(
        html! { {attribute("data-char", '&').unwrap()} }
            .render_to_string()
            .unwrap(),
        " data-char=\"&amp;\""
    );
    let (mut writer, state) = sink();
    render_incremental(&Value::new('&'), &mut writer)
        .await
        .unwrap();
    assert_eq!(state.lock().unwrap().bytes, b"[&amp;]");
    assert_eq!(state.lock().unwrap().flushes, [7]);
}

async fn golden_case<V: WriteHtmlValue + AsyncWriteHtmlValue>(
    name: &str,
    value: V,
    actual: &mut String,
) {
    let borrowed = &value;
    let page = html! { "[" {borrowed} "]" };
    let text = page.render_to_string().unwrap();
    assert_eq!(page.render_to_bytes().unwrap(), text.as_bytes());
    let (mut writer, state) = sink();
    render_incremental(&Value::new(&value), &mut writer)
        .await
        .unwrap();
    assert_eq!(state.lock().unwrap().bytes, text.as_bytes());
    actual.push_str(&format!("{name}: {text}\n"));
}

#[tokio::test]
async fn pinned_csharp_golden_matches_common_body_nullable_and_attribute_characters() {
    let mut actual = String::new();
    for (name, value) in [
        ("letter", 'A'),
        ("digit", '7'),
        ("space", ' '),
        ("ampersand", '&'),
        ("less-than", '<'),
        ("greater-than", '>'),
        ("double-quote", '"'),
        ("dash", '-'),
        ("underscore", '_'),
    ] {
        golden_case(name, value, &mut actual).await;
    }
    golden_case("nullable-none", None::<char>, &mut actual).await;
    golden_case("nullable-some", Some('Z'), &mut actual).await;
    golden_case(
        "attribute-scalar",
        attribute("data-char", '&').unwrap(),
        &mut actual,
    )
    .await;
    golden_case(
        "attribute-none",
        attribute("data-char", None::<char>).unwrap(),
        &mut actual,
    )
    .await;
    golden_case(
        "attribute-some",
        attribute("data-char", Some('<')).unwrap(),
        &mut actual,
    )
    .await;
    assert_eq!(actual, include_str!("fixtures/character-values.txt"));
}

#[tokio::test]
async fn utf8_scalars_and_escapes_use_exactly_the_existing_string_encoder() {
    for value in [
        '\0',
        '\t',
        '\n',
        '\r',
        '&',
        '<',
        '>',
        '"',
        '\'',
        'a',
        '\u{7f}',
        '\u{80}',
        '\u{7ff}',
        '\u{800}',
        '\u{d7ff}',
        '\u{e000}',
        '\u{ffff}',
        '\u{10000}',
        '\u{e9}',
        '\u{96ea}',
        '\u{1f980}',
        char::MAX,
    ] {
        let mut utf8 = [0; 4];
        let mut expected = Vec::new();
        encode_html_to_writer(value.encode_utf8(&mut utf8), &mut expected).unwrap();
        let mut direct = Vec::new();
        value.write_html_to(&mut direct).unwrap();
        assert_eq!(direct, expected);
        assert_eq!(
            html! { {value} }.render_to_string().unwrap().as_bytes(),
            expected
        );
        let (mut writer, state) = sink();
        AsyncHtmlWriter::new(&mut writer)
            .write_html_value(&value)
            .await
            .unwrap();
        let state = state.lock().unwrap();
        assert_eq!(state.bytes, expected);
        assert!(state.flushes.is_empty());
        assert!(state.max_write <= 6);
    }
}

#[tokio::test]
async fn optional_and_referenced_attribute_characters_keep_omission_and_encoding() {
    for value in ['&', '"', '\'', '\u{96ea}', '\0', '0'] {
        let mut utf8 = [0; 4];
        let text: &str = value.encode_utf8(&mut utf8);
        let mut expected = Vec::new();
        attribute("data-char", text)
            .unwrap()
            .write_html_to(&mut expected)
            .unwrap();
        let mut direct = Vec::new();
        attribute("data-char", Some(&value))
            .unwrap()
            .write_html_to(&mut direct)
            .unwrap();
        assert_eq!(direct, expected);
        let (mut writer, state) = sink();
        AsyncHtmlWriter::new(&mut writer)
            .write_html_value(&attribute("data-char", Some(&value)).unwrap())
            .await
            .unwrap();
        assert_eq!(state.lock().unwrap().bytes, expected);
        assert!(state.lock().unwrap().flushes.is_empty());
    }
    let (mut writer, state) = sink();
    state.lock().unwrap().fail_at = Some(0);
    attribute("data-char", None::<char>)
        .unwrap()
        .write_html_to(&mut writer)
        .unwrap();
    AsyncHtmlWriter::new(&mut writer)
        .write_html_value(&attribute("data-char", None::<char>).unwrap())
        .await
        .unwrap();
    assert!(state.lock().unwrap().bytes.is_empty());
    assert_eq!(
        attribute("bad name", 'x').unwrap_err().byte_index(),
        Some(3)
    );
}

fn assert_original(error: io::Error) {
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    assert!(error.get_ref().unwrap().is::<Original>());
}

async fn every_failure<V: WriteHtmlValue + AsyncWriteHtmlValue>(value: V, expected: &str) {
    for limit in 0..expected.len() {
        let borrowed = &value;
        let page = html! { "[" {borrowed} "]" };
        let (mut writer, state) = sink();
        state.lock().unwrap().fail_at = Some(limit);
        assert_original(page.write_to(&mut writer).unwrap_err());
        assert_eq!(state.lock().unwrap().bytes, &expected.as_bytes()[..limit]);
        assert!(state.lock().unwrap().flushes.is_empty());
        let (mut writer, state) = sink();
        state.lock().unwrap().fail_at = Some(limit);
        let AsyncRenderError::Write(error) = render_incremental(&Value::new(&value), &mut writer)
            .await
            .unwrap_err()
        else {
            panic!("expected original async writer error");
        };
        assert_original(error);
        assert_eq!(state.lock().unwrap().bytes, &expected.as_bytes()[..limit]);
        assert!(state.lock().unwrap().flushes.is_empty());
    }
}

#[tokio::test]
async fn body_attribute_and_partial_utf8_failures_keep_original_errors_and_stop_suffix() {
    every_failure('&', "[&amp;]").await;
    every_failure('\u{1f980}', "[\u{1f980}]").await;
    every_failure(
        attribute("data-char", '"').unwrap(),
        "[ data-char=\"&quot;\"]",
    )
    .await;
    let (mut writer, state) = sink();
    state.lock().unwrap().fail_flush = true;
    let AsyncRenderError::Write(error) = render_incremental(&Value::new('&'), &mut writer)
        .await
        .unwrap_err()
    else {
        panic!("expected original final-flush error");
    };
    assert_original(error);
    assert_eq!(state.lock().unwrap().bytes, b"[&amp;]");
    assert_eq!(state.lock().unwrap().flushes, [7]);
}

struct Noop;
impl Wake for Noop {
    fn wake(self: Arc<Self>) {}
}

#[test]
fn held_character_write_preserves_backpressure_cancellation_and_drop() {
    for outcome in ["complete", "drop", "cancel"] {
        let (mut writer, state) = sink();
        state.lock().unwrap().hold_after = Some(1);
        let token = CancellationToken::new();
        let context = RenderContext::new((), token.clone());
        let page = Value::new('\u{96ea}');
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
                        panic!("cancellation must stop a held character write");
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
                "[\u{96ea}]".as_bytes()
            } else {
                b"["
            }
        );
        assert_eq!(state.flushes.len(), usize::from(outcome == "complete"));
    }
}

async fn primitive<V: WriteHtmlValue + AsyncWriteHtmlValue>(value: V, expected: &str) {
    let (mut writer, state) = sink();
    value.write_html_to(&mut writer).unwrap();
    AsyncHtmlWriter::new(&mut writer)
        .write_html_value(&value)
        .await
        .unwrap();
    let state = state.lock().unwrap();
    assert_eq!(state.bytes, expected.repeat(2).as_bytes());
    assert!(state.flushes.is_empty());
}

#[tokio::test]
async fn existing_boolean_and_special_float_representations_are_not_changed() {
    primitive(true, "true").await;
    primitive(false, "false").await;
    for value in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -0.0,
        1e-30,
        1e30,
    ] {
        primitive(value, &value.to_string()).await;
    }
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.0] {
        primitive(value, &value.to_string()).await;
    }
}

#[tokio::test]
async fn completion_threshold_counts_encoded_and_multibyte_characters_in_both_modes() {
    for native in [false, true] {
        for value in ['&', '\u{96ea}', '\u{1f980}'] {
            let borrowed = &value;
            let body = html! { "[" {borrowed} "]" }.render_to_string().unwrap();
            for size in [16_383, 16_384, 16_385] {
                let (mut writer, state) = sink();
                writer.native = native;
                let padding = "x".repeat(size - body.len());
                let mut output = AsyncHtmlWriter::new(&mut writer);
                output.write_literal(&padding).await.unwrap();
                assert!(state.lock().unwrap().flushes.is_empty());
                output.write_slice(&Value::new(value)).await.unwrap();
                let state = state.lock().unwrap();
                assert_eq!(state.bytes, format!("{padding}{body}").as_bytes());
                assert_eq!(
                    state.flushes,
                    if size >= 16_384 { vec![size] } else { vec![] }
                );
            }
        }
    }
}
