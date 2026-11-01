use razor_slices::{
    async_template, format_integer, html, render_incremental, render_incremental_with_context,
    AsyncHtmlWriter, AsyncRenderError, AsyncSliceWriter, AsyncWriteHtmlValue, CancellationToken,
    FormattedInteger, IntegerFormat, RenderCancelled, RenderContext, Slice, WriteHtmlValue,
};
use std::{
    fmt,
    future::{poll_fn, Future},
    io,
    pin::Pin,
    sync::{Arc, Mutex},
    task::Poll,
};

async_template! {
    struct Page { model: FormattedInteger } => { "P" {model} "S" }
}

#[derive(Default)]
struct State {
    bytes: Vec<u8>,
    fail_at: Option<usize>,
    hold_at: Option<usize>,
    flushes: usize,
    drops: usize,
    max_write: usize,
}

struct Writer {
    state: Arc<Mutex<State>>,
    failure: Arc<()>,
}

impl Writer {
    fn new(fail_at: Option<usize>) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                fail_at,
                ..State::default()
            })),
            failure: Arc::new(()),
        }
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        self.state.lock().unwrap().drops += 1;
    }
}

#[derive(Debug)]
struct Original(Arc<()>);
impl fmt::Display for Original {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("original integer destination failure")
    }
}
impl std::error::Error for Original {}

fn accepted(state: &State, requested: usize) -> usize {
    state.fail_at.map_or(requested, |limit| {
        requested.min(limit.saturating_sub(state.bytes.len()))
    })
}

impl io::Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut state = self.state.lock().unwrap();
        state.max_write = state.max_write.max(bytes.len());
        let count = accepted(&state, bytes.len());
        if count == 0 && !bytes.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                Original(self.failure.clone()),
            ));
        }
        state.bytes.extend_from_slice(&bytes[..count]);
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.state.lock().unwrap().flushes += 1;
        Ok(())
    }
}

impl AsyncSliceWriter for Writer {
    async fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        let mut offset = 0;
        poll_fn(|_| {
            let mut state = self.state.lock().unwrap();
            state.max_write = state.max_write.max(bytes.len());
            if state
                .hold_at
                .is_some_and(|limit| state.bytes.len() >= limit)
            {
                return Poll::Pending;
            }
            let mut count = accepted(&state, bytes.len() - offset);
            if let Some(limit) = state.hold_at {
                count = count.min(limit.saturating_sub(state.bytes.len()));
            }
            state
                .bytes
                .extend_from_slice(&bytes[offset..offset + count]);
            offset += count;
            if offset == bytes.len() {
                Poll::Ready(Ok(()))
            } else if state.hold_at == Some(state.bytes.len()) {
                Poll::Pending
            } else {
                Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    Original(self.failure.clone()),
                )))
            }
        })
        .await
    }

    async fn flush(&mut self) -> io::Result<()> {
        io::Write::flush(self)
    }
}

fn assert_original(error: io::Error, writer: &Writer) {
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    let original = error.get_ref().unwrap().downcast_ref::<Original>().unwrap();
    assert!(Arc::ptr_eq(&original.0, &writer.failure));
}

fn expected(value: u64, format: IntegerFormat, precision: u8) -> String {
    let width = usize::from(precision);
    match format {
        IntegerFormat::Decimal => format!("{value:0width$}"),
        IntegerFormat::LowerHex => format!("{value:0width$x}"),
        IntegerFormat::UpperHex => format!("{value:0width$X}"),
    }
}

fn formats() -> [(IntegerFormat, &'static str); 3] {
    [
        (IntegerFormat::Decimal, "D"),
        (IntegerFormat::LowerHex, "x"),
        (IntegerFormat::UpperHex, "X"),
    ]
}

fn row(label: &str, status: &str, bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(14695981039346656037_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(1099511628211)
    });
    format!(
        "common/{label}: status={status};bytes={};fnv1a={hash:016x}",
        bytes.len()
    )
}

#[tokio::test]
async fn pinned_uint64_decimal_and_hex_formats_match_sync_and_incremental() {
    let mut rows = Vec::new();
    for value in [0, 1, 15, 16, 42, 255, 256, u64::MAX] {
        for (format, letter) in formats() {
            for precision in [0, 1, 4, 20, 32, 255] {
                let model = format_integer(value, format, precision);
                let expected = format!("P{}S", expected(value, format, precision));
                let page = html! { "P" {model} "S" };
                assert_eq!(page.render_to_bytes().unwrap(), expected.as_bytes());
                assert_eq!(page.render_to_string().unwrap(), expected);
                let mut writer = Writer::new(None);
                render_incremental(&Box::new(&Page::new(model)), &mut writer)
                    .await
                    .unwrap();
                let state = writer.state.lock().unwrap();
                assert_eq!(state.bytes, expected.as_bytes());
                assert_eq!(state.flushes, 1);
                assert!(state.max_write <= 255);
                rows.push(row(
                    &format!("{value}/{letter}{precision}"),
                    "ok",
                    &state.bytes,
                ));
            }
        }
    }
    for precision in [4, 255] {
        let offsets: &[usize] = if precision == 4 {
            &[0, 1, 2, 3, 4, 5]
        } else {
            &[0, 1, 2, 128, 255, 256]
        };
        for &offset in offsets {
            let model = format_integer(255, IntegerFormat::UpperHex, precision);
            let expected = format!("P{}S", expected(255, IntegerFormat::UpperHex, precision));
            for incremental in [false, true] {
                let mut writer = Writer::new(Some(offset));
                if incremental {
                    let error = render_incremental(&Page::new(model), &mut writer)
                        .await
                        .unwrap_err();
                    let AsyncRenderError::Write(error) = error else {
                        panic!("expected original I/O error")
                    };
                    assert_original(error, &writer);
                } else {
                    let error = html! { "P" {model} "S" }
                        .render_to_writer(&mut writer)
                        .unwrap_err();
                    assert_original(error, &writer);
                }
                let state = writer.state.lock().unwrap();
                assert_eq!(state.bytes, expected.as_bytes()[..offset]);
                assert_eq!(state.flushes, 0);
                if incremental {
                    rows.push(row(
                        &format!("error/{precision}/{offset}"),
                        "same-error",
                        &state.bytes,
                    ));
                }
            }
        }
    }
    let fixture: Vec<_> = include_str!("fixtures/integer-formatting.txt")
        .lines()
        .collect();
    assert_eq!(fixture.len(), 156);
    assert_eq!(rows, fixture);
}

#[tokio::test]
async fn every_precision_extrema_and_all_single_byte_values_use_bounded_identical_paths() {
    for (format, _) in formats() {
        for precision in 0..=u8::MAX {
            for value in [0, 1, 15, 16, 255, 256, 1 << 63, u64::MAX] {
                let model = format_integer(value, format, precision);
                let expected = expected(value, format, precision);
                let mut sync = Vec::new();
                model.write_html_to(&mut sync).unwrap();
                assert_eq!(sync, expected.as_bytes());
                let mut writer = Writer::new(None);
                model
                    .write_html_async(&mut AsyncHtmlWriter::new(&mut writer))
                    .await
                    .unwrap();
                let state = writer.state.lock().unwrap();
                assert_eq!(state.bytes, sync);
                assert_eq!(state.flushes, 0);
                assert!(state.max_write <= 255);
            }
        }
        for value in 0..=u8::MAX {
            let model = format_integer(u64::from(value), format, 0);
            assert_eq!(
                html! { {model} }.render_to_string().unwrap(),
                expected(u64::from(value), format, 0)
            );
        }
    }
}

async fn poll_once<F: Future>(mut future: Pin<&mut F>) -> Poll<F::Output> {
    poll_fn(|cx| Poll::Ready(future.as_mut().poll(cx))).await
}

#[tokio::test]
async fn held_partial_write_resumes_without_replay_and_drop_or_cancel_keeps_exact_prefix() {
    let model = format_integer(u64::MAX, IntegerFormat::UpperHex, 255);
    let page = Page::new(model);
    let expected = format!("P{}S", expected(u64::MAX, IntegerFormat::UpperHex, 255));
    for offset in 0..expected.len() {
        for mode in ["resume", "drop", "legacy-drop", "cancel"] {
            let mut writer = Writer::new(None);
            let state = writer.state.clone();
            state.lock().unwrap().hold_at = Some(offset);
            let context = RenderContext::new((), CancellationToken::new());
            let mut render = Box::pin(async {
                if mode == "legacy-drop" {
                    render_incremental(&page, &mut writer).await
                } else {
                    render_incremental_with_context(&page, &mut writer, &context).await
                }
            });
            fn assert_send<T: Send>(_: &T) {}
            assert_send(&render);
            assert!(poll_once(render.as_mut()).await.is_pending());
            assert_eq!(state.lock().unwrap().bytes, expected.as_bytes()[..offset]);
            if mode == "resume" {
                state.lock().unwrap().hold_at = None;
                assert!(matches!(
                    poll_once(render.as_mut()).await,
                    Poll::Ready(Ok(()))
                ));
            } else if mode == "cancel" {
                context.cancellation_token().cancel();
                let Poll::Ready(Err(AsyncRenderError::Write(error))) =
                    poll_once(render.as_mut()).await
                else {
                    panic!("expected observable cancellation");
                };
                assert_eq!(error.kind(), io::ErrorKind::Interrupted);
                assert!(error.get_ref().unwrap().is::<RenderCancelled>());
            }
            drop(render);
            {
                let state = state.lock().unwrap();
                let prefix = if mode == "resume" {
                    expected.as_bytes()
                } else {
                    &expected.as_bytes()[..offset]
                };
                assert_eq!(state.bytes, prefix);
                assert_eq!(state.flushes, usize::from(mode == "resume"));
                assert_eq!(state.drops, 0);
            }
            assert_eq!(
                context.cancellation_token().is_cancelled(),
                mode == "cancel"
            );
            {
                let mut state = state.lock().unwrap();
                state.hold_at = None;
                state.bytes.clear();
            }
            render_incremental(&page, &mut writer).await.unwrap();
            assert_eq!(state.lock().unwrap().bytes, expected.as_bytes());
            drop(writer);
            assert_eq!(state.lock().unwrap().drops, 1);
        }
    }
}

#[tokio::test]
async fn original_error_at_every_maximum_width_byte_stops_suffix_without_flush() {
    let model = format_integer(255, IntegerFormat::UpperHex, 255);
    let expected = format!("P{}S", expected(255, IntegerFormat::UpperHex, 255));
    for offset in 0..expected.len() {
        let mut sync = Writer::new(Some(offset));
        let error = html! { "P" {model} "S" }
            .render_to_writer(&mut sync)
            .unwrap_err();
        assert_original(error, &sync);
        let mut asynchronous = Writer::new(Some(offset));
        let error = render_incremental(&Page::new(model), &mut asynchronous)
            .await
            .unwrap_err();
        let AsyncRenderError::Write(error) = error else {
            panic!("expected I/O error")
        };
        assert_original(error, &asynchronous);
        for writer in [&sync, &asynchronous] {
            let state = writer.state.lock().unwrap();
            assert_eq!(state.bytes, expected.as_bytes()[..offset]);
            assert_eq!(state.flushes, 0);
        }
    }
}

#[tokio::test]
async fn reusable_options_references_and_defaults_remain_explicit() {
    let model = format_integer(42, IntegerFormat::Decimal, 4);
    let page = html! { {Some(&model)} {None::<FormattedInteger>} "|" {255_u64} "|" {true} };
    for _ in 0..2 {
        assert_eq!(page.render_to_string().unwrap(), "0042|255|true");
    }
    async_template! {
        struct Optional { model: Option<FormattedInteger> } => { {model} "|" {255_u64} }
    }
    for model in [Some(model), None] {
        let mut writer = Writer::new(None);
        let page = Optional::new(model);
        for _ in 0..2 {
            writer.state.lock().unwrap().bytes.clear();
            render_incremental(&page, &mut writer).await.unwrap();
            assert_eq!(
                writer.state.lock().unwrap().bytes,
                if model.is_some() {
                    &b"0042|255"[..]
                } else {
                    &b"|255"[..]
                }
            );
        }
    }
}
