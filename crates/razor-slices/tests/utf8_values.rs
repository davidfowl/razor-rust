use razor_slices::{
    async_template, encode_html_to_writer, html, render_incremental,
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
    held: bool,
}

struct Writer {
    state: Arc<Mutex<State>>,
    native: bool,
}

#[derive(Debug)]
struct Original;
impl std::fmt::Display for Original {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("original UTF-8 sink failure")
    }
}
impl std::error::Error for Original {}

impl io::Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut state = self.state.lock().unwrap();
        state.max_write = state.max_write.max(bytes.len());
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
            if state.held && !state.bytes.is_empty() {
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

async_template! {
    struct Value<V> where V: AsyncWriteHtmlValue { model: V } => { "[" {model} "]" }
}

fn encoded(bytes: &[u8]) -> Vec<u8> {
    let mut expected = Vec::new();
    encode_html_to_writer(&String::from_utf8_lossy(bytes), &mut expected).unwrap();
    expected
}

async fn assert_paths<V: WriteHtmlValue + AsyncWriteHtmlValue>(value: V, expected: &[u8]) {
    let borrowed = &value;
    let page = html! { "[" {borrowed} "]" };
    let expected = [b"[".as_slice(), expected, b"]"].concat();
    for _ in 0..2 {
        assert_eq!(page.render_to_bytes().unwrap(), expected);
        assert_eq!(page.render_to_string().unwrap().as_bytes(), expected);
    }
    let page = Value::new(value);
    for _ in 0..2 {
        let (mut writer, state) = sink();
        render_incremental(&page, &mut writer).await.unwrap();
        let state = state.lock().unwrap();
        assert_eq!(state.bytes, expected);
        assert_eq!(state.flushes, [expected.len()]);
    }
}

#[tokio::test]
async fn pinned_csharp_common_golden_matches_generated_sync_and_async_output() {
    let mut actual = String::new();
    for (name, bytes) in [
        ("empty", b"".as_slice()),
        ("ascii", b"Hello 123"),
        ("escaped", b"A&<B>\""),
    ] {
        let expected = encoded(bytes);
        assert_paths(bytes, &expected).await;
        assert_paths(bytes.to_vec(), &expected).await;
        actual.push_str(&format!(
            "{name}: [{}]\n",
            std::str::from_utf8(&expected).unwrap()
        ));
    }
    assert_eq!(actual, include_str!("fixtures/utf8-values.txt"));
    assert_paths(*b"A&<B>\"", b"A&amp;&lt;B&gt;&quot;").await;
    assert_paths(Some(&b"&"[..]), b"&amp;").await;
    assert_paths(Some(Some(vec![b'<'])), b"&lt;").await;
    assert_paths(None::<Vec<u8>>, b"").await;
    assert_paths(None::<&[u8]>, b"").await;
}

#[tokio::test]
async fn unicode_and_malformed_input_match_replacement_decoded_strings() {
    for bytes in [
        b"\0\t\n\r'".as_slice(),
        "\u{96ea}\u{1f980}\u{10ffff}".as_bytes(),
        &[0xff, b'&'],
        &[0xe2, b'(', 0xa1],
        &[0xf0, 0x9f, 0xa6],
        &[0xc0, 0xaf],
        &[0xed, 0xa0, 0x80],
        &[0xf4, 0x90, 0x80, 0x80],
        &[b'<', 0xe2, 0x82, 0xac, 0xff, b'&', 0xf0, 0x9f],
    ] {
        assert_paths(bytes, &encoded(bytes)).await;
    }
}

#[tokio::test]
async fn empty_and_absent_byte_values_do_not_touch_a_failing_destination() {
    let (mut writer, state) = sink();
    state.lock().unwrap().fail_at = Some(0);
    b"".write_html_to(&mut writer).unwrap();
    Vec::<u8>::new().write_html_to(&mut writer).unwrap();
    None::<Vec<u8>>.write_html_to(&mut writer).unwrap();
    let mut output = AsyncHtmlWriter::new(&mut writer);
    output.write_html_value(b"").await.unwrap();
    output.write_html_value(&Vec::<u8>::new()).await.unwrap();
    output.write_html_value(&None::<Vec<u8>>).await.unwrap();
    let state = state.lock().unwrap();
    assert!(state.bytes.is_empty());
    assert!(state.flushes.is_empty());
}

#[test]
fn all_short_byte_sequences_and_long_malformed_runs_match_standard_decoding() {
    for first in 0..=u8::MAX {
        for bytes in [&[first][..], &[first, 0xff][..]] {
            let mut actual = Vec::new();
            bytes.write_html_to(&mut actual).unwrap();
            assert_eq!(actual, encoded(bytes));
        }
        for second in 0..=u8::MAX {
            let bytes = [first, second];
            let mut actual = Vec::new();
            bytes.write_html_to(&mut actual).unwrap();
            assert_eq!(actual, encoded(&bytes));
        }
    }
    let mut seed = 0x1234_5678_u32;
    for _ in 0..2_048 {
        let bytes: Vec<_> = (0..64)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 24) as u8
            })
            .collect();
        let mut actual = Vec::new();
        bytes.write_html_to(&mut actual).unwrap();
        assert_eq!(actual, encoded(&bytes));
    }
}

fn assert_original(error: io::Error) {
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    assert!(error.get_ref().unwrap().is::<Original>());
}

#[tokio::test]
async fn every_write_failure_preserves_original_error_prefix_and_stops_render() {
    let bytes = [b'&', 0xff, 0xf0, 0x9f, 0xa6, 0x80, b'<'];
    let expected = [b"[".as_slice(), &encoded(&bytes), b"]"].concat();
    let page = html! { "[" {bytes} "]" };
    let async_page = Value::new(bytes);
    for limit in 0..expected.len() {
        let (mut writer, state) = sink();
        state.lock().unwrap().fail_at = Some(limit);
        assert_original(page.write_to(&mut writer).unwrap_err());
        assert_eq!(state.lock().unwrap().bytes, expected[..limit]);
        assert!(state.lock().unwrap().flushes.is_empty());
        let (mut writer, state) = sink();
        state.lock().unwrap().fail_at = Some(limit);
        let AsyncRenderError::Write(error) = render_incremental(&async_page, &mut writer)
            .await
            .unwrap_err()
        else {
            panic!("expected original writer error");
        };
        assert_original(error);
        assert_eq!(state.lock().unwrap().bytes, expected[..limit]);
        assert!(state.lock().unwrap().flushes.is_empty());
    }
    let (mut writer, state) = sink();
    state.lock().unwrap().fail_flush = true;
    let AsyncRenderError::Write(error) = render_incremental(&async_page, &mut writer)
        .await
        .unwrap_err()
    else {
        panic!("expected original final-flush error");
    };
    assert_original(error);
    assert_eq!(state.lock().unwrap().bytes, expected);
    assert_eq!(state.lock().unwrap().flushes, [expected.len()]);
}

struct Noop;
impl Wake for Noop {
    fn wake(self: Arc<Self>) {}
}

#[test]
fn pending_byte_value_retains_backpressure_and_observable_cancellation_or_drop() {
    for outcome in ["complete", "cancel", "drop"] {
        let (mut writer, state) = sink();
        state.lock().unwrap().held = true;
        let token = CancellationToken::new();
        let context = RenderContext::new((), token.clone());
        let page = Value::new(vec![b'&', 0xff]);
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
            match outcome {
                "complete" => {
                    state.lock().unwrap().held = false;
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
                        panic!("held value must observe cancellation");
                    };
                    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
                    assert!(error.get_ref().unwrap().is::<RenderCancelled>());
                }
                "drop" => {}
                _ => unreachable!(),
            }
        }
        let state = state.lock().unwrap();
        let expected = if outcome == "complete" {
            "[&amp;\u{fffd}]".as_bytes()
        } else {
            b"["
        };
        assert_eq!(state.bytes, expected);
        assert_eq!(state.flushes.len(), usize::from(outcome == "complete"));
    }
}

#[tokio::test]
async fn large_values_use_bounded_async_writes_without_value_level_flushes() {
    let mut bytes = vec![b'&'; 1_023];
    bytes.extend_from_slice("\u{1f980}".as_bytes());
    bytes.extend_from_slice(&[0xff, b'<', 0xf0, 0x9f]);
    let bytes = bytes.repeat(128);
    let (mut writer, state) = sink();
    AsyncHtmlWriter::new(&mut writer)
        .write_html_value(&bytes)
        .await
        .unwrap();
    let state = state.lock().unwrap();
    assert_eq!(state.bytes, encoded(&bytes));
    assert!(state.flushes.is_empty());
    assert!(state.max_write <= 6_144);
}

#[tokio::test]
async fn byte_values_count_encoded_replacement_and_unicode_bytes_at_completion() {
    for native in [false, true] {
        for bytes in [b"&".as_slice(), &[0xff], "\u{1f980}".as_bytes()] {
            let expected = [b"[".as_slice(), &encoded(bytes), b"]"].concat();
            for size in [16_383, 16_384, 16_385] {
                let (mut writer, state) = sink();
                writer.native = native;
                let padding = "x".repeat(size - expected.len());
                let mut output = AsyncHtmlWriter::new(&mut writer);
                output.write_literal(&padding).await.unwrap();
                assert!(state.lock().unwrap().flushes.is_empty());
                output.write_slice(&Value::new(bytes)).await.unwrap();
                let state = state.lock().unwrap();
                assert_eq!(state.bytes.len(), size);
                assert_eq!(&state.bytes[padding.len()..], expected);
                assert_eq!(
                    state.flushes,
                    if size >= 16_384 { vec![size] } else { vec![] }
                );
            }
        }
    }
}
